//! Columnar BED features for a loaded region, and their table schema.

use crate::{
    error::TGVError,
    intervals::{IntervalSchema, IntervalTable, Region, pack_lanes},
    table_schema::{ColumnDoc, TableSchema},
};
use noodles::bed::{self, feature::record::Strand};
use polars::prelude::*;
use std::{io, sync::Arc};

/// BED features overlapping the loaded region of one track.
#[derive(Debug)]
pub struct BedTable {
    /// Rows are sorted by contig, start, end, and row ID.
    pub data: DataFrame,
    pub contig_index: usize,
    pub lanes: BedLanes,
    data_complete_left_bound: u64,
    data_complete_right_bound: u64,
}

/// Editable lane positions for one contig, including lanes with no features.
#[derive(Debug, Clone, Copy, Default)]
pub struct BedLanes {
    pub count: u32,
    /// No selection means that edits apply across all lanes.
    pub active: Option<u32>,
}

impl BedLanes {
    /// Keeps a selected lane inside a bounded lane viewport.
    pub fn first_visible(self, visible: u32) -> u32 {
        self.active
            .map_or(0, |lane| lane.saturating_sub(visible.saturating_sub(1)))
    }
}

/// The ordered columns for BED features.
pub struct BedSchema;

impl BedSchema {
    pub const LANE: &'static str = IntervalSchema::LANE;
    pub const ROW_ID: &'static str = IntervalSchema::ROW_ID;
    pub const CONTIG_INDEX: &'static str = IntervalSchema::CONTIG_INDEX;
    pub const START: &'static str = IntervalSchema::START;
    pub const END: &'static str = IntervalSchema::END;
    pub const NAME: &'static str = "name";
    pub const SCORE: &'static str = "score";
    pub const STRAND: &'static str = "strand";
    /// Columns 7 and up, joined by tabs, so writing a file keeps them.
    pub const EXTRA: &'static str = "extra";
}

impl TableSchema for BedSchema {
    fn schema() -> SchemaRef {
        let mut schema = Schema::with_capacity(9);
        schema.insert(Self::ROW_ID.into(), DataType::UInt64);
        schema.insert(Self::CONTIG_INDEX.into(), DataType::UInt64);
        schema.insert(Self::START.into(), DataType::UInt64);
        schema.insert(Self::END.into(), DataType::UInt64);
        schema.insert(Self::NAME.into(), DataType::String);
        schema.insert(Self::SCORE.into(), DataType::UInt16);
        schema.insert(Self::STRAND.into(), DataType::String);
        schema.insert(Self::EXTRA.into(), DataType::String);
        schema.insert(Self::LANE.into(), DataType::UInt32);
        Arc::new(schema)
    }

    fn column_docs() -> &'static [ColumnDoc] {
        &[
            ColumnDoc {
                name: Self::ROW_ID,
                description: "The zero-based feature index within the loaded data, in file order: within the whole file for plain BED files, and within the loaded region for indexed BED and bigBed files.",
            },
            ColumnDoc {
                name: Self::START,
                description: "The 1-based first position, converted from the BED 0-based start.",
            },
            ColumnDoc {
                name: Self::END,
                description: "The last position, inclusive.",
            },
            ColumnDoc {
                name: Self::NAME,
                description: "The BED name, or null when absent.",
            },
            ColumnDoc {
                name: Self::SCORE,
                description: "The BED score, from 0 to 1000 by convention, or null when absent or `.`.",
            },
            ColumnDoc {
                name: Self::STRAND,
                description: "`+` or `-`, or null when absent.",
            },
            ColumnDoc {
                name: Self::EXTRA,
                description: "Columns 7 and up as the file has them, joined by tabs, or null when the line has six or fewer columns.",
            },
            ColumnDoc {
                name: Self::LANE,
                description: "The zero-based packed lane, computed per contig for plain files and per loaded window for indexed files. Overlapping inclusive intervals occupy different lanes.",
            },
        ]
    }
}

impl Default for BedTable {
    fn default() -> Self {
        Self {
            data: BedSchema::empty(),
            contig_index: 0,
            lanes: BedLanes::default(),
            data_complete_left_bound: u64::MAX,
            data_complete_right_bound: 0,
        }
    }
}

impl IntervalTable for BedTable {
    fn query(&self, contig_index: usize, start: u64, end: u64) -> Result<DataFrame, TGVError> {
        if start == 0 {
            return Err(TGVError::ValueError(
                "Interval queries require a positive start.".into(),
            ));
        }
        if start > end {
            return Ok(DataFrame::full_null(self.data.schema(), 0));
        }
        Ok(self
            .data
            .clone()
            .lazy()
            .filter(
                col(BedSchema::CONTIG_INDEX)
                    .eq(lit(contig_index as u64))
                    .and(col(BedSchema::START).lt_eq(lit(end)))
                    .and(col(BedSchema::END).gt_eq(lit(start))),
            )
            .collect()?)
    }
}

impl BedTable {
    /// Whether the table holds every feature overlapping the region.
    pub fn has_complete_data(&self, region: &Region) -> bool {
        region.contig_index() == self.contig_index
            && region.start() >= self.data_complete_left_bound
            && region.end() <= self.data_complete_right_bound
    }

    /// Wraps a frame holding every feature of one contig, read before contig indexes were known.
    pub(super) fn whole_contig(
        data: &DataFrame,
        contig_index: usize,
        lanes: BedLanes,
    ) -> Result<Self, TGVError> {
        let data = data
            .clone()
            .lazy()
            .with_columns([lit(contig_index as u64)
                .cast(DataType::UInt64)
                .alias(BedSchema::CONTIG_INDEX)])
            .collect()?;
        Ok(Self {
            data,
            contig_index,
            lanes,
            data_complete_left_bound: 1,
            data_complete_right_bound: u64::MAX,
        })
    }
}

/// Collects BED features into table columns, numbering rows in the order they are added.
#[derive(Default)]
pub(super) struct BedColumns {
    contigs: Vec<u64>,
    starts: Vec<u64>,
    ends: Vec<u64>,
    names: Vec<Option<String>>,
    scores: Vec<Option<u16>>,
    strands: Vec<Option<&'static str>>,
    extras: Vec<Option<String>>,
    /// The most standard fields (three to six) that any added line has.
    pub(super) standard_columns: usize,
}

impl BedColumns {
    /// Adds a BED line on the given contig, parsed and validated by noodles. Skips blank,
    /// comment, `track`, and `browser` lines.
    ///
    /// The line is read with as many standard fields as it has, up to six. Further columns are
    /// kept as they are in `EXTRA`.
    pub(super) fn push_line(&mut self, line: &str, contig_index: usize) -> Result<(), TGVError> {
        let line = line.trim_end();
        if !is_data_line(line) {
            return Ok(());
        }
        let fields: Vec<&str> = line.split('\t').collect();
        // `.` is a common placeholder for a missing score, but noodles requires an integer.
        let score_missing = fields.get(4) == Some(&".");
        let extra = (fields.len() > 6).then(|| fields[6..].join("\t"));
        self.standard_columns = self.standard_columns.max(fields.len().min(6));
        let parsed = |error: io::Error| {
            TGVError::ValueError(format!("BED line {line:?} is invalid: {error}"))
        };
        match fields.len() {
            0..=2 => Err(TGVError::ValueError(format!(
                "BED line {line:?} has fewer than three tab-separated fields."
            ))),
            3 => self.push_record(
                &read_record::<3>(line, |reader, record| reader.read_record(record))
                    .map_err(parsed)?,
                contig_index,
                score_missing,
                extra,
            ),
            4 => self.push_record(
                &read_record::<4>(line, |reader, record| reader.read_record(record))
                    .map_err(parsed)?,
                contig_index,
                score_missing,
                extra,
            ),
            5 => self.push_record(
                &read_record::<5>(line, |reader, record| reader.read_record(record))
                    .map_err(parsed)?,
                contig_index,
                score_missing,
                extra,
            ),
            _ => self.push_record(
                &read_record::<6>(line, |reader, record| reader.read_record(record))
                    .map_err(parsed)?,
                contig_index,
                score_missing,
                extra,
            ),
        }
    }

    /// Adds a record whose fields noodles has validated.
    fn push_record<const N: usize>(
        &mut self,
        record: &impl bed::feature::Record<N>,
        contig_index: usize,
        score_missing: bool,
        extra: Option<String>,
    ) -> Result<(), TGVError> {
        let start = record.feature_start()?.get() as u64;
        // noodles reads an end of 0 as missing, which the check below rejects.
        let end = record
            .feature_end()
            .transpose()?
            .map_or(0, |end| end.get() as u64);
        if end < start {
            return Err(TGVError::ValueError(format!(
                "Invalid BED interval [{start}, {end}] at row {}.",
                self.starts.len()
            )));
        }
        let score = if score_missing {
            None
        } else {
            record.score().transpose()?
        };
        self.contigs.push(contig_index as u64);
        self.starts.push(start);
        self.ends.push(end);
        self.names
            .push(record.name().flatten().map(|name| name.to_string()));
        self.scores.push(score);
        self.strands
            .push(match record.strand().transpose()?.flatten() {
                Some(Strand::Forward) => Some("+"),
                Some(Strand::Reverse) => Some("-"),
                None => None,
            });
        self.extras.push(extra);
        Ok(())
    }

    /// Builds the columns into a frame in the order rows were added, numbering them from 0.
    pub(super) fn into_frame(self) -> Result<DataFrame, TGVError> {
        let count = self.starts.len();
        Ok(DataFrame::new(
            self.starts.len(),
            vec![
                Column::new(
                    BedSchema::ROW_ID.into(),
                    (0..self.starts.len() as u64).collect::<Vec<_>>(),
                ),
                Column::new(BedSchema::CONTIG_INDEX.into(), self.contigs),
                Column::new(BedSchema::START.into(), self.starts),
                Column::new(BedSchema::END.into(), self.ends),
                Column::new(BedSchema::NAME.into(), self.names),
                Column::new(BedSchema::SCORE.into(), self.scores),
                Column::new(BedSchema::STRAND.into(), self.strands),
                Column::new(BedSchema::EXTRA.into(), self.extras),
                Column::new(BedSchema::LANE.into(), vec![0u32; count]),
            ],
        )?)
    }

    pub(super) fn into_table(
        self,
        contig_index: usize,
        loaded_bounds: (u64, u64),
    ) -> Result<BedTable, TGVError> {
        let mut data = self
            .into_frame()?
            .lazy()
            .sort(
                [
                    BedSchema::CONTIG_INDEX,
                    BedSchema::START,
                    BedSchema::END,
                    BedSchema::ROW_ID,
                ],
                SortMultipleOptions::default(),
            )
            .collect()?;
        pack_lanes(&mut data)?;
        let lanes = BedLanes {
            count: crate::intervals::lane_count(&data)?,
            active: None,
        };
        Ok(BedTable {
            lanes,
            data,
            contig_index,
            data_complete_left_bound: loaded_bounds.0,
            data_complete_right_bound: loaded_bounds.1,
        })
    }
}

/// Whether a BED line holds a feature, rather than being blank, a comment, or a `track` or
/// `browser` line.
pub(super) fn is_data_line(line: &str) -> bool {
    !(line.trim_end().is_empty()
        || line.starts_with('#')
        || line.starts_with("track")
        || line.starts_with("browser"))
}

/// Parses one line as a noodles record with `N` standard fields.
///
/// noodles provides `read_record` for each `N` separately, so callers pass it in.
fn read_record<const N: usize>(
    line: &str,
    read: impl FnOnce(&mut bed::io::Reader<N, &[u8]>, &mut bed::Record<N>) -> io::Result<usize>,
) -> io::Result<bed::Record<N>>
where
    bed::Record<N>: Default,
{
    let mut record = bed::Record::default();
    let mut reader = bed::io::Reader::new(line.as_bytes());
    if read(&mut reader, &mut record)? == 0 {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "empty line"));
    }
    Ok(record)
}

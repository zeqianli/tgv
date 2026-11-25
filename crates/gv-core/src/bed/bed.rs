//! Columnar BED features for a loaded region, and their table schema.

use crate::{
    error::Error,
    intervals::{IntervalSchema, IntervalTable, Region},
    stacking::{STACKING_GAP, empty_with_rows, filter_rows, interval_depth, with_stacked_rows},
    table_schema::{ColumnDoc, TableSchema},
};
use noodles::bed::{self, feature::record::Strand};
use polars::prelude::*;
use std::{io, ops::Range, sync::Arc};

/// BED features overlapping the loaded region of one track.
#[derive(Debug, Clone)]
pub struct BedTable {
    /// Rows are sorted by contig, start, end, and row ID.
    pub data: DataFrame,
    pub contig_index: usize,
    data_complete_left_bound: u64,
    data_complete_right_bound: u64,
}

/// The ordered columns for BED features.
pub struct BedSchema;

impl BedSchema {
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
        let mut schema = Schema::with_capacity(8);
        schema.insert(Self::ROW_ID.into(), DataType::UInt64);
        schema.insert(Self::CONTIG_INDEX.into(), DataType::UInt64);
        schema.insert(Self::START.into(), DataType::UInt64);
        schema.insert(Self::END.into(), DataType::UInt64);
        schema.insert(Self::NAME.into(), DataType::String);
        schema.insert(Self::SCORE.into(), DataType::UInt16);
        schema.insert(Self::STRAND.into(), DataType::String);
        schema.insert(Self::EXTRA.into(), DataType::String);
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
        ]
    }
}

impl Default for BedTable {
    fn default() -> Self {
        Self {
            data: empty_with_rows(BedSchema::empty()),
            contig_index: 0,
            data_complete_left_bound: u64::MAX,
            data_complete_right_bound: 0,
        }
    }
}

impl IntervalTable for BedTable {
    fn query(&self, contig_index: usize, start: u64, end: u64) -> Result<DataFrame, Error> {
        if start == 0 {
            return Err(Error::ValueError(
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
    pub fn depth(&self) -> Result<usize, Error> {
        interval_depth(&self.data)
    }

    pub fn has_complete_data(&self, region: &Region) -> bool {
        region.contig_index() == self.contig_index
            && region.start() >= self.data_complete_left_bound
            && region.end() <= self.data_complete_right_bound
    }

    /// Wraps a frame holding every feature of one contig, read before contig indexes were known.
    pub(super) fn whole_contig(data: &DataFrame, contig_index: usize) -> Result<Self, Error> {
        Ok(Self {
            data: with_stacked_rows(
                data.clone()
                    .lazy()
                    .with_columns([lit(contig_index as u64)
                        .cast(DataType::UInt64)
                        .alias(BedSchema::CONTIG_INDEX)])
                    .collect()?,
                STACKING_GAP,
            )?,
            contig_index,
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
    pub(super) fn push_line(&mut self, line: &str, contig_index: usize) -> Result<(), Error> {
        let line = line.trim_end();
        if !is_data_line(line) {
            return Ok(());
        }
        let fields: Vec<&str> = line.split('\t').collect();
        // `.` is a common placeholder for a missing score, but noodles requires an integer.
        let score_missing = fields.get(4) == Some(&".");
        let extra = (fields.len() > 6).then(|| fields[6..].join("\t"));
        self.standard_columns = self.standard_columns.max(fields.len().min(6));
        let parsed =
            |error: io::Error| Error::ValueError(format!("BED line {line:?} is invalid: {error}"));
        match fields.len() {
            0..=2 => Err(Error::ValueError(format!(
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
    ) -> Result<(), Error> {
        let start = record.feature_start()?.get() as u64;
        // noodles reads an end of 0 as missing, which the check below rejects.
        let end = record
            .feature_end()
            .transpose()?
            .map_or(0, |end| end.get() as u64);
        if end < start {
            return Err(Error::ValueError(format!(
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
    pub(super) fn into_frame(self) -> Result<DataFrame, Error> {
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
            ],
        )?)
    }

    pub(super) fn into_table(
        self,
        contig_index: usize,
        loaded_bounds: (u64, u64),
    ) -> Result<BedTable, Error> {
        let data = self
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
        Ok(BedTable {
            data: with_stacked_rows(data, STACKING_GAP)?,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BedFeature {
    pub start: u64,
    pub end: u64,
    pub y: u64,
    pub name: Option<String>,
    pub strand: Option<crate::strand::Strand>,
}

impl BedTable {
    pub fn features(
        &self,
        contig_index: usize,
        start: u64,
        end: u64,
        rows: Range<u64>,
    ) -> Result<Vec<BedFeature>, Error> {
        let rows = filter_rows(self.query(contig_index, start, end)?, rows)?;
        let starts = rows.column(BedSchema::START)?.u64()?;
        let ends = rows.column(BedSchema::END)?.u64()?;
        let ys = rows.column(IntervalSchema::Y)?.u64()?;
        let names = rows.column(BedSchema::NAME)?.str()?;
        let strands = rows.column(BedSchema::STRAND)?.str()?;
        (0..rows.height())
            .map(|row| {
                Ok(BedFeature {
                    start: starts.get(row).ok_or(Error::NullValue {
                        column: BedSchema::START,
                    })?,
                    end: ends.get(row).ok_or(Error::NullValue {
                        column: BedSchema::END,
                    })?,
                    y: ys.get(row).ok_or(Error::NullValue {
                        column: IntervalSchema::Y,
                    })?,
                    name: names
                        .get(row)
                        .filter(|name| !name.is_empty())
                        .map(str::to_string),
                    strand: match strands.get(row) {
                        Some("+") => Some(crate::strand::Strand::Forward),
                        Some("-") => Some(crate::strand::Strand::Reverse),
                        _ => None,
                    },
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strand::Strand as FeatureStrand;
    use rstest::rstest;

    fn table() -> BedTable {
        let data = DataFrame::new(
            5,
            vec![
                Column::new(BedSchema::CONTIG_INDEX.into(), vec![0u64, 0, 0, 0, 1]),
                Column::new(BedSchema::START.into(), vec![10u64, 30, 50, 70, 10]),
                Column::new(BedSchema::END.into(), vec![20u64, 40, 60, 80, 20]),
                Column::new(
                    BedSchema::NAME.into(),
                    vec![Some("a"), Some(""), None, Some("d"), Some("other")],
                ),
                Column::new(
                    BedSchema::STRAND.into(),
                    vec![Some("+"), Some("-"), Some("."), None, Some("+")],
                ),
            ],
        )
        .unwrap();
        BedTable {
            data: with_stacked_rows(data, STACKING_GAP).unwrap(),
            ..BedTable::default()
        }
    }

    fn feature(
        start: u64,
        end: u64,
        name: Option<&str>,
        strand: Option<FeatureStrand>,
    ) -> BedFeature {
        BedFeature {
            start,
            end,
            y: 0,
            name: name.map(str::to_string),
            strand,
        }
    }

    #[rstest]
    #[case::whole_contig(0, 1, 100, vec![
        feature(10, 20, Some("a"), Some(FeatureStrand::Forward)),
        feature(30, 40, None, Some(FeatureStrand::Reverse)),
        feature(50, 60, None, None),
        feature(70, 80, Some("d"), None),
    ])]
    #[case::partial_overlap(0, 15, 35, vec![
        feature(10, 20, Some("a"), Some(FeatureStrand::Forward)),
        feature(30, 40, None, Some(FeatureStrand::Reverse)),
    ])]
    #[case::inclusive_bounds(0, 20, 30, vec![
        feature(10, 20, Some("a"), Some(FeatureStrand::Forward)),
        feature(30, 40, None, Some(FeatureStrand::Reverse)),
    ])]
    #[case::between_features(0, 21, 29, vec![])]
    #[case::other_contig(1, 1, 100, vec![
        feature(10, 20, Some("other"), Some(FeatureStrand::Forward)),
    ])]
    #[case::contig_without_features(2, 1, 100, vec![])]
    #[case::reversed_bounds(0, 60, 50, vec![])]
    fn features(
        #[case] contig_index: usize,
        #[case] start: u64,
        #[case] end: u64,
        #[case] expected: Vec<BedFeature>,
    ) {
        assert_eq!(
            table()
                .features(contig_index, start, end, 0..u64::MAX)
                .unwrap(),
            expected
        );
    }

    #[test]
    fn features_reject_a_zero_start() {
        assert!(table().features(0, 0, 100, 0..u64::MAX).is_err());
    }
}

use crate::alignment::{
    coverage::CoverageTable,
    tables::{AlignmentTables, CigarRunSchema, ReadSchema},
    viewport::AlignmentViewport,
};
use crate::error::TGVError;
use crate::intervals::{GenomeInterval, Region};
use crate::message::{AlignmentFilter, AlignmentSort};
use crate::sequence::Sequence;
use crate::table_schema::TableSchema;
use noodles::sam::alignment::RecordBuf;
use polars::prelude::*;

/// Per-read bases and sorting priorities at one reference position.
pub(super) struct BaseEventSchema;

impl BaseEventSchema {
    pub const READ_ID: &'static str = ReadSchema::READ_ID;
    pub const BASE: &'static str = "base";
    pub const SORT_KEY: &'static str = "sort_key";
}

impl TableSchema for BaseEventSchema {
    fn schema() -> SchemaRef {
        let mut schema = Schema::with_capacity(3);
        schema.insert(Self::READ_ID.into(), DataType::UInt64);
        schema.insert(Self::BASE.into(), DataType::String);
        schema.insert(Self::SORT_KEY.into(), DataType::UInt8);
        std::sync::Arc::new(schema)
    }
}

/// An alignment stack
#[derive(Debug)]
pub struct Alignment {
    /// Contig of the current alignment
    pub contig_index: usize,

    /// The original decoded records in stable read-ID order.
    pub records: Vec<RecordBuf>,

    /// The queryable alignment representation.
    pub tables: AlignmentTables,

    /// Derived coverage of the visible reads.
    pub coverage: CoverageTable,

    /// The left bound of region with complete data.
    /// 1-based, inclusive.
    data_complete_left_bound: u64,

    /// The right bound of region with complete data.
    /// 1-based, inclusive.
    data_complete_right_bound: u64,
}

impl Default for Alignment {
    fn default() -> Self {
        Self {
            contig_index: 0,
            records: Vec::new(),
            tables: AlignmentTables::default(),
            coverage: CoverageTable::default(),
            data_complete_left_bound: 0,
            data_complete_right_bound: 0,
        }
    }
}

impl Alignment {
    /// Check if data in [left, right] is all loaded.
    /// 1-based, inclusive.
    pub fn has_complete_data(&self, region: &Region) -> bool {
        (region.contig_index() == self.contig_index)
            && (region.start() >= self.data_complete_left_bound)
            && (region.end() <= self.data_complete_right_bound)
    }

    pub(super) fn ensure_position_has_complete_data(&self, position: u64) -> Result<(), TGVError> {
        if position < self.data_complete_left_bound || position > self.data_complete_right_bound {
            return Err(TGVError::AlignmentSortPositionNotLoaded {
                position,
                loaded_left: self.data_complete_left_bound,
                loaded_right: self.data_complete_right_bound,
            });
        }

        Ok(())
    }

    /// Return the number of alignment tracks.
    pub fn depth(&self) -> Result<usize, TGVError> {
        if !self.tables.reads.column(ReadSchema::SHOW)?.bool()?.any() {
            return Ok(0);
        }
        Ok(self
            .tables
            .reads
            .column(ReadSchema::Y)?
            .u64()?
            .max()
            .map_or(0, |y| y as usize + 1))
    }

    /// Return the read at x_coordinate, yth track
    pub fn read_overlapping(
        &self,
        left: u64,
        right: u64,
        y: usize,
    ) -> Result<Option<usize>, TGVError> {
        if left > right {
            return Ok(None);
        }
        let hits = self
            .tables
            .reads
            .clone()
            .lazy()
            .filter(
                col(ReadSchema::SHOW)
                    .and(col(ReadSchema::Y).eq(lit(y as u64)))
                    .and(col(ReadSchema::STACKING_START).lt_eq(lit(right)))
                    .and(col(ReadSchema::STACKING_END).gt_eq(lit(left))),
            )
            .select([col(ReadSchema::READ_ID)])
            .sort([ReadSchema::READ_ID], SortMultipleOptions::default())
            .limit(1)
            .collect()?;
        Ok(hits
            .column(ReadSchema::READ_ID)?
            .u64()?
            .iter()
            .next()
            .flatten()
            .map(|id| id as usize))
    }

    pub fn query_viewport(
        &self,
        region: &Region,
        read_ids: &[usize],
    ) -> Result<AlignmentViewport, TGVError> {
        if region.contig_index() != self.contig_index || region.start() > region.end() {
            return self.tables.viewport(1, 1, &[]);
        }
        self.tables.viewport(region.start(), region.end(), read_ids)
    }

    pub fn from_records(
        records: Vec<RecordBuf>,
        contig_index: usize,
        data_complete_bound: (u64, u64),
        reference_sequence: &Sequence,
    ) -> Result<Self, TGVError> {
        let tables =
            AlignmentTables::default().add_records(&records, reference_sequence, contig_index)?;
        Self::from_tables(
            records,
            tables,
            contig_index,
            data_complete_bound,
            reference_sequence,
        )
    }

    pub(crate) fn from_tables(
        records: Vec<RecordBuf>,
        tables: AlignmentTables,
        contig_index: usize,
        data_complete_bound: (u64, u64),
        reference_sequence: &Sequence,
    ) -> Result<Self, TGVError> {
        assert_eq!(
            records.len(),
            tables.reads.height(),
            "record IDs and table rows have matching shapes"
        );
        let y = stack_tracks(&tables.reads, 3)?;
        let reads = tables
            .reads
            .clone()
            .lazy()
            .with_columns([lit(Series::new(ReadSchema::Y.into(), y)).alias(ReadSchema::Y)])
            .collect()?;
        let mut alignment = Self {
            records,
            tables: AlignmentTables { reads, ..tables },
            coverage: CoverageTable::default(),
            contig_index,
            data_complete_left_bound: data_complete_bound.0,
            data_complete_right_bound: data_complete_bound.1,
        };
        alignment.build_coverage(reference_sequence)?;
        Ok(alignment)
    }

    pub fn build_coverage(&mut self, reference_sequence: &Sequence) -> Result<&mut Self, TGVError> {
        let visible = self
            .tables
            .reads
            .clone()
            .lazy()
            .filter(col(ReadSchema::SHOW))
            .select([col(ReadSchema::READ_ID)])
            .collect()?;
        let selected = visible
            .column(ReadSchema::READ_ID)?
            .u64()?
            .into_no_null_iter()
            .map(|id| id as usize)
            .collect::<Vec<_>>();
        let viewport = self.tables.viewport(1, u64::MAX, &selected)?;
        self.coverage = CoverageTable::from_runs(&viewport, self.contig_index, reference_sequence)?;

        Ok(self)
    }

    pub fn filter(
        &mut self,
        filter: AlignmentFilter,
        reference_sequence: &Sequence,
    ) -> Result<(), TGVError> {
        let selected = match filter {
            AlignmentFilter::Base(position, base) => {
                let events = self
                    .base_events(position)?
                    .lazy()
                    .filter(col(BaseEventSchema::BASE).eq(lit(base.to_string())))
                    .select([col(ReadSchema::READ_ID)])
                    .collect()?;
                events
                    .column(ReadSchema::READ_ID)?
                    .u64()?
                    .into_no_null_iter()
                    .map(|id| id as usize)
                    .collect::<Vec<_>>()
            }
            AlignmentFilter::BaseSoftclip(position) => {
                let ids = (0..self.records.len()).collect::<Vec<_>>();
                let viewport = self.tables.viewport(position, position, &ids)?;
                let clips = viewport
                    .runs
                    .lazy()
                    .filter(col(CigarRunSchema::KIND).eq(lit(
                        noodles::sam::alignment::record::cigar::op::Kind::SoftClip as u8,
                    )))
                    .select([col(CigarRunSchema::READ_ID)])
                    .collect()?;
                clips
                    .column(CigarRunSchema::READ_ID)?
                    .u64()?
                    .into_no_null_iter()
                    .map(|id| id as usize)
                    .collect()
            }
            _ => (0..self.records.len()).collect(),
        };
        let selected = Series::new(
            ReadSchema::READ_ID.into(),
            selected.into_iter().map(|id| id as u64).collect::<Vec<_>>(),
        );
        let reads = self
            .tables
            .reads
            .clone()
            .lazy()
            .with_columns([col(ReadSchema::READ_ID)
                .is_in(lit(selected).implode(true), false)
                .and(col(ReadSchema::STACKING_START).is_not_null())
                .alias(ReadSchema::SHOW)])
            .collect()?;
        let y = stack_tracks(&reads, 3)?;
        self.tables.reads = reads
            .lazy()
            .with_columns([lit(Series::new(ReadSchema::Y.into(), y)).alias(ReadSchema::Y)])
            .collect()?;
        self.build_coverage(reference_sequence)?;

        Ok(())
    }

    pub fn sort(&mut self, option: AlignmentSort) -> Result<(), TGVError> {
        match option {
            AlignmentSort::BaseAt(position) => self.sort_by_base_at(position),
            option => Err(TGVError::ValueError(format!(
                "Alignment sorting is not implemented yet for option {option}"
            ))),
        }
    }

    fn sort_by_base_at(&mut self, position: u64) -> Result<(), TGVError> {
        self.ensure_position_has_complete_data(position)?;

        let events = self.base_events(position)?;
        let items = self
            .tables
            .reads
            .clone()
            .lazy()
            .left_join(
                events
                    .lazy()
                    .select([col(ReadSchema::READ_ID), col(BaseEventSchema::SORT_KEY)]),
                col(ReadSchema::READ_ID),
                col(ReadSchema::READ_ID),
            )
            .sort([ReadSchema::READ_ID], SortMultipleOptions::default())
            .collect()?;
        let y = stack_tracks_by_sort_key(&items, ReadSchema::READ_ID, 3)?;
        self.tables.reads = self
            .tables
            .reads
            .clone()
            .lazy()
            .with_columns([lit(Series::new(ReadSchema::Y.into(), y)).alias(ReadSchema::Y)])
            .collect()?;
        Ok(())
    }

    /// Query aligned bases and event priorities at a one-based position.
    pub(super) fn base_events(&self, position: u64) -> Result<DataFrame, TGVError> {
        use noodles::sam::alignment::record::cigar::op::Kind;
        let kind = col(CigarRunSchema::KIND);
        let aligned = kind
            .clone()
            .eq(lit(Kind::Match as u8))
            .or(kind.clone().eq(lit(Kind::SequenceMatch as u8)))
            .or(kind.clone().eq(lit(Kind::SequenceMismatch as u8)));
        let insertion = kind.clone().eq(lit(Kind::Insertion as u8));
        let deletion = kind
            .clone()
            .eq(lit(Kind::Deletion as u8))
            .or(kind.eq(lit(Kind::Skip as u8)));
        let hit = when(insertion.clone())
            .then(col(CigarRunSchema::REF_START).eq(lit(position)))
            .otherwise(
                col(CigarRunSchema::REF_START).lt_eq(lit(position)).and(
                    (col(CigarRunSchema::REF_START)
                        + col(CigarRunSchema::OP_LEN).cast(DataType::UInt64))
                    .gt(lit(position)),
                ),
            );
        let base = when(aligned.clone())
            .then(col(CigarRunSchema::SEQ).str().slice(
                (lit(position) - col(CigarRunSchema::REF_START)).cast(DataType::Int64),
                lit(1u64),
            ))
            .otherwise(lit(NULL).cast(DataType::String));
        let upper = col(BaseEventSchema::BASE).str().to_uppercase();
        let rank = when(insertion.clone())
            .then(lit(7u8))
            .when(deletion.clone())
            .then(lit(6u8))
            .when(col(BaseEventSchema::BASE).is_null())
            .then(lit(NULL).cast(DataType::UInt8))
            .when(upper.clone().eq(lit("A")))
            .then(lit(0u8))
            .when(upper.clone().eq(lit("T")))
            .then(lit(1u8))
            .when(upper.clone().eq(lit("C")))
            .then(lit(2u8))
            .when(upper.clone().eq(lit("G")))
            .then(lit(3u8))
            .when(upper.eq(lit("N")))
            .then(lit(4u8))
            .otherwise(lit(5u8));
        let events = self
            .tables
            .cigar_runs
            .clone()
            .lazy()
            .filter(aligned.or(insertion).or(deletion).and(hit))
            .with_columns([base.alias(BaseEventSchema::BASE)])
            .select([
                col(BaseEventSchema::READ_ID),
                col(BaseEventSchema::BASE),
                rank.cast(DataType::UInt8).alias(BaseEventSchema::SORT_KEY),
            ])
            .group_by([col(BaseEventSchema::READ_ID)])
            .agg([
                col(BaseEventSchema::BASE).drop_nulls().first(),
                col(BaseEventSchema::SORT_KEY).min(),
            ]);
        Ok(self
            .tables
            .reads
            .clone()
            .lazy()
            .select([col(ReadSchema::READ_ID)])
            .left_join(events, col(ReadSchema::READ_ID), col(ReadSchema::READ_ID))
            .sort([ReadSchema::READ_ID], SortMultipleOptions::default())
            .collect()?)
    }
}

pub(super) fn stack_tracks(items: &DataFrame, min_gap: u64) -> Result<Vec<u64>, TGVError> {
    let starts = items.column(ReadSchema::STACKING_START)?.u64()?;
    let ends = items.column(ReadSchema::STACKING_END)?.u64()?;
    let show = items.column(ReadSchema::SHOW)?.bool()?;
    let mut left = Vec::new();
    let mut right = Vec::new();
    Ok((0..items.height())
        .map(|id| {
            if show.get(id).expect("visibility is non-null") {
                find_track(
                    starts.get(id).expect("visible items have bounds"),
                    ends.get(id).expect("visible items have bounds"),
                    &mut left,
                    &mut right,
                    min_gap,
                ) as u64
            } else {
                0
            }
        })
        .collect())
}

pub(super) fn stack_tracks_by_sort_key(
    items: &DataFrame,
    id_column: &str,
    min_gap: u64,
) -> Result<Vec<u64>, TGVError> {
    let visible = items.clone().lazy().filter(col(ReadSchema::SHOW));
    let sorted = visible
        .clone()
        .filter(col(BaseEventSchema::SORT_KEY).is_not_null())
        .sort(
            [BaseEventSchema::SORT_KEY, id_column],
            SortMultipleOptions::default(),
        )
        .collect()?;
    let mut ys = vec![0; items.height()];
    let mut track_left_bounds = Vec::with_capacity(sorted.height());
    let mut track_right_bounds = Vec::with_capacity(sorted.height());
    let ids = sorted.column(id_column)?.u64()?;
    let starts = sorted.column(ReadSchema::STACKING_START)?.u64()?;
    let ends = sorted.column(ReadSchema::STACKING_END)?.u64()?;
    for row in 0..sorted.height() {
        ys[ids.get(row).expect("item IDs are non-null") as usize] = row as u64;
        track_left_bounds.push(starts.get(row).expect("visible items have bounds"));
        track_right_bounds.push(ends.get(row).expect("visible items have bounds"));
    }
    let remaining = visible
        .filter(col(BaseEventSchema::SORT_KEY).is_null())
        .collect()?;
    let ids = remaining.column(id_column)?.u64()?;
    let starts = remaining.column(ReadSchema::STACKING_START)?.u64()?;
    let ends = remaining.column(ReadSchema::STACKING_END)?.u64()?;
    for row in 0..remaining.height() {
        let id = ids.get(row).expect("item IDs are non-null") as usize;
        ys[id] = find_track(
            starts.get(row).expect("visible items have bounds"),
            ends.get(row).expect("visible items have bounds"),
            &mut track_left_bounds,
            &mut track_right_bounds,
            min_gap,
        ) as u64;
    }
    Ok(ys)
}

pub(super) fn find_track(
    start: u64,
    end: u64,
    track_left_bounds: &mut Vec<u64>,
    track_right_bounds: &mut Vec<u64>,
    min_gap: u64,
) -> usize {
    for (y, left_bound) in track_left_bounds.iter_mut().enumerate() {
        if end + min_gap < *left_bound {
            *left_bound = start;

            return y;
        }
    }

    for (y, right_bound) in track_right_bounds.iter_mut().enumerate() {
        if start > *right_bound + min_gap {
            *right_bound = end;
            return y;
        }
    }

    track_left_bounds.push(start);
    track_right_bounds.push(end);
    track_left_bounds.len() - 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use noodles::sam::{
        self,
        alignment::{
            record::{
                Flags,
                cigar::{Op, op::Kind},
            },
            record_buf::Cigar,
        },
    };

    fn read(
        name: &str,
        start: u64,
        cigar_ops: impl IntoIterator<Item = (Kind, usize)>,
        sequence: &[u8],
    ) -> RecordBuf {
        let cigar: Cigar = cigar_ops
            .into_iter()
            .map(|(kind, len)| Op::new(kind, len))
            .collect();

        let record = sam::alignment::RecordBuf::builder()
            .set_name(name)
            .set_flags(Flags::default())
            .set_alignment_start(noodles::core::Position::try_from(start as usize).unwrap())
            .set_cigar(cigar)
            .set_sequence(sam::alignment::record_buf::Sequence::from(sequence))
            .build();

        record
    }

    fn alignment_with_reads(reads: Vec<RecordBuf>, data_complete_bound: (u64, u64)) -> Alignment {
        Alignment::from_records(
            reads,
            0,
            data_complete_bound,
            &Sequence {
                start: 1,
                sequence: vec![b'A'; 100],
                contig_index: 0,
            },
        )
        .unwrap()
    }

    #[test]
    fn sort_by_base_orders_visible_reads_by_base_event_kind() {
        let mut alignment = alignment_with_reads(
            vec![
                read("g", 12, [(Kind::Match, 1)], b"G"),
                read("t", 12, [(Kind::Match, 1)], b"T"),
                read("ins", 10, [(Kind::Match, 2), (Kind::Insertion, 1)], b"AAI"),
                read("a", 12, [(Kind::Match, 1)], b"A"),
                read("n", 12, [(Kind::Match, 1)], b"N"),
                read("del", 12, [(Kind::Deletion, 1)], b""),
                read("c", 12, [(Kind::Match, 1)], b"C"),
                read("hidden-a", 12, [(Kind::Match, 1)], b"A"),
            ],
            (1, 100),
        );
        alignment.tables.reads = alignment
            .tables
            .reads
            .clone()
            .lazy()
            .with_columns([col(ReadSchema::READ_ID)
                .neq(lit(7u64))
                .alias(ReadSchema::SHOW)])
            .collect()
            .unwrap();

        alignment.sort(AlignmentSort::BaseAt(12)).unwrap();

        assert_eq!(
            alignment
                .tables
                .reads
                .column(ReadSchema::Y)
                .unwrap()
                .u64()
                .unwrap()
                .into_no_null_iter()
                .collect::<Vec<_>>(),
            vec![3, 1, 6, 0, 4, 5, 2, 0]
        );
        for (y, id) in [3, 1, 6, 0, 4, 5, 2].into_iter().enumerate() {
            assert_eq!(alignment.read_overlapping(10, 12, y).unwrap(), Some(id));
        }
    }

    #[test]
    fn sort_by_base_packs_remaining_reads_into_sorted_rows_when_possible() {
        let mut alignment = alignment_with_reads(
            vec![
                read("sorted", 50, [(Kind::Match, 1)], b"A"),
                read("left", 10, [(Kind::Match, 11)], b"AAAAAAAAAAA"),
                read("right", 80, [(Kind::Match, 11)], b"AAAAAAAAAAA"),
                read("too-close-left", 48, [(Kind::Match, 1)], b"A"),
            ],
            (1, 100),
        );

        alignment.sort(AlignmentSort::BaseAt(50)).unwrap();

        assert_eq!(
            alignment
                .tables
                .reads
                .column(ReadSchema::Y)
                .unwrap()
                .u64()
                .unwrap()
                .into_no_null_iter()
                .collect::<Vec<_>>(),
            vec![0, 0, 0, 1]
        );
        assert_eq!(alignment.read_overlapping(10, 20, 0).unwrap(), Some(1));
        assert_eq!(alignment.read_overlapping(80, 90, 0).unwrap(), Some(2));
        assert_eq!(alignment.read_overlapping(48, 48, 1).unwrap(), Some(3));
    }

    #[test]
    fn sort_by_base_returns_dedicated_error_when_position_is_not_loaded() {
        let mut alignment =
            alignment_with_reads(vec![read("a", 12, [(Kind::Match, 1)], b"A")], (10, 20));

        let error = alignment.sort(AlignmentSort::BaseAt(21)).unwrap_err();

        assert!(matches!(
            error,
            TGVError::AlignmentSortPositionNotLoaded {
                position: 21,
                loaded_left: 10,
                loaded_right: 20,
            }
        ));
    }

    #[test]
    fn find_track_returns_zero_based_new_and_reused_tracks() {
        let mut track_left_bounds = Vec::new();
        let mut track_right_bounds = Vec::new();

        assert_eq!(
            find_track(10, 20, &mut track_left_bounds, &mut track_right_bounds, 3),
            0
        );
        assert_eq!(
            find_track(21, 25, &mut track_left_bounds, &mut track_right_bounds, 3),
            1
        );
        assert_eq!(
            find_track(1, 5, &mut track_left_bounds, &mut track_right_bounds, 3),
            0
        );
    }
}

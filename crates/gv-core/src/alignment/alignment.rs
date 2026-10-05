use crate::alignment::{
    coverage::CoverageTable,
    tables::{AlignmentTables, CigarSchema, ReadSchema},
};
use crate::error::TGVError;
use crate::intervals::{GenomeInterval, Region};
use crate::message::{AlignmentFilter, AlignmentSort};
use crate::sequence::Sequence;
use noodles::sam::alignment::RecordBuf;
use polars::prelude::*;

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
        // PERF: this is built base-by-base.
        let runs = self.tables.cigar_runs.clone().lazy().inner_join(
            self.tables
                .reads
                .clone()
                .lazy()
                .filter(col(ReadSchema::SHOW))
                .select([col(ReadSchema::READ_ID)]),
            col(CigarSchema::READ_ID),
            col(ReadSchema::READ_ID),
        );
        self.coverage = CoverageTable::from_runs(runs, self.contig_index, reference_sequence)?;

        Ok(self)
    }

    pub fn filter(
        &mut self,
        filter: AlignmentFilter,
        reference_sequence: &Sequence,
    ) -> Result<(), TGVError> {
        let predicate = match filter {
            AlignmentFilter::Base(position, base) => {
                let kind = col(CigarSchema::KIND);
                Some(
                    kind.clone()
                        .eq(lit(CigarSchema::MATCH))
                        .or(kind.clone().eq(lit(CigarSchema::SEQUENCE_MATCH)))
                        .or(kind.eq(lit(CigarSchema::SEQUENCE_MISMATCH)))
                        .and(col(CigarSchema::DISPLAY_START).lt_eq(lit(position)))
                        .and(col(CigarSchema::DISPLAY_END).gt_eq(lit(position)))
                        .and(
                            col(CigarSchema::SEQ)
                                .str()
                                .slice(
                                    (lit(position as i128)
                                        - col(CigarSchema::REF_START).cast(DataType::Int128))
                                    .cast(DataType::Int64),
                                    lit(1u64),
                                )
                                .eq(lit(base.to_string())),
                        ),
                )
            }
            AlignmentFilter::BaseSoftclip(position) => Some(
                col(CigarSchema::KIND)
                    .eq(lit(CigarSchema::SOFT_CLIP))
                    .and(col(CigarSchema::DISPLAY_START).lt_eq(lit(position)))
                    .and(col(CigarSchema::DISPLAY_END).gt_eq(lit(position))),
            ),
            _ => None,
        };
        let reads = self.tables.reads.clone().lazy();
        let reads = if let Some(predicate) = predicate {
            let selected = self
                .tables
                .cigar_runs
                .clone()
                .lazy()
                .filter(predicate)
                .group_by([col(CigarSchema::READ_ID)])
                .agg([lit(true).alias(ReadSchema::SHOW)]);
            reads
                .drop(cols([ReadSchema::SHOW]))
                .left_join(
                    selected,
                    col(ReadSchema::READ_ID),
                    col(CigarSchema::READ_ID),
                )
                .with_columns([col(ReadSchema::SHOW)
                    .fill_null(lit(false))
                    .and(col(ReadSchema::STACKING_START).is_not_null())
                    .alias(ReadSchema::SHOW)])
        } else {
            reads.with_columns([col(ReadSchema::STACKING_START)
                .is_not_null()
                .alias(ReadSchema::SHOW)])
        };
        let reads = reads
            .sort([ReadSchema::READ_ID], SortMultipleOptions::default())
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

        let kind = col(CigarSchema::KIND);
        let keys = self
            .tables
            .cigar_runs
            .clone()
            .lazy()
            .filter(
                kind.clone()
                    .eq(lit(CigarSchema::MATCH))
                    .or(kind.clone().eq(lit(CigarSchema::SEQUENCE_MATCH)))
                    .or(kind.clone().eq(lit(CigarSchema::SEQUENCE_MISMATCH)))
                    .or(kind.clone().eq(lit(CigarSchema::INSERTION)))
                    .or(kind.clone().eq(lit(CigarSchema::DELETION)))
                    .or(kind.eq(lit(CigarSchema::REFERENCE_SKIP)))
                    .and(col(CigarSchema::DISPLAY_START).lt_eq(lit(position)))
                    .and(col(CigarSchema::DISPLAY_END).gt_eq(lit(position))),
            )
            .group_by([col(CigarSchema::READ_ID)])
            .agg([base_sort_key_at(position).min().alias(ReadSchema::SORT_KEY)]);
        let items = self
            .tables
            .reads
            .clone()
            .lazy()
            .left_join(keys, col(ReadSchema::READ_ID), col(CigarSchema::READ_ID))
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
}

/// Rank CIGAR events in a query without collecting a separate base-event table.
pub(super) fn base_sort_key_at(position: u64) -> Expr {
    let kind = col(CigarSchema::KIND);
    let base = col(CigarSchema::SEQ)
        .str()
        .slice(
            (lit(position as i128) - col(CigarSchema::REF_START).cast(DataType::Int128))
                .cast(DataType::Int64),
            lit(1u64),
        )
        .str()
        .to_uppercase();
    when(kind.clone().eq(lit(CigarSchema::INSERTION)))
        .then(lit(7u8))
        .when(
            kind.clone()
                .eq(lit(CigarSchema::DELETION))
                .or(kind.eq(lit(CigarSchema::REFERENCE_SKIP))),
        )
        .then(lit(6u8))
        .when(base.clone().is_null())
        .then(lit(NULL).cast(DataType::UInt8))
        .when(base.clone().eq(lit("A")))
        .then(lit(0u8))
        .when(base.clone().eq(lit("T")))
        .then(lit(1u8))
        .when(base.clone().eq(lit("C")))
        .then(lit(2u8))
        .when(base.clone().eq(lit("G")))
        .then(lit(3u8))
        .when(base.eq(lit("N")))
        .then(lit(4u8))
        .otherwise(lit(5u8))
}

pub(super) fn stack_tracks(items: &DataFrame, min_gap: u64) -> Result<Vec<u64>, TGVError> {
    let starts = items.column(ReadSchema::STACKING_START)?.u64()?;
    let ends = items.column(ReadSchema::STACKING_END)?.u64()?;
    let show = items.column(ReadSchema::SHOW)?.bool()?;
    let mut tracks = TrackBounds::default();
    Ok((0..items.height())
        .map(|id| {
            if show.get(id).expect("visibility is non-null") {
                tracks.place(
                    starts.get(id).expect("visible items have bounds"),
                    ends.get(id).expect("visible items have bounds"),
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
    let sorted = items
        .clone()
        .lazy()
        .filter(col(ReadSchema::SHOW))
        .sort(
            [ReadSchema::SORT_KEY, id_column],
            SortMultipleOptions::default().with_nulls_last(true),
        )
        .collect()?;
    let ranked = sorted.height() - sorted.column(ReadSchema::SORT_KEY)?.null_count();
    let mut ys = vec![0; items.height()];
    let mut tracks = TrackBounds::with_capacity(ranked);
    let ids = sorted.column(id_column)?.u64()?;
    let starts = sorted.column(ReadSchema::STACKING_START)?.u64()?;
    let ends = sorted.column(ReadSchema::STACKING_END)?.u64()?;
    for row in 0..ranked {
        ys[ids.get(row).expect("item IDs are non-null") as usize] = tracks.push(
            starts.get(row).expect("visible items have bounds"),
            ends.get(row).expect("visible items have bounds"),
        ) as u64;
    }
    for row in ranked..sorted.height() {
        let id = ids.get(row).expect("item IDs are non-null") as usize;
        ys[id] = tracks.place(
            starts.get(row).expect("visible items have bounds"),
            ends.get(row).expect("visible items have bounds"),
            min_gap,
        ) as u64;
    }
    Ok(ys)
}

/// One-based, inclusive occupied bounds of each zero-based stacking row.
#[derive(Debug, Default)]
pub(super) struct TrackBounds {
    left: Vec<u64>,
    right: Vec<u64>,

    /// The largest left bound across rows.
    ///
    /// Coordinate-sorted items never end before an existing row starts, so this lets them
    /// skip the left pass instead of scanning every row twice.
    max_left: u64,
}

impl TrackBounds {
    pub(super) fn with_capacity(capacity: usize) -> Self {
        Self {
            left: Vec::with_capacity(capacity),
            right: Vec::with_capacity(capacity),
            max_left: 0,
        }
    }

    /// Open a new row holding [start, end], and return its index.
    pub(super) fn push(&mut self, start: u64, end: u64) -> usize {
        self.left.push(start);
        self.right.push(end);
        self.max_left = self.max_left.max(start);
        self.left.len() - 1
    }

    /// Place [start, end] in the first row it fits before, then in the first row it fits
    /// after, or else in a new row. Returns the row index.
    pub(super) fn place(&mut self, start: u64, end: u64, min_gap: u64) -> usize {
        if end + min_gap < self.max_left
            && let Some(y) = self.left.iter().position(|&left| end + min_gap < left)
        {
            let previous = std::mem::replace(&mut self.left[y], start);
            if previous == self.max_left {
                self.max_left = self.left.iter().copied().max().unwrap_or(0);
            }
            return y;
        }

        if let Some(y) = self
            .right
            .iter()
            .position(|&right| start > right + min_gap)
        {
            self.right[y] = end;
            return y;
        }

        self.push(start, end)
    }
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
    fn track_bounds_place_returns_zero_based_new_and_reused_tracks() {
        let mut tracks = TrackBounds::default();

        assert_eq!(tracks.place(10, 20, 3), 0);
        assert_eq!(tracks.place(21, 25, 3), 1);
        assert_eq!(tracks.place(1, 5, 3), 0);
    }
}

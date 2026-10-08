use crate::{
    alignment::{
        alignment::{Alignment, base_sort_key_at, stack_tracks_by_sort_key},
        tables::{CigarSchema, ReadSchema},
    },
    error::TGVError,
    message::AlignmentSort,
    table_schema::TableSchema,
};
use polars::prelude::*;
use std::sync::Arc;

/// State and utilities for paired alignment display.
#[derive(Debug)]
pub struct PairedAlignment {
    /// Stable pair IDs and their member read IDs.
    pub pairs: DataFrame,

    /// Unpaired, unnamed, secondary, and supplementary records.
    pub singles: DataFrame,
}

/// Grouped mate IDs, display bounds, visibility, and stacking rows.
/// The second read ID and display bounds may be null.
pub struct PairSchema;

impl PairSchema {
    pub const PAIR_ID: &'static str = "pair_id";
    pub const READ_1_ID: &'static str = "read_1_id";
    pub const READ_2_ID: &'static str = "read_2_id";
    pub const STACKING_START: &'static str = ReadSchema::STACKING_START;
    pub const STACKING_END: &'static str = ReadSchema::STACKING_END;
    pub const SHOW: &'static str = ReadSchema::SHOW;
    pub const Y: &'static str = ReadSchema::Y;
    pub const SORT_KEY: &'static str = ReadSchema::SORT_KEY;

    // Temporary columns used while deriving query results.
    pub const ITEM_ID: &'static str = "item_id";
    pub const FIRST_ID: &'static str = "first_id";
    pub const FIRST_SHOW: &'static str = "first_show";
    pub const FIRST_KEY: &'static str = "first_key";
    pub const FIRST_START: &'static str = "first_start";
    pub const FIRST_END: &'static str = "first_end";
    pub const SECOND_ID: &'static str = "second_id";
    pub const SECOND_SHOW: &'static str = "second_show";
    pub const SECOND_KEY: &'static str = "second_key";
    pub const SECOND_START: &'static str = "second_start";
    pub const SECOND_END: &'static str = "second_end";
}

impl TableSchema for PairSchema {
    fn schema() -> SchemaRef {
        let mut schema = Schema::with_capacity(7);
        schema.insert(Self::PAIR_ID.into(), DataType::UInt64);
        schema.insert(Self::READ_1_ID.into(), DataType::UInt64);
        schema.insert(Self::READ_2_ID.into(), DataType::UInt64);
        schema.insert(Self::STACKING_START.into(), DataType::UInt64);
        schema.insert(Self::STACKING_END.into(), DataType::UInt64);
        schema.insert(Self::SHOW.into(), DataType::Boolean);
        schema.insert(Self::Y.into(), DataType::UInt64);
        Arc::new(schema)
    }
}

impl PairedAlignment {
    pub fn new(alignment: &Alignment) -> Result<Self, TGVError> {
        let reads = alignment.tables.reads.clone().lazy();
        let eligible = col(ReadSchema::PAIRED)
            .and(col(ReadSchema::SECONDARY).not())
            .and(col(ReadSchema::SUPPLEMENTARY).not())
            .and(col(ReadSchema::QNAME).is_not_null());
        let pairs = reads
            .clone()
            .filter(eligible.clone())
            .group_by([ReadSchema::QNAME])
            .agg([
                col(ReadSchema::READ_ID).min().alias(PairSchema::READ_1_ID),
                // Only the first and last records represent a qname with more than two records.
                when(len().gt(lit(1u32)))
                    .then(col(ReadSchema::READ_ID).max())
                    .otherwise(lit(NULL).cast(DataType::UInt64))
                    .alias(PairSchema::READ_2_ID),
                col(ReadSchema::STACKING_START).min(),
                col(ReadSchema::STACKING_END).max(),
                col(ReadSchema::SHOW).any(false).alias(PairSchema::SHOW),
            ])
            .select([
                col(PairSchema::READ_1_ID),
                col(PairSchema::READ_2_ID),
                col(PairSchema::STACKING_START),
                col(PairSchema::STACKING_END),
                col(PairSchema::SHOW),
            ])
            .sort([PairSchema::READ_1_ID], SortMultipleOptions::default())
            .with_row_index(PairSchema::PAIR_ID, None)
            .with_columns([
                col(PairSchema::PAIR_ID).cast(DataType::UInt64),
                lit(NULL).cast(DataType::UInt8).alias(PairSchema::SORT_KEY),
            ])
            .collect()?;
        let singles = reads
            .filter(eligible.not())
            .sort([ReadSchema::READ_ID], SortMultipleOptions::default())
            .with_columns([lit(NULL).cast(DataType::UInt8).alias(ReadSchema::SORT_KEY)])
            .collect()?;
        let mut result = Self { pairs, singles };
        result.assign_rows()?;
        Ok(result)
    }

    /// Return the combined depth of the visible pairs and singles.
    pub fn depth(&self) -> Result<usize, TGVError> {
        if !self.pairs.column(PairSchema::SHOW)?.bool()?.any()
            && !self.singles.column(ReadSchema::SHOW)?.bool()?.any()
        {
            return Ok(0);
        }
        let pair_y = self.pairs.column(PairSchema::Y)?.u64()?.max();
        let single_y = self.singles.column(ReadSchema::Y)?.u64()?.max();
        Ok(pair_y.max(single_y).map_or(0, |y| y as usize + 1))
    }

    fn assign_rows(&mut self) -> Result<(), TGVError> {
        let items = concat(
            [
                self.pairs.clone().lazy().select([
                    col(PairSchema::SHOW),
                    col(PairSchema::STACKING_START),
                    col(PairSchema::STACKING_END),
                    col(PairSchema::SORT_KEY),
                ]),
                self.singles.clone().lazy().select([
                    col(ReadSchema::SHOW),
                    col(ReadSchema::STACKING_START),
                    col(ReadSchema::STACKING_END),
                    col(ReadSchema::SORT_KEY),
                ]),
            ],
            UnionArgs::default(),
        )?
        .with_row_index(PairSchema::ITEM_ID, None)
        .with_columns([col(PairSchema::ITEM_ID).cast(DataType::UInt64)])
        .collect()?;
        let y = stack_tracks_by_sort_key(&items, PairSchema::ITEM_ID, 10)?;
        let (pair_y, single_y) = y.split_at(self.pairs.height());
        let pairs = self
            .pairs
            .clone()
            .lazy()
            .with_columns([lit(Series::new(PairSchema::Y.into(), pair_y)).alias(PairSchema::Y)])
            .drop(cols([PairSchema::SORT_KEY]))
            .collect()?;
        let singles = self
            .singles
            .clone()
            .lazy()
            .with_columns([lit(Series::new(ReadSchema::Y.into(), single_y)).alias(ReadSchema::Y)])
            .drop(cols([ReadSchema::SORT_KEY]))
            .collect()?;
        self.pairs = pairs;
        self.singles = singles;
        Ok(())
    }

    /// Find a read in a visible pair or singleton at the displayed row. Both mates of a visible
    /// pair are displayed, even when only one passes the display options.
    pub fn read_overlapping(
        &self,
        alignment: &Alignment,
        left: u64,
        right: u64,
        y: usize,
    ) -> Result<Option<usize>, TGVError> {
        if left > right {
            return Ok(None);
        }
        let hit = col(PairSchema::SHOW)
            .and(col(PairSchema::Y).eq(lit(y as u64)))
            .and(col(PairSchema::STACKING_START).lt_eq(lit(right)))
            .and(col(PairSchema::STACKING_END).gt_eq(lit(left)));
        let pairs = self
            .pairs
            .clone()
            .lazy()
            .filter(hit.clone())
            .select([col(PairSchema::READ_1_ID), col(PairSchema::READ_2_ID)])
            .collect()?;
        let first = pairs.column(PairSchema::READ_1_ID)?.u64()?;
        let second = pairs.column(PairSchema::READ_2_ID)?.u64()?;
        let mut candidates = first
            .into_no_null_iter()
            .zip(second.iter())
            .flat_map(|(first, second)| std::iter::once(first).chain(second))
            .collect::<Vec<_>>();
        let singles = self
            .singles
            .clone()
            .lazy()
            .filter(hit)
            .select([col(ReadSchema::READ_ID)])
            .collect()?;
        candidates.extend(
            singles
                .column(ReadSchema::READ_ID)?
                .u64()?
                .into_no_null_iter(),
        );
        let selected = Series::new(ReadSchema::READ_ID.into(), candidates);
        let hits = alignment
            .tables
            .reads
            .clone()
            .lazy()
            .filter(
                col(ReadSchema::READ_ID)
                    .is_in(lit(selected).implode(true), false)
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

    pub fn sort(&mut self, alignment: &Alignment, option: AlignmentSort) -> Result<(), TGVError> {
        match option {
            AlignmentSort::BaseAt(position) => self.sort_by_base_at(alignment, position),
            option => Err(TGVError::ValueError(format!(
                "Paired alignment sorting is not implemented yet for option {option}"
            ))),
        }
    }

    fn sort_by_base_at(&mut self, alignment: &Alignment, position: u64) -> Result<(), TGVError> {
        alignment.ensure_position_has_complete_data(position)?;

        let kind = col(CigarSchema::KIND);
        let keys = alignment
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
        let reads = alignment.tables.reads.clone().lazy().left_join(
            keys,
            col(ReadSchema::READ_ID),
            col(CigarSchema::READ_ID),
        );
        let mate = |[id, show, key, start, end]: [&str; 5]| {
            reads.clone().select([
                col(ReadSchema::READ_ID).alias(id),
                col(ReadSchema::SHOW).alias(show),
                when(col(ReadSchema::SHOW))
                    .then(col(ReadSchema::SORT_KEY))
                    .otherwise(lit(NULL).cast(DataType::UInt8))
                    .alias(key),
                col(ReadSchema::STACKING_START).alias(start),
                col(ReadSchema::STACKING_END).alias(end),
            ])
        };
        let pos = lit(position);
        let gap = col(PairSchema::FIRST_END)
            .lt(col(PairSchema::SECOND_START))
            .and(pos.clone().gt(col(PairSchema::FIRST_END)))
            .and(pos.clone().lt(col(PairSchema::SECOND_START)))
            .or(col(PairSchema::SECOND_END)
                .lt(col(PairSchema::FIRST_START))
                .and(pos.clone().gt(col(PairSchema::SECOND_END)))
                .and(pos.lt(col(PairSchema::FIRST_START))));
        let items = self
            .pairs
            .clone()
            .lazy()
            .left_join(
                mate([
                    PairSchema::FIRST_ID,
                    PairSchema::FIRST_SHOW,
                    PairSchema::FIRST_KEY,
                    PairSchema::FIRST_START,
                    PairSchema::FIRST_END,
                ]),
                col(PairSchema::READ_1_ID),
                col(PairSchema::FIRST_ID),
            )
            .left_join(
                mate([
                    PairSchema::SECOND_ID,
                    PairSchema::SECOND_SHOW,
                    PairSchema::SECOND_KEY,
                    PairSchema::SECOND_START,
                    PairSchema::SECOND_END,
                ]),
                col(PairSchema::READ_2_ID),
                col(PairSchema::SECOND_ID),
            )
            .with_columns([
                col(PairSchema::FIRST_SHOW)
                    .or(col(PairSchema::SECOND_SHOW).fill_null(lit(false)))
                    .alias(PairSchema::SHOW),
                coalesce(&[
                    col(PairSchema::FIRST_KEY),
                    col(PairSchema::SECOND_KEY),
                    when(gap)
                        .then(lit(8u8))
                        .otherwise(lit(NULL).cast(DataType::UInt8)),
                ])
                .alias(PairSchema::SORT_KEY),
            ])
            .sort([PairSchema::PAIR_ID], SortMultipleOptions::default());
        let pairs = items
            .select([
                col(PairSchema::PAIR_ID),
                col(PairSchema::READ_1_ID),
                col(PairSchema::READ_2_ID),
                col(PairSchema::STACKING_START),
                col(PairSchema::STACKING_END),
                col(PairSchema::SHOW),
                col(PairSchema::SORT_KEY),
            ])
            .collect()?;
        let singles = self
            .singles
            .clone()
            .lazy()
            .select([col(ReadSchema::READ_ID)])
            .left_join(reads, col(ReadSchema::READ_ID), col(ReadSchema::READ_ID))
            .sort([ReadSchema::READ_ID], SortMultipleOptions::default())
            .collect()?;
        let mut replacement = Self { pairs, singles };
        replacement.assign_rows()?;
        *self = replacement;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sequence::Sequence;
    use noodles::sam::alignment::RecordBuf;
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
            .set_flags(Flags::from(0x1))
            .set_alignment_start(noodles::core::Position::try_from(start as usize).unwrap())
            .set_cigar(cigar)
            .set_sequence(sam::alignment::record_buf::Sequence::from(sequence))
            .build();

        record
    }

    fn alignment_from_reads(reads: Vec<RecordBuf>) -> Alignment {
        Alignment::from_records(
            reads,
            0,
            (1, 100),
            &Sequence {
                start: 1,
                sequence: vec![b'A'; 100],
                contig_index: 0,
            },
        )
        .unwrap()
    }

    #[test]
    fn paired_sort_uses_read_1_then_read_2_then_pair_gap() {
        let alignment = alignment_from_reads(vec![
            read("read-1-wins", 10, [(Kind::Match, 1)], b"T"),
            read("read-1-wins", 10, [(Kind::Match, 1)], b"A"),
            read("sorts-first", 10, [(Kind::Match, 1)], b"A"),
            read("sorts-first", 10, [(Kind::Match, 1)], b"T"),
            read("read-2-fallback", 20, [(Kind::Match, 1)], b"G"),
            read("read-2-fallback", 10, [(Kind::Match, 1)], b"C"),
            read("pair-gap", 5, [(Kind::Match, 1)], b"G"),
            read("pair-gap", 15, [(Kind::Match, 1)], b"G"),
        ]);
        let mut paired_alignment = PairedAlignment::new(&alignment).unwrap();

        paired_alignment
            .sort(&alignment, AlignmentSort::BaseAt(10))
            .unwrap();

        assert_eq!(
            paired_alignment
                .pairs
                .column(PairSchema::Y)
                .unwrap()
                .u64()
                .unwrap()
                .into_no_null_iter()
                .collect::<Vec<_>>(),
            vec![1, 0, 2, 3]
        );
        assert_eq!(paired_alignment.depth().unwrap(), 4);
        assert_eq!(
            paired_alignment
                .read_overlapping(&alignment, 10, 10, 0)
                .unwrap(),
            Some(2)
        );
    }

    #[test]
    fn paired_sort_visibility_follows_underlying_reads() {
        let mut singleton = read("singleton", 10, [(Kind::Match, 1)], b"G");
        *singleton.flags_mut() = Flags::default();
        let mut secondary = read("visible", 30, [(Kind::Match, 1)], b"A");
        *secondary.flags_mut() = Flags::from(0x101);
        let mut alignment = alignment_from_reads(vec![
            read("hidden", 10, [(Kind::Match, 1)], b"A"),
            read("hidden", 10, [(Kind::Match, 1)], b"A"),
            read("visible", 10, [(Kind::Match, 1)], b"T"),
            read("visible", 10, [(Kind::Match, 1)], b"T"),
            singleton,
            secondary,
        ]);
        alignment.tables.reads = alignment
            .tables
            .reads
            .clone()
            .lazy()
            .with_columns([col(ReadSchema::READ_ID)
                .gt_eq(lit(2u64))
                .alias(ReadSchema::SHOW)])
            .collect()
            .unwrap();

        let mut paired_alignment = PairedAlignment::new(&alignment).unwrap();
        paired_alignment
            .sort(&alignment, AlignmentSort::BaseAt(10))
            .unwrap();

        assert_eq!(
            paired_alignment
                .pairs
                .column(PairSchema::SHOW)
                .unwrap()
                .bool()
                .unwrap()
                .iter()
                .collect::<Vec<_>>(),
            vec![Some(false), Some(true)]
        );
        assert_eq!(paired_alignment.depth().unwrap(), 2);
        assert_eq!(
            paired_alignment
                .singles
                .column(ReadSchema::READ_ID)
                .unwrap()
                .u64()
                .unwrap()
                .into_no_null_iter()
                .collect::<Vec<_>>(),
            vec![4, 5]
        );
        assert_eq!(
            paired_alignment
                .singles
                .column(ReadSchema::Y)
                .unwrap()
                .u64()
                .unwrap()
                .into_no_null_iter()
                .collect::<Vec<_>>(),
            vec![1, 0]
        );
        assert_eq!(
            paired_alignment
                .read_overlapping(&alignment, 10, 10, 1)
                .unwrap(),
            Some(4)
        );
        assert_eq!(
            paired_alignment
                .read_overlapping(&alignment, 30, 30, 0)
                .unwrap(),
            Some(5)
        );
        assert_eq!(
            paired_alignment
                .read_overlapping(&alignment, 10, 10, 0)
                .unwrap(),
            Some(2)
        );
    }
}

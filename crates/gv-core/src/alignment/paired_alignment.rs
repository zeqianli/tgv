use crate::{
    alignment::alignment::{Alignment, stack_tracks_by_sort_key},
    error::TGVError,
    message::AlignmentSort,
};
use polars::prelude::*;

/// State and utilities for paired alignment display.
#[derive(Debug)]
pub struct PairedAlignment {
    /// Stable pair IDs and their member read IDs.
    pub pairs: DataFrame,

    /// Unpaired, unnamed, secondary, and supplementary records.
    pub singles: DataFrame,
}

impl PairedAlignment {
    pub fn new(alignment: &Alignment) -> Result<Self, TGVError> {
        let reads = alignment.tables.reads.clone().lazy();
        let eligible = col("paired")
            .and(col("secondary").not())
            .and(col("supplementary").not())
            .and(col("qname").is_not_null());
        let pairs = reads
            .clone()
            .filter(eligible.clone())
            .group_by(["qname"])
            .agg([
                col("read_id").min().alias("read_1_id"),
                // Only the first and last records represent a qname with more than two records.
                when(len().gt(lit(1u32)))
                    .then(col("read_id").max())
                    .otherwise(lit(NULL).cast(DataType::UInt64))
                    .alias("read_2_id"),
                col("stacking_start").min(),
                col("stacking_end").max(),
                col("show").any(false).alias("show"),
            ])
            .select([
                col("read_1_id"),
                col("read_2_id"),
                col("stacking_start"),
                col("stacking_end"),
                col("show"),
            ])
            .sort(["read_1_id"], SortMultipleOptions::default())
            .with_row_index("pair_id", None)
            .with_columns([
                col("pair_id").cast(DataType::UInt64),
                lit(NULL).cast(DataType::UInt8).alias("sort_key"),
            ])
            .collect()?;
        let singles = reads
            .filter(eligible.not())
            .sort(["read_id"], SortMultipleOptions::default())
            .with_columns([lit(NULL).cast(DataType::UInt8).alias("sort_key")])
            .collect()?;
        let mut result = Self { pairs, singles };
        result.assign_rows()?;
        Ok(result)
    }

    /// Return the combined depth of the visible pairs and singles.
    pub fn depth(&self) -> Result<usize, TGVError> {
        if !self.pairs.column("show")?.bool()?.any() && !self.singles.column("show")?.bool()?.any()
        {
            return Ok(0);
        }
        let pair_y = self.pairs.column("y")?.u64()?.max();
        let single_y = self.singles.column("y")?.u64()?.max();
        Ok(pair_y.max(single_y).map_or(0, |y| y as usize + 1))
    }

    fn assign_rows(&mut self) -> Result<(), TGVError> {
        let items = concat(
            [
                self.pairs.clone().lazy().select([
                    col("show"),
                    col("stacking_start"),
                    col("stacking_end"),
                    col("sort_key"),
                ]),
                self.singles.clone().lazy().select([
                    col("show"),
                    col("stacking_start"),
                    col("stacking_end"),
                    col("sort_key"),
                ]),
            ],
            UnionArgs::default(),
        )?
        .with_row_index("item_id", None)
        .with_columns([col("item_id").cast(DataType::UInt64)])
        .collect()?;
        let y = stack_tracks_by_sort_key(&items, "item_id", 10)?;
        let (pair_y, single_y) = y.split_at(self.pairs.height());
        let pairs = self
            .pairs
            .clone()
            .lazy()
            .with_columns([lit(Series::new("y".into(), pair_y)).alias("y")])
            .drop(cols(["sort_key"]))
            .collect()?;
        let singles = self
            .singles
            .clone()
            .lazy()
            .with_columns([lit(Series::new("y".into(), single_y)).alias("y")])
            .drop(cols(["sort_key"]))
            .collect()?;
        self.pairs = pairs;
        self.singles = singles;
        Ok(())
    }

    pub fn pair_count(&self) -> usize {
        self.pairs.height()
    }

    /// The stable read IDs belonging to a pair.
    pub fn members(&self, pair_id: usize) -> (usize, Option<usize>) {
        let first = self
            .pairs
            .column("read_1_id")
            .unwrap()
            .u64()
            .unwrap()
            .get(pair_id)
            .unwrap() as usize;
        let second = self
            .pairs
            .column("read_2_id")
            .unwrap()
            .u64()
            .unwrap()
            .get(pair_id)
            .map(|id| id as usize);
        (first, second)
    }

    /// Iterate over indexed pairs whose display spans overlap an inclusive interval.
    /// The span includes soft clips and the gap between mates.
    pub fn overlapping_pairs<'a>(
        &'a self,
        alignment: &'a Alignment,
        contig_index: usize,
        start: u64,
        end: u64,
    ) -> Result<Vec<usize>, TGVError> {
        if alignment.contig_index != contig_index || start > end {
            return Ok(Vec::new());
        }
        let hits = self
            .pairs
            .clone()
            .lazy()
            .filter(
                col("stacking_start")
                    .lt_eq(lit(end))
                    .and(col("stacking_end").gt_eq(lit(start))),
            )
            .select([col("pair_id")])
            .collect()?;
        Ok(hits
            .column("pair_id")?
            .u64()?
            .into_no_null_iter()
            .map(|id| id as usize)
            .collect())
    }

    /// Find a read in a visible pair or singleton at the displayed row.
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
        let hit = col("show")
            .and(col("y").eq(lit(y as u64)))
            .and(col("stacking_start").lt_eq(lit(right)))
            .and(col("stacking_end").gt_eq(lit(left)));
        let pairs = self
            .pairs
            .clone()
            .lazy()
            .filter(hit.clone())
            .select([col("read_1_id"), col("read_2_id")])
            .collect()?;
        let first = pairs.column("read_1_id")?.u64()?;
        let second = pairs.column("read_2_id")?.u64()?;
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
            .select([col("read_id")])
            .collect()?;
        candidates.extend(singles.column("read_id")?.u64()?.into_no_null_iter());
        let selected = Series::new("selected".into(), candidates);
        let hits = alignment
            .tables
            .reads
            .clone()
            .lazy()
            .filter(
                col("show")
                    .and(col("read_id").is_in(lit(selected).implode(true), false))
                    .and(col("stacking_start").lt_eq(lit(right)))
                    .and(col("stacking_end").gt_eq(lit(left))),
            )
            .select([col("read_id")])
            .sort(["read_id"], SortMultipleOptions::default())
            .limit(1)
            .collect()?;
        Ok(hits
            .column("read_id")?
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

        let events = alignment.base_events(position)?;
        let reads = alignment.tables.reads.clone().lazy().left_join(
            events.lazy(),
            col("read_id"),
            col("read_id"),
        );
        let mate = |prefix: &str| {
            reads.clone().select([
                col("read_id").alias(format!("{prefix}_id")),
                col("show").alias(format!("{prefix}_show")),
                when(col("show"))
                    .then(col("sort_key"))
                    .otherwise(lit(NULL).cast(DataType::UInt8))
                    .alias(format!("{prefix}_key")),
                col("stacking_start").alias(format!("{prefix}_start")),
                col("stacking_end").alias(format!("{prefix}_end")),
            ])
        };
        let pos = lit(position);
        let gap = col("first_end")
            .lt(col("second_start"))
            .and(pos.clone().gt(col("first_end")))
            .and(pos.clone().lt(col("second_start")))
            .or(col("second_end")
                .lt(col("first_start"))
                .and(pos.clone().gt(col("second_end")))
                .and(pos.lt(col("first_start"))));
        let items = self
            .pairs
            .clone()
            .lazy()
            .left_join(mate("first"), col("read_1_id"), col("first_id"))
            .left_join(mate("second"), col("read_2_id"), col("second_id"))
            .with_columns([
                col("first_show")
                    .or(col("second_show").fill_null(lit(false)))
                    .alias("show"),
                coalesce(&[
                    col("first_key"),
                    col("second_key"),
                    when(gap)
                        .then(lit(8u8))
                        .otherwise(lit(NULL).cast(DataType::UInt8)),
                ])
                .alias("sort_key"),
            ])
            .sort(["pair_id"], SortMultipleOptions::default())
            .collect()?;
        let pairs = items
            .lazy()
            .select([
                col("pair_id"),
                col("read_1_id"),
                col("read_2_id"),
                col("stacking_start"),
                col("stacking_end"),
                col("show"),
                col("sort_key"),
            ])
            .collect()?;
        let singles = self
            .singles
            .clone()
            .lazy()
            .select([col("read_id")])
            .left_join(reads, col("read_id"), col("read_id"))
            .drop(cols(["base"]))
            .sort(["read_id"], SortMultipleOptions::default())
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
                .column("y")
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
            .with_columns([col("read_id").gt_eq(lit(2u64)).alias("show")])
            .collect()
            .unwrap();

        let mut paired_alignment = PairedAlignment::new(&alignment).unwrap();
        paired_alignment
            .sort(&alignment, AlignmentSort::BaseAt(10))
            .unwrap();

        assert_eq!(
            paired_alignment
                .pairs
                .column("show")
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
                .column("read_id")
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
                .column("y")
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

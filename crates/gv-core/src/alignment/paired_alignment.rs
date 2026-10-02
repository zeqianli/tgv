use crate::{
    alignment::alignment::{Alignment, find_track, stack_tracks_by_sort_key},
    error::TGVError,
    message::AlignmentSort,
};
use polars::prelude::*;

/// State and utilities for paired alignment display.
#[derive(Debug)]
pub struct PairedAlignment {
    /// Stable pair IDs and their member read IDs.
    pub pairs: DataFrame,

    /// Pair index to y locations.
    pub ys: Vec<usize>,

    /// y to pair indexes at y location.
    pub ys_index: Vec<Vec<usize>>,

    /// Whether to display the pair.
    pub show_pair: Vec<bool>,
}

impl PairedAlignment {
    pub fn new(alignment: &Alignment) -> Result<Self, TGVError> {
        let reads = &alignment.tables.reads;
        let eligible = reads.column("paired")?.bool()?.clone()
            & !reads.column("secondary")?.bool()?.clone()
            & !reads.column("supplementary")?.bool()?.clone()
            & reads.column("qname")?.is_not_null();
        let candidates = reads.filter(&eligible)?;
        let grouped = candidates.group_by_stable(["qname"])?.groups()?;
        let groups = grouped.column("groups")?.list()?;
        let candidate_ids = candidates.column("read_id")?.u64()?;
        let mut read_1_id = Vec::new();
        let mut read_2_id = Vec::new();
        for row in 0..groups.len() {
            let group = groups
                .get_as_series(row)
                .expect("qname groups are non-null");
            let ids = group
                .idx()?
                .into_no_null_iter()
                .map(|row| {
                    candidate_ids
                        .get(row as usize)
                        .expect("read IDs are non-null")
                })
                .collect::<Vec<_>>();
            // Duplicate primary records with one qname remain consecutive pairs.
            for members in ids.chunks(2) {
                read_1_id.push(members[0]);
                read_2_id.push(members.get(1).copied());
            }
        }
        for id in reads
            .column("read_id")?
            .filter(&!eligible)?
            .u64()?
            .into_no_null_iter()
        {
            read_1_id.push(id);
            read_2_id.push(None);
        }
        let mut pairs = DataFrame::new(
            read_1_id.len(),
            vec![
                Column::new("read_1_id".into(), read_1_id),
                Column::new("read_2_id".into(), read_2_id),
            ],
        )?
        .sort(["read_1_id"], SortMultipleOptions::default())?;
        let first = pairs.column("read_1_id")?.u64()?;
        let second = pairs.column("read_2_id")?.u64()?;
        let bounds = first
            .into_no_null_iter()
            .zip(second.iter())
            .map(|(first, second)| {
                pair_bounds((first as usize, second.map(|id| id as usize)), alignment)
            })
            .collect::<Vec<_>>();
        let show_pair = first
            .into_no_null_iter()
            .zip(second.iter())
            .map(|(first, second)| {
                alignment.show_read[first as usize]
                    || second.is_some_and(|id| alignment.show_read[id as usize])
            })
            .collect::<Vec<_>>();
        pairs.with_column(Column::new(
            "pair_id".into(),
            (0..pairs.height() as u64).collect::<Vec<_>>(),
        ))?;
        pairs.with_column(Column::new(
            "stacking_start".into(),
            bounds.iter().map(|b| b.map(|b| b.0)).collect::<Vec<_>>(),
        ))?;
        pairs.with_column(Column::new(
            "stacking_end".into(),
            bounds.iter().map(|b| b.map(|b| b.1)).collect::<Vec<_>>(),
        ))?;
        let mut left = Vec::new();
        let mut right = Vec::new();
        let ys = bounds
            .iter()
            .zip(&show_pair)
            .map(|(bounds, show)| {
                if *show {
                    let (start, end) = bounds.expect("visible pairs have bounds");
                    find_track(start, end, &mut left, &mut right, 10)
                } else {
                    0
                }
            })
            .collect();
        let mut paired_alignment = Self {
            pairs,
            show_pair,
            ys,
            ys_index: Vec::new(),
        };

        paired_alignment.build_y_index()?;

        Ok(paired_alignment)
    }

    /// Return the number of paired alignment tracks.
    pub fn depth(&self) -> usize {
        self.ys_index.len()
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
        let left = self.pairs.column("stacking_start")?.u64()?;
        let right = self.pairs.column("stacking_end")?.u64()?;
        let mask = left.lt_eq(end) & right.gt_eq(start);
        let hits = self.pairs.column("pair_id")?.filter(&mask)?;
        Ok(hits
            .u64()?
            .into_no_null_iter()
            .map(|id| id as usize)
            .collect())
    }

    pub fn pair_overlapping(
        &self,
        alignment: &Alignment,
        left: u64,
        right: u64,
        y: usize,
    ) -> Result<Option<usize>, TGVError> {
        if y >= self.depth() {
            return Ok(None);
        }
        let hits = self.overlapping_pairs(alignment, alignment.contig_index, left, right)?;
        Ok(self.ys_index[y]
            .iter()
            .copied()
            .find(|id| hits.contains(id)))
    }

    fn build_y_index(&mut self) -> Result<(), TGVError> {
        let mut ys_index = vec![Vec::new(); *self.ys.iter().max().unwrap_or(&0) + 1];
        for (pair_index, (y, show_pair)) in self.ys.iter().zip(self.show_pair.iter()).enumerate() {
            if *show_pair {
                ys_index[*y].push(pair_index);
            }
        }
        self.ys_index = ys_index;

        Ok(())
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
        let read_keys = events.column("sort_key")?.u8()?;
        let mut keys = Vec::with_capacity(self.pair_count());
        for id in 0..self.pair_count() {
            let (first, second) = self.members(id);
            self.show_pair[id] =
                alignment.show_read[first] || second.is_some_and(|id| alignment.show_read[id]);
            let key = alignment.show_read[first]
                .then(|| read_keys.get(first))
                .flatten()
                .or_else(|| {
                    second
                        .filter(|id| alignment.show_read[*id])
                        .and_then(|id| read_keys.get(id))
                })
                .or_else(|| {
                    second
                        .filter(|id| {
                            pair_gap_at(
                                alignment.stacking_bounds(first),
                                alignment.stacking_bounds(*id),
                                position,
                            )
                        })
                        .map(|_| 8)
                });
            keys.push(key);
        }
        let mut items = self.pairs.clone();
        items.with_column(Column::new("sort_key".into(), keys))?;
        self.ys = stack_tracks_by_sort_key(&items, "pair_id", &self.show_pair, 10)?;
        self.build_y_index()
    }
}

fn pair_gap_at(read_1: Option<(u64, u64)>, read_2: Option<(u64, u64)>, position: u64) -> bool {
    let (Some(first), Some(second)) = (read_1, read_2) else {
        return false;
    };
    if position == 0 {
        return false;
    }
    (first.1 < second.0 && position > first.1 && position < second.0)
        || (second.1 < first.0 && position > second.1 && position < first.0)
}

fn pair_bounds(pair: (usize, Option<usize>), alignment: &Alignment) -> Option<(u64, u64)> {
    let first = alignment.stacking_bounds(pair.0);
    let second = pair.1.and_then(|id| alignment.stacking_bounds(id));
    match (first, second) {
        (Some(a), Some(b)) => Some((a.0.min(b.0), a.1.max(b.1))),
        (Some(a), None) | (None, Some(a)) => Some(a),
        (None, None) => None,
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

        assert_eq!(paired_alignment.ys, vec![1, 0, 2, 3]);
        assert_eq!(
            paired_alignment.ys_index,
            vec![vec![1], vec![0], vec![2], vec![3]]
        );
    }

    #[test]
    fn paired_sort_visibility_follows_underlying_reads() {
        let mut alignment = alignment_from_reads(vec![
            read("hidden", 10, [(Kind::Match, 1)], b"A"),
            read("hidden", 10, [(Kind::Match, 1)], b"A"),
            read("visible", 10, [(Kind::Match, 1)], b"T"),
            read("visible", 10, [(Kind::Match, 1)], b"T"),
        ]);
        alignment.show_read[0] = false;
        alignment.show_read[1] = false;

        let mut paired_alignment = PairedAlignment::new(&alignment).unwrap();
        paired_alignment
            .sort(&alignment, AlignmentSort::BaseAt(10))
            .unwrap();

        assert_eq!(paired_alignment.show_pair, vec![false, true]);
        assert_eq!(paired_alignment.ys_index, vec![vec![1]]);
    }
}

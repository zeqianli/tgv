use crate::{
    alignment::{
        alignment::{
            Alignment, BaseSortKey, SortableStackItem, find_track, read_base_sort_key_at,
            stack_tracks_by_sort_key,
        },
        read::{AlignedReadRef, ReadPair},
    },
    error::TGVError,
    message::AlignmentSort,
};
use noodles::sam::alignment::RecordBuf;
use polars::prelude::*;
use std::collections::HashMap;

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
        let mate_map = calculate_mate_map(&alignment.records)?;
        let read_pairs = build_read_pairs(alignment, &mate_map)?;
        let n_pair = read_pairs.len();
        let show_pair = read_pairs
            .iter()
            .map(|read_pair| pair_has_visible_read(read_pair, &alignment.show_read))
            .collect::<Vec<_>>();
        let ys = stack_tracks_for_pairs(alignment, &read_pairs, &show_pair);
        let mut pair_id = Vec::with_capacity(n_pair);
        let mut read_1_id = Vec::with_capacity(n_pair);
        let mut read_2_id = Vec::with_capacity(n_pair);
        let mut stacking_start = Vec::with_capacity(n_pair);
        let mut stacking_end = Vec::with_capacity(n_pair);
        for (id, pair) in read_pairs.iter().enumerate() {
            pair_id.push(id as u64);
            read_1_id.push(pair.read_1_index as u64);
            read_2_id.push(pair.read_2_index.map(|id| id as u64));
            let bounds = pair_bounds(pair, alignment);
            stacking_start.push(bounds.map(|(start, _)| start));
            stacking_end.push(bounds.map(|(_, end)| end));
        }
        let pairs = DataFrame::new(
            n_pair,
            vec![
                Column::new("pair_id".into(), pair_id),
                Column::new("read_1_id".into(), read_1_id),
                Column::new("read_2_id".into(), read_2_id),
                Column::new("stacking_start".into(), stacking_start),
                Column::new("stacking_end".into(), stacking_end),
            ],
        )?;

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
        let pair = self.pair(pair_id);
        (pair.read_1_index, pair.read_2_index)
    }

    fn pair(&self, pair_index: usize) -> ReadPair {
        let read_1_index = self
            .pairs
            .column("read_1_id")
            .unwrap()
            .u64()
            .unwrap()
            .get(pair_index)
            .unwrap() as usize;
        let read_2_index = self
            .pairs
            .column("read_2_id")
            .unwrap()
            .u64()
            .unwrap()
            .get(pair_index)
            .map(|id| id as usize);
        ReadPair {
            read_1_index,
            read_2_index,
        }
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

        self.show_pair = (0..self.pair_count())
            .map(|i| pair_has_visible_read(&self.pair(i), &alignment.show_read))
            .collect::<Vec<_>>();

        let items = (0..self.pair_count())
            .map(|i| self.pair(i))
            .zip(self.show_pair.iter())
            .map(|(read_pair, show_pair)| SortableStackItem {
                show: *show_pair,
                stacking_start: pair_bounds(&read_pair, alignment).map_or(0, |b| b.0),
                stacking_end: pair_bounds(&read_pair, alignment).map_or(0, |b| b.1),
                sort_key: pair_base_sort_key_at(&read_pair, alignment, position),
            })
            .collect::<Vec<_>>();

        self.ys = stack_tracks_by_sort_key(&items, 10);
        self.build_y_index()
    }
}

pub fn calculate_mate_map(reads: &[RecordBuf]) -> Result<Vec<usize>, TGVError> {
    let mut read_id_map = HashMap::<Vec<u8>, usize>::new();

    let mut output = vec![reads.len(); reads.len()];

    for (i, read) in reads.iter().enumerate() {
        if show_as_pair(read)
            && let Some(read_name) = read.name()
        {
            let read_name = read_name.to_vec();
            match read_id_map.remove(&read_name) {
                Some(mate_index) => {
                    output[i] = mate_index;
                    output[mate_index] = i;
                }
                _ => {
                    read_id_map.insert(read_name, i);
                }
            }
        };
    }

    Ok(output)
}

fn build_read_pairs(
    alignment: &Alignment,
    mate_map: &Vec<usize>,
) -> Result<Vec<ReadPair>, TGVError> {
    let mate_not_found_flag = mate_map.len();
    let mut read_pairs = Vec::new();
    let mut read_index_is_built = vec![false; alignment.read_count()];

    // FIXME: all these scenarios display a read alone with the same color:
    // - Not paired.
    // - Paired, but the mate is not loaded.
    // - Supplementary alignment.
    // - Secondary alignment.
    // Introduce some option, for example, coloring, to separate these scenarios.

    for (i, read) in alignment.records.iter().enumerate() {
        if read_index_is_built[i] {
            continue;
        }
        if show_as_pair(read) {
            let mate_index = *mate_map.get(i).ok_or_else(|| {
                TGVError::StateError(format!(
                    "Mate index out of bounds while building read pairs: {i}"
                ))
            })?;
            if mate_index == mate_not_found_flag {
                read_pairs.push(ReadPair {
                    read_1_index: i,
                    read_2_index: None,
                });
                read_index_is_built[i] = true;
            } else {
                if mate_index >= alignment.read_count() {
                    return Err(TGVError::StateError(format!(
                        "Mate index out of bounds while building read pairs: {mate_index}"
                    )));
                }
                read_pairs.push(ReadPair {
                    read_1_index: i,
                    read_2_index: Some(mate_index),
                });
                read_index_is_built[i] = true;
                read_index_is_built[mate_index] = true;
            }
        } else {
            read_pairs.push(ReadPair {
                read_1_index: i,
                read_2_index: None,
            });
            read_index_is_built[i] = true;
        };
    }

    Ok(read_pairs)
}

fn stack_tracks_for_pairs(
    alignment: &Alignment,
    read_pairs: &[ReadPair],
    show_pairs: &[bool],
) -> Vec<usize> {
    let mut track_left_bounds: Vec<u64> = Vec::new();
    let mut track_right_bounds: Vec<u64> = Vec::new();

    read_pairs
        .iter()
        .zip(show_pairs.iter())
        .map(|(read_pair, show_pair)| {
            if *show_pair && let Some((start, end)) = pair_bounds(read_pair, alignment) {
                find_track(
                    start,
                    end,
                    &mut track_left_bounds,
                    &mut track_right_bounds,
                    10,
                )
            } else {
                0
            }
        })
        .collect()
}

fn pair_has_visible_read(read_pair: &ReadPair, show_reads: &[bool]) -> bool {
    show_reads[read_pair.read_1_index]
        || read_pair
            .read_2_index
            .is_some_and(|read_2_index| show_reads[read_2_index])
}

fn pair_base_sort_key_at(
    read_pair: &ReadPair,
    alignment: &Alignment,
    position: u64,
) -> Option<BaseSortKey> {
    if alignment.show_read[read_pair.read_1_index]
        && let Some(sort_key) = alignment
            .stacking_bounds(read_pair.read_1_index)
            .map(|_| {
                AlignedReadRef::borrowed(alignment.record(read_pair.read_1_index))
                    .expect("positioned reads have valid display bounds")
            })
            .and_then(|read| read_base_sort_key_at(&read, position))
    {
        return Some(sort_key);
    }

    if let Some(read_2_index) = read_pair.read_2_index {
        if alignment.show_read[read_2_index]
            && let Some(sort_key) = alignment
                .stacking_bounds(read_2_index)
                .map(|_| {
                    AlignedReadRef::borrowed(alignment.record(read_2_index))
                        .expect("positioned reads have valid display bounds")
                })
                .and_then(|read| read_base_sort_key_at(&read, position))
        {
            return Some(sort_key);
        }

        if pair_gap_at(
            alignment.stacking_bounds(read_pair.read_1_index),
            alignment.stacking_bounds(read_2_index),
            position,
        ) {
            return Some(BaseSortKey::PairGap);
        }
    }

    None
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

fn show_as_pair(read: &RecordBuf) -> bool {
    read.flags().is_segmented() && !read.flags().is_supplementary() && !read.flags().is_secondary()
}

fn pair_bounds(pair: &ReadPair, alignment: &Alignment) -> Option<(u64, u64)> {
    let first = alignment.stacking_bounds(pair.read_1_index);
    let second = pair
        .read_2_index
        .and_then(|id| alignment.stacking_bounds(id));
    match (first, second) {
        (Some(a), Some(b)) => Some((a.0.min(b.0), a.1.max(b.1))),
        (Some(a), None) | (None, Some(a)) => Some(a),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{alignment::read::AlignedRead, sequence::Sequence};
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
    ) -> AlignedRead {
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

        AlignedRead::try_from(record).unwrap()
    }

    fn alignment_from_reads(reads: Vec<AlignedRead>) -> Alignment {
        Alignment::from_aligned_reads(
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

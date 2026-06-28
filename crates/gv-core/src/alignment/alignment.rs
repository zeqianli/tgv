use crate::alignment::{
    coverage::{BaseCoverage, DEFAULT_COVERAGE, calculate_basewise_coverage},
    read::{AlignedRead, RenderingContext, calculate_rendering_contexts},
};
use crate::error::TGVError;
use crate::intervals::{GenomeInterval, Region};
use crate::message::{AlignmentFilter, AlignmentSort};
use crate::sequence::Sequence;
use std::collections::{BTreeMap, HashMap, hash_map::Entry};

pub(super) const RENDERING_CONTEXT_NOT_CALCULATED: u64 = u64::MAX;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum BaseSortKey {
    A,
    T,
    C,
    G,
    N,
    OtherBase,
    Deletion,
    Insertion,
    PairGap,
}

impl BaseSortKey {
    fn from_base(base: u8) -> Self {
        match base.to_ascii_uppercase() {
            b'A' => Self::A,
            b'T' => Self::T,
            b'C' => Self::C,
            b'G' => Self::G,
            b'N' => Self::N,
            _ => Self::OtherBase,
        }
    }
}

pub(super) struct SortableStackItem {
    pub show: bool,
    pub stacking_start: u64,
    pub stacking_end: u64,
    pub sort_key: Option<BaseSortKey>,
}

/// An alignment stack
#[derive(Debug, Default)]
pub struct Alignment {
    /// Contig of the current alignment
    pub contig_index: usize,

    pub reads: Vec<AlignedRead>,

    /// Base mismatches with the reference.
    pub rendering_contexts: Vec<Vec<RenderingContext>>,

    /// Read index to rendering context index.
    pub read_rendering_context_indexes: Vec<u64>,

    // /// Paired alignment view state.
    // paired_alignment: Option<PairedAlignment>,

    // read index -> y locations
    pub ys: Vec<usize>,

    /// y -> read indexes at y location
    pub ys_index: Vec<Vec<usize>>,

    /// Coverage at each position. Keys are 1-based, inclusive.
    /// Calculated as needed.
    coverage: BTreeMap<u64, BaseCoverage>,

    /// The left bound of region with complete data.
    /// 1-based, inclusive.
    data_complete_left_bound: u64,

    /// The right bound of region with complete data.
    /// 1-based, inclusive.
    data_complete_right_bound: u64,

    // Whether to display the read
    pub show_read: Vec<bool>,
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
    pub fn depth(&self) -> usize {
        self.ys_index.len()
    }

    /// Basewise coverage at position.
    /// 1-based, inclusive.
    pub fn coverage_at(&self, pos: u64) -> &BaseCoverage {
        match self.coverage.get(&pos) {
            Some(coverage) => coverage,
            None => &DEFAULT_COVERAGE,
        }
    }

    /// Return the read at x_coordinate, yth track
    pub fn read_overlapping(&self, left: u64, right: u64, y: usize) -> Option<&AlignedRead> {
        if y >= self.depth() {
            return None;
        }

        self.ys_index[y]
            .iter()
            .find(|i_read| self.reads[**i_read].full_read_overlaps(left, right))
            .map(|index| &self.reads[*index])
    }

    pub fn from_aligned_reads(
        reads: Vec<AlignedRead>,
        contig_index: usize,
        data_complete_bound: (u64, u64),
        reference_sequence: &Sequence,
    ) -> Result<Self, TGVError> {
        let show_reads = vec![true; reads.len()];
        let ys = stack_tracks_for_reads(&reads, &show_reads);
        let mut alignment = Self {
            rendering_contexts: Vec::new(),
            read_rendering_context_indexes: vec![RENDERING_CONTEXT_NOT_CALCULATED; reads.len()],
            reads,
            contig_index,
            coverage: BTreeMap::new(),
            data_complete_left_bound: data_complete_bound.0,
            data_complete_right_bound: data_complete_bound.1,
            ys: ys.clone(),
            show_read: show_reads,
            ys_index: Vec::new(),
        };
        alignment
            .build_y_index()?
            .build_coverage(reference_sequence)?;
        Ok(alignment)
    }

    /// Build indexes, coverages after key assets are set: reads, show_read, ys
    pub fn build_y_index(&mut self) -> Result<&mut Self, TGVError> {
        let mut ys_index = vec![Vec::new(); *self.ys.iter().max().unwrap_or(&0) + 1];
        self.ys
            .iter()
            .zip(self.show_read.iter())
            .enumerate()
            .for_each(|(i, (y, show_read))| {
                if *show_read {
                    ys_index[*y].push(i)
                }
            });
        self.ys_index = ys_index;

        Ok(self)
    }

    /// If rendering context is calculated for read_index, return the rendering context index in self.rendering_contexts
    /// Return None if not yet calculated.
    pub fn get_rendering_context_index(&self, read_index: usize) -> Option<u64> {
        match self.read_rendering_context_indexes[read_index] {
            RENDERING_CONTEXT_NOT_CALCULATED => None,
            i => Some(i),
        }
    }

    /// Calculate and write rendering context for read_index.
    /// The new context is added to the end of the context vector.
    /// Returns the index of the new contexts.
    pub fn calculate_read_rendering_context(
        &mut self,
        read_index: usize,
        reference_sequence: &Sequence,
    ) -> Result<u64, TGVError> {
        let read = &self.reads[read_index];

        let mut contexts = Vec::new();
        calculate_rendering_contexts(
            &mut contexts,
            read.start,
            &read.record.flags(),
            read.record.cigar().as_ref(),
            read.record.sequence(),
            read.record.data(),
            reference_sequence,
        )?;

        self.rendering_contexts.push(contexts);
        let rendering_context_index = (self.rendering_contexts.len() - 1) as u64;
        self.read_rendering_context_indexes[read_index] = rendering_context_index;

        Ok(rendering_context_index)
    }

    // pub fn apply_options(
    //     &mut self,
    //     options: &Vec<AlignmentDisplayOption>,
    //     reference_sequence: &Sequence,
    // ) -> Result<&mut Self, TGVError> {

    // }

    /// Reset alignment options
    // pub fn reset(&mut self, reference_sequence: &Sequence) -> Result<&mut Self, TGVError> {
    //     // TODO: reference sequence could be empty.
    //     self.show_read = vec![true; self.reads.len()];
    //     self.ys = stack_tracks_for_reads(&self.reads, &self.show_read);
    //     self.paired_alignment = None;

    //     self.build_y_index()?.build_coverage(reference_sequence)
    // }

    pub fn build_coverage(&mut self, reference_sequence: &Sequence) -> Result<&mut Self, TGVError> {
        // TODO: optimize
        let mut coverage_hashmap: HashMap<u64, BaseCoverage> = HashMap::new();
        for (read, show_read) in self.reads.iter().zip(self.show_read.iter()) {
            if !*show_read {
                continue;
            }
            let read_coverage = calculate_basewise_coverage(
                read.start,
                read.record.cigar(),
                read.record.sequence(),
                reference_sequence,
            )?; // TODO: seq() is called twice. Optimize this in the future.
            for (i, coverage) in read_coverage.into_iter() {
                match coverage_hashmap.entry(i) {
                    Entry::Occupied(mut oe) => oe.get_mut().add(&coverage),
                    Entry::Vacant(ve) => {
                        ve.insert(coverage);
                    }
                }
            }
        }

        self.coverage = coverage_hashmap.into_iter().collect();

        Ok(self)
    }

    pub fn filter(
        &mut self,
        filter: AlignmentFilter,
        reference_sequence: &Sequence,
    ) -> Result<(), TGVError> {
        for (i, read) in self.reads.iter().enumerate() {
            self.show_read[i] = read.passes_filter(&filter)
        }

        self.ys = stack_tracks_for_reads(&self.reads, &self.show_read);
        self.build_y_index()?.build_coverage(reference_sequence)?;

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

        let items = self
            .reads
            .iter()
            .zip(self.show_read.iter())
            .map(|(read, show_read)| SortableStackItem {
                show: *show_read,
                stacking_start: read.stacking_start(),
                stacking_end: read.stacking_end(),
                sort_key: read_base_sort_key_at(read, position),
            })
            .collect::<Vec<_>>();

        self.ys = stack_tracks_by_sort_key(&items, 3);
        self.build_y_index()?;

        Ok(())
    }
}

fn stack_tracks_for_reads(reads: &Vec<AlignedRead>, show_reads: &Vec<bool>) -> Vec<usize> {
    let mut track_left_bounds: Vec<u64> = Vec::new();
    let mut track_right_bounds: Vec<u64> = Vec::new();

    reads
        .iter()
        .zip(show_reads.iter())
        .map(|(read, show_read)| {
            if *show_read {
                find_track(
                    read.stacking_start(),
                    read.stacking_end(),
                    &mut track_left_bounds,
                    &mut track_right_bounds,
                    3,
                )
            } else {
                0
            }
        })
        .collect::<Vec<usize>>()
}

pub(super) fn read_base_sort_key_at(read: &AlignedRead, position: u64) -> Option<BaseSortKey> {
    if let Some(base) = read.base_at(position) {
        return Some(BaseSortKey::from_base(base));
    }

    if read.is_deletion_at(position) {
        return Some(BaseSortKey::Deletion);
    }

    if read.has_insertion_at(position) {
        return Some(BaseSortKey::Insertion);
    }

    None
}

pub(super) fn stack_tracks_by_sort_key(items: &[SortableStackItem], min_gap: u64) -> Vec<usize> {
    let mut ys = vec![0; items.len()];
    let mut sorted_item_indexes = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            if !item.show {
                return None;
            }

            Some((item.sort_key?, index))
        })
        .collect::<Vec<_>>();

    sorted_item_indexes.sort_by_key(|(sort_key, index)| (*sort_key, *index));

    let mut is_sorted_item = vec![false; items.len()];
    let mut track_left_bounds = Vec::with_capacity(sorted_item_indexes.len());
    let mut track_right_bounds = Vec::with_capacity(sorted_item_indexes.len());
    for (y, (_sort_key, index)) in sorted_item_indexes.iter().enumerate() {
        ys[*index] = y;
        is_sorted_item[*index] = true;
        track_left_bounds.push(items[*index].stacking_start);
        track_right_bounds.push(items[*index].stacking_end);
    }

    for (index, item) in items.iter().enumerate() {
        if !item.show || is_sorted_item[index] {
            continue;
        }

        ys[index] = find_track(
            item.stacking_start,
            item.stacking_end,
            &mut track_left_bounds,
            &mut track_right_bounds,
            min_gap,
        );
    }

    ys
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
    use std::collections::BTreeMap;

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
            .set_flags(Flags::default())
            .set_alignment_start(noodles::core::Position::try_from(start as usize).unwrap())
            .set_cigar(cigar)
            .set_sequence(sam::alignment::record_buf::Sequence::from(sequence))
            .build();

        AlignedRead::try_from(record).unwrap()
    }

    fn alignment_with_reads(reads: Vec<AlignedRead>, data_complete_bound: (u64, u64)) -> Alignment {
        let read_count = reads.len();
        let show_read = vec![true; read_count];
        let ys = stack_tracks_for_reads(&reads, &show_read);
        let mut alignment = Alignment {
            contig_index: 0,
            reads,
            rendering_contexts: Vec::new(),
            read_rendering_context_indexes: vec![RENDERING_CONTEXT_NOT_CALCULATED; read_count],
            ys,
            ys_index: Vec::new(),
            coverage: BTreeMap::new(),
            data_complete_left_bound: data_complete_bound.0,
            data_complete_right_bound: data_complete_bound.1,
            show_read,
        };
        alignment.build_y_index().unwrap();
        alignment
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
        alignment.show_read[7] = false;

        alignment.sort(AlignmentSort::BaseAt(12)).unwrap();

        assert_eq!(alignment.ys, vec![3, 1, 6, 0, 4, 5, 2, 0]);
        assert_eq!(
            alignment.ys_index,
            vec![
                vec![3],
                vec![1],
                vec![6],
                vec![0],
                vec![4],
                vec![5],
                vec![2]
            ]
        );
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

        assert_eq!(alignment.ys, vec![0, 0, 0, 1]);
        assert_eq!(alignment.ys_index, vec![vec![0, 1, 2], vec![3]]);
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

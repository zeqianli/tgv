use crate::error::TGVError;
use crate::intervals::{GenomeInterval, Region};
use crate::message::{AlignmentFilter, AlignmentSort};
use crate::sequence::Sequence;
use crate::{
    alignment::{
        coverage::{BaseCoverage, DEFAULT_COVERAGE, calculate_basewise_coverage},
        paired_alignment::PairedAlignment,
        read::{AlignedRead, RenderingContext, calculate_rendering_contexts},
    },
    message::AlignmentDisplayOption,
};
use std::collections::{BTreeMap, HashMap, hash_map::Entry};

pub(super) const RENDERING_CONTEXT_NOT_CALCULATED: u64 = u64::MAX;

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

    /// Paired alignment view state.
    paired_alignment: Option<PairedAlignment>,

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
    show_read: Vec<bool>,
}

impl Alignment {
    /// Check if data in [left, right] is all loaded.
    /// 1-based, inclusive.
    pub fn has_complete_data(&self, region: &Region) -> bool {
        (region.contig_index() == self.contig_index)
            && (region.start() >= self.data_complete_left_bound)
            && (region.end() <= self.data_complete_right_bound)
    }

    /// Return the number of alignment tracks.
    pub fn depth(&self) -> usize {
        self.paired_alignment
            .as_ref()
            .map(PairedAlignment::depth)
            .unwrap_or(self.ys_index.len())
    }

    /// Basewise coverage at position.
    /// 1-based, inclusive.
    pub fn coverage_at(&self, pos: u64) -> &BaseCoverage {
        match self.coverage.get(&pos) {
            Some(coverage) => coverage,
            None => &DEFAULT_COVERAGE,
        }
    }

    /// Return the read at x, yth track
    pub fn read_at(&self, x: u64, y: usize) -> Option<&AlignedRead> {
        if let Some(paired_alignment) = &self.paired_alignment {
            return paired_alignment.read_at(&self.reads, x, y);
        }

        if y >= self.depth() {
            return None;
        }

        self.ys_index[y]
            .iter()
            .find(|i_read| self.reads[**i_read].full_read_covers(x))
            .map(|index| &self.reads[*index])
    }

    fn view_as_pairs(&mut self, reference_sequence: &Sequence) -> Result<&mut Self, TGVError> {
        self.paired_alignment = Some(PairedAlignment::new(self, reference_sequence)?);
        Ok(self)
    }

    /// Return the read at x_coordinate, yth track
    pub fn read_overlapping(&self, left: u64, right: u64, y: usize) -> Option<&AlignedRead> {
        if let Some(paired_alignment) = &self.paired_alignment {
            return paired_alignment.read_overlapping(&self.reads, left, right, y);
        }

        if y >= self.depth() {
            return None;
        }

        self.ys_index[y]
            .iter()
            .find(|i_read| self.reads[**i_read].full_read_overlaps(left, right))
            .map(|index| &self.reads[*index])
    }

    pub fn y_of(&self, read_index: usize) -> Option<usize> {
        if *self.show_read.get(read_index)? {
            Some(*self.ys.get(read_index)?)
        } else {
            None
        }
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
            paired_alignment: None,
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

    pub fn get_rendering_contexts(&self, read_index: usize) -> Option<&[RenderingContext]> {
        match self.read_rendering_context_indexes[read_index] {
            RENDERING_CONTEXT_NOT_CALCULATED => None,
            i => Some(self.rendering_contexts[i as usize].as_ref()),
        }
    }

    /// Calculate and write rendering context for read_index.
    /// The new context is added to the end of the context vector.
    /// Returns the index of the new contexts.
    pub(super) fn calculate_read_rendering_context(
        &mut self,
        read_index: usize,
        reference_sequence: &Sequence,
    ) -> Result<u64, TGVError> {
        let read = &self.reads[read_index];

        let cigars = read.cigars()?;
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
    //     let view_as_pairs = options
    //         .iter()
    //         .any(|option| matches!(option, AlignmentDisplayOption::ViewAsPairs));

    //     for option in options {
    //         match option {
    //             AlignmentDisplayOption::Filter(filter) => {
    //                 self.filter(filter, reference_sequence)?;
    //             }
    //             AlignmentDisplayOption::Sort(sort) => {
    //                 self.sort(sort)?;
    //             }
    //             AlignmentDisplayOption::ViewAsPairs => {}
    //         }
    //     }

    //     if view_as_pairs {
    //         self.view_as_pairs(reference_sequence)?;
    //     }

    //     Ok(self)
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
            let cigars = read.cigars()?;
            let read_coverage = calculate_basewise_coverage(
                read.start,
                &cigars,
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
        filter: &AlignmentFilter,
        reference_sequence: &Sequence,
    ) -> Result<&mut Self, TGVError> {
        for (i, read) in self.reads.iter().enumerate() {
            self.show_read[i] = read.passes_filter(filter)
        }

        self.ys = stack_tracks_for_reads(&self.reads, &self.show_read);
        self.paired_alignment = None;
        self.build_y_index()?.build_coverage(reference_sequence)?;

        Ok(self)
    }

    pub fn sort(&mut self, option: &AlignmentSort) -> Result<&mut Self, TGVError> {
        Err(TGVError::ValueError(format!(
            "Alignment sorting is not implemented yet for option {option}"
        )))
    }

    pub fn pair_rendering_contexts(
        &self,
        pair_index: usize,
    ) -> Result<&[RenderingContext], TGVError> {
        self.paired_alignment
            .as_ref()
            .ok_or_else(|| {
                TGVError::StateError("Read pairs are not calculated before rendering.".to_string())
            })?
            .pair_rendering_contexts(pair_index)
    }

    pub fn visible_read_pairs(&self) -> Result<Vec<(usize, usize)>, TGVError> {
        self.paired_alignment
            .as_ref()
            .ok_or_else(|| {
                TGVError::StateError("Read pairs are not calculated before rendering.".to_string())
            })?
            .visible_pairs()
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

fn find_track(
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
    track_left_bounds.len()
}

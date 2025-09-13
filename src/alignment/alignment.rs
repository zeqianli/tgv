use crate::error::TGVError;
use crate::intervals::Region;
use crate::message::{AlignmentFilter, AlignmentSort};
use crate::sequence::Sequence;
use crate::{
    alignment::{
        coverage::{calculate_basewise_coverage, BaseCoverage, DEFAULT_COVERAGE},
        read::{calculate_paired_context, AlignedRead, ReadPair},
    },
    message::AlignmentDisplayOption,
};
use std::collections::{hash_map::Entry, BTreeMap, HashMap};

/// A alignment region on a contig.
pub struct Alignment {
    pub reads: Vec<AlignedRead>,

    pub contig_index: usize,

    /// Coverage at each position. Keys are 1-based, inclusive.
    coverage: BTreeMap<usize, BaseCoverage>,

    /// The left bound of region with complete data.
    /// 1-based, inclusive.
    data_complete_left_bound: usize,

    /// The right bound of region with complete data.
    /// 1-based, inclusive.
    data_complete_right_bound: usize,

    // read index -> y locations
    ys: Vec<usize>,

    // Whether to display the read
    show_read: Vec<bool>,

    /// y -> read indexes at y location
    pub ys_index: Vec<Vec<usize>>,

    /// Default ys
    default_ys: Vec<usize>,

    /// read index -> mate read index (if present)
    pub mate_map: Option<Vec<usize>>,

    pub read_pairs: Option<Vec<ReadPair>>,

    /// Whether to show the pair
    show_pairs: Option<Vec<bool>>,
}

impl Alignment {
    /// Check if data in [left, right] is all loaded.
    /// 1-based, inclusive.
    pub fn has_complete_data(&self, region: &Region) -> bool {
        (region.contig_index == self.contig_index)
            && (region.start >= self.data_complete_left_bound)
            && (region.end <= self.data_complete_right_bound)
    }

    /// Return the number of alignment tracks.
    pub fn depth(&self) -> usize {
        self.ys_index.len()
    }

    /// Basewise coverage at position.
    /// 1-based, inclusive.
    pub fn coverage_at(&self, pos: usize) -> &BaseCoverage {
        match self.coverage.get(&pos) {
            Some(coverage) => coverage,
            None => &DEFAULT_COVERAGE,
        }
    }

    /// Return the read at x_coordinate, yth track
    pub fn read_at(&self, x_coordinate: usize, y: usize) -> Option<&AlignedRead> {
        if y >= self.depth() {
            return None;
        }

        self.ys_index[y]
            .iter()
            .find(|i_read| self.reads[**i_read].full_read_covers(x_coordinate))
            .map(|index| &self.reads[*index])
    }

    /// Return the read at x_coordinate, yth track
    pub fn read_overlapping(
        &self,
        x_left_coordinate: usize,
        x_right_coordinate: usize,
        y: usize,
    ) -> Option<&AlignedRead> {
        if y >= self.depth() {
            return None;
        }

        self.ys_index[y]
            .iter()
            .find(|i_read| {
                self.reads[**i_read].full_read_overlaps(x_left_coordinate, x_right_coordinate)
            })
            .map(|index| &self.reads[*index])
    }

    pub fn y_of(&self, read: &AlignedRead) -> Option<usize> {
        if self.show_read[read.index] {
            Some(self.ys[read.index])
        } else {
            None
        }
    }

    pub fn from_aligned_reads(
        reads: Vec<AlignedRead>,
        region: &Region,
        reference_sequence: Option<&Sequence>,
    ) -> Result<Self, TGVError> {
        let show_reads = vec![true; reads.len()];
        let ys = stack_tracks_for_reads(&reads, &show_reads);
        let mut alignment = Self {
            reads,
            contig_index: region.contig_index,
            coverage: BTreeMap::new(),
            data_complete_left_bound: region.start,
            data_complete_right_bound: region.end,
            ys: ys.clone(),
            default_ys: ys,
            show_read: show_reads,
            ys_index: Vec::new(),
            mate_map: None,
            read_pairs: None,
            show_pairs: None,
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

    /// Build mate index
    pub fn build_mate_index(&mut self) -> Result<&mut Self, TGVError> {
        self.mate_map = Some(calculate_mate_map(&self.reads)?);
        Ok(self)
    }

    pub fn build_mate_rendering_contexts(&mut self) -> Result<&mut Self, TGVError> {
        if self.mate_map.is_none() {
            return Ok(self);
        }

        let mate_map = self.mate_map.as_ref().unwrap();
        let MATE_NOT_FOUND_FLAG = mate_map.len();

        let mut read_pairs = Vec::new();
        let mut show_pairs = Vec::new();

        let mut read_index_is_built = vec![false; self.reads.len()];

        // FIXME
        // Now, all these scenrios display a read alone with the same color:
        // - Not paired
        // - Paired but the mate is not loaded
        // - Supplementary alignment
        // - Secondary alignment
        // Introduce some option (e.g. coloring) to seprate these scenarios.

        for (i, read) in self.reads.iter().enumerate() {
            if read_index_is_built[i] {
                continue;
            }
            if read.show_as_pair() {
                let mate_index = mate_map[i];
                if mate_index == MATE_NOT_FOUND_FLAG {
                    read_pairs.push(self.make_read_pair(read_pairs.len(), i, None));
                    show_pairs.push(self.show_read[i]);
                    read_index_is_built[i] = true;
                } else {
                    read_pairs.push(self.make_read_pair(read_pairs.len(), i, Some(mate_index)));
                    show_pairs.push(self.show_read[i] || self.show_read[mate_index]);
                    read_index_is_built[i] = true;
                    read_index_is_built[mate_index] = true;
                }
            } else {
                read_pairs.push(self.make_read_pair(read_pairs.len(), i, None));
                show_pairs.push(self.show_read[i]);
                read_index_is_built[i] = true;
            };
        }

        self.read_pairs = Some(read_pairs);
        self.show_pairs = Some(show_pairs);

        Ok(self)
    }

    pub fn apply_options(
        &mut self,
        options: &Vec<AlignmentDisplayOption>,
        reference_sequence: Option<&Sequence>,
    ) -> Result<&mut Self, TGVError> {
        for option in options {
            if let AlignmentDisplayOption::Filter(filter) = option {
                self.filter(filter, reference_sequence)?;
            }
        }

        Ok(self)
    }

    /// Reset alignment options
    pub fn reset(&mut self, reference_sequence: Option<&Sequence>) -> Result<&mut Self, TGVError> {
        self.ys = self.default_ys.clone();
        self.show_read = vec![true; self.reads.len()];

        self.build_y_index()?.build_coverage(reference_sequence)
    }

    pub fn make_read_pair(
        &self,
        pair_index: usize,
        read_index_1: usize,
        read_index_2: Option<usize>,
    ) -> ReadPair {
        match read_index_2 {
            Some(read_index_2) => {
                let (read_1, read_2) = (&self.reads[read_index_1], &self.reads[read_index_2]);

                let stacking_start = usize::min(read_1.stacking_start(), read_2.stacking_start());
                let stacking_end = usize::min(read_1.stacking_end(), read_2.stacking_end());
                let rendering_contexts = calculate_paired_context(
                    read_1.rendering_contexts.clone(),
                    read_2.rendering_contexts.clone(),
                );

                ReadPair {
                    read_1_index: read_index_1,
                    read_2_index: Some(read_index_2),
                    stacking_start: stacking_start,
                    stacking_end: stacking_end,
                    index: pair_index,
                    rendering_contexts: rendering_contexts,
                }
            }
            None => {
                let read = &self.reads[read_index_1];
                ReadPair {
                    read_1_index: read_index_1,
                    read_2_index: None,
                    stacking_start: read.stacking_start(),
                    stacking_end: read.stacking_end(),
                    index: pair_index,
                    rendering_contexts: read.rendering_contexts.clone(),
                }
            }
        }
    }

    pub fn build_coverage(
        &mut self,
        reference_sequence: Option<&Sequence>,
    ) -> Result<&mut Self, TGVError> {
        // coverage

        let mut coverage_hashmap: HashMap<usize, BaseCoverage> = HashMap::new();
        for (read, show_read) in self.reads.iter().zip(self.show_read.iter()) {
            if !*show_read {
                continue;
            }
            let read_coverage = calculate_basewise_coverage(
                read.start,
                &read.cigar,
                read.leading_softclips,
                &read.read.seq(),
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

        let mut coverage: BTreeMap<usize, BaseCoverage> = BTreeMap::new();
        for (k, v) in coverage_hashmap.into_iter() {
            coverage.insert(k, v);
        }

        self.coverage = coverage;

        Ok(self)
    }

    pub fn filter(
        &mut self,
        filter: &AlignmentFilter,
        reference_sequence: Option<&Sequence>,
    ) -> Result<&mut Self, TGVError> {
        for (i, read) in self.reads.iter().enumerate() {
            self.show_read[i] = read.passes_filter(filter)
        }

        self.ys = stack_tracks_for_reads(&self.reads, &self.show_read);
        self.build_y_index()?.build_coverage(reference_sequence)
    }
}

pub fn sort_alignment(alignment: &mut Alignment, option: AlignmentSort) -> Result<(), TGVError> {
    // FIXME
    todo!();
}

pub fn view_as_pairs(alignment: &mut Alignment) -> Result<(), TGVError> {
    if alignment.mate_map.is_none() {
        alignment.build_mate_index()?;
    }
    alignment.build_mate_rendering_contexts()?;

    // build y index
    alignment.ys = stack_tracks_for_paired_reads(
        alignment.read_pairs.as_ref().unwrap(),
        alignment.show_pairs.as_ref().unwrap(),
    );
    alignment.build_y_index()?;

    Ok(())
}

fn calculate_mate_map(reads: &Vec<AlignedRead>) -> Result<Vec<usize>, TGVError> {
    let mut read_id_map = HashMap::<Vec<u8>, usize>::new();

    let mut output = vec![reads.len(); reads.len()];

    for (i, read) in reads.iter().enumerate() {
        if read.show_as_pair() {
            let read_name = read.read.qname().to_vec();
            match read_id_map.remove(&read_name) {
                Some(mate_index) => {
                    output[i] = mate_index;
                    output[mate_index] = i;
                }
                _ => {
                    read_id_map.insert(read_name, i);
                }
            }
        }
    }

    Ok(output)
}

const MIN_HORIZONTAL_GAP_BETWEEN_READS: usize = 3;
fn stack_tracks_for_reads(reads: &Vec<AlignedRead>, show_reads: &Vec<bool>) -> Vec<usize> {
    let mut track_left_bounds: Vec<usize> = Vec::new();
    let mut track_right_bounds: Vec<usize> = Vec::new();

    let ys = reads
        .iter()
        .zip(show_reads.iter())
        .map(|(read, show_read)| {
            if *show_read {
                find_track(
                    read.stacking_start(),
                    read.stacking_end(),
                    &mut track_left_bounds,
                    &mut track_right_bounds,
                )
            } else {
                0
            }
        })
        .collect::<Vec<usize>>();

    ys
}
fn stack_tracks_for_paired_reads(reads: &Vec<ReadPair>, show_reads: &Vec<bool>) -> Vec<usize> {
    let mut track_left_bounds: Vec<usize> = Vec::new();
    let mut track_right_bounds: Vec<usize> = Vec::new();

    let ys = reads
        .iter()
        .zip(show_reads.iter())
        .map(|(read, show_read)| {
            if *show_read {
                find_track(
                    read.stacking_start,
                    read.stacking_end,
                    &mut track_left_bounds,
                    &mut track_right_bounds,
                )
            } else {
                0
            }
        })
        .collect::<Vec<usize>>();

    ys
}

fn find_track(
    start: usize,
    end: usize,
    track_left_bounds: &mut Vec<usize>,
    track_right_bounds: &mut Vec<usize>,
) -> usize {
    for (y, left_bound) in track_left_bounds.iter_mut().enumerate() {
        if end + MIN_HORIZONTAL_GAP_BETWEEN_READS < *left_bound {
            *left_bound = start;

            return y;
        }
    }

    for (y, right_bound) in track_right_bounds.iter_mut().enumerate() {
        if start > *right_bound + MIN_HORIZONTAL_GAP_BETWEEN_READS {
            *right_bound = end;
            return y;
        }
    }

    track_left_bounds.push(start);
    track_right_bounds.push(end);
    track_left_bounds.len()
}

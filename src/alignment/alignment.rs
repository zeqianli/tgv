use crate::alignment::{
    coverage::{calculate_basewise_coverage, BaseCoverage, DEFAULT_COVERAGE},
    read::AlignedRead,
};
use crate::error::TGVError;
use crate::message::{AlignmentFilter, AlignmentSort};
use crate::region::Region;
use crate::sequence::Sequence;
use crate::window::ViewingWindow;
use ratatui::layout::Rect;
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
}

/// Data loading
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
        let ys = stack_tracks_for_reads(&reads, &show_reads)?;
        let mut alignment = Self {
            reads: reads,
            contig_index: region.contig_index,
            coverage: BTreeMap::new(),
            data_complete_left_bound: region.start,
            data_complete_right_bound: region.end,
            ys: ys,
            show_read: show_reads,
            ys_index: Vec::new(),
        };
        alignment
            .build_y_index()?
            .build_coverage(reference_sequence)?;
        return Ok(alignment);
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
                    Entry::Occupied(mut oe) => oe.get_mut().add(coverage),
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
        filter: AlignmentFilter,
        window: &ViewingWindow,
        area: &Rect,
        reference_sequence: Option<&Sequence>,
    ) -> Result<&mut Self, TGVError> {
        for (i, read) in self.reads.iter().enumerate() {
            self.show_read[i] = read.passes_filter(&filter, window, area)
        }

        self.ys = stack_tracks_for_reads(&self.reads, &self.show_read)?;
        self.build_y_index()?.build_coverage(reference_sequence)
    }
}

pub fn sort_alignment(alignment: &mut Alignment, option: AlignmentSort) -> Result<(), TGVError> {
    todo!();
}

const MIN_HORIZONTAL_GAP_BETWEEN_READS: usize = 3;
fn stack_tracks_for_reads(
    reads: &Vec<AlignedRead>,
    show_reads: &Vec<bool>,
) -> Result<Vec<usize>, TGVError> {
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

    Ok(ys)
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

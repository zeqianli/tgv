use crate::error::TGVError;
use crate::models::{contig::Contig, region::Region};
use rust_htslib::bam::ext::BamRecordExtensions;
use rust_htslib::bam::{Header, IndexedReader, Read, Record};
use std::collections::{BTreeMap, HashMap};
use url::Url;

#[derive(Clone, Debug)]
/// An aligned read with viewing coordinates.
pub struct AlignedRead {
    /// Alignment record data
    pub read: Record,

    /// Non-clipped start genome coordinate on the alignment view
    /// 1-based, inclusive
    pub start: usize,
    /// Non-clipped end genome coordinate on the alignment view
    /// Note that this includes the soft-clipped reads and differ from the built-in methods. TODO
    /// 1-based, inclusive
    pub end: usize,

    /// Leading softclips. Used for track stacking calculation.
    pub leading_softclips: usize,

    /// Trailing softclips. Used for track stacking calculation.
    pub trailing_softclips: usize,

    /// Y coordinate in the alignment view
    /// 0-based.
    pub y: usize,
}

impl AlignedRead {
    /// Return an 1-based range iterator that includes all bases of the alignment.
    pub fn range(&self) -> impl Iterator<Item = usize> {
        self.start..self.end + 1
    }

    fn stacking_start(&self) -> usize {
        usize::max(self.start.saturating_sub(self.leading_softclips), 1)
    }

    fn stacking_end(&self) -> usize {
        self.end.saturating_add(self.trailing_softclips)
    }
}

/// A alignment region on a contig.
pub struct Alignment {
    pub reads: Vec<AlignedRead>,

    pub contig: Contig, // contig name

    /// Coverage at each position. Keys are 1-based, inclusive.
    coverage: BTreeMap<usize, usize>,

    /// The left bound of region with complete data.
    /// 1-based, inclusive.
    data_complete_left_bound: usize,

    /// The right bound of region with complete data.
    /// 1-based, inclusive.
    data_complete_right_bound: usize,

    depth: usize,
}

/// Data loading
impl Alignment {
    /// Check if data in [left, right] is all loaded.
    /// 1-based, inclusive.
    pub fn has_complete_data(&self, region: &Region) -> bool {
        (region.contig == self.contig)
            && (region.start >= self.data_complete_left_bound)
            && (region.end <= self.data_complete_right_bound)
    }

    /// Return the number of alignment tracks.
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// Basewise coverage at position.
    /// 1-based, inclusive.
    pub fn coverage_at(&self, pos: usize) -> usize {
        if pos < self.data_complete_left_bound || pos > self.data_complete_right_bound {
            return 0;
        }
        match self.coverage.get(&pos) {
            Some(coverage) => *coverage,
            None => 0,
        }
    }

    /// Mean basewise coverage in [left, right].
    /// 1-based, inclusive.
    pub fn mean_basewise_coverage_in(&self, left: usize, right: usize) -> Result<usize, TGVError> {
        if right < left {
            return Err(TGVError::ValueError("Right is less than left".to_string()));
        }

        if right < self.data_complete_left_bound || left > self.data_complete_right_bound {
            return Ok(0);
        }

        if right == left {
            return Ok(self.coverage_at(left));
        }

        Ok(self
            .coverage
            .range(left..right + 1)
            .map(|(_, coverage)| coverage)
            .sum::<usize>()
            / (right - left + 1))
    }
}

pub struct AlignmentBuilder {
    aligned_reads: Vec<AlignedRead>,
    coverage_hashmap: HashMap<usize, usize>,

    track_left_bounds: Vec<usize>,
    track_right_bounds: Vec<usize>,

    track_most_left_bound: usize,
    track_most_right_bound: usize,

    region: Option<Region>,
}

impl AlignmentBuilder {
    pub fn new() -> Result<Self, TGVError> {
        Ok(Self {
            aligned_reads: Vec::new(),
            coverage_hashmap: HashMap::new(),
            track_left_bounds: Vec::new(),
            track_right_bounds: Vec::new(),

            track_most_left_bound: usize::MAX,
            track_most_right_bound: 0,

            region: None,
        })
    }

    /// Add a read to the alignment. Note that this function does not update coverage.
    pub fn add_read(&mut self, read: Record) -> Result<&mut Self, TGVError> {
        let read_start = read.pos() as usize + 1;
        let read_end = read.reference_end() as usize;
        let leading_softclips = read.cigar().leading_softclips() as usize;
        let trailing_softclips = read.cigar().trailing_softclips() as usize;
        // read.pos() in htslib: 0-based, inclusive, excluding leading hardclips and softclips
        // read.reference_end() in htslib: 0-based, exclusive, excluding trailing hardclips and softclips

        let y = self.find_track(
            read_start.saturating_sub(leading_softclips),
            read_end.saturating_add(trailing_softclips),
        );

        let aligned_read = AlignedRead {
            read,
            start: read_start,
            end: read_end,
            leading_softclips,
            trailing_softclips,
            y,
        };

        // Track bounds + depth update
        if self.aligned_reads.is_empty() || aligned_read.y >= self.track_left_bounds.len() {
            // Add to a new track
            self.track_left_bounds.push(aligned_read.stacking_start());
            self.track_right_bounds.push(aligned_read.stacking_end());
        } else {
            // Add to an existing track
            if aligned_read.stacking_start() < self.track_left_bounds[aligned_read.y] {
                self.track_left_bounds[aligned_read.y] = aligned_read.stacking_start();
            }
            if aligned_read.stacking_end() > self.track_right_bounds[aligned_read.y] {
                self.track_right_bounds[aligned_read.y] = aligned_read.stacking_end();
            }
        }

        // Most left/right bound update
        if aligned_read.stacking_start() < self.track_most_left_bound {
            self.track_most_left_bound = aligned_read.stacking_start();
        }
        if aligned_read.stacking_end() > self.track_most_right_bound {
            self.track_most_right_bound = aligned_read.stacking_end();
        }

        // update coverge hashmap
        for i in aligned_read.range() {
            // TODO: check exclusivity here
            *self.coverage_hashmap.entry(i).or_insert(1) += 1;
        }

        // Add to reads
        self.aligned_reads.push(aligned_read);

        Ok(self)
    }

    const MIN_HORIZONTAL_GAP_BETWEEN_READS: usize = 3;

    fn find_track(&mut self, read_start: usize, read_end: usize) -> usize {
        if self.aligned_reads.is_empty() {
            return 0;
        }

        for (y, left_bound) in self.track_left_bounds.iter().enumerate() {
            if read_end + Self::MIN_HORIZONTAL_GAP_BETWEEN_READS < *left_bound {
                return y;
            }
        }

        for (y, right_bound) in self.track_right_bounds.iter().enumerate() {
            if read_start > *right_bound + Self::MIN_HORIZONTAL_GAP_BETWEEN_READS {
                return y;
            }
        }

        self.track_left_bounds.len()
    }

    /// Set the alignment complete region.
    pub fn region(&mut self, region: &Region) -> Result<&mut Self, TGVError> {
        self.region = Some(region.clone());

        Ok(self)
    }

    pub fn build(&self) -> Result<Alignment, TGVError> {
        let mut coverage: BTreeMap<usize, usize> = BTreeMap::new();

        // Convert hashmap to BTreeMap
        for (k, v) in &self.coverage_hashmap {
            *coverage.entry(*k).or_insert(*v) += v;
        }

        if self.region.is_none() {
            return Err(TGVError::StateError(
                "AlignmentBuilder is missin Region.".to_string(),
            ));
        }

        let region = self.region.clone().unwrap();

        Ok(Alignment {
            reads: self.aligned_reads.clone(), // TODO: lookup on how to move this
            contig: region.contig,
            coverage: coverage,
            data_complete_left_bound: region.start,
            data_complete_right_bound: region.end,

            depth: self.track_left_bounds.len(),
        })
    }
}

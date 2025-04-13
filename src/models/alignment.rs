use crate::error::TGVError;
use crate::helpers::is_url;
use crate::models::{contig::Contig, region::Region};
use rust_htslib::bam;
use rust_htslib::bam::ext::BamRecordExtensions;
use rust_htslib::bam::{Header, IndexedReader, Read, Record};
use std::collections::{BTreeMap, HashMap};
use url::Url;

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

    /// The left most position of alignment segments that are loaded.
    /// 1-based, inclusive.
    track_most_left_bound: usize,

    /// The right most position of alignment segments that are loaded.
    /// 1-based, inclusive.
    track_most_right_bound: usize,

    /// The leftmost position in each alignment track.
    /// 1-based, inclusive.
    track_left_bounds: Vec<usize>,

    /// The rightmost position in each alignment track.
    /// 1-based, inclusive.
    track_right_bounds: Vec<usize>,
}

impl Alignment {
    fn new(contig: &Contig) -> Self {
        Self {
            reads: Vec::new(),
            coverage: BTreeMap::new(),
            track_left_bounds: Vec::new(),
            track_right_bounds: Vec::new(),
            contig: contig.clone(),
            track_most_left_bound: 0,
            track_most_right_bound: 0,
            data_complete_left_bound: 0,
            data_complete_right_bound: 0,
        }
    }

    pub fn from_bam_path(
        bam_path: &String,
        bai_path: Option<&String>,
        region: &Region,
    ) -> Result<Self, TGVError> {
        let is_remote_path = is_url(bam_path);
        let mut bam = match bai_path {
            Some(bai_path) => {
                if is_remote_path {
                    return Err(TGVError::IOError(
                        "Remote BAM files are not supported yet.".to_string(),
                    ));
                }
                IndexedReader::from_path_and_index(bam_path, bai_path)
                    .map_err(|e| TGVError::IOError(e.to_string()))?
            }
            None => {
                if is_remote_path {
                    IndexedReader::from_url(
                        &Url::parse(bam_path).map_err(|e| TGVError::IOError(e.to_string()))?,
                    )
                    .unwrap()
                } else {
                    IndexedReader::from_path(bam_path)
                        .map_err(|e| TGVError::IOError(e.to_string()))?
                }
            }
        };

        let header = bam::Header::from_template(bam.header());

        let query_contig_string = Self::get_query_contig_string(&header, region)?;
        bam.fetch((
            &query_contig_string,
            region.start as i32 - 1,
            region.end as i32,
        ))
        .map_err(|e| TGVError::IOError(e.to_string()))?;

        let mut alignment = Self::new(&region.contig);
        let mut coverage_hashmap: HashMap<usize, usize> = HashMap::new(); // First use a hashmap to store coverage, then convert to BTreeMap

        for record in bam.records() {
            let read = record.map_err(|e| TGVError::IOError(e.to_string()))?;
            alignment.add_read(read);
            let aligned_read = alignment.reads.last().unwrap();

            // update coverage hashmap
            for i in aligned_read.range() {
                // TODO: check exclusivity here
                *coverage_hashmap.entry(i).or_insert(1) += 1;
            }
        }

        // Convert hashmap to BTreeMap
        for (k, v) in coverage_hashmap {
            *alignment.coverage.entry(k).or_insert(v) += v;
        }

        alignment.data_complete_left_bound = region.start;
        alignment.data_complete_right_bound = region.end;

        Ok(alignment)
    }

    /// Get the query string for a region.
    /// Look through the header to decide if the bam file chromosome names are abbreviated or full.
    fn get_query_contig_string(header: &Header, region: &Region) -> Result<String, TGVError> {
        let full_chromsome_str = region.contig.full_name();
        let abbreviated_chromsome_str = region.contig.abbreviated_name();

        for (_key, records) in header.to_hashmap().iter() {
            for record in records {
                if record.contains_key("SN") {
                    let reference_name = record["SN"].to_string();
                    if reference_name == full_chromsome_str {
                        return Ok(full_chromsome_str);
                    }

                    if reference_name == abbreviated_chromsome_str {
                        return Ok(abbreviated_chromsome_str);
                    }
                }
            }
        }

        Err(TGVError::IOError("Contig not found in header".to_string()))
    }
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
        self.track_left_bounds.len()
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

/// Read stacking
impl Alignment {
    const MIN_HORIZONTAL_GAP_BETWEEN_READS: usize = 3;

    fn find_track(&mut self, read_start: usize, read_end: usize) -> usize {
        if self.reads.is_empty() {
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

        self.depth()
    }

    /// Add a read to the alignment. Note that this function does not update coverage.

    fn add_read(&mut self, read: Record) {
        let read_start = read.pos() as usize + 1;
        let read_end = read.reference_end() as usize;
        let leading_softclips = read.cigar().leading_softclips() as usize;
        let trailing_softclips = read.cigar().trailing_softclips() as usize;
        // read.pos() in htslib: 0-based, inclusive, excluding leading hardclips and softclips
        // read.reference_end() in htslib: 0-based, exclusive, excluding trailing hardclips and softclips

        let y = self.find_track(read_start.saturating_sub(leading_softclips), read_end.saturating_add(trailing_softclips));

        let aligned_read = AlignedRead {
            read,
            start: read_start,
            end: read_end,
            leading_softclips,
            trailing_softclips,
            y,
        };

        // Track bounds + depth update
        if self.reads.is_empty() || aligned_read.y >= self.track_left_bounds.len() {
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

        // Add to reads
        self.reads.push(aligned_read);
    }
}


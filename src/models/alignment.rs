use crate::models::{contig::Contig, region::Region};
use noodles_bam as bam;
use noodles_core::Region as NoodlesRegion;
use noodles_sam::alignment::record::cigar::op::Kind;
use noodles_sam::header::Header;
use std::collections::{BTreeMap, HashMap};
use std::io;

/// An aligned read with viewing coordinates.
pub struct AlignedRead {
    /// Alignment record data
    pub read: bam::Record,

    /// Start genome coordinate on the alignment view.
    /// 1-based, inclusive. Same as noodles_bam.
    pub start: usize,

    /// End genome coordinate on the alignment view.
    /// Note that this includes the soft-clipped reads and differ from the built-in methods. TODO
    /// 1-based, inclusive. Same as noodles region parsing.
    pub end: usize,

    /// Y coordinate in the alignment view
    /// 0-based.
    pub y: usize,
}

impl AlignedRead {
    /// Return an 1-based range iterator that includes all bases of the alignment.
    pub fn range(&self) -> impl Iterator<Item = usize> {
        self.start..self.end + 1
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

    pub fn from_bam_path(bam_path: &String, region: &Region) -> io::Result<Self> {
        let mut reader = bam::io::indexed_reader::Builder::default().build_from_path(bam_path)?;
        let header = reader.read_header()?;

        let query_str = Self::get_query_str(&header, region)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Invalid region format"))?;
        let noodles_region: NoodlesRegion = query_str
            .parse()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?; // noodles region

        let records = reader.query(&header, &noodles_region)?;
        Alignment::from_records(records, region)
    }

    pub fn from_records<I>(records: I, region: &Region) -> io::Result<Self>
    where
        I: Iterator<Item = io::Result<bam::Record>>,
    {
        let mut alignment = Self::new(&region.contig);
        let mut coverage_hashmap: HashMap<usize, usize> = HashMap::new(); // First use a hashmap to store coverage, then convert to BTreeMap

        for record in records {
            let read = record?;
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
    fn get_query_str(header: &Header, region: &Region) -> Result<String, ()> {
        let full_chromsome_str = region.contig.full_name();
        let abbreviated_chromsome_str = region.contig.abbreviated_name();

        for (reference_name, _) in header.reference_sequences().iter() {
            if *reference_name == full_chromsome_str {
                return Ok(format!(
                    "{}:{}-{}",
                    full_chromsome_str, region.start, region.end
                ));
            }

            if *reference_name == abbreviated_chromsome_str {
                return Ok(format!(
                    "{}:{}-{}",
                    abbreviated_chromsome_str, region.start, region.end
                ));
            }
        }

        Err(())
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
    pub fn mean_basewise_coverage_in(&self, left: usize, right: usize) -> Result<usize, ()> {
        if right < left {
            panic!("{}, {}", left, right);
            return Err(());
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

    /// Add a read to the alignment. Note that this function does not update coverage.
    fn find_track(&mut self, read_start: usize, read_end: usize) -> usize {
        if self.reads.is_empty() {
            return 0;
        }

        for (y, left_bound) in self.track_left_bounds.iter().enumerate() {
            if read_end <= left_bound.saturating_sub(Self::MIN_HORIZONTAL_GAP_BETWEEN_READS) {
                return y;
            }
        }

        for (y, right_bound) in self.track_right_bounds.iter().enumerate() {
            if read_start >= right_bound.saturating_add(Self::MIN_HORIZONTAL_GAP_BETWEEN_READS) {
                return y;
            }
        }

        self.depth()
    }

    fn add_read(&mut self, read: bam::Record) {
        let read_start = read.alignment_start().unwrap().unwrap().get();
        let read_end = read_start + self.get_alignment_length(&read) - 1;
        let y = self.find_track(read_start, read_end);

        let aligned_read = AlignedRead {
            read,
            start: read_start,
            end: read_end,
            y,
        };

        // Track bounds + depth update
        if self.reads.is_empty() || aligned_read.y >= self.track_left_bounds.len() {
            // Add to a new track
            self.track_left_bounds.push(aligned_read.start);
            self.track_right_bounds.push(aligned_read.end);
        } else {
            // Add to an existing track
            if aligned_read.start < self.track_left_bounds[aligned_read.y] {
                self.track_left_bounds[aligned_read.y] = aligned_read.start;
            }
            if aligned_read.end > self.track_right_bounds[aligned_read.y] {
                self.track_right_bounds[aligned_read.y] = aligned_read.end;
            }
        }

        // Most left/right bound update
        if aligned_read.start < self.track_most_left_bound {
            self.track_most_left_bound = aligned_read.start;
        }
        if aligned_read.end > self.track_most_right_bound {
            self.track_most_right_bound = aligned_read.end;
        }

        // Add to reads
        self.reads.push(aligned_read);
    }

    fn get_alignment_length(&self, read: &bam::Record) -> usize {
        // See: https://samtools.github.io/hts-specs/SAMv1.pdf
        let mut len = 0;
        for op in read.cigar().iter() {
            let op = op.unwrap();
            match op.kind() {
                Kind::Insertion | Kind::HardClip | Kind::Pad => continue,
                Kind::Deletion | Kind::Skip => len += op.len(),
                Kind::Match | Kind::SequenceMatch | Kind::SequenceMismatch => len += op.len(),
                Kind::SoftClip => len += op.len(),
            }
        }
        len
    }
}

use crate::{contig_header::ContigHeader, error::TGVError};
use std::collections::HashMap;

pub trait GenomeInterval {
    fn contig_index(&self) -> usize;
    fn start(&self) -> usize;
    fn end(&self) -> usize;
    fn length(&self) -> usize {
        self.end() - self.start() + 1
    }

    fn covers(&self, position: usize) -> bool {
        self.start() <= position && self.end() >= position
    }

    fn overlaps(&self, other: &impl GenomeInterval) -> bool {
        self.contig_index() == other.contig_index()
            && self.start() <= other.end()
            && self.end() >= other.start()
    }

    fn contains(&self, other: &impl GenomeInterval) -> bool {
        self.contig_index() == other.contig_index()
            && self.start() <= other.start()
            && self.end() >= other.end()
    }

    // The region ends at the end of the genome. Inclusive.
    fn is_properly_bounded(&self, end: Option<usize>) -> bool {
        match end {
            Some(e) => self.start() <= self.end() && self.end() <= e,
            None => self.start() <= self.end(),
        }
    }

    fn middle(&self) -> usize {
        (self.start() + self.end()).div_ceil(2)
    }
}

#[derive(Debug, Clone)]
pub struct SortedIntervalCollection<T: GenomeInterval> {
    /// Assumption: sorted by (contig, start, end)
    pub intervals: Vec<T>,

    /// {contig_name: [variant_indexes,... ]}
    contig_lookup: HashMap<usize, Vec<usize>>,
}

/// This is now O(N) for overlapping lookup. There are data structures for faster lookup, but TGV doesn't work with large interval collections.
/// So O(N) might be ok or even faster.
/// The interval tree data structure:
/// - https://github.com/dcjones/coitrees
/// - https://github.com/sstadick/rust-lapper
/// - https://crates.io/crates/intervaltree
/// - https://github.com/rust-bio/rust-bio/blob/master/src/data_structures/interval_tree/avl_interval_tree.rs
impl<T> SortedIntervalCollection<T>
where
    T: GenomeInterval,
{
    pub fn new(intervals: Vec<T>) -> Result<Self, TGVError> {
        let mut contig_lookup: HashMap<usize, Vec<usize>> = HashMap::new();

        for (i, interval) in intervals.iter().enumerate() {
            contig_lookup
                .entry(interval.contig_index())
                .and_modify(|indexes| indexes.push(i))
                .or_insert(vec![i]);
        }

        Ok(SortedIntervalCollection {
            intervals,
            contig_lookup,
        })
    }

    /// Get intervals overlapping a region.
    pub fn overlapping(&self, region: &Region) -> Result<Vec<&T>, TGVError> {
        let indexes = match self.contig_lookup.get(&region.contig_index()) {
            Some(indexes) => indexes,
            None => return Ok(Vec::new()),
        };

        Ok(indexes
            .iter()
            .filter_map(|i| {
                if self.intervals[*i].overlaps(region) {
                    Some(&self.intervals[*i])
                } else {
                    None
                }
            })
            .collect::<Vec<&T>>())
    }
}

/// A genomic region.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    /// contig id. Need to read the header for full contig string name.
    pub contig_index: usize,

    /// Start coordinate of a genome region.
    /// 1-based, inclusive.
    pub start: usize,

    /// End coordinate of a genome region.
    /// 1-based, inclusive.
    pub end: usize,
}

impl GenomeInterval for Region {
    fn start(&self) -> usize {
        self.start
    }

    fn end(&self) -> usize {
        self.end
    }

    fn contig_index(&self) -> usize {
        self.contig_index
    }
}

impl Region {
    pub fn to_bam_region_str(&self, header: &ContigHeader) -> Option<String> {
        header.get_bam_name(self.contig_index).ok().map(|s| {
            format!(
                "{}:{}-{}",
                s, // TODO: implement the bam name lookup
                self.start,
                self.end
            )
        })
    }
}

impl Region {
    /// Width of a genome region.
    pub fn width(&self) -> usize {
        self.length()
    }
}

use crate::{error::TGVError, region::Region};
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

    #[allow(dead_code)]
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
    #[allow(dead_code)]
    fn is_properly_bounded(&self, end: Option<usize>) -> bool {
        match end {
            Some(e) => self.start() <= self.end() && self.end() <= e,
            None => self.start() <= self.end(),
        }
    }

    #[allow(dead_code)]
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

/// TODO:
/// This is now O(1) for overlapping lookup.
/// Use interval tree to get O(log n) lookup. Options:
/// - https://github.com/BurntSushi/rust-interval-tree
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

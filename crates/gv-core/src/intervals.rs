use crate::{contig_header::ContigHeader, error::TGVError};
use noodles::{self, vcf::header::record::value::map::contig};
use std::collections::HashMap;

pub trait GenomeInterval {
    fn contig_index(&self) -> usize;
    fn start(&self) -> u64;
    fn end(&self) -> u64;
    fn length(&self) -> u64 {
        self.end() - self.start() + 1
    }

    fn covers(&self, position: u64) -> bool {
        self.start() <= position && self.end() >= position
    }

    fn middle(&self) -> u64 {
        (self.start() + self.end()) / 2
    }

    fn overlaps(&self, contig_index: usize, start: u64, end: u64) -> bool {
        self.contig_index() == contig_index && self.start() <= end && self.end() >= start
    }

    fn contains(&self, other: &impl GenomeInterval) -> bool {
        self.contig_index() == other.contig_index()
            && self.start() <= other.start()
            && self.end() >= other.end()
    }

    // The region ends at the end of the genome. Inclusive.
    fn is_properly_bounded(&self, end: Option<u64>) -> bool {
        match end {
            Some(e) => self.start() <= self.end() && self.end() <= e,
            None => self.start() <= self.end(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SortedIntervalCollection<T: GenomeInterval> {
    /// Assumption: sorted by (contig, start, end)
    pub intervals: Vec<T>,

    /// {contig_name: [variant_indexes,... ]}
    contig_lookup: HashMap<usize, Vec<usize>>,
}

impl<T> Default for SortedIntervalCollection<T>
where
    T: GenomeInterval,
{
    fn default() -> Self {
        Self {
            intervals: Vec::new(),
            contig_lookup: HashMap::new(),
        }
    }
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
    pub fn overlapping(
        &self,
        contig_index: usize,
        start: u64,
        end: u64,
    ) -> Result<Vec<&T>, TGVError> {
        let indexes = match self.contig_lookup.get(&contig_index) {
            Some(indexes) => indexes,
            None => return Ok(Vec::new()),
        };

        Ok(indexes
            .iter()
            .filter_map(|i| {
                if self.intervals[*i].overlaps(contig_index, start, end) {
                    Some(&self.intervals[*i])
                } else {
                    None
                }
            })
            .collect::<Vec<&T>>())
    }
}

/// A genomic region.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Region {
    /// contig id. Need to read the header for full contig string name.
    pub focus: Focus,

    /// End coordinate of a genome region.
    /// 1-based, inclusive.
    pub half_width: u64,
}

impl GenomeInterval for Region {
    fn start(&self) -> u64 {
        self.focus.position.saturating_sub(self.half_width)
    }

    fn end(&self) -> u64 {
        self.focus.position + self.half_width
    }

    fn contig_index(&self) -> usize {
        self.focus.contig_index
    }

    // override
    fn middle(&self) -> u64 {
        self.focus.position
    }

    /// Width of a genome region.
    // override
    fn length(&self) -> u64 {
        self.half_width + 2 + 1
    }
}

impl Region {
    pub fn move_to(self, position: u64) -> Self {
        Self {
            focus: self.focus.move_to(position),
            half_width: self.half_width,
        }
    }

    pub fn alignment(
        &self,
        header: &ContigHeader,
    ) -> Result<Option<noodles::core::Region>, TGVError> {
        let start = noodles::core::Position::try_from(self.start() as usize).map_err(|_| {
            TGVError::StateError(format!("Failed to convert to alignment region: {:?}", self))
        })?;
        let end = noodles::core::Position::try_from(self.end() as usize).map_err(|_| {
            TGVError::StateError(format!("Failed to convert to alignment region: {:?}", self))
        })?;
        Ok(header
            .try_get(self.contig_index())?
            .get_alignment_name()
            .map(|name| noodles::core::Region::new(name, start..=end)))
    }

    pub fn noodles_sequence(
        &self,
        header: &ContigHeader,
    ) -> Result<Option<noodles::core::Region>, TGVError> {
        let start = noodles::core::Position::try_from(self.start() as usize).map_err(|_| {
            TGVError::StateError(format!(
                "Failed to convert to noodles sequence region: {:?}",
                self
            ))
        })?;
        let end = noodles::core::Position::try_from(self.end() as usize).map_err(|_| {
            TGVError::StateError(format!(
                "Failed to convert to noodles sequence region: {:?}",
                self
            ))
        })?;
        Ok(header
            .try_get(self.contig_index())?
            .get_sequence_name()
            .map(|name| noodles::core::Region::new(name, start..=end)))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Focus {
    pub contig_index: usize,

    pub position: u64,
}

impl Focus {
    pub fn move_to(self, position: u64) -> Self {
        Self {
            contig_index: self.contig_index,
            position,
        }
    }

    pub fn move_left(self, n: u64) -> Self {
        Self {
            contig_index: self.contig_index,
            position: self.position.saturating_sub(n),
        }
    }

    pub fn move_right(self, n: u64) -> Self {
        Self {
            contig_index: self.contig_index,
            position: self.position.saturating_add(n),
        }
    }
}

use crate::{contig::Contig, error::TGVError, region::Region};
use std::ops::Bound::{Excluded, Included};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    hash::Hash,
};

pub trait GenomeInterval {
    fn contig(&self) -> &Contig;
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
        self.contig() == other.contig()
            && self.start() <= other.end()
            && self.end() >= other.start()
    }

    fn contains(&self, other: &impl GenomeInterval) -> bool {
        self.contig() == other.contig()
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

pub struct SortedIntervalCollection<T: GenomeInterval> {
    /// Assumption: sorted by (contig, start, end)
    pub intervals: Vec<T>,

    /// {contig_name: ({start: [variant_indexes,... ], ...}, {end: [variant_indexes,... ], ...}}
    start_end_lookup: HashMap<String, (BTreeMap<usize, Vec<usize>>, BTreeMap<usize, Vec<usize>>)>,
}

impl<T> SortedIntervalCollection<T>
where
    T: GenomeInterval,
{
    pub fn new(intervals: Vec<T>) -> Result<Self, TGVError> {
        let mut start_end_lookup: HashMap<
            String,
            (BTreeMap<usize, Vec<usize>>, BTreeMap<usize, Vec<usize>>),
        > = HashMap::new();

        for (i, interval) in intervals.iter().enumerate() {
            start_end_lookup
                .entry(interval.contig().name.clone())
                .and_modify(|(start_map, end_map)| {
                    start_map
                        .entry(interval.start())
                        .and_modify(|indexes| indexes.push(i))
                        .or_insert(vec![i]);
                    end_map
                        .entry(interval.end())
                        .and_modify(|indexes| indexes.push(i))
                        .or_insert(vec![i]);
                })
                .or_insert((
                    BTreeMap::from([(interval.start(), vec![i])]),
                    BTreeMap::from([(interval.start(), vec![i])]),
                ));
        }
        return Ok(SortedIntervalCollection {
            intervals,
            start_end_lookup,
        });
    }

    /// Get intervals overlapping a region.
    pub fn overlapping(&self, region: &Region) -> Result<Vec<&T>, TGVError> {
        let (start_map, end_map) = match self.start_end_lookup.get(&region.contig.name) {
            Some((start_map, end_map)) => (start_map, end_map),
            None => return Ok(Vec::new()),
        };

        let mut interval_indices: BTreeSet<usize> = BTreeSet::new();

        for (_, indexes) in start_map.range((Included(region.start), Included(region.end))) {
            for i in indexes {
                interval_indices.insert(*i);
            }
        }

        for (_, indexes) in end_map.range((Included(region.start), Included(region.end))) {
            for i in indexes {
                interval_indices.insert(*i);
            }
        }

        //for end_map.range(Included(region.start), Included(region.end))

        Ok(interval_indices
            .iter()
            .map(|i| &self.intervals[*i])
            .collect::<Vec<&T>>())
    }
}

use crate::{
    error::TGVError,
    feature::{Gene, SubGeneFeature},
    intervals::{GenomeInterval, Region},
};

use std::collections::{BTreeMap, HashMap};
use std::ops::Bound::{Excluded, Included};

// A track is a collections of features on a single contig.
#[derive(Debug)]
pub struct Track<T: GenomeInterval> {
    pub features: Vec<T>, // TODO: what about hierarchy in features? e.g. exons of a gene?
    pub contig_index: usize,

    features_by_start: BTreeMap<u64, usize>, // start -> index in features
    features_by_end: BTreeMap<u64, usize>,   // end -> index in features

    /// Left bound
    /// 1-based, inclusive.
    most_left_bound: u64,

    /// Right bound
    /// 1-based, exclusive.
    most_right_bound: u64,

    /// Only for Track<Gene>
    /// (i, j) -> exon [self.genes[i].exon_starts[j], self.genes[i].exon_ends[j]]
    exons_by_start: BTreeMap<u64, (usize, usize)>,
    exons_by_end: BTreeMap<u64, (usize, usize)>,

    /// feature name -> index in features
    feature_lookup: HashMap<String, usize>,
}

impl<T: GenomeInterval> Default for Track<T> {
    fn default() -> Self {
        Self {
            features: Vec::<T>::new(),
            contig_index: 0,
            features_by_start: BTreeMap::new(),
            features_by_end: BTreeMap::new(),

            most_left_bound: u64::MAX,
            most_right_bound: u64::MIN,

            exons_by_start: BTreeMap::new(),
            exons_by_end: BTreeMap::new(),
            feature_lookup: HashMap::new(),
        }
    }
}

impl<T: GenomeInterval> Track<T> {
    /// Create a track from a list of features.
    /// Assumes no feature overlapping.
    pub fn from_features(features: Vec<T>, contig_index: usize) -> Result<Self, TGVError> {
        let mut features = features;
        features.sort_by_key(|feature| feature.start());

        let mut features_by_start = BTreeMap::new();
        let mut features_by_end = BTreeMap::new();

        let mut most_left_bound = u64::MAX;
        let mut most_right_bound = u64::MIN;

        for (i_feature, feature) in features.iter().enumerate() {
            if feature.start() < most_left_bound {
                most_left_bound = feature.start();
            }
            if feature.end() > most_right_bound {
                most_right_bound = feature.end();
            }

            features_by_start.insert(feature.start(), i_feature);
            features_by_end.insert(feature.end(), i_feature);
        }

        Ok(Self {
            features,
            contig_index,
            features_by_start,
            features_by_end,
            // data_complete_left_bound: data_complete_left_bound,
            // data_complete_right_bound: data_complete_right_bound,
            most_left_bound,
            most_right_bound,
            exons_by_start: BTreeMap::new(),
            exons_by_end: BTreeMap::new(),
            feature_lookup: HashMap::new(),
        })
    }

    pub fn is_empty(&self) -> bool {
        self.features.is_empty()
    }

    /// Check if the track has complete data in [left, right].
    /// Note that this is assuming that the track has complete data.
    /// left: 1-based, inclusive.
    /// right: 1-based, exclusive.
    pub fn has_complete_data(&self, region: &Region) -> bool {
        self.contains(region)
    }
}

impl<T: GenomeInterval> GenomeInterval for Track<T> {
    fn start(&self) -> u64 {
        self.most_left_bound
    }

    fn end(&self) -> u64 {
        self.most_right_bound
    }

    fn contig_index(&self) -> usize {
        self.contig_index
    }
}

impl<T: GenomeInterval> Track<T> {
    /// Get the feature covering a given position.
    /// position: 1-based.
    pub fn get_feature_at(&self, position: u64) -> Option<&T> {
        if !self.covers(position) {
            return None;
        }
        let range_right = self.features_by_end.range(position..).next();
        let range_left = self.features_by_start.range(0..=position).next_back();

        match (range_left, range_right) {
            (Some((_, start_index)), Some((_, end_index))) => {
                if start_index == end_index {
                    Some(&self.features[*start_index])
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    pub fn get_features_overlapping(&self, region: &Region) -> Vec<&T> {
        // region.end between [start, end] or region.start between [start, end]

        let mut features: Vec<&T> = self
            .features_by_end
            .range((Included(region.start()), Excluded(region.end())))
            .map(|(_, index)| &self.features[*index])
            .collect();

        // feature overlapping region.end
        if let Some(feature) = self.get_feature_at(region.end()) {
            features.push(feature);
        } else {
            //panic!("{}, {}, {}, {}", region.start(), region.end(), features.len(), features.last().unwrap().end());
        }

        features
    }

    pub fn get_k_features_before(&self, position: u64, k: usize) -> Option<&T> {
        if k == 0 {
            return self.get_feature_at(position);
        }

        if self.features.is_empty() {
            return None;
        }

        if position < self.most_left_bound {
            return None;
        }

        let range = self
            .features_by_end
            .range((Included(0), Excluded(position)))
            .nth_back(k - 1);

        match range {
            Some((_, index)) => Some(&self.features[*index]),
            _ => None,
        }
    }

    pub fn get_k_features_after(&self, position: u64, k: usize) -> Option<&T> {
        if k == 0 {
            return self.get_feature_at(position);
        }

        if self.features.is_empty() {
            return None;
        }

        if position > self.most_right_bound {
            return None;
        }

        let range = self.features_by_start.range(position..).nth(k - 1);

        match range {
            Some((_, index)) => Some(&self.features[*index]),
            _ => None,
        }
    }

    pub fn get_saturating_k_features_after(&self, position: u64, k: usize) -> Option<&T> {
        if k == 0 {
            return None;
        }

        if self.features.is_empty() {
            return None;
        }

        match self.get_k_features_after(position, k) {
            Some(feature) => Some(feature),
            _ => Some(self.features.last().unwrap()),
        }
    }

    pub fn get_saturating_k_features_before(&self, position: u64, k: usize) -> Option<&T> {
        if k == 0 {
            return None;
        }

        if self.features.is_empty() {
            return None;
        }

        match self.get_k_features_before(position, k) {
            Some(feature) => Some(feature),
            _ => Some(self.features.first().unwrap()),
        }
    }

    pub fn get_features_between(&self, start: u64, end: u64) -> Vec<&T> {
        self.features_by_start
            .range(start..=end)
            .filter(|(_, index)| self.features[**index].end() <= end)
            .map(|(_, index)| &self.features[*index])
            .collect()
    }
}

impl Track<Gene> {
    /// Alias for the features field.
    pub fn genes(&self) -> &Vec<Gene> {
        &self.features
    }

    pub fn gene_by_name(&self, gene_name: &str) -> Option<&Gene> {
        match self.feature_lookup.get(gene_name) {
            None => None,
            Some(index) => self.features.get(*index),
        }
    }

    pub fn from_genes(genes: Vec<Gene>, contig_index: usize) -> Result<Self, TGVError> {
        let mut genes = genes;
        genes.sort_by_key(|gene| gene.start());

        let mut features_by_start = BTreeMap::new();
        let mut features_by_end = BTreeMap::new();
        let mut most_left_bound = u64::MAX;
        let mut most_right_bound = u64::MIN;

        let mut exons_by_start = BTreeMap::new();
        let mut exons_by_end = BTreeMap::new();

        for (i_gene, gene) in genes.iter().enumerate() {
            if gene.start() < most_left_bound {
                most_left_bound = gene.start();
            }
            if gene.end() > most_right_bound {
                most_right_bound = gene.end();
            }

            for i in 0..gene.n_exons() {
                exons_by_start.insert(gene.exon_starts[i], (i_gene, i));
                exons_by_end.insert(gene.exon_ends[i], (i_gene, i));
            }

            features_by_start.insert(gene.start(), i_gene);
            features_by_end.insert(gene.end(), i_gene);
        }

        Ok(Self {
            features: genes,
            contig_index,
            features_by_start,
            features_by_end,
            most_left_bound,
            most_right_bound,
            exons_by_start: exons_by_start,
            exons_by_end: exons_by_end,
            feature_lookup: HashMap::new(),
        })
    }

    /// Alias for get_feature_at when the track is a gene track.
    pub fn get_gene_at(&self, position: u64) -> Option<&Gene> {
        self.get_feature_at(position)
    }

    /// Alias for get_k_features_before when the track is a gene track.
    pub fn get_k_genes_before(&self, position: u64, k: usize) -> Option<&Gene> {
        self.get_k_features_before(position, k)
    }

    /// Alias for get_k_features_after when the track is a gene track.
    pub fn get_k_genes_after(&self, position: u64, k: usize) -> Option<&Gene> {
        self.get_k_features_after(position, k)
    }

    /// Alias for get_saturating_k_features_after when the track is a gene track.
    pub fn get_saturating_k_genes_after(&self, position: u64, k: usize) -> Option<&Gene> {
        self.get_saturating_k_features_after(position, k)
    }

    /// Alias for get_saturating_k_features_before when the track is a gene track.
    pub fn get_saturating_k_genes_before(&self, position: u64, k: usize) -> Option<&Gene> {
        self.get_saturating_k_features_before(position, k)
    }

    /// Alias for get_features_between when the track is a gene track.
    pub fn get_genes_between(&self, start: u64, end: u64) -> Vec<&Gene> {
        self.get_features_between(start, end)
    }

    /// position: 1-based.
    pub fn get_exon_at(&self, position: u64) -> Option<SubGeneFeature> {
        let range_end = self.exons_by_end.range(position..).next();
        let range_start = self.exons_by_start.range(0..=position).next_back();

        match (range_start, range_end) {
            (
                Some((_, (start_gene_idx, start_exon_idx))),
                Some((_, (end_gene_idx, end_exon_idx))),
            ) => {
                if start_gene_idx == end_gene_idx && start_exon_idx == end_exon_idx {
                    Some(
                        self.features[*start_gene_idx]
                            .get_exon(*start_exon_idx)
                            .unwrap(),
                    )
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    pub fn get_k_exons_before(&self, position: u64, k: usize) -> Option<SubGeneFeature> {
        if k == 0 {
            return self.get_exon_at(position);
        }

        if self.features.is_empty() {
            return None;
        }

        if position < self.most_left_bound {
            return None;
        }

        let range = self.exons_by_end.range(0..=position).nth_back(k - 1);

        match range {
            Some((_, (gene_idx, exon_idx))) => {
                Some(self.features[*gene_idx].get_exon(*exon_idx).unwrap())
            }
            _ => None,
        }
    }

    pub fn get_k_exons_after(&self, position: u64, k: usize) -> Option<SubGeneFeature> {
        if k == 0 {
            return self.get_exon_at(position);
        }

        if self.features.is_empty() {
            return None;
        }

        if position > self.most_right_bound {
            return None;
        }

        let range = self.exons_by_start.range(position..).nth(k - 1);

        match range {
            Some((_, (gene_idx, exon_idx))) => {
                Some(self.features[*gene_idx].get_exon(*exon_idx).unwrap())
            }
            _ => None,
        }
    }

    pub fn get_saturating_k_exons_after(&self, position: u64, k: usize) -> Option<SubGeneFeature> {
        if k == 0 {
            return None;
        }

        if self.exons_by_start.is_empty() {
            return None;
        }

        match self.get_k_exons_after(position, k) {
            Some(exon) => Some(exon),
            _ => {
                let (_, (gene_idx, exon_idx)) = self.exons_by_start.iter().last().unwrap();
                Some(self.features[*gene_idx].get_exon(*exon_idx).unwrap())
            }
        }
    }

    pub fn get_saturating_k_exons_before(&self, position: u64, k: usize) -> Option<SubGeneFeature> {
        if k == 0 {
            return None;
        }

        if self.exons_by_start.is_empty() {
            return None;
        }

        match self.get_k_exons_before(position, k) {
            Some(exon) => Some(exon),
            _ => {
                let (_, (gene_idx, exon_idx)) = self.exons_by_start.iter().next().unwrap();
                Some(self.features[*gene_idx].get_exon(*exon_idx).unwrap())
            }
        }
    }
}

#[cfg(test)]
mod tests {

    use crate::strand::Strand;

    /// Test track: [gene1: [2,5], [8,10]], [gene_no_exon (21-30)], [gene2: [41,50]]
    fn get_test_track() -> Track<Gene> {
        let genes = vec![
            Gene {
                id: "gene1".to_string(),
                name: "gene1".to_string(),
                strand: Strand::Forward,
                contig_index: 0,
                transcription_start: 2,
                transcription_end: 10,
                cds_start: 2,
                cds_end: 10,
                exon_starts: vec![2, 8],
                exon_ends: vec![5, 10],
                has_exons: true,
            },
            Gene {
                id: "gene_no_exon".to_string(),
                name: "gene_no_exon".to_string(),
                strand: Strand::Forward,
                contig_index: 0,
                transcription_start: 21,
                transcription_end: 30,
                cds_start: 25,
                cds_end: 25,
                exon_starts: vec![],
                exon_ends: vec![],
                has_exons: false,
            },
            Gene {
                id: "gene2".to_string(),
                name: "gene2".to_string(),
                strand: Strand::Forward,
                contig_index: 0,
                transcription_start: 41,
                transcription_end: 50,
                cds_start: 45,
                cds_end: 50,
                exon_starts: vec![41],
                exon_ends: vec![50],
                has_exons: true,
            },
        ];

        Track::from_genes(genes, 0).unwrap()
    }

    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case(1, None)]
    #[case(2, Some("gene1"))]
    #[case(5, Some("gene1"))]
    #[case(10, Some("gene1"))]
    #[case(42, Some("gene2"))]
    #[case(51, None)]
    fn test_get_genes_at(#[case] position: u64, #[case] expected: Option<&str>) {
        let track = get_test_track();
        match expected {
            Some(gene_name) => assert_eq!(track.get_feature_at(position).unwrap().name, gene_name),
            None => assert!(track.get_feature_at(position).is_none()),
        }
    }

    #[rstest]
    #[case(2, 0, Some("gene1"))]
    #[case(2, 1, None)]
    #[case(11, 1, Some("gene1"))]
    #[case(35, 1, Some("gene_no_exon"))]
    #[case(51, 0, None)]
    #[case(51, 1, Some("gene2"))]
    fn test_get_k_genes_before(
        #[case] position: u64,
        #[case] k: usize,
        #[case] expected: Option<&str>,
    ) {
        let track = get_test_track();
        match expected {
            Some(gene_name) => assert_eq!(
                track.get_k_features_before(position, k).unwrap().name,
                gene_name
            ),
            None => assert!(track.get_k_features_before(position, k).is_none()),
        }
    }

    #[rstest]
    #[case(2, 0, Some("gene1"))]
    #[case(2, 1, Some("gene_no_exon"))]
    #[case(2, 2, Some("gene2"))]
    #[case(2, 3, None)]
    #[case(11, 1, Some("gene_no_exon"))]
    #[case(51, 1, None)]
    #[case(1, 1, Some("gene1"))]
    #[case(1, 0, None)]
    fn test_get_k_genes_after(
        #[case] position: u64,
        #[case] k: usize,
        #[case] expected: Option<&str>,
    ) {
        let track = get_test_track();
        match expected {
            Some(gene_name) => assert_eq!(
                track.get_k_features_after(position, k).unwrap().name,
                gene_name
            ),
            None => assert!(track.get_k_features_after(position, k).is_none()),
        }
    }

    #[rstest]
    #[case(1, None)]
    #[case(5, Some(2))]
    #[case(15, None)]
    #[case(25, None)]
    #[case(51, None)]
    fn test_get_exon_at(#[case] position: u64, #[case] expected: Option<u64>) {
        let track = get_test_track();
        match expected {
            Some(exon_idx) => assert_eq!(track.get_exon_at(position).unwrap().start(), exon_idx),
            None => assert!(track.get_exon_at(position).is_none()),
        }
    }

    #[rstest]
    #[case(1, 0, None)]
    #[case(2, 0, Some(2))]
    #[case(2, 1, None)]
    #[case(35, 1, Some(8))]
    #[case(51, 1, Some(41))]
    #[case(51, 2, Some(8))]
    fn test_get_k_exons_before(
        #[case] position: u64,
        #[case] k: usize,
        #[case] expected: Option<u64>,
    ) {
        let track = get_test_track();
        match expected {
            Some(exon_idx) => assert_eq!(
                track.get_k_exons_before(position, k).unwrap().start(),
                exon_idx
            ),
            None => assert!(track.get_k_exons_before(position, k).is_none()),
        }
    }

    #[rstest]
    #[case(1, 0, None)]
    #[case(1, 2, Some(8))]
    #[case(2, 0, Some(2))]
    #[case(2, 1, Some(8))]
    #[case(35, 1, Some(41))]
    #[case(35, 2, None)]
    #[case(51, 0, None)]
    #[case(51, 1, None)]
    fn test_get_k_exons_after(
        #[case] position: u64,
        #[case] k: usize,
        #[case] expected: Option<u64>,
    ) {
        let track = get_test_track();
        match expected {
            Some(exon_idx) => assert_eq!(
                track.get_k_exons_after(position, k).unwrap().start(),
                exon_idx
            ),
            None => assert!(track.get_k_exons_after(position, k).is_none()),
        }
    }
}

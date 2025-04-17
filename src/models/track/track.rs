use crate::{
    error::TGVError,
    models::{
        contig::Contig,
        region::Region,
        strand::Strand,
        track::feature::{SubGeneFeature,Gene},
    },
    traits::GenomeInterval,
};


// A track is a collections of features on a single contig.
pub struct Track<T: GenomeInterval> {
    pub features: Vec<T>, // TODO: what about hierarchy in features? e.g. exons of a gene?
    pub contig: Contig,

    // data_complete_left_bound: usize,
    // data_complete_right_bound: usize,
    /// Left bound
    /// 1-based, inclusive.
    most_left_bound: usize,

    /// Right bound
    /// 1-based, exclusive.
    most_right_bound: usize,

    /// Only for Track<Gene>
    /// (i, j) -> exon [self.genes[i].exon_starts[j], self.genes[i].exon_ends[j]]
    exon_indexes: Option<Vec<(usize, usize)>>, 
}

impl<T: GenomeInterval> Track<T> {
    /// Create a track from a list of features.
    /// Assumes no feature overlapping.
    pub fn from_features(features: Vec<T>, contig: Contig) -> Result<Self, TGVError> {
        let mut features = features;
        features.sort_by_key(|feature| feature.start());

        let mut most_left_bound = usize::MAX;
        let mut most_right_bound = usize::MIN;

        for (i_feature, feature) in features.iter().enumerate() {
            if feature.start() < most_left_bound {
                most_left_bound = feature.start();
            }
            if feature.end() > most_right_bound {
                most_right_bound = feature.end();
            }

        }

        Ok(Self {
            features,
            contig,
            // data_complete_left_bound: data_complete_left_bound,
            // data_complete_right_bound: data_complete_right_bound,
            most_left_bound,
            most_right_bound,
            exon_indexes: None,
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
    fn start(&self) -> usize {
        self.most_left_bound
    }

    fn end(&self) -> usize {
        self.most_right_bound
    }

    fn contig(&self) -> &Contig {
        &self.contig
    }
}

impl<T: GenomeInterval> Track<T> {
    /// Get the feature covering a given position.
    /// position: 1-based.
    pub fn get_feature_at(&self, position: usize) -> Option<&T> {
        self.features.iter().find(|&feature| feature.covers(position))
    }

    pub fn get_k_features_before(&self, position: usize, k: usize) -> Option<&T> {
        if k == 0 {
            return self.get_feature_at(position);
        }

        if self.features.is_empty() {
            return None;
        }

        if position < self.most_left_bound {
            return None;
        }

        if position > self.most_right_bound {
            if self.features.len() < k {
                return None;
            }
            let feature_index = self.features.len() - k;
            return Some(&self.features[feature_index]);
        }

        for (i, feature) in self.features.iter().enumerate() {
            if feature.end() < position {
                continue;
            }
            if i < k {
                return None;
            }

            let feature_index = i - k;

            return Some(&self.features[feature_index]);
        }

        None
    }

    pub fn get_k_features_after(&self, position: usize, k: usize) -> Option<&T> {
        if k == 0 {
            return self.get_feature_at(position);
        }

        if self.features.is_empty() {
            return None;
        }

        if position > self.most_right_bound {
            return None;
        }

        if position < self.most_left_bound {
            if self.features.len() < k {
                return None;
            }
            let feature_index = k - 1;
            return Some(&self.features[feature_index]);
        }

        for (i, feature) in self.features.iter().enumerate() {
            if feature.start() <= position {
                continue;
            }

            if i + k > self.features.len() {
                return None;
            }

            let feature_index = i + k - 1;
            return Some(&self.features[feature_index]);
        }

        None
    }

    pub fn get_saturating_k_features_after(&self, position: usize, k: usize) -> Option<&T> {
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

    pub fn get_saturating_k_features_before(&self, position: usize, k: usize) -> Option<&T> {
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
}

impl Track<Gene> {

    pub fn from_genes(genes: Vec<Gene>, contig: Contig) -> Result<Self, TGVError> {
        let mut genes = genes;
        genes.sort_by_key(|gene| gene.start());

        let mut most_left_bound = usize::MAX;
        let mut most_right_bound = usize::MIN;

        let mut exon_indexes = Vec::new();

        for (i_gene, gene) in genes.iter().enumerate() {
            if gene.start() < most_left_bound {
                most_left_bound = gene.start();
            }
            if gene.end() > most_right_bound {
                most_right_bound = gene.end();
            }

            for i in 0..gene.n_exons() {
                exon_indexes.push((i_gene, i));
            }
        }

        Ok(Self {
            features: genes,
            contig,
            // data_complete_left_bound: data_complete_left_bound,
            // data_complete_right_bound: data_complete_right_bound,
            most_left_bound,
            most_right_bound,
            exon_indexes: Some(exon_indexes),
        })
    }

    /// Alias for get_feature_at when the track is a gene track.
    pub fn get_gene_at(&self, position: usize) -> Option<&Gene> {
        self.get_feature_at(position)
    }

    /// Alias for get_k_features_before when the track is a gene track.
    pub fn get_k_genes_before(&self, position: usize, k: usize) -> Option<&Gene> {
        self.get_k_features_before(position, k)
    }

    /// Alias for get_k_features_after when the track is a gene track.
    pub fn get_k_genes_after(&self, position: usize, k: usize) -> Option<&Gene> {
        self.get_k_features_after(position, k)
    }

    /// Alias for get_saturating_k_features_after when the track is a gene track.
    pub fn get_saturating_k_genes_after(&self, position: usize, k: usize) -> Option<&Gene> {
        self.get_saturating_k_features_after(position, k)
    }

    /// Alias for get_saturating_k_features_before when the track is a gene track.
    pub fn get_saturating_k_genes_before(&self, position: usize, k: usize) -> Option<&Gene> {
        self.get_saturating_k_features_before(position, k)
    }

    /// O(1) search for the exon covering a given position.
    /// Inspiration: https://www.youtube.com/watch?v=ig-dtw8Um_k
    /// position: 1-based.
    pub fn get_exon_at(&self, position: usize) -> Option<SubGeneFeature> {
        if let Some(exon_indexes) = &self.exon_indexes {
            for (gene_idx, exon_idx) in exon_indexes.iter() {
                let gene = &self.features[*gene_idx];
            let (exon_start, exon_end) = (gene.exon_starts[*exon_idx], gene.exon_ends[*exon_idx]);

            if exon_start <= position && position <= exon_end {
                    return Some(gene.get_exon(*exon_idx).unwrap());
                }
            }
        }

        None
    }

    pub fn get_k_exons_before(&self, position: usize, k: usize) -> Option<SubGeneFeature> {
        let exon_indexes = self.exon_indexes.as_ref().unwrap();


        if k == 0 {
            return self.get_exon_at(position);
        }

        if self.features.is_empty() {
            return None;
        }

        if position < self.most_left_bound {
            return None;
        }

        if position > self.most_right_bound {
            if exon_indexes.len() < k {
                return None;
            }
            let i_exon = exon_indexes.len() - k;
            let (gene_idx, exon_idx) = exon_indexes[i_exon];
            return Some(self.features[gene_idx].get_exon(exon_idx).unwrap());
        }

        for (i, (gene_idx, exon_idx)) in exon_indexes.iter().enumerate() {
            let gene = &self.features[*gene_idx];
            let (_exon_start, exon_end) = (gene.exon_starts[*exon_idx], gene.exon_ends[*exon_idx]);
            if exon_end < position {
                continue;
            }
            if i < k {
                return None;
            }

            let i_exon = i - k;
            let (gene_idx, exon_idx) = exon_indexes[i_exon];

            return Some(self.features[gene_idx].get_exon(exon_idx).unwrap());
        }

        None
    }

    pub fn get_k_exons_after(&self, position: usize, k: usize) -> Option<SubGeneFeature> {
        let exon_indexes = self.exon_indexes.as_ref().unwrap();

        if k == 0 {
            return self.get_exon_at(position);
        }

        if self.features.is_empty() {
            return None;
        }

        if position > self.most_right_bound {
            return None;
        }

        if position < self.most_left_bound {
            if exon_indexes.len() < k {
                return None;
            }
            let i_exon = k - 1;
            let (gene_idx, exon_idx) = exon_indexes[i_exon];
            return Some(self.features[gene_idx].get_exon(exon_idx).unwrap());
        }

        for (i, (gene_idx, exon_idx)) in exon_indexes.iter().enumerate() {
            let gene = &self.features[*gene_idx];
            let (exon_start, _exon_end) = (gene.exon_starts[*exon_idx], gene.exon_ends[*exon_idx]);
            if exon_start <= position {
                continue;
            }

            if i + k > exon_indexes.len() {
                return None;
            }

            let i_exon = i + k - 1;
            let (gene_idx, exon_idx) = exon_indexes[i_exon];
            return Some(self.features[gene_idx].get_exon(exon_idx).unwrap());
        }

        None
    }

    pub fn get_saturating_k_exons_after(&self, position: usize, k: usize) -> Option<SubGeneFeature> {
        let exon_indexes = self.exon_indexes.as_ref().unwrap();

        if k == 0 {
            return None;
        }

        if exon_indexes.is_empty() {
            return None;
        }

        match self.get_k_exons_after(position, k) {
            Some(exon) => Some(exon),
            _ => {
                let (gene_idx, exon_idx) = exon_indexes[exon_indexes.len() - 1];
                Some(self.features[gene_idx].get_exon(exon_idx).unwrap())
            }
        }
    }

    pub fn get_saturating_k_exons_before(&self, position: usize, k: usize) -> Option<SubGeneFeature> {
        let exon_indexes = self.exon_indexes.as_ref().unwrap();

        if k == 0 {
            return None;
        }

        if let Some(exon_indexes) = &self.exon_indexes {
            if exon_indexes.is_empty() {
                return None;
            }
        }

        match self.get_k_exons_before(position, k) {
            Some(exon) => Some(exon),
            _ => {
                let (gene_idx, exon_idx) = exon_indexes[0];
                Some(self.features[gene_idx].get_exon(exon_idx).unwrap())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    /// Test track: [gene1: [2,5], [8,10]], [gene_no_exon (21-30)], [gene2: [41,50]]
    fn get_test_track() -> Track<Gene> {
        let genes = vec![
            Gene {
                id: "gene1".to_string(),
                name: "gene1".to_string(),
                strand: Strand::Forward,
                contig: Contig::chrom("chr1"),
                transcription_start: 2,
                transcription_end: 10,
                cds_start: 2,
                cds_end: 10,
                exon_starts: vec![2, 8],
                exon_ends: vec![5, 10],
            },
            Gene {
                // should not happen, but just in case
                id: "gene_no_exon".to_string(),
                name: "gene_no_exon".to_string(),
                strand: Strand::Forward,
                contig: Contig::chrom("chr1"),
                transcription_start: 21,
                transcription_end: 30,
                cds_start: 25,
                cds_end: 25,
                exon_starts: vec![],
                exon_ends: vec![],
            },
            Gene {
                id: "gene2".to_string(),
                name: "gene2".to_string(),
                strand: Strand::Forward,
                contig: Contig::chrom("chr1"),
                transcription_start: 41,
                transcription_end: 50,
                cds_start: 45,
                cds_end: 50,
                exon_starts: vec![41],
                exon_ends: vec![50],
            },
        ];

        Track::from_genes(genes, Contig::chrom("chr1")).unwrap()
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
    fn test_get_genes_at(#[case] position: usize, #[case] expected: Option<&str>) {
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
        #[case] position: usize,
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
        #[case] position: usize,
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
    fn test_get_exon_at(#[case] position: usize, #[case] expected: Option<usize>) {
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
        #[case] position: usize,
        #[case] k: usize,
        #[case] expected: Option<usize>,
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
        #[case] position: usize,
        #[case] k: usize,
        #[case] expected: Option<usize>,
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

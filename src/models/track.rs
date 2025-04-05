use crate::{
    error::TGVError,
    models::{contig::Contig, region::Region, strand::Strand},
    traits::GenomeInterval,
};

// A feature is a interval on a contig.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeatureType {
    Exon,
    Intron,
    NonCDSExon,
}

#[derive(Debug, Clone)]
pub struct Feature {
    pub contig: Contig,
    pub start: usize,
    pub end: usize,
    pub feature_type: FeatureType,
}

impl GenomeInterval for Feature {
    fn start(&self) -> usize {
        self.start
    }

    fn end(&self) -> usize {
        self.end
    }

    fn contig(&self) -> &Contig {
        &self.contig
    }
}

#[derive(Debug, Clone)]
pub struct Gene {
    pub id: String,
    pub name: String,
    pub strand: Strand,
    pub contig: Contig,
    pub transcription_start: usize, // 1-based, inclusive
    pub transcription_end: usize,   // 1-based, exclusive
    pub cds_start: usize,           // 1-based, inclusive
    pub cds_end: usize,             // 1-based, exclusive
    pub exon_starts: Vec<usize>,    // 1-based, inclusive
    pub exon_ends: Vec<usize>,      // 1-based, exclusive
}

impl GenomeInterval for Gene {
    fn start(&self) -> usize {
        self.transcription_start
    }

    fn end(&self) -> usize {
        self.transcription_end
    }

    fn contig(&self) -> &Contig {
        &self.contig
    }
}

enum ExonPosition {
    PreCDS,
    CDS,
    PostCDS,
}

impl Gene {
    pub fn get_exon(&self, idx: usize) -> Option<Feature> {
        if idx >= self.exon_starts.len() {
            return None;
        }

        Some(Feature {
            contig: self.contig.clone(),
            start: self.exon_starts[idx],
            end: self.exon_ends[idx],
            feature_type: FeatureType::Exon,
        })
    }

    pub fn n_exons(&self) -> usize {
        self.exon_starts.len()
    }

    pub fn features(&self) -> Vec<(usize, usize, FeatureType, usize)> {
        // TODO: prevent labeling overlap.
        let mut features: Vec<(usize, usize, FeatureType)> = Vec::new();
        let mut last_exon_end = self.transcription_start;

        let mut n_cds_exons = 0;
        let mut n_introns = 0;

        // Add exon exons
        for (i, (exon_start, exon_end)) in self
            .exon_starts
            .iter()
            .zip(self.exon_ends.iter())
            .enumerate()
        {
            // Add intron
            if *exon_start > last_exon_end {
                features.push((last_exon_end + 1, *exon_start, FeatureType::Intron));
                n_introns += 1;
            }

            // Add exon
            let exon_start_position =
                match (*exon_start >= self.cds_start, *exon_start <= self.cds_end) {
                    (true, true) => ExonPosition::CDS,
                    (false, _) => ExonPosition::PreCDS,
                    (true, false) => ExonPosition::PostCDS,
                };

            let exon_end_position = match (*exon_end >= self.cds_start, *exon_end <= self.cds_end) {
                (true, true) => ExonPosition::CDS,
                (false, _) => ExonPosition::PreCDS,
                (true, false) => ExonPosition::PostCDS,
            };

            match (exon_start_position, exon_end_position) {
                (ExonPosition::PreCDS, ExonPosition::PreCDS) => {
                    features.push((*exon_start, *exon_end, FeatureType::NonCDSExon));
                    n_cds_exons += 1;
                }

                (ExonPosition::PreCDS, ExonPosition::CDS) => {
                    features.push((*exon_start, self.cds_start - 1, FeatureType::NonCDSExon));
                    features.push((self.cds_start, *exon_end, FeatureType::Exon));
                    n_cds_exons += 1;
                }
                (ExonPosition::PreCDS, ExonPosition::PostCDS) => {
                    features.push((*exon_start, self.cds_start - 1, FeatureType::NonCDSExon));
                    features.push((self.cds_start, self.cds_end, FeatureType::Exon));
                    features.push((self.cds_end + 1, *exon_end, FeatureType::NonCDSExon));
                    n_cds_exons += 1;
                }
                (ExonPosition::CDS, ExonPosition::CDS) => {
                    features.push((*exon_start, *exon_end, FeatureType::Exon));
                    n_cds_exons += 1;
                }
                (ExonPosition::CDS, ExonPosition::PostCDS) => {
                    features.push((*exon_start, self.cds_end, FeatureType::Exon));
                    features.push((self.cds_end + 1, *exon_end, FeatureType::NonCDSExon));
                    n_cds_exons += 1;
                }
                (ExonPosition::PostCDS, ExonPosition::PostCDS) => {
                    features.push((*exon_start, *exon_end, FeatureType::NonCDSExon));
                    n_cds_exons += 1;
                }
                _ => {} // should not happen
            }

            last_exon_end = *exon_end;
        }

        let mut output: Vec<(usize, usize, FeatureType, usize)> = Vec::new();
        let mut i_cds_exon = 0;
        let mut i_intron = 0;
        for (start, end, feature_type) in features {
            match feature_type {
                FeatureType::Exon => {
                    if self.strand == Strand::Forward {
                        output.push((start, end, feature_type, i_cds_exon + 1));
                        i_cds_exon += 1;
                    } else {
                        output.push((start, end, feature_type, n_cds_exons - i_cds_exon));
                        i_cds_exon += 1;
                    }
                }
                FeatureType::Intron => {
                    if self.strand == Strand::Forward {
                        output.push((start, end, feature_type, i_intron + 1));
                        i_intron += 1;
                    } else {
                        output.push((start, end, feature_type, n_introns - i_intron));
                        i_intron += 1;
                    }
                }
                FeatureType::NonCDSExon => {
                    output.push((start, end, feature_type, 0));
                }
            }
        }

        output
    }
}

// A track is a collections of features on a single contig.
pub struct Track {
    pub genes: Vec<Gene>, // TODO: what about hierarchy in features? e.g. exons of a gene?
    pub contig: Contig,

    // data_complete_left_bound: usize,
    // data_complete_right_bound: usize,
    /// Left bound
    /// 1-based, inclusive.
    most_left_bound: usize,

    /// Right bound
    /// 1-based, exclusive.
    most_right_bound: usize,

    exon_indexes: Vec<(usize, usize)>, // (i, j) -> exon [self.genes[i].exon_starts[j], self.genes[i].exon_ends[j]]
}

impl Track {
    /// Create a track from a list of genes.
    /// Assumes no feature overlapping.
    pub fn from(genes: Vec<Gene>, contig: Contig) -> Result<Self, TGVError> {
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
            genes,
            contig,
            // data_complete_left_bound: data_complete_left_bound,
            // data_complete_right_bound: data_complete_right_bound,
            most_left_bound,
            most_right_bound,
            exon_indexes,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.genes.is_empty()
    }

    /// Check if the track has complete data in [left, right].
    /// Note that this is assuming that the track has complete data.
    /// left: 1-based, inclusive.
    /// right: 1-based, exclusive.
    pub fn has_complete_data(&self, region: &Region) -> bool {
        self.contains(region)
    }
}

impl GenomeInterval for Track {
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

impl Track {
    /// Get the feature covering a given position.
    /// position: 1-based.
    pub fn get_gene_at(&self, position: usize) -> Option<&Gene> {
        for (i, gene) in self.genes.iter().enumerate() {
            if gene.covers(position) {
                return Some(gene);
            }
        }

        None
    }

    pub fn get_k_genes_before(&self, position: usize, k: usize) -> Option<&Gene> {
        if k == 0 {
            return self.get_gene_at(position);
        }

        if self.genes.is_empty() {
            return None;
        }

        if position < self.most_left_bound {
            return None;
        }

        if position > self.most_right_bound {
            if self.genes.len() < k {
                return None;
            }
            let gene_index = self.genes.len() - k;
            return Some(&self.genes[gene_index]);
        }

        for (i, gene) in self.genes.iter().enumerate() {
            if gene.end() < position {
                continue;
            }
            if i < k {
                return None;
            }

            let gene_index = i - k;

            return Some(&self.genes[gene_index]);
        }

        None
    }

    pub fn get_k_genes_after(&self, position: usize, k: usize) -> Option<&Gene> {
        if k == 0 {
            return self.get_gene_at(position);
        }

        if self.genes.is_empty() {
            return None;
        }

        if position > self.most_right_bound {
            return None;
        }

        if position < self.most_left_bound {
            if self.genes.len() < k {
                return None;
            }
            let gene_index = k - 1;
            return Some(&self.genes[gene_index]);
        }

        for (i, gene) in self.genes.iter().enumerate() {
            if gene.start() <= position {
                continue;
            }

            if i + k > self.genes.len() {
                return None;
            }

            let gene_index = i + k - 1;
            return Some(&self.genes[gene_index]);
        }

        None
    }

    pub fn get_saturating_k_genes_after(&self, position: usize, k: usize) -> Option<&Gene> {
        if k == 0 {
            return None;
        }

        if self.genes.is_empty() {
            return None;
        }

        match self.get_k_genes_after(position, k) {
            Some(gene) => Some(gene),
            _ => Some(self.genes.last().unwrap()),
        }
    }

    pub fn get_saturating_k_genes_before(&self, position: usize, k: usize) -> Option<&Gene> {
        if k == 0 {
            return None;
        }

        if self.genes.is_empty() {
            return None;
        }

        match self.get_k_genes_before(position, k) {
            Some(gene) => Some(gene),
            _ => Some(self.genes.first().unwrap()),
        }
    }
}

impl Track {
    /// O(1) search for the exon covering a given position.
    /// Inspiration: https://www.youtube.com/watch?v=ig-dtw8Um_k
    /// position: 1-based.
    pub fn get_exon_at(&self, position: usize) -> Option<Feature> {
        for (gene_idx, exon_idx) in self.exon_indexes.iter() {
            let gene = &self.genes[*gene_idx];
            let (exon_start, exon_end) = (gene.exon_starts[*exon_idx], gene.exon_ends[*exon_idx]);

            if exon_start <= position && position <= exon_end {
                return Some(gene.get_exon(*exon_idx).unwrap());
            }
        }

        None
    }

    pub fn get_k_exons_before(&self, position: usize, k: usize) -> Option<Feature> {
        if k == 0 {
            return self.get_exon_at(position);
        }

        if self.genes.is_empty() {
            return None;
        }

        if position < self.most_left_bound {
            return None;
        }

        if position > self.most_right_bound {
            if self.exon_indexes.len() < k {
                return None;
            }
            let i_exon = self.exon_indexes.len() - k;
            let (gene_idx, exon_idx) = self.exon_indexes[i_exon];
            return Some(self.genes[gene_idx].get_exon(exon_idx).unwrap());
        }

        for (i, (gene_idx, exon_idx)) in self.exon_indexes.iter().enumerate() {
            let gene = &self.genes[*gene_idx];
            let (exon_start, exon_end) = (gene.exon_starts[*exon_idx], gene.exon_ends[*exon_idx]);
            if exon_end < position {
                continue;
            }
            if i < k {
                return None;
            }

            let i_exon = i - k;
            let (gene_idx, exon_idx) = self.exon_indexes[i_exon];

            return Some(self.genes[gene_idx].get_exon(exon_idx).unwrap());
        }

        None
    }

    pub fn get_k_exons_after(&self, position: usize, k: usize) -> Option<Feature> {
        if k == 0 {
            return self.get_exon_at(position);
        }

        if self.genes.is_empty() {
            return None;
        }

        if position > self.most_right_bound {
            return None;
        }

        if position < self.most_left_bound {
            if self.exon_indexes.len() < k {
                return None;
            }
            let i_exon = k - 1;
            let (gene_idx, exon_idx) = self.exon_indexes[i_exon];
            return Some(self.genes[gene_idx].get_exon(exon_idx).unwrap());
        }

        for (i, (gene_idx, exon_idx)) in self.exon_indexes.iter().enumerate() {
            let gene = &self.genes[*gene_idx];
            let (exon_start, exon_end) = (gene.exon_starts[*exon_idx], gene.exon_ends[*exon_idx]);
            if exon_start <= position {
                continue;
            }

            if i + k > self.exon_indexes.len() {
                return None;
            }

            let i_exon = i + k - 1;
            let (gene_idx, exon_idx) = self.exon_indexes[i_exon];
            return Some(self.genes[gene_idx].get_exon(exon_idx).unwrap());
        }

        None
    }

    pub fn get_saturating_k_exons_after(&self, position: usize, k: usize) -> Option<Feature> {
        if k == 0 {
            return None;
        }

        if self.exon_indexes.is_empty() {
            return None;
        }

        match self.get_k_exons_after(position, k) {
            Some(exon) => Some(exon),
            _ => {
                let (gene_idx, exon_idx) = self.exon_indexes[self.exon_indexes.len() - 1];
                Some(self.genes[gene_idx].get_exon(exon_idx).unwrap())
            }
        }
    }

    pub fn get_saturating_k_exons_before(&self, position: usize, k: usize) -> Option<Feature> {
        if k == 0 {
            return None;
        }

        if self.exon_indexes.is_empty() {
            return None;
        }

        match self.get_k_exons_before(position, k) {
            Some(exon) => Some(exon),
            _ => {
                let (gene_idx, exon_idx) = self.exon_indexes[0];
                Some(self.genes[gene_idx].get_exon(exon_idx).unwrap())
            }
        }
    }
}

use crate::{
    error::TGVError,
    models::{contig::Contig, region::Region, strand::Strand},
    traits::GenomeInterval,
};

/// A feature is a interval on a contig.
///

#[derive(Debug, Clone, PartialEq, Eq)]
enum FeatureType {
    Exon,
    Intron,
}

#[derive(Debug, Clone)]
pub struct Feature {
    contig: Contig,
    start: usize,
    end: usize,
    feature_type: FeatureType,
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
    id: String,
    name: String,
    strand: Strand,
    contig: Contig,
    transcription_start: usize, // 1-based, inclusive
    transcription_end: usize,   // 1-based, exclusive
    cds_start: usize,           // 1-based, inclusive
    cds_end: usize,             // 1-based, exclusive
    exon_starts: Vec<usize>,    // 1-based, inclusive
    exon_ends: Vec<usize>,      // 1-based, exclusive

    features: Vec<Feature>,
    exon_indexes: Vec<usize>,
    intron_indexes: Vec<usize>,
}

impl Gene {
    pub fn new(
        id: String,
        name: String,
        strand: Strand,
        contig: Contig,

        transcription_start: usize,
        transcription_end: usize,
        cds_start: usize,
        cds_end: usize,
        exon_starts: Vec<usize>,
        exon_ends: Vec<usize>,
    ) -> Self {
        let mut last_exon_end = transcription_start;
        let mut features: Vec<Feature> = Vec::new();
        let mut exon_indexes: Vec<usize> = Vec::new();
        let mut intron_indexes: Vec<usize> = Vec::new();

        let mut exon_idx = 0;
        let mut intron_idx = 0;
        // Add exon exons
        for (i, (exon_start, exon_end)) in exon_starts.iter().zip(exon_ends.iter()).enumerate() {
            // Add intron
            if *exon_start > last_exon_end {
                features.push(Feature {
                    contig: contig.clone(),
                    start: last_exon_end + 1,
                    end: *exon_start,
                    feature_type: FeatureType::Intron,
                });
                intron_indexes.push(intron_idx);
                intron_idx += 1;
            }

            // Add exon
            features.push(Feature {
                contig: contig.clone(),
                start: *exon_start,
                end: *exon_end,
                feature_type: FeatureType::Exon,
            });

            exon_indexes.push(exon_idx);
            exon_idx += 1;

            last_exon_end = *exon_end;
        }

        Self {
            id,
            name,
            strand,
            contig: contig.clone(),
            transcription_start,
            transcription_end,
            cds_start,
            cds_end,
            exon_starts,
            exon_ends,
            features,
            exon_indexes,
            intron_indexes,
        }
    }
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

impl Gene {
    pub fn get_exon(&self, idx: usize) -> Option<&Feature> {
        if idx >= self.exon_indexes.len() {
            return None;
        }

        Some(&self.features[self.exon_indexes[idx]])
    }

    pub fn n_exons(&self) -> usize {
        self.exon_indexes.len()
    }

    pub fn has_exon(&self) -> bool {
        self.exon_indexes.is_empty()
    }

    pub fn get_k_left_exons_index(&self, idx: usize, k: usize) -> Option<usize> {
        if k > idx {
            return None;
        }

        Some(idx - k)
    }

    pub fn get_k_right_exons_index(&self, idx: usize, k: usize) -> Option<usize> {
        Some((idx + k).min(self.n_exons() - 1))
    }

    pub fn get_saturating_k_left_exons_index(&self, idx: usize, k: usize) -> Option<usize> {
        Some(idx.saturating_sub(k))
    }

    pub fn get_saturating_k_right_exons_index(&self, idx: usize, k: usize) -> Option<usize> {
        Some((idx + k).min(self.n_exons() - 1))
    }

    pub fn exons(&self) -> Vec<&Feature> {
        self.features
            .iter()
            .filter(|f| f.feature_type == FeatureType::Exon)
            .collect()
    }
}

impl Gene {
    pub fn get_intron(&self, idx: usize) -> Option<&Feature> {
        if idx >= self.intron_indexes.len() {
            return None;
        }

        Some(&self.features[self.intron_indexes[idx]])
    }

    pub fn n_introns(&self) -> usize {
        self.intron_indexes.len()
    }
}

impl Gene {
    pub fn get_exon_index_at(&self, position: usize) -> Option<usize> {
        if self.exon_indexes.is_empty() {
            return None;
        }

        if !self.covers(position) {
            return None;
        }

        for (i, idx_exon) in self.exon_indexes.iter().enumerate() {
            let exon = self.get_exon(*idx_exon).unwrap();
            if exon.covers(position) {
                return Some(*idx_exon);
            }

            if exon.start() > position {
                break;
            }
        }

        None
    }

    pub fn get_nearest_left_exon_index(&self, position: usize) -> Option<usize> {
        if self.exon_indexes.is_empty() {
            return None;
        }

        if position < self.start() {
            return None;
        }

        if position > self.end() {
            return Some(self.exon_indexes[self.exon_indexes.len() - 1]);
        }

        for (i, idx_exon) in self.exon_indexes.iter().enumerate() {
            if self.exon_starts[*idx_exon] > position {
                if i == 0 {
                    return None;
                }

                return Some(self.exon_indexes[i - 1]);
            }
        }

        Some(self.exon_indexes[self.exon_indexes.len() - 1])
    }

    pub fn get_nearest_right_exon_index(&self, position: usize) -> Option<usize> {
        if self.exon_indexes.is_empty() {
            return None;
        }

        if position < self.start() {
            return Some(self.exon_indexes[0]);
        }

        if position >= self.end() {
            return None;
        }

        for (i, idx_exon) in self.exon_indexes.iter().enumerate() {
            if self.exon_starts[*idx_exon] > position {
                return Some(*idx_exon);
            }
        }

        None
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
    pub most_left_bound: usize,

    /// Right bound
    /// 1-based, exclusive.
    pub most_right_bound: usize,
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
    pub fn get_gene(&self, idx: usize) -> Option<&Gene> {
        if idx >= self.genes.len() {
            return None;
        }
        Some(&self.genes[idx])
    }

    pub fn n_genes(&self) -> usize {
        self.genes.len()
    }

    pub fn has_gene(&self) -> bool {
        self.genes.is_empty()
    }

    pub fn get_k_left_genes_index(&self, idx: usize, k: usize) -> Option<usize> {
        if k > idx {
            return None;
        }
        Some(idx - k)
    }

    pub fn get_k_right_genes_index(&self, idx: usize, k: usize) -> Option<usize> {
        Some((idx + k).min(self.n_genes() - 1))
    }

    pub fn get_saturating_k_left_genes_index(&self, idx: usize, k: usize) -> Option<usize> {
        Some(idx.saturating_sub(k))
    }

    pub fn get_saturating_k_right_genes_index(&self, idx: usize, k: usize) -> Option<usize> {
        Some((idx + k).min(self.n_genes() - 1))
    }

    pub fn genes(&self) -> &Vec<Gene> {
        &self.genes
    }
}

impl Track {
    /// Create a track from a list of features.
    /// Features must be sorted.
    /// TODO: unclear if feature overlapping is ok.
    pub fn from(
        genes: Vec<Gene>,
        contig: Contig,
        // data_complete_left_bound: usize,
        // data_complete_right_bound: usize,
    ) -> Result<Self, String> {
        let mut genes = genes;
        genes.sort_by_key(|gene| gene.start());

        let mut most_left_bound = usize::MAX;
        let mut most_right_bound = usize::MIN;

        for gene in genes.iter() {
            if gene.start() < most_left_bound {
                most_left_bound = gene.start();
            }
            if gene.end() > most_right_bound {
                most_right_bound = gene.end();
            }
        }

        Ok(Self {
            genes,
            contig,
            // data_complete_left_bound: data_complete_left_bound,
            // data_complete_right_bound: data_complete_right_bound,
            most_left_bound,
            most_right_bound,
        })
    }

    /// Check if the track has complete data in [left, right].
    /// Note that this is assuming that the track has complete data.
    /// left: 1-based, inclusive.
    /// right: 1-based, exclusive.
    pub fn has_complete_data(&self, region: &Region) -> bool {
        self.contains(region)
    }
}

impl Track {
    pub fn get_exon(&self, idx_gene: usize, idx_exon: usize) -> Option<&Feature> {
        self.get_gene(idx_gene)
            .and_then(|gene| gene.get_exon(idx_exon))
    }

    pub fn get_first_exon_index(&self) -> Option<(usize, usize)> {
        for idx_gene in 0..self.genes.len() {
            if self.genes[idx_gene].has_exon() {
                return Some((idx_gene, 0));
            }
        }
        None
    }

    pub fn get_last_exon_index(&self) -> Option<(usize, usize)> {
        if self.genes.is_empty() {
            return None;
        }
        for i in 0..self.genes.len() {
            let idx_gene = self.genes.len() - 1 - i;
            if self.genes[idx_gene].has_exon() {
                return Some((idx_gene, self.genes[idx_gene].n_exons() - 1));
            }
        }
        None
    }

    pub fn get_k_left_exons_index(
        &self,
        idx_gene: usize,
        idx_exon: usize,
        k: usize,
    ) -> Option<(usize, usize)> {
        if idx_gene >= self.genes.len() {
            return None;
        }

        if idx_exon >= self.genes[idx_gene].n_exons() {
            return None;
        }

        let mut idx_gene = idx_gene;
        let mut idx_exon = idx_exon;

        let mut k = k;

        while k > 0 {
            if idx_exon >= k {
                // found the gene!
                idx_exon -= k;
                k = 0;
            } else if idx_gene > 0 {
                // move to the previous gene
                k -= (idx_exon + 1);
                idx_gene -= 1;
                idx_exon = self.genes[idx_gene].n_exons() - 1;
            } else {
                // out of genes
                return None;
            }
        }

        if k == 0 {
            return Some((idx_gene, idx_exon));
        }

        None
    }

    pub fn get_k_right_exons_index(
        &self,
        idx_gene: usize,
        idx_exon: usize,
        k: usize,
    ) -> Option<(usize, usize)> {
        if idx_gene >= self.genes.len() {
            return None;
        }

        if idx_exon >= self.genes[idx_gene].n_exons() {
            return None;
        }

        let mut idx_gene = idx_gene;
        let mut idx_exon = idx_exon;

        let mut k = k;

        while k > 0 {
            if idx_exon + k < self.genes[idx_gene].n_exons() {
                // found the gene!
                idx_exon += k;
                k = 0;
            } else if idx_gene < self.genes.len() - 1 {
                // move to the next gene
                k -= (self.genes[idx_gene].n_exons() - idx_exon);
                idx_gene += 1;
                idx_exon = 0;
            } else {
                // out of genes
                return None;
            }
        }

        if k == 0 {
            return Some((idx_gene, idx_exon));
        }

        None
    }

    pub fn get_saturating_k_exons_before(
        &self,
        idx_gene: usize,
        idx_exon: usize,
        k: usize,
    ) -> Option<(usize, usize)> {
        if idx_gene >= self.genes.len() {
            return None;
        }

        if idx_exon >= self.genes[idx_gene].n_exons() {
            return None;
        }
        match self.get_k_left_exons_index(idx_gene, idx_exon, k) {
            Some((idx_gene, idx_exon)) => Some((idx_gene, idx_exon)),
            None => self.get_first_exon_index(),
        }
    }

    pub fn get_saturating_k_exons_after(
        &self,
        idx_gene: usize,
        idx_exon: usize,
        k: usize,
    ) -> Option<(usize, usize)> {
        if idx_gene >= self.genes.len() {
            return None;
        }
        if idx_exon >= self.genes[idx_gene].n_exons() {
            return None;
        }
        match self.get_k_right_exons_index(idx_gene, idx_exon, k) {
            Some((idx_gene, idx_exon)) => Some((idx_gene, idx_exon)),
            None => self.get_last_exon_index(),
        }
    }
}

impl Track {
    pub fn get_exon_index_at(&self, position: usize) -> Option<(usize, usize)> {
        if self.genes.is_empty() {
            return None;
        }

        // Check if position is within the track bounds
        if !self.covers(position) {
            return None;
        }

        // Iterate through genes to find the one containing the position
        for (gene_idx, gene) in self.genes.iter().enumerate() {
            if gene.covers(position) {
                // Found the gene, now find the exon
                if let Some(exon_idx) = gene.get_exon_index_at(position) {
                    return Some((gene_idx, exon_idx));
                } else {
                    return None;
                }
            }
        }

        None
    }

    pub fn get_gene_index_at(&self, position: usize) -> Option<usize> {
        if self.genes.is_empty() {
            return None;
        }

        // Check if position is within the track bounds
        if !self.covers(position) {
            return None;
        }

        // Iterate through genes to find the one containing the position
        for (gene_idx, gene) in self.genes.iter().enumerate() {
            if gene.covers(position) {
                return Some(gene_idx);
            }
        }

        None
    }

    pub fn get_nearest_left_gene_index(&self, position: usize) -> Option<usize> {
        if self.genes.is_empty() {
            return None;
        }

        if position < self.start() {
            return None;
        }

        if position > self.end() {
            return Some(self.genes.len() - 1);
        }

        for (i, gene) in self.genes.iter().enumerate() {
            if gene.start() > position {
                if i == 0 {
                    return None;
                }

                return Some(i - 1);
            }
        }

        Some(self.genes.len() - 1)
    }

    pub fn get_nearest_right_gene_index(&self, position: usize) -> Option<usize> {
        if self.genes.is_empty() {
            return None;
        }

        if position < self.start() {
            return Some(0);
        }

        if position >= self.end() {
            return None;
        }

        for (i, gene) in self.genes.iter().enumerate() {
            if gene.start() > position {
                return Some(i);
            }
        }

        None
    }

    pub fn get_nearest_left_exon_index(&self, position: usize) -> Option<(usize, usize)> {
        if self.genes.is_empty() {
            return None;
        }

        if position < self.start() {
            return None;
        }

        if position > self.end() {
            return self.get_last_exon_index();
        }

        if let Some(idx_gene) = self.get_gene_index_at(position) {
            let gene = &self.genes[idx_gene];
            if let Some(idx_exon) = gene.get_nearest_left_exon_index(position) {
                return Some((idx_gene, idx_exon));
            }
        }

        if let Some(idx_gene) = self.get_nearest_left_gene_index(position) {
            let mut idx_gene = idx_gene;
            while idx_gene >= 0 {
                let gene = &self.genes[idx_gene];
                if gene.has_exon() {
                    return Some((idx_gene, gene.n_exons() - 1));
                }

                if idx_gene > 0 {
                    idx_gene -= 1;
                } else {
                    break;
                }
            }
        }

        None
    }

    pub fn get_nearest_right_exon_index(&self, position: usize) -> Option<(usize, usize)> {
        if self.genes.is_empty() {
            return None;
        }

        if position < self.start() {
            return self.get_first_exon_index();
        }

        if position > self.end() {
            return None;
        }

        if let Some(idx_gene) = self.get_gene_index_at(position) {
            let gene = &self.genes[idx_gene];
            if let Some(idx_exon) = gene.get_nearest_right_exon_index(position) {
                return Some((idx_gene, idx_exon));
            }
        }

        if let Some(idx_gene) = self.get_nearest_right_gene_index(position) {
            let mut idx_gene = idx_gene;
            while idx_gene < self.genes.len() {
                let gene = &self.genes[idx_gene];
                if gene.has_exon() {
                    return Some((idx_gene, 0));
                }

                if idx_gene < self.genes.len() - 1 {
                    idx_gene += 1;
                } else {
                    break;
                }
            }
        }

        None
    }
}

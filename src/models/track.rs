use crate::models::{contig::Contig, region::Region, strand::Strand};

/// A feature is a interval on a contig.

#[derive(Debug, Clone)]
pub enum Feature {
    Gene {
        id: String,
        name: String,
        strand: Strand,
        contig: Contig,
        start: usize,            // 1-based, inclusive
        end: usize,              // 1-based, exclusive
        exon_starts: Vec<usize>, // 1-based, inclusive
        exon_ends: Vec<usize>,   // 1-based, exclusive
    },
    Exon {
        id: String,
        name: String,
        strand: Strand,
        contig: Contig,
        start: usize, // 1-based, inclusive
        end: usize,   // 1-based, exclusive
                      //parent_gene: String,
    },
    Intron {
        id: String,
        name: String,
        strand: Strand,
        contig: Contig,
        start: usize, // 1-based, inclusive
        end: usize,   // 1-based, exclusive
                      //parent_gene: String,
    },
}

impl Feature {
    pub fn id(&self) -> &str {
        match self {
            Feature::Gene { id, .. } => id,
            Feature::Exon { id, .. } => id,
            Feature::Intron { id, .. } => id,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Feature::Gene { name, .. } => name,
            Feature::Exon { name, .. } => name,
            Feature::Intron { name, .. } => name,
        }
    }

    pub fn strand(&self) -> Strand {
        match self {
            Feature::Gene { strand, .. } => strand.clone(),
            Feature::Exon { strand, .. } => strand.clone(),
            Feature::Intron { strand, .. } => strand.clone(),
        }
    }

    pub fn contig(&self) -> Contig {
        match self {
            Feature::Gene { contig, .. } => contig.clone(),
            Feature::Exon { contig, .. } => contig.clone(),
            Feature::Intron { contig, .. } => contig.clone(),
        }
    }

    pub fn start(&self) -> usize {
        match self {
            Feature::Gene { start, .. } => *start,
            Feature::Exon { start, .. } => *start,
            Feature::Intron { start, .. } => *start,
        }
    }

    pub fn end(&self) -> usize {
        match self {
            Feature::Gene { end, .. } => *end,
            Feature::Exon { end, .. } => *end,
            Feature::Intron { end, .. } => *end,
        }
    }

    pub fn covers(&self, position: usize) -> bool {
        self.start() <= position && self.end() > position // TODO: exlusivity
    }

    pub fn length(&self) -> usize {
        self.end() - self.start()
    }

    /// Expand a parent feature into a list of child features.
    /// gene -> exons and introns
    pub fn expand(&self) -> Result<Vec<Feature>, ()> {
        match self {
            Feature::Gene {
                id,
                name,
                strand,
                contig,
                start,
                end,
                exon_starts,
                exon_ends,
            } => {
                // TODO: directions
                let mut features: Vec<Feature> = Vec::new();
                let mut last_exon_end = *start;

                // Add exon exons
                for (i, (exon_start, exon_end)) in
                    exon_starts.iter().zip(exon_ends.iter()).enumerate()
                {
                    // Add intron
                    if *exon_start > last_exon_end {
                        features.push(Feature::Intron {
                            id: format!("{}.intron{}", id.clone(), i + 1),
                            name: format!("{}.intron{}", name.clone(), i + 1),
                            strand: strand.clone(),
                            contig: contig.clone(),
                            start: last_exon_end,
                            end: *exon_start,
                        });
                    }

                    // Add exon
                    features.push(Feature::Exon {
                        id: format!("{}.exon{}", id.clone(), i + 1),
                        name: format!("{}.{}", name.clone(), i + 1),
                        strand: strand.clone(),
                        contig: contig.clone(),
                        start: *exon_start,
                        end: *exon_end,
                    });

                    last_exon_end = *exon_end;
                }

                Ok(features)
            }
            Feature::Exon { .. } => Err(()),
            Feature::Intron { .. } => Err(()),
        }
    }

    pub fn exons(&self) -> Result<Vec<Feature>, ()> {
        match self {
            Feature::Gene {
                id,
                name,
                strand,
                contig,
                start,
                end,
                exon_starts,
                exon_ends,
            } => {
                // TODO: directions
                let mut features: Vec<Feature> = Vec::new();

                // Add exon exons
                for (i, (exon_start, exon_end)) in
                    exon_starts.iter().zip(exon_ends.iter()).enumerate()
                {
                    // Add exon
                    features.push(Feature::Exon {
                        id: format!("{}.exon{}", id.clone(), i + 1),
                        name: format!("{}.{}", name.clone(), i + 1),
                        strand: strand.clone(),
                        contig: contig.clone(),
                        start: *exon_start,
                        end: *exon_end,
                    });
                }

                Ok(features)
            }
            Feature::Exon { .. } => Err(()),
            Feature::Intron { .. } => Err(()),
        }
    }
}

// A track is a collections of features on a single contig.
pub struct Track {
    pub features: Vec<Feature>, // TODO: what about hierarchy in features? e.g. exons of a gene?
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

impl Track {
    /// Create a track from a list of features.
    /// Features must be sorted.
    /// TODO: unclear if feature overlapping is ok.
    pub fn from(
        features: Vec<Feature>,
        contig: Contig,
        // data_complete_left_bound: usize,
        // data_complete_right_bound: usize,
    ) -> Result<Self, String> {
        let mut features = features;
        features.sort_by_key(|feature| feature.start());

        let mut most_left_bound = usize::MAX;
        let mut most_right_bound = usize::MIN;

        for feature in features.iter() {
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
        (region.contig == self.contig)
            && (region.start >= self.most_left_bound)
            && (region.end <= self.most_right_bound)
    }

    /// Check if the track overlaps with [left, right].
    /// Note that this is assuming that the track has complete data.
    /// left: 1-based, inclusive.
    /// right: 1-based, exclusive.
    pub fn overlaps(&self, region: &Region) -> bool {
        (region.contig == self.contig)
            && ((region.start <= self.most_right_bound) && (region.end >= self.most_left_bound))
    }

    /// Check if the track covers a given position.
    /// position: 1-based.
    pub fn covers(&self, position: usize) -> bool {
        self.most_left_bound <= position && self.most_right_bound >= position
    }

    /// Get the feature covering a given position.
    /// position: 1-based.
    pub fn get_feature_at(&self, position: usize) -> Option<(usize, &Feature)> {
        for (i, feature) in self.features.iter().enumerate() {
            if feature.covers(position) {
                return Some((i, feature));
            }
        }

        None
    }

    pub fn get_k_features_before(&self, position: usize, k: usize) -> Option<(usize, &Feature)> {
        if k == 0 {
            return self.get_feature_at(position);
        }

        if self.is_empty() {
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
            return Some((feature_index, &self.features[feature_index]));
        }

        for (i, feature) in self.features.iter().enumerate() {
            if feature.end() < position {
                continue;
            }
            if i < k {
                return None;
            }

            let feature_index = i - k;

            return Some((feature_index, &self.features[feature_index]));
        }

        None
    }

    pub fn get_k_features_after(&self, position: usize, k: usize) -> Option<(usize, &Feature)> {
        if k == 0 {
            return self.get_feature_at(position);
        }

        if self.is_empty() {
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
            return Some((feature_index, &self.features[feature_index]));
        }

        for (i, feature) in self.features.iter().enumerate() {
            if feature.start() <= position {
                continue;
            }

            if i + k > self.features.len() {
                return None;
            }

            let feature_index = i + k - 1;
            return Some((feature_index, &self.features[feature_index]));
        }

        None
    }

    pub fn get_saturating_k_features_after(
        &self,
        position: usize,
        k: usize,
    ) -> Option<(usize, &Feature)> {
        if k == 0 {
            return None;
        }

        if self.is_empty() {
            return None;
        }

        match self.get_k_features_after(position, k) {
            Some((feature_index, feature)) => Some((feature_index, feature)),
            _ => Some((self.features.len() - 1, self.features.last().unwrap())),
        }
    }

    pub fn get_saturating_k_features_before(
        &self,
        position: usize,
        k: usize,
    ) -> Option<(usize, &Feature)> {
        if k == 0 {
            return None;
        }

        if self.is_empty() {
            return None;
        }

        match self.get_k_features_before(position, k) {
            Some((feature_index, feature)) => Some((feature_index, feature)),
            _ => Some((0, self.features.first().unwrap())),
        }
    }
}

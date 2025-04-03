use crate::{
    error::TGVError,
    models::{contig::Contig, region::Region, strand::Strand},
    traits::{GenomeInterval, IntervalCollection},
};

/// A feature is a interval on a contig.
///

#[derive(Debug, Clone, PartialEq, Eq)]
enum FeatureType {
    Exon,
    Intron,
}

#[derive(Debug, Clone)]
struct Feature {
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

    exons: Vec<Feature>,
    introns: Vec<Feature>,
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
        let mut exons: Vec<Feature> = Vec::new();
        let mut introns: Vec<Feature> = Vec::new();
        let mut last_exon_end = transcription_start;
        let mut exon_indexes: Vec<usize> = Vec::new();

        let mut exon_idx = 0;
        // Add exon exons
        for (i, (exon_start, exon_end)) in exon_starts.iter().zip(exon_ends.iter()).enumerate() {
            // Add intron
            if *exon_start > last_exon_end {
                introns.push(Feature {
                    contig: contig.clone(),
                    start: last_exon_end + 1,
                    end: *exon_start,
                    feature_type: FeatureType::Intron,
                });
            }

            // Add exon
            exons.push(Feature {
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
            exons,
            introns,
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

impl IntervalCollection<Feature> for Gene {
    fn get(&self, idx: usize) -> Option<&Feature> {
        if idx >= self.exons.len() {
            return None;
        }

        Some(&self.exons[idx])
    }

    fn intervals(&self) -> &Vec<Feature> {
        return &self.exons;
    }

    fn len(&self) -> usize {
        self.exons.len()
    }
}

impl Gene {
    /// Expand a parent feature into a list of child features.
    pub fn exons(&self) -> Result<Vec<&Feature>, TGVError> {
        Ok(self.intervals().iter().collect())
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

impl IntervalCollection<Gene> for Track {
    fn get(&self, idx: usize) -> Option<&Gene> {
        if idx >= self.genes.len() {
            return None;
        }
        Some(&self.genes[idx])
    }

    fn intervals(&self) -> &Vec<Gene> {
        &self.genes
    }

    fn len(&self) -> usize {
        self.genes.len()
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
    pub fn get_exon_at(&self, position: usize) -> Option<&Feature> {
        self.get_at(position)
            .and_then(|(_, gene)| gene.get_at(position).and_then(|(_, exon)| Some(exon)))
    }

    pub fn get_k_exons_before(&self, position: usize, k: usize) -> Option<&Feature> {
        None
    }

    pub fn get_k_exons_after(&self, position: usize, k: usize) -> Option<&Feature> {
        None
    }

    pub fn get_saturating_k_exons_before(&self, position: usize, k: usize) -> Option<&Feature> {
        None
    }

    pub fn get_saturating_k_exons_after(&self, position: usize, k: usize) -> Option<&Feature> {
        None
    }
}

use crate::{contig::Contig, strand::Strand, traits::GenomeInterval};
use serde::Deserialize;

// A feature is a interval on a contig.

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::upper_case_acronyms)]
pub enum SubGeneFeatureType {
    Exon,
    Intron,
    NonCDSExon,
}

#[derive(Debug, Clone)]
pub struct SubGeneFeature {
    pub contig: Contig,
    pub start: usize,
    pub end: usize,
    pub feature_type: SubGeneFeatureType,
}

impl GenomeInterval for SubGeneFeature {
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

#[derive(Debug, Clone, Deserialize)]
pub struct Gene {
    #[serde(rename = "name")]
    pub id: String,

    #[serde(rename = "name2")]
    pub name: String,

    #[serde(rename = "strand", deserialize_with = "deserialize_strand")]
    pub strand: Strand,
    #[serde(skip)]
    pub contig: Contig, // We'll set this after deserialization
    #[serde(rename = "txStart")]
    pub transcription_start: usize,
    #[serde(rename = "txEnd")]
    pub transcription_end: usize,
    #[serde(rename = "cdsStart")]
    pub cds_start: usize,
    #[serde(rename = "cdsEnd")]
    pub cds_end: usize,
    #[serde(
        rename = "exonStarts",
        deserialize_with = "deserialize_comma_separated_list"
    )]
    pub exon_starts: Vec<usize>,
    #[serde(
        rename = "exonEnds",
        deserialize_with = "deserialize_comma_separated_list"
    )]
    pub exon_ends: Vec<usize>,
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

/// Custom deserializer for strand field
fn deserialize_strand<'de, D>(deserializer: D) -> Result<Strand, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    Strand::from_str(s).map_err(serde::de::Error::custom)
}

/// Custom deserializer for comma-separated lists in UCSC response
fn deserialize_comma_separated_list<'de, D>(deserializer: D) -> Result<Vec<usize>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    s.trim_end_matches(',')
        .split(',')
        .filter(|s| !s.is_empty())
        .map(|num| num.parse::<usize>().map_err(serde::de::Error::custom))
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::upper_case_acronyms)]
enum ExonPosition {
    PreCDS,
    CDS,
    PostCDS,
}

impl Gene {
    pub fn get_exon(&self, idx: usize) -> Option<SubGeneFeature> {
        if idx >= self.exon_starts.len() {
            return None;
        }

        Some(SubGeneFeature {
            contig: self.contig.clone(),
            start: self.exon_starts[idx],
            end: self.exon_ends[idx],
            feature_type: SubGeneFeatureType::Exon,
        })
    }

    pub fn n_exons(&self) -> usize {
        self.exon_starts.len()
    }

    pub fn features(&self) -> Vec<(usize, usize, SubGeneFeatureType, usize)> {
        // TODO: prevent labeling overlap.
        let mut features: Vec<(usize, usize, SubGeneFeatureType)> = Vec::new();
        let mut last_exon_end = self.transcription_start;

        let mut n_cds_exons = 0;
        let mut n_introns = 0;

        // Add exon exons
        for (exon_start, exon_end) in self.exon_starts.iter().zip(self.exon_ends.iter()) {
            // Add intron
            if *exon_start > last_exon_end {
                features.push((last_exon_end + 1, *exon_start, SubGeneFeatureType::Intron));
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
                    features.push((*exon_start, *exon_end, SubGeneFeatureType::NonCDSExon));
                    n_cds_exons += 1;
                }

                (ExonPosition::PreCDS, ExonPosition::CDS) => {
                    features.push((
                        *exon_start,
                        self.cds_start - 1,
                        SubGeneFeatureType::NonCDSExon,
                    ));
                    features.push((self.cds_start, *exon_end, SubGeneFeatureType::Exon));
                    n_cds_exons += 1;
                }
                (ExonPosition::PreCDS, ExonPosition::PostCDS) => {
                    features.push((
                        *exon_start,
                        self.cds_start - 1,
                        SubGeneFeatureType::NonCDSExon,
                    ));
                    features.push((self.cds_start, self.cds_end, SubGeneFeatureType::Exon));
                    features.push((self.cds_end + 1, *exon_end, SubGeneFeatureType::NonCDSExon));
                    n_cds_exons += 1;
                }
                (ExonPosition::CDS, ExonPosition::CDS) => {
                    features.push((*exon_start, *exon_end, SubGeneFeatureType::Exon));
                    n_cds_exons += 1;
                }
                (ExonPosition::CDS, ExonPosition::PostCDS) => {
                    features.push((*exon_start, self.cds_end, SubGeneFeatureType::Exon));
                    features.push((self.cds_end + 1, *exon_end, SubGeneFeatureType::NonCDSExon));
                    n_cds_exons += 1;
                }
                (ExonPosition::PostCDS, ExonPosition::PostCDS) => {
                    features.push((*exon_start, *exon_end, SubGeneFeatureType::NonCDSExon));
                    n_cds_exons += 1;
                }
                _ => {} // should not happen
            }

            last_exon_end = *exon_end;
        }

        let mut output: Vec<(usize, usize, SubGeneFeatureType, usize)> = Vec::new();
        let mut i_cds_exon = 0;
        let mut i_intron = 0;
        for (start, end, feature_type) in features {
            match feature_type {
                SubGeneFeatureType::Exon => {
                    if self.strand == Strand::Forward {
                        output.push((start, end, feature_type, i_cds_exon + 1));
                        i_cds_exon += 1;
                    } else {
                        output.push((start, end, feature_type, n_cds_exons - i_cds_exon));
                        i_cds_exon += 1;
                    }
                }
                SubGeneFeatureType::Intron => {
                    if self.strand == Strand::Forward {
                        output.push((start, end, feature_type, i_intron + 1));
                        i_intron += 1;
                    } else {
                        output.push((start, end, feature_type, n_introns - i_intron));
                        i_intron += 1;
                    }
                }
                SubGeneFeatureType::NonCDSExon => {
                    output.push((start, end, feature_type, 0));
                }
            }
        }

        output
    }
}

use crate::{intervals::GenomeInterval, strand::Strand};

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
    pub contig_index: usize,
    pub start: u64,
    pub end: u64,
    pub feature_type: SubGeneFeatureType,
}

impl GenomeInterval for SubGeneFeature {
    fn start(&self) -> u64 {
        self.start
    }

    fn end(&self) -> u64 {
        self.end
    }

    fn contig_index(&self) -> usize {
        self.contig_index
    }
}

/*

FIXME

UCSC has different formats for track features. I don't fully understand them yet.

The current schema is taken from hg38/hg19 ncbiRefSeqSelected. But for other genomes, there are API responses like this:

(GCF_028858775.2:NC_072398.2)

{
"chrom": "NC_072398.2",
"chromStart": 130929426,
"chromEnd": 130985030,
"name": "NM_001142759.1",
"score": 0,
"strand": "+",
"thickStart": 130929440,
"thickEnd": 130982945,
"reserved": "0",
"blockCount": 13,
"blockSizes": "65,124,76,182,122,217,167,126,78,192,72,556,374,",
"chromStarts": "0,8926,14265,18877,31037,33561,34781,36127,39014,43150,43484,53351,55230,",
"name2": "DBT",
"cdsStartStat": "cmpl",
"cdsEndStat": "cmpl",
"exonFrames": "0,0,1,2,1,0,1,0,0,0,0,0,-1,",
"type": "",
"geneName": "NM_001142759.1",
"geneName2": "DBT",
"geneType": ""
}

If someone has experience in this, please help.
For now, I don't interprete sub-gene features for API responses with this format.

*/
#[derive(Debug, Clone)]
pub struct Gene {
    pub id: String,

    pub name: String,

    pub strand: Strand,
    pub contig_index: usize,
    pub transcription_start: u64,
    pub transcription_end: u64,

    pub cds_start: u64,

    pub cds_end: u64,

    pub exon_starts: Vec<u64>,

    pub exon_ends: Vec<u64>,

    pub has_exons: bool,
}

impl GenomeInterval for Gene {
    fn start(&self) -> u64 {
        self.transcription_start
    }

    fn end(&self) -> u64 {
        self.transcription_end
    }

    fn contig_index(&self) -> usize {
        self.contig_index
    }
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
            contig_index: self.contig_index,
            start: self.exon_starts[idx],
            end: self.exon_ends[idx],
            feature_type: SubGeneFeatureType::Exon,
        })
    }

    pub fn n_exons(&self) -> usize {
        self.exon_starts.len()
    }

    pub fn features(&self) -> Vec<(u64, u64, SubGeneFeatureType, usize)> {
        // TODO: prevent labeling overlap.
        let mut features: Vec<(u64, u64, SubGeneFeatureType)> = Vec::new();
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

        let mut output: Vec<(u64, u64, SubGeneFeatureType, usize)> = Vec::new();
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

use crate::contig::Contig;
use crate::error::TGVError;
use std::fmt;

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Reference {
    Hg19,
    Hg38,
    UcscGenome(String),
    UcscAccession(String),
}

impl Reference {
    pub const HG19: &str = "hg19";
    pub const HG38: &str = "hg38";
    pub const SUPPORTED_REFERENCES: [&str; 2] = [Self::HG19, Self::HG38];

    pub fn from_str(s: &str) -> Result<Self, TGVError> {
        if s == Self::HG19 {
            Ok(Self::Hg19)
        } else if s == Self::HG38 {
            Ok(Self::Hg38)
        } else if s.starts_with("GCA_") || s.starts_with("GCF_") {
            Ok(Self::UcscAccession(s.to_string()))
        } else {
            Ok(Self::UcscGenome(s.to_string()))
        }
    }

    pub fn to_string(&self) -> String {
        match self {
            Self::Hg19 => Self::HG19.to_string(),
            Self::Hg38 => Self::HG38.to_string(),
            Self::UcscGenome(s) => s.clone(),
            Self::UcscAccession(s) => s.clone(),
        }
    }
}

impl fmt::Display for Reference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hg19 => write!(f, "{}", Self::HG19),
            Self::Hg38 => write!(f, "{}", Self::HG38),
            Self::UcscGenome(s) => write!(f, "{}", s),
            Self::UcscAccession(s) => write!(f, "{}", s),
        }
    }
}

impl Reference {
    pub fn contigs_and_lengths(&self) -> Result<Vec<(Contig, usize)>, TGVError> {
        match self {
            Self::Hg19 => {
                Ok(vec![
                    (Contig::chrom("chr1"), 249250621),
                    (Contig::chrom("chr2"), 243199373),
                    (Contig::chrom("chr3"), 198022430),
                    (Contig::chrom("chr4"), 191154276),
                    (Contig::chrom("chr5"), 180915260),
                    (Contig::chrom("chr6"), 171115067),
                    (Contig::chrom("chr7"), 159138663),
                    (Contig::chrom("chr8"), 155270560),
                    (Contig::chrom("chr9"), 146364022),
                    (Contig::chrom("chr10"), 141213431),
                    (Contig::chrom("chr11"), 135534747),
                    (Contig::chrom("chr12"), 135006516),
                    (Contig::chrom("chr13"), 133851895),
                    (Contig::chrom("chr14"), 115169878),
                    (Contig::chrom("chr15"), 107349540),
                    (Contig::chrom("chr16"), 102531392),
                    (Contig::chrom("chr17"), 90354753),
                    (Contig::chrom("chr18"), 81195210),
                    (Contig::chrom("chr19"), 78077248),
                    (Contig::chrom("chr20"), 63025520),
                    (Contig::chrom("chr21"), 59373566),
                    (Contig::chrom("chr22"), 59128983),
                    (Contig::chrom("chrX"), 51304566),
                    (Contig::chrom("chrY"), 48129895),
                ]) // TODO: MT
            }
            Self::Hg38 => Ok(vec![
                (Contig::chrom("chr1"), 248956422),
                (Contig::chrom("chr2"), 242193529),
                (Contig::chrom("chr3"), 198295559),
                (Contig::chrom("chr4"), 190214555),
                (Contig::chrom("chr5"), 181538259),
                (Contig::chrom("chr6"), 170805979),
                (Contig::chrom("chr7"), 159345973),
                (Contig::chrom("chrX"), 156040895),
                (Contig::chrom("chr8"), 145138636),
                (Contig::chrom("chr9"), 138394717),
                (Contig::chrom("chr11"), 135086622),
                (Contig::chrom("chr10"), 133797422),
                (Contig::chrom("chr12"), 133275309),
                (Contig::chrom("chr13"), 114364328),
                (Contig::chrom("chr14"), 107043718),
                (Contig::chrom("chr15"), 101991189),
                (Contig::chrom("chr16"), 90338345),
                (Contig::chrom("chr17"), 83257441),
                (Contig::chrom("chr18"), 80373285),
                (Contig::chrom("chr20"), 64444167),
                (Contig::chrom("chr19"), 58617616),
                (Contig::chrom("chrY"), 57227415),
                (Contig::chrom("chr22"), 50818468),
                (Contig::chrom("chr21"), 46709983),
            ]),
            _ => Err(TGVError::IOError(format!(
                "Cannot get contigs for this reference: {}. Need to query the UCSC API.",
                self
            ))),
        }
    }
}

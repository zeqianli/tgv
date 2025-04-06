use crate::error::TGVError;
use crate::models::contig::Contig;
use std::fmt;

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Reference {
    Hg19,
    Hg38,
}

impl Reference {
    pub const HG19: &str = "hg19";
    pub const HG38: &str = "hg38";
    pub const SUPPORTED_REFERENCES: [&str; 2] = [Self::HG19, Self::HG38];

    pub fn from_str(s: &str) -> Result<Self, TGVError> {
        match s {
            Self::HG19 => Ok(Self::Hg19),
            Self::HG38 => Ok(Self::Hg38),
            _ => Err(TGVError::ParsingError(format!("Invalid reference: {}", s))),
        }
    }
}

impl fmt::Display for Reference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hg19 => write!(f, "{}", Self::HG19),
            Self::Hg38 => write!(f, "{}", Self::HG38),
        }
    }
}

impl Reference {
    /// Return chromosome length read.
    /// Data is from the UCSC database. See ./resources/*_chrominfo.csv   
    pub fn length(&self, contig: &Contig) -> Option<usize> {
        match self {
            Self::Hg19 => match contig.full_name().as_str() {
                "chr1" => Some(249250621),
                "chr2" => Some(243199373),
                "chr3" => Some(198022430),
                "chr4" => Some(191154276),
                "chr5" => Some(180915260),
                "chr6" => Some(171115067),
                "chr7" => Some(159138663),
                "chrX" => Some(155270560),
                "chr8" => Some(146364022),
                "chr9" => Some(141213431),
                "chr10" => Some(135534747),
                "chr11" => Some(135006516),
                "chr12" => Some(133851895),
                "chr13" => Some(115169878),
                "chr14" => Some(107349540),
                "chr15" => Some(102531392),
                "chr16" => Some(90354753),
                "chr17" => Some(81195210),
                "chr18" => Some(78077248),
                "chr20" => Some(63025520),
                "chrY" => Some(59373566),
                "chr19" => Some(59128983),
                "chr22" => Some(51304566),
                "chr21" => Some(48129895),
                _ => None,
            },
            Self::Hg38 => match contig.full_name().as_str() {
                "chr1" => Some(248956422),
                "chr2" => Some(242193529),
                "chr3" => Some(198295559),
                "chr4" => Some(190214555),
                "chr5" => Some(181538259),
                "chr6" => Some(170805979),
                "chr7" => Some(159345973),
                "chrX" => Some(156040895),
                "chr8" => Some(145138636),
                "chr9" => Some(138394717),
                "chr11" => Some(135086622),
                "chr10" => Some(133797422),
                "chr12" => Some(133275309),
                "chr13" => Some(114364328),
                "chr14" => Some(107043718),
                "chr15" => Some(101991189),
                "chr16" => Some(90338345),
                "chr17" => Some(83257441),
                "chr18" => Some(80373285),
                "chr20" => Some(64444167),
                "chr19" => Some(58617616),
                "chrY" => Some(57227415),
                "chr22" => Some(50818468),
                "chr21" => Some(46709983),
                _ => None,
            },
        }
    }
}

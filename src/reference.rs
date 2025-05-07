use crate::error::TGVError;
use std::fmt;

// Added: Embed the CSV content as static bytes
const DEFAULT_DB_CSV: &[u8] = include_bytes!("resources/defaultDb.csv");

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

    pub fn get_common_genome_names() -> Result<Vec<(String, String)>, TGVError> {
        let mut common_genome_names = Vec::new();
        let csv_content = std::str::from_utf8(DEFAULT_DB_CSV)
            .map_err(|e| TGVError::ParsingError(format!("Failed to read embedded CSV: {}", e)))?;
        for line in csv_content.lines().skip(1) {
            if let Some((genome, name)) = line.split_once(',') {
                common_genome_names.push((genome.to_string(), name.to_string()));
            }
        }
        Ok(common_genome_names)
    }

    pub fn from_str(s: &str) -> Result<Self, TGVError> {
        if s == Self::HG19 {
            return Ok(Self::Hg19);
        }
        if s == Self::HG38 {
            return Ok(Self::Hg38);
        }
        if s.starts_with("GCA_") || s.starts_with("GCF_") {
            // Matches an accession pattern
            return Ok(Self::UcscAccession(s.to_string()));
        }

        // Check for common names

        let s_standardized = standardize_common_genome_name(s)?;

        for (genome, name) in Reference::get_common_genome_names()? {
            let genome_standardized = standardize_common_genome_name(genome.as_str())?;
            let name_trimmed = name.trim().trim_matches('"');

            if genome_standardized == s_standardized {
                // Found a match in the "genome" column
                if name_trimmed.starts_with("GCF_") || name_trimmed.starts_with("GCA_") {
                    return Ok(Self::UcscAccession(name_trimmed.to_string()));
                } else {
                    return Ok(Self::UcscGenome(name_trimmed.to_string()));
                }
            }
        }
        // Silently ignore lines that don't split correctly

        // Last option: treat it as a UcscGenome name directly.
        Ok(Self::UcscGenome(s.to_string()))
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

// impl Reference {
//     pub fn contigs_and_lengths(&self) -> Result<Vec<(Contig, usize)>, TGVError> {
//         match self {
//             Self::Hg19 => {
//                 Ok(vec![
//                     (Contig::new("chr1"), 249250621),
//                     (Contig::new("chr2"), 243199373),
//                     (Contig::new("chr3"), 198022430),
//                     (Contig::new("chr4"), 191154276),
//                     (Contig::new("chr5"), 180915260),
//                     (Contig::new("chr6"), 171115067),
//                     (Contig::new("chr7"), 159138663),
//                     (Contig::new("chr8"), 155270560),
//                     (Contig::new("chr9"), 146364022),
//                     (Contig::new("chr10"), 141213431),
//                     (Contig::new("chr11"), 135534747),
//                     (Contig::new("chr12"), 135006516),
//                     (Contig::new("chr13"), 133851895),
//                     (Contig::new("chr14"), 115169878),
//                     (Contig::new("chr15"), 107349540),
//                     (Contig::new("chr16"), 102531392),
//                     (Contig::new("chr17"), 90354753),
//                     (Contig::new("chr18"), 81195210),
//                     (Contig::new("chr19"), 78077248),
//                     (Contig::new("chr20"), 63025520),
//                     (Contig::new("chr21"), 59373566),
//                     (Contig::new("chr22"), 59128983),
//                     (Contig::new("chrX"), 51304566),
//                     (Contig::new("chrY"), 48129895),
//                 ]) // TODO: MT
//             }
//             Self::Hg38 => Ok(vec![
//                 (Contig::new("chr1"), 248956422),
//                 (Contig::new("chr2"), 242193529),
//                 (Contig::new("chr3"), 198295559),
//                 (Contig::new("chr4"), 190214555),
//                 (Contig::new("chr5"), 181538259),
//                 (Contig::new("chr6"), 170805979),
//                 (Contig::new("chr7"), 159345973),
//                 (Contig::new("chrX"), 156040895),
//                 (Contig::new("chr8"), 145138636),
//                 (Contig::new("chr9"), 138394717),
//                 (Contig::new("chr11"), 135086622),
//                 (Contig::new("chr10"), 133797422),
//                 (Contig::new("chr12"), 133275309),
//                 (Contig::new("chr13"), 114364328),
//                 (Contig::new("chr14"), 107043718),
//                 (Contig::new("chr15"), 101991189),
//                 (Contig::new("chr16"), 90338345),
//                 (Contig::new("chr17"), 83257441),
//                 (Contig::new("chr18"), 80373285),
//                 (Contig::new("chr20"), 64444167),
//                 (Contig::new("chr19"), 58617616),
//                 (Contig::new("chrY"), 57227415),
//                 (Contig::new("chr22"), 50818468),
//                 (Contig::new("chr21"), 46709983),
//             ]),
//             _ => Err(TGVError::IOError(format!(
//                 "Cannot get contigs for this reference: {}. Need to query the UCSC API.",
//                 self
//             ))),
//         }
//     }
// }

// to lowercase; remove ."-_
fn standardize_common_genome_name(s: &str) -> Result<String, TGVError> {
    let lower_s = s
        .to_lowercase()
        .replace(".", "")
        .replace("-", "")
        .replace("_", "")
        .replace(" ", "");
    Ok(lower_s)
}

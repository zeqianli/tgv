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
    IndexedFasta(String),
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

        // Reference fasta?

        if s.ends_with(".fa")
            || s.ends_with(".fasta")
            || s.ends_with(".fa.gz")
            || s.ends_with(".fasta.gz")
        {
            // check that index file exists
            let s = shellexpand::tilde(s).to_string();
            if !std::path::Path::new(&s).exists() {
                return Err(TGVError::IOError(format!(
                    "Reference genome file {} does not exist",
                    s
                )));
            }
            if !std::path::Path::new(&format!("{}.fai", s)).exists() {
                return Err(TGVError::IOError(format!(
                    ".fai index file is required for custom reference genome. \nYou can create index by\n   samtools faidx {}.\n(see https://www.htslib.org/doc/samtools-faidx.html)",
                    s
                )));
            }

            return Ok(Self::IndexedFasta(s));
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
            Self::IndexedFasta(s) => s.split('/').last().unwrap().to_string(),
        }
    }
}

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

use crate::error::TGVError;
use std::path::Path;
// Added: Embed the CSV content as static bytes
const DEFAULT_DB_CSV: &[u8] = include_bytes!("resources/defaultDb.csv");

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub enum Reference {
    Hg19,
    #[default]
    Hg38,
    UcscGenome(String),
    UcscAccession(String),
    BYOIndexedFasta(String),
    BYOTwoBit(String),
    NoReference,
}

impl Reference {
    pub const HG19: &str = "hg19";
    pub const HG38: &str = "hg38";
    pub const NO_REFERENCE: &str = "no_reference";

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

    pub fn needs_track(&self) -> bool {
        match self {
            Self::Hg19 | Self::Hg38 | Self::UcscGenome(_) | Self::UcscAccession(_) => true,
            Self::BYOIndexedFasta(_) | Self::BYOTwoBit(_) | Self::NoReference => false,
        }
    }
    pub fn needs_sequence(&self) -> bool {
        match self {
            Self::Hg19
            | Self::Hg38
            | Self::UcscGenome(_)
            | Self::UcscAccession(_)
            | Self::BYOIndexedFasta(_)
            | Self::BYOTwoBit(_) => true,
            Self::NoReference => false,
        }
    }

    pub fn cache_dir(&self, parent_cache_dir: &str) -> String {
        Path::new(parent_cache_dir)
            .join(self.to_string())
            .to_str()
            .unwrap()
            .to_string()
    }
}

impl std::fmt::Display for Reference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let reference = match self {
            Self::Hg19 => Self::HG19,
            Self::Hg38 => Self::HG38,
            Self::UcscGenome(s)
            | Self::UcscAccession(s)
            | Self::BYOIndexedFasta(s)
            | Self::BYOTwoBit(s) => s,
            Self::NoReference => Self::NO_REFERENCE,
        };
        f.write_str(reference)
    }
}

impl std::str::FromStr for Reference {
    type Err = TGVError;
    fn from_str(s: &str) -> Result<Self, TGVError> {
        if s == Self::HG19 {
            return Ok(Self::Hg19);
        }
        if s == Self::HG38 {
            return Ok(Self::Hg38);
        }
        if s == Self::NO_REFERENCE {
            return Ok(Self::NoReference);
        }
        if s.starts_with("GCA_") || s.starts_with("GCF_") {
            // Matches an accession pattern
            return Ok(Self::UcscAccession(s.to_string()));
        }

        // Reference fasta
        if s.ends_with(".fa")
            || s.ends_with(".fasta")
            || s.ends_with(".fa.gz")
            || s.ends_with(".fasta.gz")
        {
            let s = absolute_path(s)?;
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

            return Ok(Self::BYOIndexedFasta(s));
        }

        // 2bit
        if s.ends_with(".2bit") {
            let s = absolute_path(s)?;
            if !std::path::Path::new(&s).exists() {
                return Err(TGVError::IOError(format!(
                    "2bit reference genome file {} does not exist",
                    s
                )));
            }

            return Ok(Self::BYOTwoBit(s));
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
}
/// Expands `~` and makes a custom reference path absolute, so it stays valid when a session is
/// resumed from another working directory.
fn absolute_path(path: &str) -> Result<String, TGVError> {
    let absolute = std::path::absolute(shellexpand::tilde(path).as_ref())?;
    absolute.to_str().map(str::to_owned).ok_or_else(|| {
        TGVError::IOError(format!(
            "The reference path {} is not valid UTF-8.",
            absolute.display()
        ))
    })
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

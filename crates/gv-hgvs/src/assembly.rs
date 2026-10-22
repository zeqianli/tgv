//! The genome builds that HGVS supports, where their prepared data lives, and how their RefSeq
//! chromosome accessions map to tgv contig names.

use std::path::{Path, PathBuf};

use gv_core::prelude::*;

use crate::error::HgvsError;

/// A genome build with ferro-hgvs reference data.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum HgvsBuild {
    Grch38,
    Grch37,
}

impl HgvsBuild {
    /// The build for a tgv reference, or `None` when HGVS doesn't support it.
    pub fn from_reference(reference: &Reference) -> Option<Self> {
        match reference {
            Reference::Hg38 => Some(Self::Grch38),
            Reference::Hg19 => Some(Self::Grch37),
            _ => None,
        }
    }

    /// Like [`Self::from_reference`], but reports an unsupported reference as an error.
    pub fn try_from_reference(reference: &Reference) -> Result<Self, HgvsError> {
        Self::from_reference(reference).ok_or_else(|| HgvsError::UnsupportedReference {
            reference: reference.to_string(),
        })
    }

    /// The build name that ferro-hgvs uses for `PrepareConfig::genome`.
    pub fn ferro_name(self) -> &'static str {
        match self {
            Self::Grch38 => "grch38",
            Self::Grch37 => "grch37",
        }
    }

    /// The chromosome accessions of the primary assembly, as `(accession, chromosome)`.
    fn chromosomes(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Grch38 => GRCH38_CHROMOSOMES,
            Self::Grch37 => GRCH37_CHROMOSOMES,
        }
    }

    /// The RefSeq accession of a tgv contig name, such as `NC_000012.12` for `chr12` or `12`.
    pub fn contig_accession(self, contig: &str) -> Option<&'static str> {
        let chromosome = contig.strip_prefix("chr").unwrap_or(contig);
        let chromosome = if chromosome == "MT" { "M" } else { chromosome };
        self.chromosomes()
            .iter()
            .find(|(_, name)| *name == chromosome)
            .map(|(accession, _)| *accession)
    }

    /// The chromosome of a RefSeq accession, such as `12` for `NC_000012.12`. tgv contig names
    /// for hg38 and hg19 are this with a `chr` prefix.
    pub fn chromosome(self, accession: &str) -> Option<&'static str> {
        self.chromosomes()
            .iter()
            .find(|(candidate, _)| *candidate == accession)
            .map(|(_, chromosome)| *chromosome)
    }
}

/// The prepared HGVS data for one reference.
#[derive(Debug, Clone)]
pub struct HgvsDataDir {
    pub build: HgvsBuild,
    pub reference: Reference,
    pub path: PathBuf,
}

impl HgvsDataDir {
    pub const DIRECTORY: &str = "hgvs";
    pub const MANIFEST: &str = "manifest.json";

    /// The HGVS data directory under a tgv cache directory, such as `~/.tgv/hg38/hgvs/`.
    pub fn new(reference: &Reference, cache_dir: &str) -> Result<Self, HgvsError> {
        Ok(Self {
            build: HgvsBuild::try_from_reference(reference)?,
            reference: reference.clone(),
            path: Path::new(&reference.cache_dir(cache_dir)).join(Self::DIRECTORY),
        })
    }

    pub fn manifest(&self) -> PathBuf {
        self.path.join(Self::MANIFEST)
    }

    /// Fails with [`HgvsError::NotPrepared`] unless `tgv download` has prepared the data.
    pub fn check_prepared(&self) -> Result<(), HgvsError> {
        if self.manifest().is_file() {
            Ok(())
        } else {
            Err(HgvsError::NotPrepared {
                reference: self.reference.to_string(),
                dir: self.path.clone(),
            })
        }
    }
}

/// GRCh38 chromosome accessions (GCF_000001405.40).
const GRCH38_CHROMOSOMES: &[(&str, &str)] = &[
    ("NC_000001.11", "1"),
    ("NC_000002.12", "2"),
    ("NC_000003.12", "3"),
    ("NC_000004.12", "4"),
    ("NC_000005.10", "5"),
    ("NC_000006.12", "6"),
    ("NC_000007.14", "7"),
    ("NC_000008.11", "8"),
    ("NC_000009.12", "9"),
    ("NC_000010.11", "10"),
    ("NC_000011.10", "11"),
    ("NC_000012.12", "12"),
    ("NC_000013.11", "13"),
    ("NC_000014.9", "14"),
    ("NC_000015.10", "15"),
    ("NC_000016.10", "16"),
    ("NC_000017.11", "17"),
    ("NC_000018.10", "18"),
    ("NC_000019.10", "19"),
    ("NC_000020.11", "20"),
    ("NC_000021.9", "21"),
    ("NC_000022.11", "22"),
    ("NC_000023.11", "X"),
    ("NC_000024.10", "Y"),
    ("NC_012920.1", "M"),
];

/// GRCh37 chromosome accessions (GCF_000001405.25). hg19's chrM is the older NC_001807.4
/// sequence rather than the GRCh37 mitochondrion, so it has no entry.
const GRCH37_CHROMOSOMES: &[(&str, &str)] = &[
    ("NC_000001.10", "1"),
    ("NC_000002.11", "2"),
    ("NC_000003.11", "3"),
    ("NC_000004.11", "4"),
    ("NC_000005.9", "5"),
    ("NC_000006.11", "6"),
    ("NC_000007.13", "7"),
    ("NC_000008.10", "8"),
    ("NC_000009.11", "9"),
    ("NC_000010.10", "10"),
    ("NC_000011.9", "11"),
    ("NC_000012.11", "12"),
    ("NC_000013.10", "13"),
    ("NC_000014.8", "14"),
    ("NC_000015.9", "15"),
    ("NC_000016.9", "16"),
    ("NC_000017.10", "17"),
    ("NC_000018.9", "18"),
    ("NC_000019.9", "19"),
    ("NC_000020.10", "20"),
    ("NC_000021.8", "21"),
    ("NC_000022.10", "22"),
    ("NC_000023.10", "X"),
    ("NC_000024.9", "Y"),
];

use crate::reference::Reference;
use crate::tracks::UcscHost;
use clap::ValueEnum;

#[derive(Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum BackendType {
    /// Always use UCSC DB / API.
    Ucsc,

    /// Always use local database.
    Local,

    /// If local cache is available, use it. Otherwise, use UCSC DB / API.
    Default,
}

impl Default for BackendType {
    fn default() -> Self {
        BackendType::Default
    }
}

/// Where the BAM file is stored.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum BamSource {
    /// File on the local filesystem.
    Local,

    /// File on AWS S3 (or any S3-compatible / HTTP remote accessed via opendal).
    S3,
}

/// Alignment input file with the auxiliary files required to read it.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum AlignmentPath {
    /// BAM file with its .bai index and the source indicating where it lives.
    Bam {
        path: String,
        index: String,
        source: BamSource,
    },

    /// CRAM file with its .crai index and the FASTA reference (plus .fai) needed for decoding.
    Cram {
        path: String,
        crai: String,
        fasta: String,
        fai: String,
    },
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Settings {
    pub alignment_path: Option<AlignmentPath>,
    pub vcf_path: Option<String>,
    pub bed_path: Option<String>,
    pub reference: Reference,
    pub backend: BackendType,

    pub ucsc_host: UcscHost,

    pub cache_dir: String,

    /// Path to a cdot-format JSON or JSON.gz transcript database used for
    /// converting c. HGVS notation to genomic coordinates.
    pub cdot_path: Option<String>,
    //pub palette: Palette,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            alignment_path: None,
            vcf_path: None,
            bed_path: None,
            reference: Reference::default(),
            backend: BackendType::default(), // Default backend
            ucsc_host: UcscHost::default(),
            cache_dir: shellexpand::tilde("~/.tgv").to_string(),
            cdot_path: None,
        }
    }
}

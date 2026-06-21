use crate::reference::Reference;
use crate::tracks::UcscHost;
use clap::ValueEnum;

#[derive(Clone, Debug, PartialEq, Eq, ValueEnum, Default)]
pub enum BackendType {
    /// Always use UCSC DB / API.
    Ucsc,

    /// Always use local database.
    Local,

    /// If local cache is available, use it. Otherwise, use UCSC DB / API.
    #[default]
    Default,
}

/// Where the BAM file is stored.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum BamSource {
    /// File on the local filesystem.
    Local,

    /// File on AWS S3 or S3-compatible object storage.
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
pub enum FilePath {
    AlignmentPath(AlignmentPath),
    VariantPath(String),
    BedPath(String),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Settings {
    pub file_paths: Vec<FilePath>,
    pub reference: Reference,
    pub backend: BackendType,

    pub ucsc_host: UcscHost,

    pub cache_dir: String,
    //pub palette: Palette,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            file_paths: Vec::new(),
            reference: Reference::default(),
            backend: BackendType::default(), // Default backend
            ucsc_host: UcscHost::default(),
            cache_dir: shellexpand::tilde("~/.tgv").to_string(),
        }
    }
}

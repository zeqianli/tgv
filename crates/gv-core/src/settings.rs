use crate::tracks::UcscHost;
use crate::{alignment::is_url, error::TGVError, reference::Reference};
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

/// Variant file extensions. Bgzipped VCF files with a `.tbi` or `.csi` index, and BCF files
/// with a `.csi` index, are read by region.
pub const VARIANT_EXTENSIONS: [&str; 4] = [".vcf", ".vcf.gz", ".vcf.bgz", ".bcf"];

/// BED file extensions. Bgzipped BED files with a `.tbi` or `.csi` index, and bigBed files,
/// are read by region.
pub const BED_EXTENSIONS: [&str; 4] = [".bed", ".bed.gz", ".bb", ".bigbed"];

/// Classify input files and build the track paths.
///
/// Returns file paths in the same order as the input paths.
pub fn classify_and_build_tracks(files: &[String]) -> Result<Vec<FilePath>, TGVError> {
    for file in files {
        let lower = file.to_lowercase();
        if lower.ends_with(".fa")
            || lower.ends_with(".fasta")
            || lower.ends_with(".fa.gz")
            || lower.ends_with(".fasta.gz")
        {
            return Err(TGVError::CliError(
                "FASTA reference files must be passed with -g/--reference, not as positional input files.".to_string(),
            ));
        } else if lower.ends_with(".cram") {
            return Err(TGVError::CliError(
                "CRAM format is not yet supported as a CLI input format.".to_string(),
            ));
        } else if !(lower.ends_with(".bam")
            || VARIANT_EXTENSIONS
                .iter()
                .any(|extension| lower.ends_with(extension))
            || BED_EXTENSIONS
                .iter()
                .any(|extension| lower.ends_with(extension)))
        {
            return Err(TGVError::CliError(format!(
                "Unrecognized file format: {}. Supported track formats: .bam, {}, {}. Use -g for custom FASTA or 2bit reference genomes.",
                file,
                VARIANT_EXTENSIONS.join(", "),
                BED_EXTENSIONS.join(", "),
            )));
        }
    }

    let mut file_paths = Vec::new();
    for file in files {
        let lower = file.to_lowercase();
        if lower.ends_with(".bam") {
            let index = format!("{file}.bai");
            file_paths.push(FilePath::AlignmentPath(AlignmentPath::Bam {
                path: file.clone(),
                index,
                source: if is_url(file.as_str()) {
                    BamSource::S3
                } else {
                    BamSource::Local
                },
            }));
        } else if VARIANT_EXTENSIONS
            .iter()
            .any(|extension| lower.ends_with(extension))
        {
            file_paths.push(FilePath::VariantPath(file.clone()));
        } else if BED_EXTENSIONS
            .iter()
            .any(|extension| lower.ends_with(extension))
        {
            file_paths.push(FilePath::BedPath(file.clone()));
        }
    }

    Ok(file_paths)
}

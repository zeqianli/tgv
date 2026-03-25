use crate::{
    message::Message,
    rendering::{DARK_THEME, Palette},
};
use clap::{Parser, Subcommand, ValueEnum};
use gv_core::alignment::is_url;
use gv_core::error::TGVError;
use gv_core::message::Movement;
use gv_core::reference::Reference;
use gv_core::settings::{AlignmentPath, BackendType, BamSource};
use gv_core::tracks::UcscHost;
use strum::Display;

#[derive(Debug, Clone, Eq, PartialEq, ValueEnum)]
pub enum UcscHostCli {
    #[value(name = "us")]
    Us,
    #[value(name = "eu")]
    Eu,
    #[value(name = "auto")]
    Auto,
}

impl Into<UcscHost> for UcscHostCli {
    fn into(self) -> UcscHost {
        match self {
            UcscHostCli::Us => UcscHost::Us,
            UcscHostCli::Eu => UcscHost::Eu,
            UcscHostCli::Auto => UcscHost::auto(),
        }
    }
}

#[derive(Subcommand, Clone, Debug)]
pub enum Commands {
    /// Download command
    Download {
        /// Name to download
        reference: String,

        /// Cache directory
        #[arg(long = "cache-dir", default_value = "~/.tgv")]
        cache_dir: String,
    },

    /// List reference genomes on UCSC.
    List {
        /// List more reference genomes (UCSC common genomes (stored locally) and UCSC assemblies).
        #[arg(long = "more")]
        more: bool,

        /// List all reference genomes (UCSC common genomes (stored locally), UCSC assemblies, and all UCSC accessions).
        #[arg(long = "all")]
        all: bool,
    },
}

#[derive(Parser, Clone)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Input files. Supported formats: .bam, .cram, .vcf, .vcf.gz, .bed, .bed.gz, .fa, .fasta.
    /// Exactly one alignment file (.bam or .cram) may be provided. CRAM files require a FASTA
    /// reference file (.fa or .fasta) to also be provided here; its .fai index is inferred
    /// automatically. BAM/CRAM index files are inferred automatically (.bam.bai, .cram.crai).
    /// To set the viewer reference (separate from the CRAM decoding reference), use -g.
    #[arg(value_name = "files")]
    files: Vec<String>,

    /// Starting region. Supported formats: [chr]:[pos] (e.g. 12:25398142); [gene] (e.g. TP53).
    /// If not provided, TGV will find a default starting region.
    #[arg(short = 'r', long = "region")]
    region: Option<String>,

    /// Reference genome.
    /// TGV supports all UCSC assemblies and accessions. See `tgv list` or `tgv list --more`.
    /// Ignored when a FASTA file is provided as an input file.
    #[arg(short = 'g', long = "reference", default_value = Reference::HG38)]
    reference: String,

    /// Do not display the reference genome.
    /// This flag cannot be used when no alignment file is provided.
    #[arg(long)]
    no_reference: bool,

    /// If true, always use the local cache. Quit the application if local cache is not available.
    #[arg(long, default_value_t = false)]
    offline: bool,

    /// If true, always use the UCSC DB / API.
    #[arg(long, default_value_t = false)]
    online: bool,

    /// [For development only] Display messages in the terminal.
    #[arg(long)]
    debug: bool,

    /// Choose the UCSC host.
    #[arg(long, value_enum, default_value_t = UcscHostCli::Auto)]
    host: UcscHostCli,

    /// Cache directory
    #[arg(long, default_value = "~/.tgv")]
    cache_dir: String,

    /// Subcommand
    #[command(subcommand)]
    pub command: Option<Commands>,
}

impl Cli {
    pub fn initial_movement(&self) -> Result<Vec<Message>, TGVError> {
        let region_string = match &self.region {
            Some(region_string) => region_string,
            None => {
                return Ok(vec![Message::Core(gv_core::message::Message::Move(
                    gv_core::message::Movement::Default,
                ))]);
            } // Interpretation 1: go to default
        };

        // Check format
        let split = region_string.split(":").collect::<Vec<&str>>();

        match split.len() {
            //  gene name
            1 => Ok(vec![Message::Core(gv_core::message::Message::Move(
                gv_core::message::Movement::Gene(region_string.to_string()),
            ))]),
            2 =>
            // genome:position
            {
                split[1]
                    .parse::<u64>()
                    .map(|n| {
                        vec![Message::Core(gv_core::message::Message::Move(
                            Movement::ContigNamePosition(split[0].to_string(), n),
                        ))]
                    })
                    .map_err(|_| {
                        TGVError::CliError(format!("Invalid genome region: {}", region_string))
                    })
            }
            _ => Err(TGVError::CliError(format!(
                "Cannot interpret the region: {}",
                region_string
            ))),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Settings {
    pub core: gv_core::settings::Settings,
    pub initial_state_messages: Vec<Message>,
    pub test_mode: bool,

    pub debug: bool,
    pub palette: Palette,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            core: gv_core::settings::Settings::default(),

            initial_state_messages: vec![Movement::Default.into()],

            test_mode: false,

            debug: false,

            palette: DARK_THEME,
        }
    }
}

/// Settings to browse alignments
impl TryFrom<Cli> for Settings {
    type Error = TGVError;
    fn try_from(cli: Cli) -> Result<Self, TGVError> {
        // If this is a download command, it should be handled separately
        if cli.command.is_some() {
            return Err(TGVError::CliError(
                "Download command should be handled separately".to_string(),
            ));
        }

        // Classify input files by extension.
        let mut alignment_file: Option<String> = None;
        let mut vcf_path: Option<String> = None;
        let mut bed_path: Option<String> = None;
        let mut fasta_path: Option<String> = None;

        for file in &cli.files {
            let lower = file.to_lowercase();
            if lower.ends_with(".bam") || lower.ends_with(".cram") {
                if alignment_file.is_some() {
                    return Err(TGVError::CliError(
                        "Only one BAM or CRAM alignment file may be provided.".to_string(),
                    ));
                }
                alignment_file = Some(file.clone());
            } else if lower.ends_with(".vcf") || lower.ends_with(".vcf.gz") {
                if vcf_path.is_some() {
                    return Err(TGVError::CliError(
                        "Only one VCF file may be provided.".to_string(),
                    ));
                }
                vcf_path = Some(file.clone());
            } else if lower.ends_with(".bed") || lower.ends_with(".bed.gz") {
                if bed_path.is_some() {
                    return Err(TGVError::CliError(
                        "Only one BED file may be provided.".to_string(),
                    ));
                }
                bed_path = Some(file.clone());
            } else if lower.ends_with(".fa")
                || lower.ends_with(".fasta")
                || lower.ends_with(".fa.gz")
                || lower.ends_with(".fasta.gz")
            {
                if fasta_path.is_some() {
                    return Err(TGVError::CliError(
                        "Only one FASTA reference file may be provided.".to_string(),
                    ));
                }
                fasta_path = Some(file.clone());
            } else {
                return Err(TGVError::CliError(format!(
                    "Unrecognized file format: {}. Supported formats: .bam, .cram, .vcf, .vcf.gz, .bed, .bed.gz, .fa, .fasta",
                    file
                )));
            }
        }

        // CRAM requires a FASTA reference file.
        if let Some(ref ap) = alignment_file {
            if ap.to_lowercase().ends_with(".cram") && fasta_path.is_none() {
                return Err(TGVError::CliError(
                    "CRAM files require a reference FASTA file (.fa or .fasta) as input."
                        .to_string(),
                ));
            }
        }

        // Reference: the -g flag sets the viewer reference. A positional FASTA is only for CRAM
        // decoding and is independent of the viewer reference.
        let reference = if cli.no_reference {
            Reference::NoReference
        } else {
            Reference::from_str(&cli.reference)?
        };

        // Initial messages
        let initial_state_messages = cli.initial_movement()?;

        // Backend
        let backend = match (cli.offline, cli.online) {
            (true, true) => {
                return Err(TGVError::CliError(
                    "Both --offline and --online flags are used. Please use only one.".to_string(),
                ));
            }
            (true, false) => BackendType::Local,
            (false, true) => BackendType::Ucsc,
            (false, false) => BackendType::Default, // If local cache is available, use it. Otherwise, use UCSC DB / API.
        };

        // Additional validations:
        // 1. If no reference is provided, the initial state messages cannot contain GoToGene.
        if !reference.needs_track() {
            for m in initial_state_messages.iter() {
                if let Message::Core(gv_core::message::Message::Move(
                    gv_core::message::Movement::Gene(gene_name),
                )) = m
                {
                    return Err(TGVError::CliError(format!(
                        "The initial region cannot not be a gene name {} when no reference is provided. ",
                        gene_name
                    )));
                }
            }
        }

        // 2. An alignment file and reference cannot both be absent.
        if alignment_file.is_none() && cli.no_reference {
            return Err(TGVError::CliError(
                "Bam file and reference cannot both be none".to_string(),
            ));
        }

        // Build the AlignmentPath. Index files are always inferred from the alignment path.
        let alignment_path = match alignment_file {
            None => None,
            Some(ap) if ap.to_lowercase().ends_with(".bam") => {
                let index = format!("{}.bai", ap);
                Some(AlignmentPath::Bam {
                    path: ap.clone(),
                    index,
                    source: if is_url(ap.as_str()) {
                        BamSource::S3
                    } else {
                        BamSource::Local
                    },
                })
            }
            Some(ap) if ap.to_lowercase().ends_with(".cram") => {
                return Err(TGVError::CliError(format!(
                    "CRAM format is not yet supported."
                ))); // TODO: debug this.
                // fasta_path is guaranteed to be Some here by the earlier CRAM validation.
                let fasta = fasta_path.unwrap();
                let crai = format!("{}.crai", ap);
                let fai = format!("{}.fai", fasta);
                Some(AlignmentPath::Cram {
                    path: ap,
                    crai,
                    fasta,
                    fai,
                })
            }
            Some(ap) => {
                return Err(TGVError::CliError(format!(
                    "{ap} is not a valid alignment file path"
                )));
            }
        };

        // cache_dir: expand ~
        let cache_dir = shellexpand::tilde(&cli.cache_dir).to_string();

        Ok(Self {
            core: gv_core::settings::Settings {
                alignment_path,
                vcf_path,
                bed_path,
                reference,
                backend,
                ucsc_host: cli.host.into(),
                cache_dir,
            },
            initial_state_messages,

            test_mode: false,
            debug: cli.debug,
            palette: DARK_THEME,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::message::Message;
    use gv_core::reference::Reference;
    use gv_core::settings::{AlignmentPath, BamSource};
    use rstest::rstest;

    fn bam(path: &str) -> Option<AlignmentPath> {
        Some(AlignmentPath::Bam {
            path: path.to_string(),
            index: format!("{}.bai", path),
            source: BamSource::Local,
        })
    }

    #[rstest]
    #[case("tgv", Ok(Settings{
        ..Settings::default()}
    ))]
    #[case("tgv input.bam", Ok(Settings {
        core: gv_core::settings::Settings {
        alignment_path: bam("input.bam"),
        ..gv_core::settings::Settings::default()
        },
        ..Settings::default()
    }))]
    #[case("tgv input.bam some.bed", Ok(Settings {
        core: gv_core::settings::Settings {
        alignment_path: bam("input.bam"),
        bed_path: Some("some.bed".to_string()),
        ..gv_core::settings::Settings::default()
        },
        ..Settings::default()
    }))]
    #[case("tgv input.bam some.vcf", Ok(Settings {
        core: gv_core::settings::Settings {
        alignment_path: bam("input.bam"),
        vcf_path: Some("some.vcf".to_string()),
        ..gv_core::settings::Settings::default()
        },
        ..Settings::default()
    }))]
    #[case("tgv input.bam --offline", Ok(Settings {
        core: gv_core::settings::Settings {
        alignment_path: bam("input.bam"),
        backend: BackendType::Local,
        ..gv_core::settings::Settings::default()
        },
        ..Settings::default()
    }))]
    #[case("tgv input.bam --online", Ok(Settings {
        core: gv_core::settings::Settings {
        alignment_path: bam("input.bam"),
        backend: BackendType::Ucsc,
        ..gv_core::settings::Settings::default()},
        ..Settings::default()
    }))]
    #[case("tgv input.bam -r chr1:12345", Ok(Settings {
        core: gv_core::settings::Settings {
        alignment_path: bam("input.bam"),
        ..gv_core::settings::Settings::default()},
        initial_state_messages: vec![Movement::ContigNamePosition(
            "chr1".to_string(),
            12345,
        ).into()],
        ..Settings::default()
    }))]
    #[case("tgv input.bam -r chr1:invalid", Err(TGVError::CliError("".to_string())))]
    #[case("tgv input.bam -r chr1:12:12345", Err(TGVError::CliError("".to_string())))]
    #[case("tgv input.bam -r TP53", Ok(Settings {
        core: gv_core::settings::Settings {
        alignment_path: bam("input.bam"),
        ..gv_core::settings::Settings::default()},
        initial_state_messages: vec![Movement::Gene("TP53".to_string()).into()],
        ..Settings::default()
    }))]
    #[case("tgv input.bam -r TP53 -g hg19", Ok(Settings {
        core: gv_core::settings::Settings {
        alignment_path: bam("input.bam"),
        reference: Reference::Hg19,
        ..gv_core::settings::Settings::default()},
        initial_state_messages: vec![Movement::Gene("TP53".to_string()).into()],
        ..Settings::default()
    }))]
    #[case("tgv input.bam -r TP53 -g mm39", Ok(Settings {
        core: gv_core::settings::Settings {
        alignment_path: bam("input.bam"),
        reference: Reference::UcscGenome("mm39".to_string()),
        ..gv_core::settings::Settings::default()},
        initial_state_messages: vec![Movement::Gene("TP53".to_string()).into()],
        ..Settings::default()
    }))]
    #[case("tgv input.bam -r 1:12345 --no-reference", Ok(Settings {
        core: gv_core::settings::Settings {
        alignment_path: bam("input.bam"),
        reference: Reference::NoReference,
        ..gv_core::settings::Settings::default()},
        initial_state_messages: vec![Movement::ContigNamePosition(
            "1".to_string(),
            12345,
        ).into()],
        ..Settings::default()
    }))]
    #[case("tgv input.cram ref.fa", Ok(Settings {
        core: gv_core::settings::Settings {
        alignment_path: Some(AlignmentPath::Cram {
            path: "input.cram".to_string(),
            crai: "input.cram.crai".to_string(),
            fasta: "ref.fa".to_string(),
            fai: "ref.fa.fai".to_string(),
        }),
        ..gv_core::settings::Settings::default()},
        ..Settings::default()
    }))]
    #[case("tgv input.cram ref.fa -g hg19", Ok(Settings {
        core: gv_core::settings::Settings {
        alignment_path: Some(AlignmentPath::Cram {
            path: "input.cram".to_string(),
            crai: "input.cram.crai".to_string(),
            fasta: "ref.fa".to_string(),
            fai: "ref.fa.fai".to_string(),
        }),
        reference: Reference::Hg19,
        ..gv_core::settings::Settings::default()},
        ..Settings::default()
    }))]
    #[case("tgv input.cram", Err(TGVError::CliError("".to_string())))]
    #[case("tgv input.bam -r TP53 -g hg19 --no-reference", Err(TGVError::CliError("".to_string())))]
    #[case("tgv --no-reference", Err(TGVError::CliError("".to_string())))]
    #[case("tgv input.bam input2.bam", Err(TGVError::CliError("".to_string())))]
    #[case("tgv input.txt", Err(TGVError::CliError("".to_string())))]
    fn test_cli_parsing(
        #[case] command_line: &str,
        #[case] expected_settings: Result<Settings, TGVError>,
    ) {
        let cli = Cli::parse_from(shlex::split(command_line).unwrap());

        let settings = Settings::try_from(cli);

        match (&settings, &expected_settings) {
            (Ok(settings), Ok(expected)) => assert_eq!(*settings, *expected),
            (Err(e), Err(expected)) => {} // OK
            _ => panic!(
                "Unexpected CLI parsing result. Expected: {:?}, Got: {:?}",
                expected_settings, settings
            ),
        }
    }
}

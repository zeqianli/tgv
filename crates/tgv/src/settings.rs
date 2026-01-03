use crate::{message::Message, rendering::Palette};
use clap::{Parser, Subcommand, ValueEnum};
use gv_core::error::TGVError;
use gv_core::message::Movement;
use gv_core::reference::Reference;
use gv_core::settings::BackendType;
use gv_core::tracks::UcscHost;

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
    /// BAM file path. Must be sorted and indexed (with .bai file in the same directory).
    /// If not provided, only reference genome will be displayed.
    #[arg(value_name = "bam_path")]
    bam_path: Option<String>,

    /// VCF file path.
    #[arg(short = 'v', long = "vcf", value_name = "vcf_path")]
    vcf_path: Option<String>,

    /// BED file path
    #[arg(short = 'b', long = "bed", value_name = "bed_path")]
    bed_path: Option<String>,

    /// Bai file path.
    /// If not provided, .bai in the same directory as the BAM file will be used.
    #[arg(short = 'i', long = "index", value_name = "bai")]
    bai: Option<String>,

    /// Starting region. Supported formats: [chr]:[pos] (e.g. 12:25398142); [gene] (e.g. TP53).
    /// If not provided, TGV will find a default starting region.
    #[arg(short = 'r', long = "region")]
    region: Option<String>,

    /// Reference genome.
    /// TGV supports all UCSC assemblies and accessions. See `tgv --list` or `tgv --list-more`.
    #[arg(short = 'g', long = "reference", default_value = Reference::HG38)]
    reference: String,

    /// Do not display the reference genome.
    /// This flag cannot be used when no BAM file is provided.
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
        let region_string = match self.region_string {
            Some(region_string) => region_string,
            None => return Ok(vec![Message::GoToDefault]), // Interpretation 1: go to default
        };

        // Check format
        let split = region_string.split(":").collect::<Vec<&str>>();

        match split.len() {
            //  gene name
            1 => Ok(vec![Message::GoToGene(region_string.to_string())]),
            2 =>
            // genome:position
            {
                split[1]
                    .parse::<usize>()
                    .map(|n| {
                        vec![Message::Core(gv_core::message::Message::Move(
                            Movement::ContigNameCoordinate(split[0].to_string(), n),
                        ))]
                    })
                    .map_err(|_| {
                        Err(TGVError::CliError(format!(
                            "Invalid genome region: {}",
                            region_string
                        )))
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

        // Reference
        let reference = if cli.no_reference {
            Reference::NoReference
        } else {
            Reference::from_str(&cli.reference)?
        };

        // Initial messages
        let initial_state_messages = cli.initial_messages(cli.region)?;

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
        // 1. If no reference is provided, the initial state messages cannot contain GoToGene
        if !reference.needs_track() {
            for m in initial_state_messages.iter() {
                if let Message::GoToGene(gene_name) = m {
                    return Err(TGVError::CliError(format!(
                        "The initial region cannot not be a gene name {} when no reference is provided. ",
                        gene_name
                    )));
                }
            }
        }

        // 2. bam file and reference cannot both be none
        if cli.bam_path.is_none() && cli.no_reference {
            return Err(TGVError::CliError(
                "Bam file and reference cannot both be none".to_string(),
            ));
        }

        let bam_path = cli.bam_path.map(|bam_path| {
            let bai_path = cli.bai.unwrap_or(format!("{}.bai", bam_path.clone()));
            (bam_path, bai_path)
        });

        // cache_dir: expand ~
        let cache_dir = shellexpand::tilde(&cli.cache_dir).to_string();

        Ok(Self {
            core: gv_core::settings::Settings {
                bam_path: bam_path,
                vcf_path: cli.vcf_path,
                bed_path: cli.bed_path,
                reference,
                backend,
                ucsc_host: cli.host.into(),
                cache_dir,
            },
            initial_state_messages,

            test_mode: false,
            debug: cli.debug,
            //palette: DARK_THEME,
        })
    }
}

impl Settings {
    fn translate_initial_state_messages(
        region_string: Option<String>,
    ) -> Result<Vec<Message>, TGVError> {
    }
}

#[cfg(test)]
mod tests {
    // use super::*;

    // use crate::message::Message;
    // use gv_core::reference::Reference;
    // use rstest::rstest;

    // // Helper function to create default settings for comparison
    // fn default_settings() -> Settings {
    //     Settings {
    //         bam_path: None,
    //         bai_path: None,
    //         vcf_path: None,
    //         bed_path: None,
    //         reference: Reference::Hg38,
    //         backend: BackendType::Default, // Default backend
    //         initial_state_messages: vec![Message::GoToDefault],
    //         test_mode: false,
    //         debug: false,
    //         ucsc_host: UcscHost::Us,
    //         cache_dir: shellexpand::tilde("~/.tgv").to_string(),
    //         palette: DARK_THEME,
    //     }
    // }

    // #[rstest]
    // #[case("tgv", Ok(default_settings()))]
    // #[case("tgv input.bam", Ok(Settings {
    //     bam_path: Some("input.bam".to_string()),
    //     bai_path: Some("input.bam.bai".to_string()),
    //     ..default_settings()
    // }))]
    // #[case("tgv input.bam -b some.bed", Ok(Settings {
    //     bam_path: Some("input.bam".to_string()),
    //     bai_path: Some("input.bam.bai".to_string()),
    //     bed_path: Some("some.bed".to_string()),
    //     ..default_settings()
    // }))]
    // #[case("tgv input.bam -v some.vcf", Ok(Settings {
    //     bam_path: Some("input.bam".to_string()),
    //     bai_path: Some("input.bam.bai".to_string()),
    //     vcf_path: Some("some.vcf".to_string()),
    //     ..default_settings()
    // }))]
    // #[case("tgv input.bam --offline", Ok(Settings {
    //     bam_path: Some("input.bam".to_string()),
    //     bai_path: Some("input.bam.bai".to_string()),
    //     backend: BackendType::Local,
    //     ..default_settings()
    // }))]
    // #[case("tgv input.bam --online", Ok(Settings {
    //     bam_path: Some("input.bam".to_string()),
    //     bai_path: Some("input.bam.bai".to_string()),
    //     backend: BackendType::Ucsc,
    //     ..default_settings()
    // }))]
    // #[case("tgv input.bam -r chr1:12345", Ok(Settings {
    //     bam_path: Some("input.bam".to_string()),
    //     bai_path: Some("input.bam.bai".to_string()),
    //     initial_state_messages: vec![Message::GotoContigNameCoordinate(
    //         "chr1".to_string(),
    //         12345,
    //     )],
    //     ..default_settings()
    // }))]
    // #[case("tgv input.bam -r chr1:invalid", Err(TGVError::CliError("".to_string())))]
    // #[case("tgv input.bam -r chr1:12:12345", Err(TGVError::CliError("".to_string())))]
    // #[case("tgv input.bam -r TP53", Ok(Settings {
    //     bam_path: Some("input.bam".to_string()),
    //     bai_path: Some("input.bam.bai".to_string()),
    //     initial_state_messages: vec![Message::GoToGene("TP53".to_string())],
    //     ..default_settings()
    // }))]
    // #[case("tgv input.bam -r TP53 -g hg19", Ok(Settings {
    //     bam_path: Some("input.bam".to_string()),
    //     bai_path: Some("input.bam.bai".to_string()),
    //     reference: Reference::Hg19,
    //     initial_state_messages: vec![Message::GoToGene("TP53".to_string())],
    //     ..default_settings()
    // }))]
    // #[case("tgv input.bam -r TP53 -g mm39", Ok(Settings {
    //     bam_path: Some("input.bam".to_string()),
    //     bai_path: Some("input.bam.bai".to_string()),
    //     reference: Reference::UcscGenome("mm39".to_string()),
    //     initial_state_messages: vec![Message::GoToGene("TP53".to_string())],
    //     ..default_settings()
    // }))]
    // #[case("tgv input.bam -r 1:12345 --no-reference", Ok(Settings {
    //     bam_path: Some("input.bam".to_string()),
    //     bai_path: Some("input.bam.bai".to_string()),
    //     reference: Reference::NoReference,
    //     initial_state_messages: vec![Message::GotoContigNameCoordinate(
    //         "1".to_string(),
    //         12345,
    //     )],
    //     ..default_settings()
    // }))]
    // #[case("tgv input.bam -r TP53 -g hg19 --no-reference", Err(TGVError::CliError("".to_string())))]
    // #[case("tgv --no-reference", Err(TGVError::CliError("".to_string())))]
    // //#[case("tgv download test-name", Err(TGVError::CliError("".to_string())))]
    // // #[case("tgv download test-name --cache-dir /custom/dir", Err(TGVError::CliError("".to_string())))]
    // fn test_cli_parsing(
    //     #[case] command_line: &str,
    //     #[case] expected_settings: Result<Settings, TGVError>,
    // ) {
    //     let cli = Cli::parse_from(shlex::split(command_line).unwrap());

    //     let settings = Settings::new(cli.clone());

    //     match (&settings, &expected_settings) {
    //         (Ok(settings), Ok(expected)) => assert_eq!(*settings, *expected),
    //         (Err(e), Err(expected)) => {} // OK
    //         _ => panic!(
    //             "Unexpected CLI parsing result. Expected: {:?}, Got: {:?}",
    //             expected_settings, settings
    //         ),
    //     }
    // }
}

use crate::error::TGVError;
use crate::rendering::{Palette, DARK_THEME};
use crate::ucsc::UcscHost;
use crate::{message::StateMessage, reference::Reference};
use clap::{Parser, Subcommand, ValueEnum};

#[derive(Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum BackendType {
    /// Always use UCSC DB / API.
    Ucsc,

    /// Always use local database.
    Local,

    /// If local cache is available, use it. Otherwise, use UCSC DB / API.
    Default,
}

#[derive(Debug, Clone, Eq, PartialEq, ValueEnum)]
pub enum UcscHostCli {
    #[value(name = "us")]
    Us,
    #[value(name = "eu")]
    Eu,
    #[value(name = "auto")]
    Auto,
}

impl UcscHostCli {
    pub fn to_host(&self) -> UcscHost {
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

    /// Index file path.
    /// If not provided, .bai in the same directory as the BAM file will be used.
    #[arg(short = 'i', long = "index", value_name = "bai")]
    index: Option<String>,

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

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Settings {
    pub bam_path: Option<String>,
    pub bai_path: Option<String>,
    pub vcf_path: Option<String>,
    pub bed_path: Option<String>,
    pub reference: Option<Reference>,
    pub backend: BackendType,

    pub initial_state_messages: Vec<StateMessage>,

    pub test_mode: bool,

    pub debug: bool,

    pub ucsc_host: UcscHost,

    pub cache_dir: String,

    pub palette: Palette,
}

/// Settings to browse alignments
impl Settings {
    pub fn needs_alignment(&self) -> bool {
        self.bam_path.is_some()
    }

    pub fn needs_track(&self) -> bool {
        self.reference.is_some()
    }

    pub fn needs_sequence(&self) -> bool {
        self.reference.is_some()
    }

    pub fn needs_variants(&self) -> bool {
        self.vcf_path.is_some()
    }

    pub fn needs_bed(&self) -> bool {
        self.bed_path.is_some()
    }

    pub fn new(cli: Cli) -> Result<Self, TGVError> {
        // If this is a download command, it should be handled separately
        if cli.command.is_some() {
            return Err(TGVError::CliError(
                "Download command should be handled separately from Settings::new()".to_string(),
            ));
        }

        // TODO: fix this for different systems. This does not work on MacOS.
        // if let Some(bam_path) = &bam_path {
        //     if is_url(bam_path) && env::var("CURL_CA_BUNDLE").is_err() {
        //         // Workaround for rust-htslib:
        //         // https://github.com/rust-bio/rust-htslib/issues/404
        //         // TODO: is this same for MacOS?
        //         env::set_var("CURL_CA_BUNDLE", "/etc/ssl/certs/ca-certificates.crt");
        //     }
        // }

        // Reference
        let reference = if cli.no_reference {
            None
        } else {
            Some(Reference::from_str(&cli.reference)?)
        };

        // Initial messages
        let initial_state_messages = Self::translate_initial_state_messages(cli.region)?;

        // Backend
        let backend = match (cli.offline, cli.online) {
            (true, true) => {
                return Err(TGVError::CliError(
                    "Both --offline and --online flags are used. Please use only one of them."
                        .to_string(),
                ));
            }
            (true, false) => BackendType::Local,
            (false, true) => BackendType::Ucsc,
            (false, false) => BackendType::Default, // If local cache is available, use it. Otherwise, use UCSC DB / API.
        };

        // Additional validations:
        // 1. If no reference is provided, the initial state messages cannot contain GoToGene
        if reference.is_none() {
            for m in initial_state_messages.iter() {
                if let StateMessage::GoToGene(gene_name) = m {
                    return Err(TGVError::CliError(format!(
                        "The initial region cannot not be a gene name {} when no reference is provided. ",
                        gene_name
                    )));
                }
            }
        }

        // 2. bam file and reference cannot both be none
        if cli.bam_path.is_none() && reference.is_none() {
            return Err(TGVError::CliError(
                "Bam file and reference cannot both be none".to_string(),
            ));
        }

        // cache_dir: expand ~
        let cache_dir = shellexpand::tilde(&cli.cache_dir).to_string();

        Ok(Self {
            bam_path: cli.bam_path,
            bai_path: cli.index,
            vcf_path: cli.vcf_path,
            bed_path: cli.bed_path,
            reference,
            backend,
            initial_state_messages,
            ucsc_host: cli.host.to_host(),
            test_mode: false,
            debug: cli.debug,
            cache_dir,
            palette: DARK_THEME,
        })
    }

    pub fn test_mode(mut self) -> Self {
        self.test_mode = true;
        self
    }

    fn translate_initial_state_messages(
        region_string: Option<String>,
    ) -> Result<Vec<StateMessage>, TGVError> {
        let region_string = match region_string {
            Some(region_string) => region_string,
            None => return Ok(vec![StateMessage::GoToDefault]), // Interpretation 1: go to default
        };

        // Check format
        let split = region_string.split(":").collect::<Vec<&str>>();
        if split.len() > 2 {
            return Err(TGVError::CliError(format!(
                "Cannot interpret the region: {}",
                region_string
            )));
        }

        // Interpretation 2: genome:position
        if split.len() == 2 {
            match split[1].parse::<usize>() {
                Ok(n) => {
                    return Ok(vec![StateMessage::GotoContigNameCoordinate(
                        split[0].to_string(),
                        n,
                    )]);
                }
                Err(_) => {
                    return Err(TGVError::CliError(format!(
                        "Invalid genome region: {}",
                        region_string
                    )))
                }
            }
        }

        // Interpretation 3: gene name
        Ok(vec![StateMessage::GoToGene(region_string.to_string())])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::message::StateMessage;
    use crate::reference::Reference;
    use rstest::rstest;

    // Helper function to create default settings for comparison
    fn default_settings() -> Settings {
        Settings {
            bam_path: None,
            bai_path: None,
            vcf_path: None,
            bed_path: None,
            reference: Some(Reference::Hg38),
            backend: BackendType::Default, // Default backend
            initial_state_messages: vec![StateMessage::GoToDefault],
            test_mode: false,
            debug: false,
            ucsc_host: UcscHost::Us,
            cache_dir: shellexpand::tilde("~/.tgv").to_string(),
            palette: DARK_THEME,
        }
    }

    #[rstest]
    #[case("tgv", Ok(default_settings()))]
    #[case("tgv input.bam", Ok(Settings {
        bam_path: Some("input.bam".to_string()),
        ..default_settings()
    }))]
    #[case("tgv input.bam -b some.bed", Ok(Settings {
        bam_path: Some("input.bam".to_string()),
        bed_path: Some("some.bed".to_string()),
        ..default_settings()
    }))]
    #[case("tgv input.bam -v some.vcf", Ok(Settings {
        bam_path: Some("input.bam".to_string()),
        vcf_path: Some("some.vcf".to_string()),
        ..default_settings()
    }))]
    #[case("tgv input.bam", Ok(Settings {
        bam_path: Some("input.bam".to_string()),
        ..default_settings()
    }))]
    #[case("tgv input.bam --offline", Ok(Settings {
        bam_path: Some("input.bam".to_string()),
        backend: BackendType::Local,
        ..default_settings()
    }))]
    #[case("tgv input.bam --online", Ok(Settings {
        bam_path: Some("input.bam".to_string()),
        backend: BackendType::Ucsc,
        ..default_settings()
    }))]
    #[case("tgv input.bam -r chr1:12345", Ok(Settings {
        bam_path: Some("input.bam".to_string()),
        initial_state_messages: vec![StateMessage::GotoContigNameCoordinate(
            "chr1".to_string(),
            12345,
        )],
        ..default_settings()
    }))]
    #[case("tgv input.bam -r chr1:invalid", Err(TGVError::CliError("".to_string())))]
    #[case("tgv input.bam -r chr1:12:12345", Err(TGVError::CliError("".to_string())))]
    #[case("tgv input.bam -r TP53", Ok(Settings {
        bam_path: Some("input.bam".to_string()),
        initial_state_messages: vec![StateMessage::GoToGene("TP53".to_string())],
        ..default_settings()
    }))]
    #[case("tgv input.bam -r TP53 -g hg19", Ok(Settings {
        bam_path: Some("input.bam".to_string()),
        reference: Some(Reference::Hg19),
        initial_state_messages: vec![StateMessage::GoToGene("TP53".to_string())],
        ..default_settings()
    }))]
    #[case("tgv input.bam -r TP53 -g mm39", Ok(Settings {
        bam_path: Some("input.bam".to_string()),
        reference: Some(Reference::UcscGenome("mm39".to_string())),
        initial_state_messages: vec![StateMessage::GoToGene("TP53".to_string())],
        ..default_settings()
    }))]
    #[case("tgv input.bam -r 1:12345 --no-reference", Ok(Settings {
        bam_path: Some("input.bam".to_string()),
        reference: None,
        initial_state_messages: vec![StateMessage::GotoContigNameCoordinate(
            "1".to_string(),
            12345,
        )],
        ..default_settings()
    }))]
    #[case("tgv input.bam -r TP53 -g hg19 --no-reference", Err(TGVError::CliError("".to_string())))]
    #[case("tgv --no-reference", Err(TGVError::CliError("".to_string())))]
    //#[case("tgv download test-name", Err(TGVError::CliError("".to_string())))]
    // #[case("tgv download test-name --cache-dir /custom/dir", Err(TGVError::CliError("".to_string())))]
    fn test_cli_parsing(
        #[case] command_line: &str,
        #[case] expected_settings: Result<Settings, TGVError>,
    ) {
        let cli = Cli::parse_from(shlex::split(command_line).unwrap());

        let settings = Settings::new(cli.clone());

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

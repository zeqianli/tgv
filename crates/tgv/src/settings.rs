//! The `tgv` command line: the shared viewer options, and the subcommands only `tgv` has.

use clap::{Parser, Subcommand, ValueEnum};
use gv_core::prelude::*;
use gv_core::settings::Settings as CoreSettings;
use gv_core::tracks::UCSCDownloadSource;
pub use gv_viewer::settings::{Settings, UcscHostCli, ViewerArgs};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum UCSCDownloadSourceCli {
    /// Try UCSC MariaDB first, then fall back to HTTPS.
    #[value(name = "auto")]
    #[default]
    Automatic,

    /// Only download from UCSC MariaDB.
    #[value(name = "db")]
    MariaDbOnly,

    /// Only download from UCSC HTTPS table dumps.
    #[value(name = "https")]
    HttpsOnly,
}

impl From<UCSCDownloadSourceCli> for UCSCDownloadSource {
    fn from(value: UCSCDownloadSourceCli) -> Self {
        match value {
            UCSCDownloadSourceCli::Automatic => Self::Automatic,
            UCSCDownloadSourceCli::MariaDbOnly => Self::MariaDbOnly,
            UCSCDownloadSourceCli::HttpsOnly => Self::HttpsOnly,
        }
    }
}

#[derive(Subcommand, Clone, Debug)]
pub enum Commands {
    /// Serve datasets to agents through MCP over standard input and output.
    #[command(alias = "serve")]
    Mcp,
    /// Download reference data.
    Download {
        /// Reference genome to download.
        reference: String,

        /// Cache directory.
        #[arg(long = "cache-dir", default_value = "~/.tgv")]
        cache_dir: String,

        /// Select the UCSC download source.
        #[arg(long, value_enum, default_value_t)]
        source: UCSCDownloadSourceCli,
    },

    /// List reference genomes.
    List {
        /// List all UCSC assemblies instead of the common genome names.
        #[arg(long = "all")]
        all: bool,
    },
}

#[derive(Parser, Clone)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[command(flatten)]
    pub viewer: ViewerArgs,

    /// Subcommand
    #[command(subcommand)]
    pub command: Option<Commands>,
}

impl Cli {
    pub fn debug_enabled(&self) -> bool {
        self.viewer.debug
    }

    /// Builds the session defaults for `tgv mcp` from the global options.
    ///
    /// The MCP server loads files only through its `load_dataset` tool, so positional files
    /// and resumed sessions are rejected.
    pub fn mcp_settings(&self) -> Result<CoreSettings, TGVError> {
        if !self.viewer.files.is_empty() || self.viewer.resume.is_some() {
            return Err(TGVError::CliError(
                "Use the load_dataset MCP tool to load files when serving.".into(),
            ));
        }
        let mut settings = Settings::default();
        self.viewer.apply_overrides(&mut settings)?;
        Ok(settings.core)
    }

    pub fn resume_path(&self) -> Option<PathBuf> {
        self.viewer.resume_path()
    }

    /// Apply CLI overrides on top of an existing [`Settings`] loaded from a resumed session.
    pub fn apply_overrides(&self, settings: &mut Settings) -> Result<(), TGVError> {
        self.viewer.apply_overrides(settings)
    }
}

/// Build [`Settings`] directly from CLI args, without loading any session file.
///
/// Used when no session is resumed.
impl TryFrom<Cli> for Settings {
    type Error = TGVError;
    fn try_from(cli: Cli) -> Result<Self, TGVError> {
        if cli.command.is_some() {
            return Err(TGVError::CliError(
                "Download command should be handled separately".to_string(),
            ));
        }
        Settings::try_from(&cli.viewer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use gv_core::message::Movement;
    use gv_core::settings::{AlignmentPath, BackendType, BamSource};
    use rstest::rstest;

    fn bam(path: &str) -> AlignmentPath {
        AlignmentPath::Bam {
            path: path.to_string(),
            index: format!("{}.bai", path),
            source: BamSource::Local,
        }
    }

    #[rstest]
    #[case("tgv", Ok(Settings{
        ..Settings::default()}
    ))]
    #[case("tgv input.bam", Ok(Settings {
        files: vec![FilePath::AlignmentPath(bam("input.bam"))],
        ..Settings::default()
    }))]
    #[case("tgv input.bam some.bed", Ok(Settings {
        files: vec![
            FilePath::AlignmentPath(bam("input.bam")),
            FilePath::BedPath("some.bed".to_string()),
        ],
        ..Settings::default()
    }))]
    #[case("tgv input.bam some.vcf", Ok(Settings {
        files: vec![
            FilePath::AlignmentPath(bam("input.bam")),
            FilePath::VariantPath("some.vcf".to_string()),
        ],
        ..Settings::default()
    }))]
    #[case("tgv input.bam --offline", Ok(Settings {
        core: gv_core::settings::Settings {
        backend: BackendType::Local,
        ..gv_core::settings::Settings::default()
        },
        files: vec![FilePath::AlignmentPath(bam("input.bam"))],
        ..Settings::default()
    }))]
    #[case("tgv input.bam --online", Ok(Settings {
        core: gv_core::settings::Settings {
        backend: BackendType::Ucsc,
        ..gv_core::settings::Settings::default()},
        files: vec![FilePath::AlignmentPath(bam("input.bam"))],
        ..Settings::default()
    }))]
    #[case("tgv input.bam --debug", Ok(Settings {
        debug: true,
        files: vec![FilePath::AlignmentPath(bam("input.bam"))],
        ..Settings::default()
    }))]
    #[case("tgv input.bam -r chr1:12345", Ok(Settings {
        initial_actions: vec![Movement::ContigNamePosition(
            "chr1".to_string(),
            12345,
        ).into()],
        files: vec![FilePath::AlignmentPath(bam("input.bam"))],
        ..Settings::default()
    }))]
    #[case("tgv input.bam -r chr1:invalid", Err(TGVError::CliError("".to_string())))]
    #[case("tgv input.bam -r chr1:12:12345", Err(TGVError::CliError("".to_string())))]
    #[case("tgv input.bam -r TP53", Ok(Settings {
        initial_actions: vec![Movement::Gene("TP53".to_string()).into()],
        files: vec![FilePath::AlignmentPath(bam("input.bam"))],
        ..Settings::default()
    }))]
    #[case("tgv input.bam -r TP53 -g hg19", Ok(Settings {
        core: gv_core::settings::Settings {
        reference: Reference::Hg19,
        ..gv_core::settings::Settings::default()},
        initial_actions: vec![Movement::Gene("TP53".to_string()).into()],
        files: vec![FilePath::AlignmentPath(bam("input.bam"))],
        ..Settings::default()
    }))]
    #[case("tgv input.bam -r TP53 -g mm39", Ok(Settings {
        core: gv_core::settings::Settings {
        reference: Reference::UcscGenome("mm39".to_string()),
        ..gv_core::settings::Settings::default()},
        initial_actions: vec![Movement::Gene("TP53".to_string()).into()],
        files: vec![FilePath::AlignmentPath(bam("input.bam"))],
        ..Settings::default()
    }))]
    #[case("tgv input.bam -r 1:12345 --no-reference", Ok(Settings {
        core: gv_core::settings::Settings {
        reference: Reference::NoReference,
        ..gv_core::settings::Settings::default()},
        initial_actions: vec![Movement::ContigNamePosition(
            "1".to_string(),
            12345,
        ).into()],
        files: vec![FilePath::AlignmentPath(bam("input.bam"))],
        ..Settings::default()
    }))]
    #[case("tgv input.bam -r TP53 -g hg19 --no-reference", Err(TGVError::CliError("".to_string())))]
    #[case("tgv --no-reference", Err(TGVError::CliError("".to_string())))]
    #[case("tgv input.bam input2.bam", Ok(Settings {
        files: vec![
            FilePath::AlignmentPath(bam("input.bam")),
            FilePath::AlignmentPath(bam("input2.bam")),
        ],
        ..Settings::default()
    }))]
    #[case("tgv input.txt", Err(TGVError::CliError("".to_string())))]
    fn test_cli_parsing(
        #[case] command_line: &str,
        #[case] expected_settings: Result<Settings, TGVError>,
    ) {
        let cli = Cli::parse_from(shlex::split(command_line).unwrap());

        let settings = Settings::try_from(cli);

        match (&settings, &expected_settings) {
            (Ok(settings), Ok(expected)) => assert_eq!(*settings, *expected),
            (Err(_e), Err(_expected)) => {}
            _ => panic!(
                "Unexpected CLI parsing result. Expected: {:?}, Got: {:?}",
                expected_settings, settings
            ),
        }
    }
}

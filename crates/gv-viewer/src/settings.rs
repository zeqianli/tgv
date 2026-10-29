//! Settings for a viewer, and the command line options that every front end accepts.

use crate::message::NavigationAction;
use crate::{
    colors::{DARK_THEME, Palette},
    message::Action,
    session::SessionFile,
};
use clap::{Args, ValueEnum};
use gv_core::alignment::QualityEncodingSetting;
use gv_core::message::Movement;
use gv_core::prelude::*;
use gv_core::settings::{BackendType, classify_and_build_tracks};
use gv_core::tracks::UcscHost;
use std::path::PathBuf;

#[derive(Debug, Clone, Eq, PartialEq, ValueEnum)]
pub enum UcscHostCli {
    #[value(name = "us")]
    Us,
    #[value(name = "eu")]
    Eu,
    #[value(name = "auto")]
    Auto,
}

impl From<UcscHostCli> for UcscHost {
    fn from(val: UcscHostCli) -> Self {
        match val {
            UcscHostCli::Us => UcscHost::Us,
            UcscHostCli::Eu => UcscHost::Eu,
            UcscHostCli::Auto => UcscHost::auto(),
        }
    }
}

/// The command line options that open a viewer, shared by every front end.
#[derive(Args, Clone, Debug)]
pub struct ViewerArgs {
    /// Input files. Supported track formats: .bam, .vcf, .vcf.gz, .vcf.bgz, .bcf, .bed, .bed.gz, .bb, .bigbed.
    /// Bgzipped VCF and BED files with a .tbi or .csi index, BCF files with a .csi index, and
    /// bigBed files load by region.
    /// BAM index files are inferred automatically as .bam.bai.
    /// To set the viewer reference, including a custom FASTA or 2bit file, use -g.
    #[arg(value_name = "files")]
    pub files: Vec<String>,

    /// Starting region. Supported formats: [chr]:[pos] (e.g. 12:25398142); [gene] (e.g. TP53).
    /// If not provided, TGV will find a default starting region.
    #[arg(short = 'r', long = "region")]
    pub region: Option<String>,

    /// Reference genome. Defaults to hg38 when not specified and no session is resumed.
    /// TGV supports all UCSC assemblies and accessions. See `tgv list` or `tgv list --all`.
    /// Custom FASTA and 2bit reference files are also supported.
    #[arg(short = 'g', long = "reference")]
    pub reference: Option<String>,

    /// Do not display the reference genome.
    /// This flag cannot be used when no alignment file is provided.
    #[arg(long)]
    pub no_reference: bool,

    /// Always use the local cache. Quit if the local cache is not available.
    #[arg(long, default_value_t = false)]
    pub offline: bool,

    /// Always use the UCSC DB / API.
    #[arg(long, default_value_t = false)]
    pub online: bool,

    /// [For development only] Save all log information in the log file.
    #[arg(long)]
    pub debug: bool,

    /// Choose the UCSC host. Defaults to auto when not specified.
    #[arg(long, value_enum)]
    pub host: Option<UcscHostCli>,

    /// Cache directory. Defaults to ~/.tgv when not specified.
    #[arg(long)]
    pub cache_dir: Option<String>,

    /// Base-quality encoding of the alignment files: 33 (Phred+33, the BAM standard) or 64
    /// (Phred+64, from old Illumina FASTQ). Defaults to auto, which assumes 33 unless the first
    /// loaded reads with qualities hold a score above the Phred+33 maximum, Q93.
    #[arg(long, value_enum)]
    pub phred: Option<QualityEncodingSetting>,

    /// Resume a saved session: a name in `~/.tgv/sessions/` or a path to a session TOML file.
    #[arg(long)]
    pub resume: Option<String>,
}

impl ViewerArgs {
    pub fn debug_enabled(&self) -> bool {
        self.debug
    }

    pub fn initial_movement(&self) -> Result<Vec<Action>, TGVError> {
        let region_string = match &self.region {
            Some(region_string) => region_string,
            None => {
                return Ok(vec![Action::Navigate(NavigationAction::Move(
                    gv_core::message::Movement::Default,
                ))]);
            }
        };

        let split = region_string.split(":").collect::<Vec<&str>>();

        match split.len() {
            1 => Ok(vec![Action::Navigate(NavigationAction::Move(
                gv_core::message::Movement::Gene(region_string.to_string()),
            ))]),
            2 => split[1]
                .parse::<u64>()
                .map(|n| {
                    vec![Action::Navigate(NavigationAction::Move(
                        Movement::ContigNamePosition(split[0].to_string(), n),
                    ))]
                })
                .map_err(|_| {
                    TGVError::CliError(format!("Invalid genome region: {}", region_string))
                }),
            _ => Err(TGVError::CliError(format!(
                "Cannot interpret the region: {}",
                region_string
            ))),
        }
    }

    pub fn resume_path(&self) -> Option<PathBuf> {
        self.resume.as_deref().map(SessionFile::resolve_path)
    }

    /// Apply CLI overrides on top of an existing [`Settings`] loaded from a resumed session.
    ///
    /// Only fields that were explicitly provided on the command line are overridden.
    pub fn apply_overrides(&self, settings: &mut Settings) -> Result<(), TGVError> {
        // Reference override.
        if self.no_reference {
            settings.core.reference = Reference::NoReference;
        } else if let Some(ref r) = self.reference {
            settings.core.reference = r.parse::<Reference>()?;
        }

        // Region / initial locus override.
        if self.region.is_some() {
            settings.initial_actions = self.initial_movement()?;
        }

        // Track override: if any files were provided, replace all session tracks.
        if !self.files.is_empty() {
            settings.files = classify_and_build_tracks(&self.files)?;
        }

        // Backend override: only when explicitly requested.
        match (self.offline, self.online) {
            (true, true) => {
                return Err(TGVError::CliError(
                    "Both --offline and --online flags are used. Please use only one.".to_string(),
                ));
            }
            (true, false) => settings.core.backend = BackendType::Local,
            (false, true) => settings.core.backend = BackendType::Ucsc,
            (false, false) => {}
        }

        if let Some(ref h) = self.host {
            settings.core.ucsc_host = h.clone().into();
        }

        if let Some(ref d) = self.cache_dir {
            settings.core.cache_dir = shellexpand::tilde(d).to_string();
        }

        if let Some(phred) = self.phred {
            settings.core.quality_encoding = phred;
        }

        settings.debug = self.debug_enabled();

        // Validate: if no reference is provided, the initial messages cannot contain GoToGene.
        if !settings.core.reference.needs_track() {
            for m in settings.initial_actions.iter() {
                if let Action::Navigate(NavigationAction::Move(gv_core::message::Movement::Gene(
                    gene_name,
                ))) = m
                {
                    return Err(TGVError::CliError(format!(
                        "The initial region cannot be a gene name {} when no reference is provided.",
                        gene_name
                    )));
                }
            }
        }

        // Validate: input data and reference cannot both be absent.
        if settings.files.is_empty() && settings.core.reference == Reference::NoReference {
            return Err(TGVError::CliError(
                "Input files and reference cannot both be none".to_string(),
            ));
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub core: gv_core::settings::Settings,
    /// The files to open at startup. Once the app runs, its dataset's track registry records
    /// the files it shows.
    pub files: Vec<FilePath>,
    pub initial_actions: Vec<Action>,
    /// Draws one frame and stops, for tests.
    pub test_mode: bool,

    pub debug: bool,
    pub palette: Palette,

    /// Initial zoom to restore from a session file, in bases per screen unit. `None` uses the
    /// default zoom.
    pub zoom: Option<f64>,

    /// The active session, set by `--resume` or the last `:w NAME`. `None` when no session is active.
    pub session_path: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            core: gv_core::settings::Settings::default(),
            files: Vec::new(),

            initial_actions: vec![Movement::Default.into()],

            test_mode: false,

            debug: false,

            palette: DARK_THEME,

            zoom: None,

            session_path: None,
        }
    }
}

/// Build [`Settings`] directly from CLI args, without loading any session file.
///
/// Used when no session is resumed.
impl TryFrom<&ViewerArgs> for Settings {
    type Error = TGVError;
    fn try_from(cli: &ViewerArgs) -> Result<Self, TGVError> {
        let reference = if cli.no_reference {
            Reference::NoReference
        } else {
            cli.reference
                .as_deref()
                .unwrap_or(Reference::HG38)
                .parse::<Reference>()?
        };

        let initial_actions = cli.initial_movement()?;

        let backend = match (cli.offline, cli.online) {
            (true, true) => {
                return Err(TGVError::CliError(
                    "Both --offline and --online flags are used. Please use only one.".to_string(),
                ));
            }
            (true, false) => BackendType::Local,
            (false, true) => BackendType::Ucsc,
            (false, false) => BackendType::Default,
        };

        // Validate: no-reference + gene movement is invalid.
        if !reference.needs_track() {
            for m in initial_actions.iter() {
                if let Action::Navigate(NavigationAction::Move(gv_core::message::Movement::Gene(
                    gene_name,
                ))) = m
                {
                    return Err(TGVError::CliError(format!(
                        "The initial region cannot not be a gene name {} when no reference is provided. ",
                        gene_name
                    )));
                }
            }
        }

        // Input data and reference cannot both be absent.
        if cli.files.is_empty() && cli.no_reference {
            return Err(TGVError::CliError(
                "Input files and reference cannot both be none".to_string(),
            ));
        }

        let files = classify_and_build_tracks(&cli.files)?;

        let cache_dir =
            shellexpand::tilde(cli.cache_dir.as_deref().unwrap_or("~/.tgv")).to_string();
        let debug = cli.debug_enabled();

        Ok(Self {
            core: gv_core::settings::Settings {
                reference,
                backend,
                ucsc_host: cli.host.clone().unwrap_or(UcscHostCli::Auto).into(),
                cache_dir,
                quality_encoding: cli.phred.unwrap_or_default(),
            },
            files,
            initial_actions,

            test_mode: false,
            debug,
            palette: DARK_THEME,
            zoom: None,
            session_path: None,
        })
    }
}

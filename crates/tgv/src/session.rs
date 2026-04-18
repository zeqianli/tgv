//! Session file support: read and write `~/.tgv/sessions/*.toml`.
//!
//! A session file is a snapshot of the current [`App`] state that can be
//! restored on the next launch. The file format is documented in the tgv
//! book under "Session files".

use crate::{app::App, message::Message, settings::Settings};
use gv_core::{
    alignment::is_url,
    error::TGVError,
    message::Movement,
    reference::Reference,
    settings::{AlignmentPath, BackendType, BamSource},
    tracks::UcscHost,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const CURRENT_VERSION: u32 = 1;

/// On-disk representation of a tgv session.
///
/// Serialize with [`toml::to_string_pretty`], deserialize with [`SessionFile::parse`].
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionFile {
    pub version: u32,
    pub locus: String,
    pub genome: Reference,
    /// `"us"`, `"eu"`, or `"auto"`. Resolved to a concrete host on load.
    pub ucsc_host: UcscHost,
    /// Bases per character.
    pub zoom: u64,
    #[serde(default)]
    pub tracks: Vec<TrackEntry>,
}

impl Default for SessionFile {
    fn default() -> Self {
        SessionFile {
            version: CURRENT_VERSION,
            locus: "chr1:1".to_string(),
            genome: Reference::default(),
            ucsc_host: UcscHost::auto(),
            zoom: 1,
            tracks: Vec::new(),
        }
    }
}

/// One entry in the `[[tracks]]` array.
#[derive(Debug, Serialize, Deserialize)]
pub struct TrackEntry {
    pub path: String,
    /// Index file. Inferred from `path` when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<String>,
    /// CRAM only: FASTA reference used for decoding (separate from the viewer reference).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    /// CRAM only: `.fai` index for the decoding FASTA. Inferred as `reference + ".fai"` when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference_index: Option<String>,
}

// ─── AlignmentPath → TrackEntry ─────────────────────────────────────────────

impl TryFrom<&AlignmentPath> for TrackEntry {
    type Error = TGVError;

    fn try_from(ap: &AlignmentPath) -> Result<Self, TGVError> {
        match ap {
            AlignmentPath::Bam { path, index, .. } => Ok(TrackEntry {
                path: path.clone(),
                index: Some(index.clone()),
                reference: None,
                reference_index: None,
            }),
            AlignmentPath::Cram {
                path,
                crai,
                fasta,
                fai,
            } => Ok(TrackEntry {
                path: path.clone(),
                index: Some(crai.clone()),
                reference: Some(fasta.clone()),
                reference_index: Some(fai.clone()),
            }),
        }
    }
}

// ─── I/O ────────────────────────────────────────────────────────────────────

impl SessionFile {
    /// Default path used when no explicit session path is given.
    pub fn default_path() -> PathBuf {
        PathBuf::from(shellexpand::tilde("~/.tgv/sessions/default.toml").as_ref())
    }

    /// Read and parse a session file from `path`.
    pub fn from_path(path: &Path) -> Result<Self, TGVError> {
        Self::parse(&std::fs::read_to_string(path)?)
    }

    /// Parse a session file from a TOML string.
    pub fn parse(content: &str) -> Result<Self, TGVError> {
        let session: Self = toml::from_str(content)
            .map_err(|e| TGVError::ParsingError(format!("Failed to parse session file: {e}")))?;
        if session.version != CURRENT_VERSION {
            return Err(TGVError::ParsingError(format!(
                "Unsupported session file version {}. Expected {}.",
                session.version, CURRENT_VERSION
            )));
        }
        Ok(session)
    }

    /// Serialize `self` to TOML and write it to `path`, creating parent directories as needed.
    pub fn write_to_path(&self, path: &Path) -> Result<(), TGVError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)
            .map_err(|e| TGVError::ParsingError(format!("Failed to serialize session: {e}")))?;
        std::fs::write(path, content)?;
        Ok(())
    }
}

// ─── Locus string parsing ────────────────────────────────────────────────────

/// Parse a locus string (`"chr1:100"` or a gene name) into initial movement messages.
pub fn parse_locus(locus: &str) -> Result<Vec<Message>, TGVError> {
    let parts: Vec<&str> = locus.split(':').collect();
    match parts.len() {
        1 => Ok(vec![Message::Core(gv_core::message::Message::Move(
            Movement::Gene(locus.to_string()),
        ))]),
        2 => parts[1]
            .parse::<u64>()
            .map(|n| {
                vec![Message::Core(gv_core::message::Message::Move(
                    Movement::ContigNamePosition(parts[0].to_string(), n),
                ))]
            })
            .map_err(|_| {
                TGVError::ParsingError(format!("Invalid position in locus \"{locus}\""))
            }),
        _ => Err(TGVError::ParsingError(format!(
            "Invalid locus format \"{locus}\": expected \"contig:position\" or a gene name"
        ))),
    }
}

// ─── SessionFile → Settings ──────────────────────────────────────────────────

impl TryFrom<SessionFile> for Settings {
    type Error = TGVError;

    fn try_from(session: SessionFile) -> Result<Self, TGVError> {
        let initial_state_messages = parse_locus(&session.locus)?;

        let mut alignment_path: Option<AlignmentPath> = None;
        let mut vcf_path: Option<String> = None;
        let mut bed_path: Option<String> = None;

        for track in session.tracks {
            let lower = track.path.to_lowercase();
            if lower.ends_with(".bam") {
                let index = track
                    .index
                    .unwrap_or_else(|| format!("{}.bai", track.path));
                alignment_path = Some(AlignmentPath::Bam {
                    source: if is_url(&track.path) {
                        BamSource::S3
                    } else {
                        BamSource::Local
                    },
                    path: track.path,
                    index,
                });
            } else if lower.ends_with(".cram") {
                let crai = track
                    .index
                    .unwrap_or_else(|| format!("{}.crai", track.path));
                let fasta = track.reference.ok_or_else(|| {
                    TGVError::ParsingError(format!(
                        "CRAM track \"{}\" requires a `reference` field in the session file",
                        track.path
                    ))
                })?;
                let fai = track
                    .reference_index
                    .unwrap_or_else(|| format!("{fasta}.fai"));
                alignment_path = Some(AlignmentPath::Cram {
                    path: track.path,
                    crai,
                    fasta,
                    fai,
                });
            } else if lower.ends_with(".vcf") || lower.ends_with(".vcf.gz") {
                vcf_path = Some(track.path);
            } else if lower.ends_with(".bed") || lower.ends_with(".bed.gz") {
                bed_path = Some(track.path);
            }
        }

        Ok(Settings {
            core: gv_core::settings::Settings {
                alignment_path,
                vcf_path,
                bed_path,
                reference: session.genome,
                backend: BackendType::Default,
                ucsc_host: session.ucsc_host,
                cache_dir: gv_core::settings::Settings::default().cache_dir,
            },
            initial_state_messages,
            zoom: Some(session.zoom),
            test_mode: false,
            debug: false,
            palette: crate::rendering::DARK_THEME,
        })
    }
}

// ─── App → SessionFile ───────────────────────────────────────────────────────

/// Snapshot the current [`App`] state into a [`SessionFile`].
///
/// The locus is taken from `app.alignment_view.focus` (the live viewport position),
/// not from `app.settings.initial_state_messages`.
impl TryFrom<&App> for SessionFile {
    type Error = TGVError;

    fn try_from(app: &App) -> Result<Self, TGVError> {
        let locus = app
            .alignment_view
            .focus
            .to_locus_str(&app.state.contig_header)?;

        let mut tracks = Vec::new();

        if let Some(ap) = &app.settings.core.alignment_path {
            tracks.push(TrackEntry::try_from(ap)?);
        }

        if let Some(vcf) = &app.settings.core.vcf_path {
            tracks.push(TrackEntry {
                path: vcf.clone(),
                index: None,
                reference: None,
                reference_index: None,
            });
        }

        if let Some(bed) = &app.settings.core.bed_path {
            tracks.push(TrackEntry {
                path: bed.clone(),
                index: None,
                reference: None,
                reference_index: None,
            });
        }

        Ok(SessionFile {
            version: CURRENT_VERSION,
            locus,
            genome: app.settings.core.reference.clone(),
            ucsc_host: app.settings.core.ucsc_host.clone(),
            zoom: app.alignment_view.zoom,
            tracks,
        })
    }
}

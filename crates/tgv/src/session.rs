//! Session file support: read and write `~/.tgv/sessions/*.toml`.
//!
//! A session file is a snapshot of the current [`App`] state that can be
//! restored on the next launch. The file format is documented in the tgv
//! book under "Session files".

use crate::app::App;
use gv_core::{
    error::TGVError,
    reference::Reference,
    settings::AlignmentPath,
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
    pub cache_dir: String,
    /// Bases per character. Omitted when absent (uses the viewer default of 1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zoom: Option<u64>,
    #[serde(default)]
    pub tracks: Vec<TrackEntry>,
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
        toml::from_str(content)
            .map_err(|e| TGVError::ParsingError(format!("Failed to parse session file: {e}")))
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

        let genome = app.settings.core.reference.clone();
        let ucsc_host = app.settings.core.ucsc_host.clone();
        let cache_dir = app.settings.core.cache_dir.clone();
        let zoom = Some(app.alignment_view.zoom);

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
            genome,
            ucsc_host,
            cache_dir,
            zoom,
            tracks,
        })
    }
}

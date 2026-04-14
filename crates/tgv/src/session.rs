//! Session file support: read and write `~/.tgv/sessions/*.toml`.
//!
//! A session file is a snapshot of the current [`App`] state that can be
//! restored on the next launch. The file format is documented in the tgv
//! book under "Session files".

use crate::{app::App, message::Message, rendering::DARK_THEME, settings::Settings};
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
    #[serde(default = "default_genome")]
    pub genome: String,
    /// `"us"`, `"eu"`, or `"auto"`. Resolved to a concrete host on load.
    #[serde(default = "default_ucsc_host")]
    pub ucsc_host: String,
    #[serde(default = "default_cache_dir")]
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

fn default_genome() -> String {
    Reference::HG38.to_string()
}

fn default_ucsc_host() -> String {
    "auto".to_string()
}

fn default_cache_dir() -> String {
    shellexpand::tilde("~/.tgv").to_string()
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
        let content = std::fs::read_to_string(path)?;
        Self::parse(&content)
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

        let genome = app.settings.core.reference.to_string();
        let ucsc_host = match app.settings.core.ucsc_host {
            UcscHost::Us => "us",
            UcscHost::Eu => "eu",
        }
        .to_string();
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

// ─── SessionFile → Settings ─────────────────────────────────────────────────

/// Build a [`Settings`] from a parsed session file.
///
/// The `backend` field is not stored in the session file; it defaults to
/// [`BackendType::Default`] on load.
impl TryFrom<SessionFile> for Settings {
    type Error = TGVError;

    fn try_from(session: SessionFile) -> Result<Self, TGVError> {
        if session.version != CURRENT_VERSION {
            return Err(TGVError::ParsingError(format!(
                "Unsupported session version {}; expected {}.",
                session.version, CURRENT_VERSION
            )));
        }

        let initial_state_messages = parse_locus(&session.locus)?;
        let reference = Reference::from_str(&session.genome)?;
        let ucsc_host = session.ucsc_host.parse::<UcscHost>()?;
        let cache_dir = shellexpand::tilde(&session.cache_dir).to_string();

        let mut alignment_path: Option<AlignmentPath> = None;
        let mut vcf_path: Option<String> = None;
        let mut bed_path: Option<String> = None;

        for track in session.tracks {
            let lower = track.path.to_lowercase();
            if lower.ends_with(".bam") {
                if alignment_path.is_some() {
                    return Err(TGVError::CliError(
                        "Only one alignment file may be provided.".to_string(),
                    ));
                }
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
                if alignment_path.is_some() {
                    return Err(TGVError::CliError(
                        "Only one alignment file may be provided.".to_string(),
                    ));
                }
                let fasta = track.reference.ok_or_else(|| {
                    TGVError::ParsingError("CRAM tracks require a `reference` field.".to_string())
                })?;
                let crai = track
                    .index
                    .unwrap_or_else(|| format!("{}.crai", track.path));
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
                if vcf_path.is_some() {
                    return Err(TGVError::CliError(
                        "Only one VCF file may be provided.".to_string(),
                    ));
                }
                vcf_path = Some(track.path);
            } else if lower.ends_with(".bed") || lower.ends_with(".bed.gz") {
                if bed_path.is_some() {
                    return Err(TGVError::CliError(
                        "Only one BED file may be provided.".to_string(),
                    ));
                }
                bed_path = Some(track.path);
            } else {
                return Err(TGVError::ParsingError(format!(
                    "Unrecognized track file format: `{}`.",
                    track.path
                )));
            }
        }

        // Gene locus requires a reference with track support.
        if !reference.needs_track() {
            for m in initial_state_messages.iter() {
                if let Message::Core(gv_core::message::Message::Move(Movement::Gene(gene))) = m {
                    return Err(TGVError::ParsingError(format!(
                        "The locus cannot be a gene name `{gene}` when no reference is provided.",
                    )));
                }
            }
        }

        // No-reference requires an alignment file.
        if matches!(reference, Reference::NoReference) && alignment_path.is_none() {
            return Err(TGVError::ParsingError(
                "An alignment file is required when no reference genome is set.".to_string(),
            ));
        }

        Ok(Settings {
            core: gv_core::settings::Settings {
                alignment_path,
                vcf_path,
                bed_path,
                reference,
                backend: BackendType::Default,
                ucsc_host,
                cache_dir,
            },
            initial_state_messages,
            test_mode: false,
            debug: false,
            palette: DARK_THEME,
            zoom: session.zoom,
        })
    }
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Parse a locus string (`"contig:pos"` or `"gene"`) into initial movement messages.
fn parse_locus(locus: &str) -> Result<Vec<Message>, TGVError> {
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
                TGVError::ParsingError(format!("Invalid position in locus `{locus}`."))
            }),
        _ => Err(TGVError::ParsingError(format!(
            "Cannot parse locus `{locus}`: expected `contig:position` or a gene name."
        ))),
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use gv_core::{
        reference::Reference,
        settings::{AlignmentPath, BackendType, BamSource},
        tracks::UcscHost,
    };
    use rstest::rstest;

    // ── UcscHost ─────────────────────────────────────────────────────────────

    #[rstest]
    #[case("us", UcscHost::Us)]
    #[case("eu", UcscHost::Eu)]
    fn test_ucsc_host_parse(#[case] input: &str, #[case] expected: UcscHost) {
        assert_eq!(input.parse::<UcscHost>().unwrap(), expected);
    }

    #[test]
    fn test_ucsc_host_parse_auto_resolves() {
        // "auto" must resolve to one of the concrete variants without error.
        let host = "auto".parse::<UcscHost>().unwrap();
        assert!(matches!(host, UcscHost::Us | UcscHost::Eu));
    }

    #[test]
    fn test_ucsc_host_parse_invalid() {
        assert!(matches!(
            "invalid".parse::<UcscHost>(),
            Err(TGVError::ParsingError(_))
        ));
    }

    // ── AlignmentPath → TrackEntry ───────────────────────────────────────────

    #[test]
    fn test_track_entry_from_bam() {
        let ap = AlignmentPath::Bam {
            path: "/data/sample.bam".to_string(),
            index: "/data/sample.bam.bai".to_string(),
            source: BamSource::Local,
        };
        let entry = TrackEntry::try_from(&ap).unwrap();
        assert_eq!(entry.path, "/data/sample.bam");
        assert_eq!(entry.index.as_deref(), Some("/data/sample.bam.bai"));
        assert!(entry.reference.is_none());
    }

    #[test]
    fn test_track_entry_from_cram() {
        let ap = AlignmentPath::Cram {
            path: "/data/sample.cram".to_string(),
            crai: "/data/sample.cram.crai".to_string(),
            fasta: "/data/ref.fa".to_string(),
            fai: "/data/ref.fa.fai".to_string(),
        };
        let entry = TrackEntry::try_from(&ap).unwrap();
        assert_eq!(entry.path, "/data/sample.cram");
        assert_eq!(entry.index.as_deref(), Some("/data/sample.cram.crai"));
        assert_eq!(entry.reference.as_deref(), Some("/data/ref.fa"));
        assert_eq!(entry.reference_index.as_deref(), Some("/data/ref.fa.fai"));
    }

    // ── parse_locus ──────────────────────────────────────────────────────────

    #[rstest]
    #[case("chr17:7572659", Movement::ContigNamePosition("chr17".to_string(), 7572659))]
    #[case("1:1000", Movement::ContigNamePosition("1".to_string(), 1000))]
    fn test_parse_locus_contig_position(#[case] input: &str, #[case] expected: Movement) {
        let msgs = parse_locus(input).unwrap();
        assert_eq!(
            msgs,
            vec![Message::Core(gv_core::message::Message::Move(expected))]
        );
    }

    #[test]
    fn test_parse_locus_gene() {
        let msgs = parse_locus("KRAS").unwrap();
        assert_eq!(
            msgs,
            vec![Message::Core(gv_core::message::Message::Move(
                Movement::Gene("KRAS".to_string())
            ))]
        );
    }

    #[test]
    fn test_parse_locus_invalid_position() {
        assert!(matches!(
            parse_locus("chr1:not_a_number"),
            Err(TGVError::ParsingError(_))
        ));
    }

    #[test]
    fn test_parse_locus_too_many_colons() {
        assert!(matches!(
            parse_locus("chr1:1000:extra"),
            Err(TGVError::ParsingError(_))
        ));
    }

    // ── SessionFile::parse ───────────────────────────────────────────────────

    #[test]
    fn test_parse_minimal() {
        let toml = r#"
version = 1
locus = "chr1:1000"
"#;
        let session = SessionFile::parse(toml).unwrap();
        assert_eq!(session.version, 1);
        assert_eq!(session.locus, "chr1:1000");
        assert_eq!(session.genome, "hg38");
        assert_eq!(session.ucsc_host, "auto");
        assert!(session.tracks.is_empty());
    }

    #[test]
    fn test_parse_with_bam_track() {
        let toml = r#"
version = 1
locus = "chr17:7572659"
genome = "hg19"

[[tracks]]
path = "/data/sample.bam"
"#;
        let session = SessionFile::parse(toml).unwrap();
        assert_eq!(session.tracks.len(), 1);
        assert_eq!(session.tracks[0].path, "/data/sample.bam");
        assert_eq!(session.tracks[0].index, None);
    }

    // ── SessionFile → Settings ───────────────────────────────────────────────

    #[test]
    fn test_settings_from_session_minimal() {
        let session = SessionFile::parse(
            r#"
version = 1
locus = "chr1:1000"
"#,
        )
        .unwrap();
        let settings = Settings::try_from(session).unwrap();
        assert_eq!(
            settings.initial_state_messages,
            vec![Message::Core(gv_core::message::Message::Move(
                Movement::ContigNamePosition("chr1".to_string(), 1000)
            ))]
        );
        assert_eq!(settings.core.reference, Reference::Hg38);
    }

    #[test]
    fn test_settings_from_session_invalid_version() {
        let session = SessionFile {
            version: 99,
            locus: "chr1:1000".to_string(),
            genome: "hg38".to_string(),
            ucsc_host: "us".to_string(),
            cache_dir: "~/.tgv".to_string(),
            zoom: None,
            tracks: vec![],
        };
        assert!(matches!(
            Settings::try_from(session),
            Err(TGVError::ParsingError(_))
        ));
    }

    #[test]
    fn test_settings_from_session_bam_index_inferred() {
        let session = SessionFile::parse(
            r#"
version = 1
locus = "chr1:1000"

[[tracks]]
path = "/data/sample.bam"
"#,
        )
        .unwrap();
        let settings = Settings::try_from(session).unwrap();
        assert!(matches!(
            settings.core.alignment_path,
            Some(AlignmentPath::Bam { ref index, .. }) if index == "/data/sample.bam.bai"
        ));
    }

    #[test]
    fn test_settings_from_session_cram_missing_reference() {
        let session = SessionFile::parse(
            r#"
version = 1
locus = "chr1:1000"

[[tracks]]
path = "/data/sample.cram"
"#,
        )
        .unwrap();
        assert!(matches!(
            Settings::try_from(session),
            Err(TGVError::ParsingError(_))
        ));
    }

    #[test]
    fn test_settings_from_session_duplicate_bam() {
        let session = SessionFile::parse(
            r#"
version = 1
locus = "chr1:1000"

[[tracks]]
path = "/data/a.bam"

[[tracks]]
path = "/data/b.bam"
"#,
        )
        .unwrap();
        assert!(matches!(
            Settings::try_from(session),
            Err(TGVError::CliError(_))
        ));
    }

    // ── File I/O ─────────────────────────────────────────────────────────────

    #[test]
    fn test_write_and_read() {
        let session = SessionFile {
            version: CURRENT_VERSION,
            locus: "chr17:7572659".to_string(),
            genome: "hg38".to_string(),
            ucsc_host: "us".to_string(),
            cache_dir: shellexpand::tilde("~/.tgv").to_string(),
            zoom: Some(4),
            tracks: vec![TrackEntry {
                path: "/data/sample.bam".to_string(),
                index: Some("/data/sample.bam.bai".to_string()),
                reference: None,
                reference_index: None,
            }],
        };

        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("sessions").join("test.toml");
        session.write_to_path(&path).unwrap();

        let loaded = SessionFile::from_path(&path).unwrap();
        assert_eq!(loaded.locus, session.locus);
        assert_eq!(loaded.genome, session.genome);
        assert_eq!(loaded.zoom, Some(4));
        assert_eq!(loaded.tracks.len(), 1);
        assert_eq!(loaded.tracks[0].path, session.tracks[0].path);
    }

    #[test]
    fn test_zoom_omitted_when_none() {
        let session = SessionFile {
            version: CURRENT_VERSION,
            locus: "chr1:1000".to_string(),
            genome: "hg38".to_string(),
            ucsc_host: "us".to_string(),
            cache_dir: "~/.tgv".to_string(),
            zoom: None,
            tracks: vec![],
        };
        let toml = toml::to_string_pretty(&session).unwrap();
        assert!(!toml.contains("zoom"));
    }

    #[test]
    fn test_zoom_loaded_into_settings() {
        let session = SessionFile::parse(
            r#"
version = 1
locus = "chr1:1000"
zoom = 8
"#,
        )
        .unwrap();
        assert_eq!(session.zoom, Some(8));
        let settings = Settings::try_from(session).unwrap();
        assert_eq!(settings.zoom, Some(8));
    }
}

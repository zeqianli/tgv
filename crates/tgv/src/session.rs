//! Session file support: read and write `~/.tgv/sessions/*.toml`.
//!
//! A session file is a snapshot of the current [`Settings`] that can be
//! restored on the next launch. The file format is documented in the tgv
//! book under "Session files".

use crate::{message::Message, rendering::DARK_THEME, settings::Settings};
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
    ///
    /// Unrecognized fields are silently ignored with a warning printed to stderr,
    /// following the same convention as other tgv configuration files.
    pub fn parse(content: &str) -> Result<Self, TGVError> {
        let mut ignored = Vec::new();
        let session: Self = serde_ignored::deserialize(toml::Deserializer::new(content), |path| {
            ignored.push(path.to_string());
        })
        .map_err(|e| TGVError::ParsingError(format!("Failed to parse session file: {e}")))?;

        for field in ignored {
            eprintln!("warning: unknown field in session file ignored: {field}");
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

// ─── Conversions ────────────────────────────────────────────────────────────

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
        let ucsc_host = parse_ucsc_host(&session.ucsc_host)?;
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
        })
    }
}

/// Snapshot a [`Settings`] into a [`SessionFile`].
///
/// Fails if `initial_state_messages` does not contain a serializable locus
/// (i.e., a `contig:position` or gene name).
impl TryFrom<&Settings> for SessionFile {
    type Error = TGVError;

    fn try_from(settings: &Settings) -> Result<Self, TGVError> {
        let locus = serialize_locus(&settings.initial_state_messages)?;

        let genome = settings.core.reference.to_string();

        let ucsc_host = match settings.core.ucsc_host {
            UcscHost::Us => "us",
            UcscHost::Eu => "eu",
        }
        .to_string();

        let cache_dir = settings.core.cache_dir.clone();

        let mut tracks = Vec::new();

        if let Some(ap) = &settings.core.alignment_path {
            match ap {
                AlignmentPath::Bam { path, index, .. } => {
                    tracks.push(TrackEntry {
                        path: path.clone(),
                        index: Some(index.clone()),
                        reference: None,
                        reference_index: None,
                    });
                }
                AlignmentPath::Cram {
                    path,
                    crai,
                    fasta,
                    fai,
                } => {
                    tracks.push(TrackEntry {
                        path: path.clone(),
                        index: Some(crai.clone()),
                        reference: Some(fasta.clone()),
                        reference_index: Some(fai.clone()),
                    });
                }
            }
        }

        if let Some(vcf) = &settings.core.vcf_path {
            tracks.push(TrackEntry {
                path: vcf.clone(),
                index: None,
                reference: None,
                reference_index: None,
            });
        }

        if let Some(bed) = &settings.core.bed_path {
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
            tracks,
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

/// Serialize initial movement messages to a locus string.
///
/// Only [`Movement::ContigNamePosition`] and [`Movement::Gene`] are serializable;
/// other variants (e.g., [`Movement::Default`]) return an error.
fn serialize_locus(messages: &[Message]) -> Result<String, TGVError> {
    for m in messages {
        match m {
            Message::Core(gv_core::message::Message::Move(
                Movement::ContigNamePosition(contig, pos),
            )) => return Ok(format!("{contig}:{pos}")),
            Message::Core(gv_core::message::Message::Move(Movement::Gene(gene))) => {
                return Ok(gene.clone())
            }
            _ => {}
        }
    }
    Err(TGVError::ParsingError(
        "Cannot serialize locus: no contig:position or gene name found in initial messages."
            .to_string(),
    ))
}

/// Parse a `ucsc_host` string, resolving `"auto"` via timezone detection.
fn parse_ucsc_host(s: &str) -> Result<UcscHost, TGVError> {
    match s {
        "us" => Ok(UcscHost::Us),
        "eu" => Ok(UcscHost::Eu),
        "auto" => Ok(UcscHost::auto()),
        _ => Err(TGVError::ParsingError(format!(
            "Invalid ucsc_host value `{s}`. Expected \"us\", \"eu\", or \"auto\"."
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

    /// Build a minimal [`Settings`] for testing.
    fn make_settings(locus: &str, genome: &str) -> Settings {
        Settings {
            core: gv_core::settings::Settings {
                alignment_path: None,
                vcf_path: None,
                bed_path: None,
                reference: Reference::from_str(genome).unwrap(),
                backend: BackendType::Default,
                ucsc_host: UcscHost::Us,
                cache_dir: shellexpand::tilde("~/.tgv").to_string(),
            },
            initial_state_messages: parse_locus(locus).unwrap(),
            test_mode: false,
            debug: false,
            palette: crate::rendering::DARK_THEME,
        }
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

    // ── serialize_locus ──────────────────────────────────────────────────────

    #[test]
    fn test_serialize_locus_contig_position() {
        let msgs = vec![Message::Core(gv_core::message::Message::Move(
            Movement::ContigNamePosition("chr17".to_string(), 7572659),
        ))];
        assert_eq!(serialize_locus(&msgs).unwrap(), "chr17:7572659");
    }

    #[test]
    fn test_serialize_locus_gene() {
        let msgs = vec![Message::Core(gv_core::message::Message::Move(
            Movement::Gene("KRAS".to_string()),
        ))];
        assert_eq!(serialize_locus(&msgs).unwrap(), "KRAS");
    }

    #[test]
    fn test_serialize_locus_default_fails() {
        let msgs = vec![Movement::Default.into()];
        assert!(matches!(
            serialize_locus(&msgs),
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

    #[test]
    fn test_parse_unknown_field_is_ignored() {
        let toml = r#"
version = 1
locus = "chr1:1000"
unknown_field = "should be silently ignored"
"#;
        // Must not error; unknown fields are warned about but not fatal.
        let session = SessionFile::parse(toml).unwrap();
        assert_eq!(session.locus, "chr1:1000");
    }

    // ── TryFrom<SessionFile> for Settings ────────────────────────────────────

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

    // ── TryFrom<&Settings> for SessionFile ───────────────────────────────────

    #[test]
    fn test_session_from_settings_minimal() {
        let settings = make_settings("chr17:7572659", "hg38");
        let session = SessionFile::try_from(&settings).unwrap();
        assert_eq!(session.version, CURRENT_VERSION);
        assert_eq!(session.locus, "chr17:7572659");
        assert_eq!(session.genome, "hg38");
        assert!(session.tracks.is_empty());
    }

    #[test]
    fn test_session_from_settings_default_locus_fails() {
        let mut settings = make_settings("chr1:1", "hg38");
        settings.initial_state_messages = vec![Movement::Default.into()];
        assert!(matches!(
            SessionFile::try_from(&settings),
            Err(TGVError::ParsingError(_))
        ));
    }

    // ── Round-trip ───────────────────────────────────────────────────────────

    #[rstest]
    #[case("chr17:7572659", "hg38", None, None, None)]
    #[case("KRAS", "hg38", None, None, None)]
    #[case("chr22:33121120", "hg19",
        Some(("/data/sample.bam", "/data/sample.bam.bai", BamSource::Local)),
        None,
        None
    )]
    #[case("chr22:33121120", "hg19",
        Some(("/data/sample.bam", "/data/sample.bam.bai", BamSource::Local)),
        Some("/data/variants.vcf.gz"),
        Some("/data/annotations.bed")
    )]
    fn test_roundtrip(
        #[case] locus: &str,
        #[case] genome: &str,
        #[case] bam: Option<(&str, &str, BamSource)>,
        #[case] vcf: Option<&str>,
        #[case] bed: Option<&str>,
    ) {
        let mut settings = make_settings(locus, genome);
        settings.core.alignment_path = bam.map(|(path, index, source)| AlignmentPath::Bam {
            path: path.to_string(),
            index: index.to_string(),
            source,
        });
        settings.core.vcf_path = vcf.map(|s| s.to_string());
        settings.core.bed_path = bed.map(|s| s.to_string());

        let session = SessionFile::try_from(&settings).unwrap();
        let settings2 = Settings::try_from(session).unwrap();

        assert_eq!(settings.core, settings2.core);
        assert_eq!(settings.initial_state_messages, settings2.initial_state_messages);
    }

    // ── File I/O ─────────────────────────────────────────────────────────────

    #[test]
    fn test_write_and_read() {
        let settings = make_settings("chr17:7572659", "hg38");
        let session = SessionFile::try_from(&settings).unwrap();

        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("sessions").join("test.toml");
        session.write_to_path(&path).unwrap();

        let session2 = SessionFile::from_path(&path).unwrap();
        let settings2 = Settings::try_from(session2).unwrap();

        assert_eq!(settings.core, settings2.core);
        assert_eq!(settings.initial_state_messages, settings2.initial_state_messages);
    }
}

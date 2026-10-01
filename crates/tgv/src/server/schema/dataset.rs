//! MCP dataset request and response types, independent of session serialization.

use crate::settings::{Settings, classify_and_build_tracks};
use gv_core::prelude::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Replaces the current dataset with a reference and a list of data files.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(in crate::server) struct DatasetRequest {
    // A required nullable value distinguishes an omitted reference from no reference.
    #[schemars(extend("type" = ["string", "null"]))]
    pub reference: serde_json::Value,
    pub files: Vec<String>,
}

impl DatasetRequest {
    /// Applies the request to the current settings and validates its file paths.
    pub(in crate::server) fn update_settings(
        &self,
        settings: &Settings,
    ) -> Result<Settings, TGVError> {
        let mut settings = settings.clone();
        settings.core.reference = match &self.reference {
            Value::Null => Reference::NoReference,
            Value::String(reference) => {
                reference
                    .parse()
                    .map_err(|error: TGVError| TGVError::McpInvalidInput {
                        field: "reference",
                        message: error.to_string(),
                    })?
            }
            _ => {
                return Err(TGVError::McpInvalidInput {
                    field: "reference",
                    message: "The reference must be a string or null.".to_owned(),
                });
            }
        };
        if self.files.is_empty() && settings.core.reference == Reference::NoReference {
            return Err(TGVError::McpInvalidInput {
                field: "files",
                message: "Provide a reference or at least one file.".to_owned(),
            });
        }
        settings.core.file_paths =
            classify_and_build_tracks(&self.files).map_err(|error| TGVError::McpInvalidInput {
                field: "files",
                message: error.to_string(),
            })?;
        Ok(settings)
    }
}

/// Describes the reference and tracks in the loaded dataset.
#[derive(Serialize)]
pub(in crate::server) struct DatasetDescription {
    pub reference: String,
    pub tracks: Vec<TrackDescription>,
}

/// Identifies one loaded track and its source file.
#[derive(Serialize)]
pub(in crate::server) struct TrackDescription {
    pub id: crate::track_registry::TrackId,
    pub r#type: TrackType,
    pub source: String,
}

/// Classifies a track by the kind of repository that provides its data.
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::server) enum TrackType {
    Alignment,
    Variant,
    Bed,
}

impl From<RepositoryFileIndex> for TrackType {
    /// Keeps the repository kind while discarding its per-kind index.
    fn from(index: RepositoryFileIndex) -> Self {
        match index {
            RepositoryFileIndex::Alignment(_) => Self::Alignment,
            RepositoryFileIndex::Variant(_) => Self::Variant,
            RepositoryFileIndex::Bed(_) => Self::Bed,
        }
    }
}

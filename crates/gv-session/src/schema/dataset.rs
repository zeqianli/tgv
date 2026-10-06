//! Dataset request and response types, independent of session file serialization.

use crate::error::SessionError;
use gv_core::{
    prelude::*,
    settings::{Settings, classify_and_build_tracks},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Replaces the current dataset with a reference and a list of data files.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DatasetRequest {
    // A required nullable value distinguishes an omitted reference from no reference.
    #[schemars(extend("type" = ["string", "null"]))]
    pub reference: serde_json::Value,
    pub files: Vec<String>,
}

impl DatasetRequest {
    /// Applies the request to the current settings and validates its file paths.
    pub fn update_settings(&self, settings: &Settings) -> Result<Settings, SessionError> {
        let mut settings = settings.clone();
        settings.reference = match &self.reference {
            Value::Null => Reference::NoReference,
            Value::String(reference) => {
                reference
                    .parse()
                    .map_err(|error: TGVError| SessionError::InvalidInput {
                        field: "reference",
                        message: error.to_string(),
                    })?
            }
            _ => {
                return Err(SessionError::InvalidInput {
                    field: "reference",
                    message: "The reference must be a string or null.".to_owned(),
                });
            }
        };
        if self.files.is_empty() && settings.reference == Reference::NoReference {
            return Err(SessionError::InvalidInput {
                field: "files",
                message: "Provide a reference or at least one file.".to_owned(),
            });
        }
        settings.file_paths =
            classify_and_build_tracks(&self.files).map_err(|error| SessionError::InvalidInput {
                field: "files",
                message: error.to_string(),
            })?;
        Ok(settings)
    }
}

/// Describes the reference and tracks in the loaded dataset.
#[derive(Debug, Serialize)]
pub struct DatasetDescription {
    pub reference: String,
    pub tracks: Vec<TrackDescription>,
}

/// Identifies one loaded track and its source file.
#[derive(Debug, Serialize)]
pub struct TrackDescription {
    pub id: gv_core::track_registry::TrackId,
    pub r#type: TrackType,
    pub source: String,
}

/// Classifies a track by the kind of repository that provides its data.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackType {
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

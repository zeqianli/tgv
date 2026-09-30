//! HTTP request and response types, independent of session serialization.

use crate::{
    server::error::ApiError,
    settings::{Settings, classify_and_build_tracks},
};
use gv_core::reference::Reference;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::server) struct DatasetRequest {
    // A required nullable value distinguishes an omitted reference from no reference.
    pub reference: serde_json::Value,
    pub files: Vec<String>,
}

impl DatasetRequest {
    pub(in crate::server) fn update_settings(
        &self,
        settings: &Settings,
    ) -> Result<Settings, ApiError> {
        let mut settings = settings.clone();
        settings.core.reference = match &self.reference {
            Value::Null => Reference::NoReference,
            Value::String(reference) => reference
                .parse()
                .map_err(|error: gv_core::error::TGVError| ApiError::invalid("reference", error))?,
            _ => {
                return Err(ApiError::invalid(
                    "reference",
                    "The reference must be a string or null.",
                ));
            }
        };
        if self.files.is_empty() && settings.core.reference == Reference::NoReference {
            return Err(ApiError::invalid(
                "files",
                "Provide a reference or at least one file.",
            ));
        }
        settings.core.file_paths = classify_and_build_tracks(&self.files)
            .map_err(|error| ApiError::invalid("files", error))?;
        Ok(settings)
    }
}

#[derive(Serialize)]
pub(in crate::server) struct DatasetDescription {
    pub reference: Option<String>,
    pub tracks: Vec<TrackDescription>,
}

#[derive(Serialize)]
pub(in crate::server) struct TrackDescription {
    pub id: crate::track_registry::TrackId,
    pub r#type: &'static str,
    pub source: String,
}

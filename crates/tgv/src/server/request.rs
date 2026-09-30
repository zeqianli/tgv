//! HTTP request types and structured failures, independent of session serialization.

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

use crate::{
    server::error::ApiError,
    settings::{Settings, classify_and_build_tracks},
};
use gv_core::{error::TGVError, reference::Reference};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(super) struct Revision(pub u64);

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(super) struct TrackId(pub String);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DatasetRequest {
    // A required nullable value distinguishes an omitted reference from no reference.
    pub reference: serde_json::Value,
    pub files: Vec<String>,
}

impl DatasetRequest {
    pub(super) fn update_settings(&self, settings: &Settings) -> Result<Settings, TGVError> {
        let mut settings = settings.clone();
        settings.core.reference = match &self.reference {
            Value::Null => Reference::NoReference,
            Value::String(reference) => reference.parse()?,

            _ => {
                return Err(TGVError::CliError(
                    "The reference must be a string or null.".to_string(),
                ));
            }
        };
        if self.files.is_empty() && settings.core.reference == Reference::NoReference {
            return Err(TGVError::CliError(
                "Provide a reference or at least one file.".to_string(),
            ));
        }

        settings.core.file_paths = classify_and_build_tracks(&self.files)?;

        Ok(settings)
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Interval {
    pub contig: String,
    pub start: u64,
    pub end: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct InspectRequest {
    pub dataset_revision: Revision,
    pub region: Interval,
    pub tracks: Option<Vec<TrackId>>,
    pub render: Option<RenderRequest>,
}

#[derive(Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum RenderFormat {
    #[default]
    Text,
    Ansi,
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct RenderRequest {
    pub format: RenderFormat,
    pub width: u16,
    pub height: u16,
}

impl Default for RenderRequest {
    fn default() -> Self {
        Self {
            format: RenderFormat::Text,
            width: 120,
            height: 40,
        }
    }
}

//! HTTP types for rendering a genomic viewport as text or ANSI output.

use super::inspect::ResponseWarning;
use crate::track_registry::TrackId;
use serde::{Deserialize, Serialize};

/// Names the 1-based center position of a drawn viewport.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::server) struct DrawCenter {
    pub contig: String,
    pub position: u64,
}

/// Selects the viewport, tracks, canvas size, and output format to draw.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::server) struct DrawRequest {
    pub center: DrawCenter,
    pub zoom: u64,
    pub half_width: u64,
    pub tracks: Option<Vec<TrackId>>,
    pub format: RenderFormat,
    pub width: u16,
    pub height: u16,
}

/// Chooses plain text or ANSI-colored terminal output.
#[derive(Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(in crate::server) enum RenderFormat {
    #[default]
    Text,
    Ansi,
}

/// Reports the inclusive genomic interval visible in a drawing.
#[derive(Serialize)]
pub(in crate::server) struct DrawInterval {
    pub contig: String,
    pub start: u64,
    pub end: u64,
}

/// Returns the drawing and the viewport that produced it.
#[derive(Serialize)]
pub(in crate::server) struct DrawResponse {
    pub center: DrawCenter,
    pub zoom: u64,
    pub half_width: u64,
    pub format: RenderFormat,
    pub width: u16,
    pub height: u16,
    pub region: DrawInterval,
    pub text: String,
    pub legend: String,
    pub warnings: Vec<ResponseWarning>,
}

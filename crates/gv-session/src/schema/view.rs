//! Request and response types for commands that act on a viewer.

use super::inspect::InspectInterval;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Requests that the viewer show a 1-based inclusive region.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NavigateRequest {
    pub region: InspectInterval,
}

/// Requests that the viewer mark 1-based inclusive intervals, with an optional label.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HighlightRequest {
    pub intervals: Vec<InspectInterval>,
    pub label: Option<String>,
}

/// Reports what the viewer shows.
#[derive(Debug, Serialize)]
pub struct ViewState {
    /// The displayed interval.
    pub region: InspectInterval,
    /// Bases per screen column.
    pub zoom: u64,
}

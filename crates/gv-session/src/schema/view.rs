//! Request and response types for commands that act on a viewer.

use super::inspect::InspectInterval;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Requests that the viewer show a 1-based inclusive region.
#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NavigateRequest {
    pub region: InspectInterval,
}

/// Requests that the viewer mark 1-based inclusive intervals, with an optional label.
#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HighlightRequest {
    pub intervals: Vec<InspectInterval>,
    pub label: Option<String>,
}

/// Which front end a viewer is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ViewerKind {
    /// The terminal UI, `tgv`.
    Tui,
    /// The window, `tgv-gui`.
    Gui,
}

/// Describes a running viewer, so an agent can tell viewers apart and choose one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ViewerInfo {
    /// The viewer's process ID, which `attach` takes.
    pub pid: u32,
    pub kind: ViewerKind,
    /// The reference genome.
    pub reference: String,
    /// The track files the viewer shows, in display order.
    pub files: Vec<String>,
    /// The displayed locus, such as `chr1:12345`.
    pub locus: String,
    /// When the viewer started, in seconds since the Unix epoch.
    pub started: u64,
}

/// Reports what the viewer shows.
#[derive(Debug, Serialize)]
pub struct ViewState {
    /// The displayed interval.
    pub region: InspectInterval,
    /// Bases per screen unit: a column in the TUI, or a pixel in the GUI.
    pub zoom: f64,
}

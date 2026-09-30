use super::inspect::{Interval, ResponseWarning};
use crate::track_registry::TrackId;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::server) struct DrawCenter {
    pub contig: String,
    pub position: u64,
}

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

#[derive(Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(in crate::server) enum RenderFormat {
    #[default]
    Text,
    Ansi,
}

#[derive(Serialize)]
pub(in crate::server) struct DrawResponse {
    pub center: DrawCenter,
    pub zoom: u64,
    pub half_width: u64,
    pub format: RenderFormat,
    pub width: u16,
    pub height: u16,
    pub region: Interval,
    pub text: String,
    pub legend: String,
    pub warnings: Vec<ResponseWarning>,
}

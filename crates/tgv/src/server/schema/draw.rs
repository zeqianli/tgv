//! HTTP types for rendering a genomic viewport as text or ANSI output.

use super::inspect::InspectWarning;
use crate::server::error::ApiError;
use crate::track_registry::TrackId;
use crossterm::style::{
    Attribute, ResetColor, SetAttribute, SetBackgroundColor, SetForegroundColor,
};
use gv_core::prelude::*;
use ratatui::{buffer::Buffer, style::Modifier};
use serde::{Deserialize, Serialize};
use std::fmt::Write;
use unicode_width::UnicodeWidthStr;

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
    pub canvas_width: u16,
    pub canvas_height: u16,
}

impl DrawRequest {
    pub const MIN_CANVAS_WIDTH: u16 = 10;
    pub const MAX_CANVAS_WIDTH: u16 = 500;
    pub const MIN_CANVAS_HEIGHT: u16 = 10;
    pub const MAX_CANVAS_HEIGHT: u16 = 500;

    /// Converts the requested center and half-width into a validated core region.
    pub fn try_to_region(&self, contigs: &ContigHeader) -> Result<Region, ApiError> {
        if self.zoom == 0 {
            return Err(ApiError::invalid("zoom", "The zoom must be positive."));
        }
        if !(Self::MIN_CANVAS_WIDTH..=Self::MAX_CANVAS_WIDTH).contains(&self.canvas_width)
            || !(Self::MIN_CANVAS_HEIGHT..=Self::MAX_CANVAS_HEIGHT).contains(&self.canvas_height)
        {
            return Err(ApiError::invalid(
                "draw",
                "The canvas width and height must each be 10–500.",
            ));
        }
        if self.center.position == 0
            || self.half_width > 49_999
            || self
                .center
                .position
                .checked_add(self.half_width)
                .is_none_or(|end| end > (usize::MAX / 16) as u64)
        {
            return Err(ApiError::invalid(
                "center",
                "Use a positive 1-based center and a half-width of at most 49999 bases within the platform coordinate range.",
            ));
        }
        let contig_index = contigs
            .try_get_index_by_str(&self.center.contig)
            .map_err(|error| ApiError::invalid("center.contig", error))?;
        if contigs.contigs[contig_index]
            .length
            .is_some_and(|length| self.center.position > length)
        {
            return Err(ApiError::invalid(
                "center.position",
                "The center is beyond the contig.",
            ));
        }
        Ok(Region {
            focus: Focus {
                contig_index,
                position: self.center.position,
            },
            half_width: self.half_width,
        })
    }
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
    pub region: DrawInterval,
    pub text: String,
    pub legend: String,
    pub warnings: Vec<DrawWarning>,
}

/// Reports unavailable data or limitations of a drawn viewport.
#[derive(Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub(in crate::server) enum DrawWarning {
    ReferenceUnavailable { message: String },
    GenesUnavailable { message: String },
    RenderLimited { track_id: TrackId, message: String },
    RenderBinned { message: String },
}

impl From<InspectWarning> for DrawWarning {
    /// Preserves availability warning codes in the draw response.
    fn from(warning: InspectWarning) -> Self {
        match warning {
            InspectWarning::ReferenceUnavailable { message } => {
                Self::ReferenceUnavailable { message }
            }
            InspectWarning::GenesUnavailable { message } => Self::GenesUnavailable { message },
        }
    }
}

impl DrawResponse {
    /// Builds the HTTP response from a rendered terminal buffer and its viewport.
    pub fn from_buffer(
        contig: String,
        displayed: &Region,
        format: RenderFormat,
        buffer: &Buffer,
        warnings: Vec<DrawWarning>,
    ) -> Self {
        Self {
            region: DrawInterval {
                contig,
                start: displayed.start(),
                end: displayed.end(),
            },
            text: format.export_buffer(buffer),
            legend: "The existing TGV palette and symbols are used. Base letters identify bases; arrows indicate orientation; coverage occupies a separate track. Read rows may be clipped. Unicode drawing characters are preserved.".to_owned(),
            warnings,
        }
    }
}

impl RenderFormat {
    /// Exports terminal cells as plain text or ANSI-colored text.
    fn export_buffer(self, buffer: &Buffer) -> String {
        let mut output = String::new();
        for y in buffer.area.top()..buffer.area.bottom() {
            let mut x = buffer.area.left();
            let mut previous_style = None;
            while x < buffer.area.right() {
                let cell = &buffer[(x, y)];
                let symbol: String = cell
                    .symbol()
                    .chars()
                    .map(|c| if c.is_control() { '�' } else { c })
                    .collect();
                if self == Self::Ansi && previous_style != Some(cell.style()) {
                    let _ = write!(
                        output,
                        "{}{}{}",
                        SetAttribute(Attribute::Reset),
                        SetForegroundColor(cell.fg.into()),
                        SetBackgroundColor(cell.bg.into())
                    );
                    for (modifier, attribute) in [
                        (Modifier::BOLD, Attribute::Bold),
                        (Modifier::DIM, Attribute::Dim),
                        (Modifier::ITALIC, Attribute::Italic),
                        (Modifier::UNDERLINED, Attribute::Underlined),
                        (Modifier::REVERSED, Attribute::Reverse),
                        (Modifier::CROSSED_OUT, Attribute::CrossedOut),
                    ] {
                        if cell.modifier.contains(modifier) {
                            let _ = write!(output, "{}", SetAttribute(attribute));
                        }
                    }
                    previous_style = Some(cell.style());
                }
                output.push_str(&symbol);
                x += UnicodeWidthStr::width(symbol.as_str()).max(1) as u16;
            }
            if self == Self::Ansi {
                let _ = write!(output, "{}{}", SetAttribute(Attribute::Reset), ResetColor);
            }
            output.push('\n');
        }
        output
    }
}

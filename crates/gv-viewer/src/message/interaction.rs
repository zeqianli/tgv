//! Viewer interactions that open menus or read details.

use crate::message::FocusTarget;
use gv_core::track_registry::TrackId;
use strum::Display;

/// Viewer interactions that open menus or read details.
#[derive(Debug, Clone, Eq, PartialEq, Display)]
pub enum InteractionAction {
    /// Opens the context menu for `target`, anchored at a point on the screen.
    OpenContextMenu {
        target: ContextMenuTarget,
        anchor: Anchor,
    },

    /// Shows the SAM record of a read in a popup.
    OpenReadDetails { track: TrackId, read_id: usize },
}

/// A point on the screen, in the front end's units: a terminal cell, or a pixel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Anchor {
    pub x: f32,
    pub y: f32,
}

impl Eq for Anchor {}

/// What a right click opened the context menu on.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ContextMenuTarget {
    Alignment {
        track: TrackId,
        position: Option<u64>,
    },
    Sidebar {
        target: Option<FocusTarget>,
    },
    Track {
        track: TrackId,
    },
    Reference,
}

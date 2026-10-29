//! Navigation through the displayed data.

use gv_core::{
    message::{Movement, Scroll, Zoom},
    track_registry::TrackId,
};
use strum::Display;

/// Navigation within the displayed data.
#[derive(Debug, Clone, Eq, PartialEq, Display)]
pub enum NavigationAction {
    Move(Movement),
    Scroll(Scroll),
    Zoom(Zoom),
    SelectContig(usize),

    /// Returns to the view before the last jump.
    JumpBack,

    /// Returns to the view that the last `JumpBack` left.
    JumpForward,

    /// Scrolls one alignment track, whatever has focus.
    ScrollAlignment {
        track: TrackId,
        scroll: Scroll,
    },
}

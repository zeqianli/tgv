//! The active scene, focus, and input state.

use crate::{register::KeyRegisterType, viewer::Scene};
use gv_core::track_registry::TrackId;
use strum::Display;

/// The focused target, scene, and input registers.
#[derive(Debug, Clone, Eq, PartialEq, Display)]
pub enum InputAction {
    SwitchScene(Scene),

    CommandChanged,

    SwitchKeyRegister(KeyRegisterType),

    /// Focuses the reference or a file track: what word motions, insert mode, and `:w` act on.
    Focus(FocusTarget),

    /// Moves the focus along the file tracks and the reference, in display order.
    MoveFocus(FocusMove),

    ClearAllKeyRegisters,
}

/// What has the focus: the reference (the sequence and gene rows) or a file track.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FocusTarget {
    Reference,
    Track(TrackId),
}

/// A focus move along the file tracks and the reference, in display order.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FocusMove {
    /// Stops at the last target.
    Next(usize),
    /// Stops at the first target.
    Previous(usize),
    /// Wraps around to the first target.
    NextWrap,
    /// Wraps around to the last target.
    PreviousWrap,
    First,
    Last,
}

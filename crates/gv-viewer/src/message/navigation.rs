use crate::message::FocusTarget;
use gv_core::message::{Movement, Scroll, Zoom};
use strum::Display;

#[derive(Debug, Clone, Eq, PartialEq, Display)]
pub enum NavigationAction {
    Move(Movement),
    Scroll(Scroll),
    Zoom(Zoom),
    SelectContig(usize),
    JumpBack,
    JumpForward,
    ScrollTrack { target: FocusTarget, scroll: Scroll },
}

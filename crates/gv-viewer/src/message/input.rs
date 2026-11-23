use crate::{keyboard::InputMode, viewer::Scene};
use gv_core::track_registry::TrackId;
use strum::Display;

#[derive(Debug, Clone, Eq, PartialEq, Display)]
pub enum InputAction {
    SwitchScene(Scene),
    CommandChanged,
    SwitchInputMode(InputMode),
    Focus(FocusTarget),
    MoveFocus(FocusMove),
    ClearAllKeyRegisters,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum FocusTarget {
    Reference,
    Track(TrackId),
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FocusMove {
    Next(usize),
    Previous(usize),
    NextWrap,
    PreviousWrap,
    First,
    Last,
}

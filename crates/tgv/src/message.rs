use crate::{
    app::{Highlight, Scene},
    register::KeyRegisterType,
};
pub use gv_core::message::{Movement, Scroll};
use gv_core::track_registry::TrackId;
use strum::Display;

/// A change to the TUI's view or input state, applied by `App::handle`.
///
/// Key and mouse input produce actions, and so do agent view requests, so every change to what
/// the screen shows goes through one path.
#[derive(Debug, Clone, Eq, PartialEq, Display)]
pub enum Action {
    Core(gv_core::message::Message),

    SwitchScene(Scene),

    SelectContig(usize),

    CommandChanged,

    SwitchKeyRegister(KeyRegisterType),

    UpdateLayout(UpdateLayoutAction),

    ClearAllKeyRegisters,

    /// Replaces the highlighted intervals.
    SetHighlights(Vec<Highlight>),

    ClearHighlights,
}

/// Layout changes.
#[derive(Debug, Clone, Eq, PartialEq, Display)]
pub enum UpdateLayoutAction {
    ToggleSidebar,
    SetSidebarWidth(u16),
    ResizeAlignmentPair {
        upper: TrackId,
        lower: TrackId,
        delta_rows: i32,
    },
}

impl Action {
    /// Wraps a status message for the message line.
    pub fn message(s: String) -> Self {
        Action::Core(gv_core::message::Message::Message(s))
    }
}

impl From<gv_core::message::Message> for Action {
    fn from(m: gv_core::message::Message) -> Self {
        Action::Core(m)
    }
}

impl From<gv_core::message::Movement> for Action {
    fn from(movement: gv_core::message::Movement) -> Self {
        gv_core::message::Message::Move(movement).into()
    }
}

impl From<gv_core::message::Scroll> for Action {
    fn from(scroll: gv_core::message::Scroll) -> Self {
        gv_core::message::Message::Scroll(scroll).into()
    }
}

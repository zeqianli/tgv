use crate::{app::Scene, register::KeyRegisterType, track_registry::TrackId};
pub use gv_core::message::{Movement, Scroll};
use strum::Display;

/// TGV messages
#[derive(Debug, Clone, Eq, PartialEq, Display)]
pub enum Message {
    Core(gv_core::message::Message),

    SwitchScene(Scene),

    SelectContig(usize),

    CommandChanged,

    SwitchKeyRegister(KeyRegisterType),

    UpdateLayout(UpdateLayoutMessage),

    ClearAllKeyRegisters,
}

/// UX layout update messages
#[derive(Debug, Clone, Eq, PartialEq, Display)]
pub enum UpdateLayoutMessage {
    ToggleSidebar,
    SetSidebarWidth(u16),
    ResizeAlignmentPair {
        upper: TrackId,
        lower: TrackId,
        delta_rows: i32,
    },
}

impl Message {
    /// Helper function for gv_core::message::Message::Message
    pub fn message(s: String) -> Self {
        Message::Core(gv_core::message::Message::Message(s))
    }
}

impl From<gv_core::message::Message> for Message {
    fn from(m: gv_core::message::Message) -> Self {
        Message::Core(m)
    }
}

impl From<gv_core::message::Movement> for Message {
    fn from(movement: gv_core::message::Movement) -> Self {
        gv_core::message::Message::Move(movement).into()
    }
}

impl From<gv_core::message::Scroll> for Message {
    fn from(scroll: gv_core::message::Scroll) -> Self {
        gv_core::message::Message::Scroll(scroll).into()
    }
}

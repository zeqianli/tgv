use crate::{app::Scene, register::KeyRegisterType};
pub use gv_core::message::{Movement, Scroll};
use strum::Display;

/// TGV messages
#[derive(Debug, Clone, Eq, PartialEq, Display)]
pub enum Message {
    Core(gv_core::message::Message),

    SwitchScene(Scene),

    SwitchKeyRegister(KeyRegisterType),

    ClearAllKeyRegisters,
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

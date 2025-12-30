use crate::{app::Scene, register::KeyRegisterType};
use strum::Display;

/// TGV messages
#[derive(Debug, Clone, Eq, PartialEq, Display)]
pub enum Message {
    Core(gv_core::message::Message),

    SwitchScene(Scene),

    SwitchKeyRegister(KeyRegisterType),

    ClearAllKeyRegisters,
}

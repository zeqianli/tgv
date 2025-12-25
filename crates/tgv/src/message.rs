use crate::{intervals::Region, register::KeyRegisterType, states::Scene, strand::Strand};

use strum::Display;

/// TGV messages
#[derive(Debug, Clone, Eq, PartialEq, Display)]
pub enum Message {
    Core(gv_core::message::Message),

    SwitchScene(Scene),

    SwitchKeyRegister(KeyRegisterType),
    // ResizeTrack {
    //     mouse_down_x: u16,
    //     mouse_down_y: u16,

    //     mouse_released_x: u16,
    //     mouse_released_y: u16,
    // },
    Quit,
    // ClearKeyRegister(KeyRegisterType),
    // ClearAllKeyRegisters,
    // SwitchKeyRegister(KeyRegisterType),
}

/// Communication between State and Data
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, Eq, PartialEq, Display)]
pub enum DataMessage {
    RequiresCompleteAlignments(Region),
    RequiresCompleteFeatures(Region),
    RequiresCompleteSequences(Region),

    RequiresCytobands(usize),
}

use rust_htslib::bam::Format;

use crate::error::TGVError;
use crate::models::{mode::InputMode, region::Region};
use std::fmt;
use strum::Display;
/// State messages
#[derive(Debug, Clone, Eq, PartialEq, Display)]
pub enum StateMessage {
    MoveLeft(usize),
    MoveRight(usize),
    MoveUp(usize),
    MoveDown(usize),

    GotoCoordinate(usize),
    GotoContig(String), // The state object decide if "chr" prefix is needed.
    GotoContigCoordinate(String, usize), // The state object decide if "chr" prefix is needed.

    GotoNextExonsStart(usize),
    GotoNextExonsEnd(usize),
    GotoPreviousExonsStart(usize),
    GotoPreviousExonsEnd(usize),
    GotoNextGenesStart(usize),
    GotoNextGenesEnd(usize),
    GotoPreviousGenesStart(usize),
    GotoPreviousGenesEnd(usize),

    GotoNextContig(usize),
    GotoPreviousContig(usize),

    GoToGene(String),

    GoToDefault, // Calculate a default location based on the genome context

    ZoomIn(usize),
    ZoomOut(usize),

    SwitchMode(InputMode),

    AddCharToNormalModeRegisters(char),
    ClearNormalModeRegisters,
    NormalModeRegisterError(String),

    AddCharToCommandModeRegisters(char),
    ClearCommandModeRegisters,
    BackspaceCommandModeRegisters,
    MoveCursorLeft(usize),
    MoveCursorRight(usize),
    CommandModeRegisterError(String),

    Error(TGVError),

    Quit,
}

impl StateMessage {
    /// Whether the message requires a reference genome.
    pub fn requires_reference(&self) -> bool {
        matches!(
            self,
            StateMessage::GotoNextExonsStart(_)
                | StateMessage::GotoNextExonsEnd(_)
                | StateMessage::GotoPreviousExonsStart(_)
                | StateMessage::GotoPreviousExonsEnd(_)
                | StateMessage::GotoNextGenesStart(_)
                | StateMessage::GotoNextGenesEnd(_)
                | StateMessage::GotoPreviousGenesStart(_)
                | StateMessage::GotoPreviousGenesEnd(_)
                | StateMessage::GoToGene(_)
        )
    }
}

/// Communication between State and Data
#[allow(clippy::enum_variant_names)]
pub enum DataMessage {
    RequiresCompleteAlignments(Region),
    RequiresCompleteFeatures(Region),
    RequiresCompleteSequences(Region),
}

impl fmt::Display for DataMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DataMessage::RequiresCompleteAlignments(region) => {
                write!(f, "RequiresCompleteAlignments({})", region)
            }
            DataMessage::RequiresCompleteFeatures(region) => {
                write!(f, "RequiresCompleteFeatures({})", region)
            }
            DataMessage::RequiresCompleteSequences(region) => {
                write!(f, "RequiresCompleteSequences({})", region)
            }
        }
    }
}

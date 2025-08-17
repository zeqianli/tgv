use crate::{display_mode::DisplayMode, region::Region};
use strum::Display;
/// State messages
#[derive(Debug, Clone, Eq, PartialEq, Display)]
pub enum StateMessage {
    MoveLeft(usize),
    MoveRight(usize),
    MoveUp(usize),
    MoveDown(usize),

    GotoCoordinate(usize),
    //GotoContigName(String), // Here is string because it can be an alias. The handler will look up the string from the contig collection.
    GotoContigNameCoordinate(String, usize), // Here is string because it can be an alias. The handler will look up the string from the contig collection.

    GotoY(usize),
    GotoYBottom,

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
    GotoContigIndex(usize),

    GoToGene(String),

    GoToDefault, // Calculate a default location based on the genome context

    ZoomIn(usize),
    ZoomOut(usize),

    Message(String),

    SetDisplayMode(DisplayMode),

    ResizeTrack {
        mouse_down_x: u16,
        mouse_down_y: u16,

        mouse_released_x: u16,
        mouse_released_y: u16,
    },

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
#[derive(Debug, Clone, Eq, PartialEq, Display)]
pub enum DataMessage {
    RequiresCompleteAlignments(Region),
    RequiresCompleteFeatures(Region),
    RequiresCompleteSequences(Region),

    RequiresCytobands(usize),
}

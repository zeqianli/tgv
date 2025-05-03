use crate::error::TGVError;
use crate::{contig::Contig, region::Region};
use strum::Display;
/// State messages
#[derive(Debug, Clone, Eq, PartialEq, Display)]
pub enum StateMessage {
    MoveLeft(usize),
    MoveRight(usize),
    MoveUp(usize),
    MoveDown(usize),

    GotoCoordinate(usize),
    GotoContig(Contig),
    GotoContigCoordinate(Contig, usize),

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

    Message(String),

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

    RequiresCytobands(Contig),
}

use crate::models::{contig::Contig, mode::InputMode, region::Region};

/// State messages
#[derive(Debug, Clone, Eq, PartialEq)]
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

    GoToGene(String),

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

    Quit,
}

/// Communication between State and Data
pub enum DataMessage {
    RequiresCompleteAlignments(Region),
    RequiresCompleteFeatures(Region),
    RequiresCompleteSequences(Region),
}

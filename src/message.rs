use crate::{display_mode::DisplayMode, error::TGVError, region::Region, strand::Strand};

use std::str::FromStr;
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

    AlignmentChange(Vec<AlignmentDisplayOption>),

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

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum AlignmentDisplayOption {
    Filter(AlignmentFilter),

    Sort(AlignmentSort),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum AlignmentFilter {
    Default,

    /// Always false (filtered out)
    False,

    /// Start in a range (1-based, both-inclusive)
    StartsIn(usize, usize),
    /// Ends in a range (1-based, both-inclusive)
    EndsIn(usize, usize),
    /// Overlaps a range (1-based, both-inclusive)
    Overlaps(usize, usize),

    /// Strand
    Strand(Strand),

    /// Base at position (1-based) equal to the character
    Base(usize, char),

    BaseAtCurrentPosition(char),

    /// Base at position (1-based is softclip)
    BaseSoftclip(usize),

    BaseAtCurrentPositionSoftClip,

    /// MAPQ greater or equal than
    MappingQualityGE(u16),

    /// MAPQ smaller or equal than
    MappingQualityLE(u16),

    /// All bits in the flag are 1 (equivalent to samtools view -f)
    FlagsAll(u32),

    /// Any bits in the flag are 1 (equivalent to samtools view -rf)
    FlagsAny(u32),

    /// Exact flag match
    FlagsEqual(u32),

    /// Tag equal to the value
    Tag(String, String),

    Not(Box<AlignmentFilter>),
    And(Box<AlignmentFilter>, Box<AlignmentFilter>),
    Or(Box<AlignmentFilter>, Box<AlignmentFilter>),
}

impl AlignmentFilter {
    pub fn and(self, other: AlignmentFilter) -> Self {
        if &self == &other {
            return self;
        }

        match (self, other) {
            (Self::FlagsAll(flag1), Self::FlagsAll(flag2)) => Self::FlagsAll(flag1 & flag2),
            (Self::Default, other) | (other, Self::Default) => other,

            (self_, other) => AlignmentFilter::And(Box::new(self_), Box::new(other)),
        }
    }

    pub fn or(self, other: AlignmentFilter) -> Self {
        if &self == &other {
            return self;
        }
        match (self, other) {
            (Self::FlagsAny(flag1), Self::FlagsAny(flag2)) => Self::FlagsAny(flag1 | flag2),
            (Self::Default, other) | (other, Self::Default) => other,
            (self_, other) => AlignmentFilter::Or(Box::new(self_), Box::new(other)),
        }
    }

    pub fn not(self) -> Self {
        match self {
            Self::Strand(strand) => Self::Strand(strand.reverse()),
            Self::Not(filter) => *filter,
            self_ => Self::Not(Box::new(self_)),
        }
    }
}

/// Sort alignment options
/// Reference: https://github.com/igvteam/igv/blob/main/src/main/java/org/broad/igv/sam/SortOption.java
///

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum AlignmentSort {
    /// Default
    Default,

    /// Start
    Start,

    /// Stand of reads at the current location
    StrandAtCurrentBase,

    /// Stand of reads covering a location
    StrandAt(usize),

    /// Base of reads at the current location
    BaseAtCurrentPosition,

    /// Stand of reads covering a location
    BaseAt(usize),

    /// MAPQ, reversed order
    MappingQuality,

    ///?
    Sample,

    ///?
    ReadGroup,

    /// First in pair, second in pair, unpaired
    ReadOrder,

    /// read name
    ReadName,

    /// alignment_end - alignment_start
    AlignedReadLength,

    /// ?
    InsertSize,

    /// ?
    ChromosomeOfMate,

    ///?
    Tag,

    /// Sort by 0 first and then 1
    Then(Box<AlignmentSort>, Box<AlignmentSort>),

    /// Reverse ordering
    Reverse(Box<AlignmentSort>),
}

impl Default for AlignmentSort {
    fn default() -> Self {
        Self::Default
    }
}

impl AlignmentSort {
    pub fn then(self, other: AlignmentSort) -> AlignmentSort {
        if &self == &other {
            return self;
        }
        match (self, other) {
            (Self::Default, other) | (other, Self::Default) => other,
            (self_, other) => Self::Then(Box::new(self_), Box::new(other)),
        }
    }

    pub fn reverse(self) -> AlignmentSort {
        match self {
            Self::Default => self,
            Self::Reverse(value) => *value,
            _ => Self::Reverse(Box::new(self)),
        }
    }
}

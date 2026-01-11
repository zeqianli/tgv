use crate::strand::Strand;

use strum::Display;

#[derive(Debug, Clone, Eq, PartialEq, Display)]
pub enum Message {
    Move(Movement),

    Scroll(Scroll),

    Zoom(Zoom),

    Quit,
    SetAlignmentOption(Vec<AlignmentDisplayOption>),

    Message(String),
}

impl From<Movement> for Message {
    fn from(value: Movement) -> Self {
        Self::Move(value)
    }
}
impl From<Scroll> for Message {
    fn from(value: Scroll) -> Self {
        Self::Scroll(value)
    }
}
impl From<Zoom> for Message {
    fn from(value: Zoom) -> Self {
        Self::Zoom(value)
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Display)]
pub enum Zoom {
    Out(u64),
    In(u64),
}

#[derive(Debug, Clone, Eq, PartialEq, Display)]
pub enum Scroll {
    Up(usize),
    Down(usize),

    Position(usize),
    Bottom,
}

// TODO: indicate which movement requires resetting y to 0
#[derive(Debug, Clone, Eq, PartialEq, Display)]
pub enum Movement {
    /// State messages
    Left(u64),
    Right(u64),

    Position(u64),
    //GotoContigName(String), // Here is string because it can be an alias. The handler will look up the string from the contig collection.
    ContigNamePosition(String, u64), // Here is string because it can be an alias. The handler will look up the string from the contig collection.

    NextExonsStart(usize),
    NextExonsEnd(usize),
    PreviousExonsStart(usize),
    PreviousExonsEnd(usize),
    NextGenesStart(usize),
    NextGenesEnd(usize),
    PreviousGenesStart(usize),
    PreviousGenesEnd(usize),

    NextContig(usize),
    PreviousContig(usize),
    ContigIndex(usize),

    Gene(String),

    Default, // Calculate a default location based on the genome context

             // ResizeTrack {
             //     mouse_down_x: u16,
             //     mouse_down_y: u16,

             //     mouse_released_x: u16,
             //     mouse_released_y: u16,
             // },
             // AddAlignmentChange(Vec<AlignmentDisplayOption>),
             // SetAlignmentChange(Vec<AlignmentDisplayOption>),

             // Quit,
             // ClearKeyRegister(KeyRegisterType),
             // ClearAllKeyRegisters,
             // SwitchKeyRegister(KeyRegisterType),
}

#[derive(Debug, Clone, Eq, PartialEq, Display)]
pub enum AlignmentDisplayOption {
    #[strum(to_string = "Filter: {0}")]
    Filter(AlignmentFilter),

    #[strum(to_string = "Sort: {0}")]
    Sort(AlignmentSort),

    ViewAsPairs,
}

#[derive(Debug, Clone, Eq, PartialEq, Display)]
pub enum AlignmentFilter {
    Default,

    /// Always false (filtered out)
    False,

    /// Start in a range (1-based, both-inclusive)
    #[strum(to_string = "Starts in [{0},{1}]")]
    StartsIn(usize, usize),
    /// Ends in a range (1-based, both-inclusive)
    #[strum(to_string = "Ends in [{0},{1}]")]
    EndsIn(usize, usize),
    /// Overlaps a range (1-based, both-inclusive)
    #[strum(to_string = "Overlaps [{0},{1}]")]
    Overlaps(usize, usize),

    /// Strand
    #[strum(to_string = "Strand={0}")]
    Strand(Strand),

    /// Base at position (1-based) equal to the character
    #[strum(to_string = "Base({0})={1}")]
    Base(u64, char),

    BaseAtCurrentPosition(char),

    /// Base at position (1-based is softclip)
    #[strum(to_string = "Base({0})=SOFTCLIP")]
    BaseSoftclip(u64),

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

    #[strum(to_string = "NOT({0})")]
    Not(Box<AlignmentFilter>),

    #[strum(to_string = "{0} AND {1}")]
    And(Box<AlignmentFilter>, Box<AlignmentFilter>),

    #[strum(to_string = "{0} OR {1}")]
    Or(Box<AlignmentFilter>, Box<AlignmentFilter>),
}

impl AlignmentFilter {
    pub fn and(self, other: AlignmentFilter) -> Self {
        if self == other {
            return self;
        }

        match (self, other) {
            (Self::FlagsAll(flag1), Self::FlagsAll(flag2)) => Self::FlagsAll(flag1 & flag2),
            (Self::Default, other) | (other, Self::Default) => other,

            (self_, other) => AlignmentFilter::And(Box::new(self_), Box::new(other)),
        }
    }

    pub fn or(self, other: AlignmentFilter) -> Self {
        if self == other {
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

#[derive(Debug, Clone, Eq, PartialEq, Display)]
pub enum AlignmentSort {
    /// Default
    Default,

    /// Start
    Start,

    /// Stand of reads at the current location
    StrandAtCurrentBase,

    /// Stand of reads covering a location
    #[strum(to_string = "Strand({0})")]
    StrandAt(u64),

    /// Base of reads at the current location
    BaseAtCurrentPosition,

    /// Stand of reads covering a location
    #[strum(to_string = "Base({0})")]
    BaseAt(u64),

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
    #[strum(to_string = "{0}, {1}")]
    Then(Box<AlignmentSort>, Box<AlignmentSort>),

    /// Reverse ordering
    #[strum(to_string = "{0} (DESC)")]
    Reverse(Box<AlignmentSort>),
}

impl Default for AlignmentSort {
    fn default() -> Self {
        Self::Default
    }
}

impl AlignmentSort {
    pub fn then(self, other: AlignmentSort) -> AlignmentSort {
        if self == other {
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

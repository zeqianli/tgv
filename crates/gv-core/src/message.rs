use crate::strand::Strand;

use strum::Display;

#[derive(Debug, Clone, Eq, PartialEq, Display)]
pub enum Zoom {
    Out(u64),
    In(u64),
    /// Shows at least `bases` bases across the track area.
    Fit {
        bases: u64,
    },
    /// Sets the zoom to a number of bases per screen unit. Front ends with whole-number scales
    /// round it. Continuous gestures, such as a pinch, zoom this way.
    Set(BasesPerUnit),
}

/// A zoom in bases per screen unit: per terminal column, or per pixel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BasesPerUnit(pub f64);

// Zooms are computed from finite screen sizes, never NaN.
impl Eq for BasesPerUnit {}

/// A vertical scroll of a track. The front end decides which track it applies to.
#[derive(Debug, Clone, Eq, PartialEq, Display)]
pub enum Scroll {
    /// Scroll up by n rows.
    Up(usize),
    /// Scroll down by n rows.
    Down(usize),
    Top,
    Bottom,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FeatureSource {
    Exons,
    Genes,
    /// A BED track with BED repository index.
    Bed(usize),
    /// A variant track with variant repository index.
    Variants(usize),
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum WordMotion {
    NextStart,
    NextEnd,
    PreviousStart,
    PreviousEnd,
}

impl WordMotion {
    pub fn forward(self) -> bool {
        matches!(self, Self::NextStart | Self::NextEnd)
    }

    pub fn to_start(self) -> bool {
        matches!(self, Self::NextStart | Self::PreviousStart)
    }
}

// TODO: indicate which movement requires resetting y to 0
#[derive(Debug, Clone, Eq, PartialEq, Display)]
pub enum Movement {
    /// Moves left by a number of bases. Front ends convert screen columns to bases.
    Left(u64),
    /// Moves right by a number of bases.
    Right(u64),

    Position(u64),
    //GotoContigName(String), // Here is string because it can be an alias. The handler will look up the string from the contig collection.
    ContigNamePosition(String, u64), // Here is string because it can be an alias. The handler will look up the string from the contig collection.

    /// Moves to the `count`-th feature boundary of `source` in the direction of `motion`.
    Feature {
        source: FeatureSource,
        motion: WordMotion,
        count: usize,
    },

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

    #[strum(to_string = "Starts in [{0},{1}]")]
    StartsIn(usize, usize),
    #[strum(to_string = "Ends in [{0},{1}]")]
    EndsIn(usize, usize),
    #[strum(to_string = "Overlaps [{0},{1}]")]
    Overlaps(usize, usize),

    #[strum(to_string = "Strand={0}")]
    Strand(Strand),

    #[strum(to_string = "Base({0})={1}")]
    Base(u64, char),

    BaseAtCurrentPosition(char),

    #[strum(to_string = "Base({0})=SOFTCLIP")]
    BaseSoftclip(u64),

    BaseAtCurrentPositionSoftClip,

    MappingQualityGE(u16),

    MappingQualityLE(u16),

    FlagsAll(u32),

    FlagsAny(u32),

    FlagsEqual(u32),

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

#[derive(Debug, Clone, Eq, PartialEq, Display, Default)]
pub enum AlignmentSort {
    #[default]
    Default,
    #[strum(to_string = "Start({0})")]
    Start(u64),
    StrandAtCurrentBase,
    #[strum(to_string = "Strand({0})")]
    StrandAt(u64),
    BaseAtCurrentPosition,
    #[strum(to_string = "Base({0})")]
    BaseAt(u64),
    #[strum(to_string = "MAPQ({0})")]
    MappingQuality(u64),
    Sample,
    ReadGroup,
    ReadOrder,
    #[strum(to_string = "ReadName({0})")]
    ReadName(u64),
    AlignedReadLength,
    #[strum(to_string = "InsertSize({0})")]
    InsertSize(u64),
    ChromosomeOfMate,
    Tag,
    #[strum(to_string = "{0}, {1}")]
    Then(Box<AlignmentSort>, Box<AlignmentSort>),
    #[strum(to_string = "{0} (DESC)")]
    Reverse(Box<AlignmentSort>),
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

use crate::{display_mode::DisplayMode, region::Region, strand::Strand};
use nom::{
    branch::alt,
    bytes::complete::{tag, tag_no_case},
    character::complete::{alpha1, anychar, char, digit1, multispace0, usize},
    combinator::{map, opt, value},
    error::ErrorKind,
    multi::{self, separated_list0},
    sequence::{delimited, pair, preceded, separated_pair, terminated},
    IResult, Parser,
};
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

#[derive(Debug, Clone)]
pub struct AlignmentDisplayOption {
    filter: AlignmentFilter,

    sort: AlignmentSort,
}

impl Default for AlignmentDisplayOption {
    fn default() -> Self {
        AlignmentDisplayOption {
            filter: AlignmentFilter::Default,
            sort: AlignmentSort::Default,
        }
    }
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

fn node_base_filter(input: &str) -> IResult<&str, AlignmentFilter> {
    let (input, (position, base)) = preceded(
        tag_no_case("BASE"),
        separated_pair(
            parse_optional_parenthesis,
            delimited(multispace0, tag("="), multispace0),
            alt((alpha1, tag_no_case("SOFTCLIP"))),
        ),
    )
    .parse(input)?;

    let is_softclip = if base.to_lowercase() == "softclip" {
        true
    } else if base.len() != 1 {
        return Err(nom::Err::Error(nom::error::Error::new(
            input,
            ErrorKind::Fail,
        )));
    } else {
        false
    };

    let filter = match (position, is_softclip) {
        (None, true) | (Some(None), true) => AlignmentFilter::BaseAtCurrentPositionSoftClip,
        (Some(Some(position)), true) => AlignmentFilter::BaseSoftclip(position),
        (None, false) | (Some(None), false) => {
            AlignmentFilter::BaseAtCurrentPosition(base.chars().next().unwrap())
        }
        (Some(Some(position)), false) => {
            AlignmentFilter::Base(position, base.chars().next().unwrap())
        }
    };

    Ok((input, filter))
}
fn node_filter(input: &str) -> IResult<&str, AlignmentFilter> {
    delimited(multispace0, alt((node_base_filter,)), multispace0).parse(input)
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
    BaseAtCurrentBase,

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

fn parse_optional_parenthesis(input: &str) -> IResult<&str, Option<Option<usize>>> {
    opt(delimited(tag("("), opt(usize), tag(")"))).parse(input)
}

// Parse STRAND with optional number in parentheses
fn strand_sort_unit(input: &str) -> IResult<&str, AlignmentSort> {
    let (input, _) = tag_no_case("STRAND")(input)?;
    let (input, digit) = parse_optional_parenthesis(input)?;

    match digit {
        Some(Some(position)) => Ok((input, AlignmentSort::StrandAt(position))),
        _ => Ok((input, AlignmentSort::StrandAtCurrentBase)),
    }
}

// Parse STRAND with optional number in parentheses
fn base_sort_unit(input: &str) -> IResult<&str, AlignmentSort> {
    let (input, _) = tag_no_case("BASE")(input)?;
    let (input, digit) = parse_optional_parenthesis(input)?;

    match digit {
        Some(Some(position)) => Ok((input, AlignmentSort::StrandAt(position))),
        _ => Ok((input, AlignmentSort::StrandAtCurrentBase)),
    }
}

// Parse basic sort options
fn sort_unit(input: &str) -> IResult<&str, AlignmentSort> {
    use nom::Parser;

    alt((
        base_sort_unit,
        strand_sort_unit,
        value(AlignmentSort::Start, tag_no_case("START")),
        value(AlignmentSort::MappingQuality, tag_no_case("MAPQ")),
        value(AlignmentSort::Sample, tag_no_case("SAMPLE")),
        value(AlignmentSort::ReadGroup, tag_no_case("READGROUP")),
        value(AlignmentSort::ReadOrder, tag_no_case("READORDER")),
        value(AlignmentSort::ReadName, tag_no_case("READNAME")),
        value(AlignmentSort::AlignedReadLength, tag_no_case("LENGTH")),
        value(AlignmentSort::InsertSize, tag_no_case("INSERTSIZE")),
        value(AlignmentSort::ChromosomeOfMate, tag_no_case("MATECONTIG")),
        value(AlignmentSort::Tag, tag_no_case("TAG")),
    ))
    .parse(input)
}

// Parse a single sort term (basic sort + optional DESC/DEC)
fn sort_and_direction(input: &str) -> IResult<&str, AlignmentSort> {
    let (input, basic_sort) = terminated(sort_unit, multispace0).parse(input)?;
    let (input, desc_opt) = opt(alt((tag_no_case("DESC"), tag_no_case("ASCE")))).parse(input)?;

    match desc_opt {
        Some("DESC") => Ok((input, basic_sort.reverse())),
        _ => Ok((input, basic_sort)),
    }
}

// Parse the complete sort expression
fn parse_sort_expression(input: &str) -> IResult<&str, AlignmentSort> {
    let (input, sorts) = delimited(
        multispace0,
        separated_list0(
            delimited(multispace0, char(','), multispace0),
            sort_and_direction,
        ),
        multispace0,
    )
    .parse(input)?;

    let result = sorts
        .into_iter()
        .reduce(|acc, sort| acc.then(sort))
        .unwrap_or(AlignmentSort::Default);

    Ok((input, result))
}

// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[test]
//     fn test_parse_alignment_sort() {
//         // Test empty string
//         assert_eq!(parse_alignment_sort(""), AlignmentSort::Default);
//         assert_eq!(parse_alignment_sort("   "), AlignmentSort::Default);

//         // Test BASE without parentheses (current base)
//         assert_eq!(
//             parse_alignment_sort("BASE"),
//             AlignmentSort::BaseAtCurrentBase
//         );
//         assert_eq!(
//             parse_alignment_sort("base"),
//             AlignmentSort::BaseAtCurrentBase
//         );

//         // Test BASE with empty parentheses (current base)
//         assert_eq!(
//             parse_alignment_sort("BASE()"),
//             AlignmentSort::BaseAtCurrentBase
//         );
//         assert_eq!(
//             parse_alignment_sort("base()"),
//             AlignmentSort::BaseAtCurrentBase
//         );

//         // Test BASE with specific position
//         assert_eq!(parse_alignment_sort("BASE(2)"), AlignmentSort::BaseAt(2));
//         assert_eq!(parse_alignment_sort("base(10)"), AlignmentSort::BaseAt(10));

//         // Test STRAND without parentheses (current base)
//         assert_eq!(
//             parse_alignment_sort("STRAND"),
//             AlignmentSort::StrandAtCurrentBase
//         );
//         assert_eq!(
//             parse_alignment_sort("strand"),
//             AlignmentSort::StrandAtCurrentBase
//         );

//         // Test STRAND with empty parentheses (current base)
//         assert_eq!(
//             parse_alignment_sort("STRAND()"),
//             AlignmentSort::StrandAtCurrentBase
//         );
//         assert_eq!(
//             parse_alignment_sort("strand()"),
//             AlignmentSort::StrandAtCurrentBase
//         );

//         // Test STRAND with specific position
//         assert_eq!(
//             parse_alignment_sort("STRAND(5)"),
//             AlignmentSort::StrandAt(5)
//         );

//         // Test simple keywords
//         assert_eq!(parse_alignment_sort("START"), AlignmentSort::Start);
//         assert_eq!(parse_alignment_sort("MAPQ"), AlignmentSort::MappingQuality);
//         assert_eq!(parse_alignment_sort("readname"), AlignmentSort::ReadName);

//         // Test with DESC/DEC
//         assert_eq!(
//             parse_alignment_sort("BASE(2) DESC"),
//             AlignmentSort::Reverse(Box::new(AlignmentSort::BaseAt(2)))
//         );
//         assert_eq!(
//             parse_alignment_sort("BASE() DEC"),
//             AlignmentSort::Reverse(Box::new(AlignmentSort::BaseAtCurrentBase))
//         );
//         assert_eq!(
//             parse_alignment_sort("STRAND desc"),
//             AlignmentSort::Reverse(Box::new(AlignmentSort::StrandAtCurrentBase))
//         );

//         // Test comma-separated (Then)
//         assert_eq!(
//             parse_alignment_sort("BASE(2), START"),
//             AlignmentSort::Then(
//                 Box::new(AlignmentSort::BaseAt(2)),
//                 Box::new(AlignmentSort::Start)
//             )
//         );

//         // Test combination with current base
//         assert_eq!(
//             parse_alignment_sort("BASE, STRAND(3)"),
//             AlignmentSort::Then(
//                 Box::new(AlignmentSort::BaseAtCurrentBase),
//                 Box::new(AlignmentSort::StrandAt(3))
//             )
//         );

//         // Test complex combination
//         assert_eq!(
//             parse_alignment_sort("BASE(2) DESC, MAPQ"),
//             AlignmentSort::Then(
//                 Box::new(AlignmentSort::Reverse(Box::new(AlignmentSort::BaseAt(2)))),
//                 Box::new(AlignmentSort::MappingQuality)
//             )
//         );

//         // Test with extra whitespace
//         assert_eq!(
//             parse_alignment_sort("  BASE(2)  ,  START  "),
//             AlignmentSort::Then(
//                 Box::new(AlignmentSort::BaseAt(2)),
//                 Box::new(AlignmentSort::Start)
//             )
//         );
//     }
// }

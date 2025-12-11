use crate::{
    error::TGVError,
    message::Message,
    message::{AlignmentDisplayOption, AlignmentFilter, AlignmentSort},
    register::{KeyRegister, KeyRegisterType},
    rendering::Scene,
    states::State,
};
use crossterm::event::{KeyCode, KeyEvent};
use itertools::Itertools;
use nom::{
    branch::alt,
    bytes::complete::{tag, tag_no_case},
    character::complete::{char, multispace0, usize},
    combinator::{opt, value},
    multi::{many0, separated_list0},
    sequence::{delimited, preceded, separated_pair, terminated},
    IResult, Parser,
};

#[derive(Default, Debug)]
pub struct CommandBuffer {
    pub input: String,
    pub cursor_position: usize,
}

impl CommandBuffer {
    pub fn clear(&mut self) {
        self.input = String::new();
        self.cursor_position = 0;
    }

    pub fn add_char(&mut self, c: char) {
        self.input.insert(self.cursor_position, c);
        self.cursor_position += 1;
    }

    pub fn backspace(&mut self) {
        if self.cursor_position > 0 {
            self.input.remove(self.cursor_position - 1);
            self.cursor_position -= 1;
        }
    }

    pub fn move_cursor_left(&mut self, by: usize) {
        self.cursor_position = self.cursor_position.saturating_sub(by);
    }

    pub fn move_cursor_right(&mut self, by: usize) {
        self.cursor_position = self
            .cursor_position
            .saturating_add(by)
            .clamp(0, self.input.len());
    }
}
#[derive(Default, Debug)]
pub struct CommandModeRegister {
    pub buffer: CommandBuffer,
}

impl KeyRegister for CommandModeRegister {
    fn handle_key_event(
        &mut self,
        key_event: KeyEvent,
        state: &State,
    ) -> Result<Vec<Message>, TGVError> {
        match key_event.code {
            KeyCode::Esc => Ok(vec![
                Message::ClearAllKeyRegisters,
                Message::SwitchKeyRegister(KeyRegisterType::Normal),
            ]),

            KeyCode::Enter => match self.buffer.input.as_ref() {
                "h" => Ok(vec![
                    Message::SwitchScene(Scene::Help),
                    Message::ClearAllKeyRegisters,
                    Message::SwitchKeyRegister(KeyRegisterType::Help),
                ]),
                "ls" | "contigs" => Ok(vec![
                    Message::SwitchScene(Scene::ContigList),
                    Message::ClearAllKeyRegisters,
                    Message::SwitchKeyRegister(KeyRegisterType::ContigList),
                ]),
                _ => Ok(self
                    .parse()
                    .unwrap_or_else(|e| vec![Message::Message(format!("{}", e))])
                    .into_iter()
                    .chain(vec![
                        Message::ClearAllKeyRegisters,
                        Message::SwitchKeyRegister(KeyRegisterType::Normal),
                    ])
                    .collect_vec()),
            },
            KeyCode::Char(c) => {
                self.buffer.add_char(c);
                Ok(vec![])
            }
            KeyCode::Backspace => {
                self.buffer.backspace();
                Ok(vec![])
            }
            KeyCode::Left => {
                self.buffer.move_cursor_left(1);
                Ok(vec![])
            }
            KeyCode::Right => {
                self.buffer.move_cursor_right(1);
                Ok(vec![])
            }
            _ => Err(TGVError::RegisterError(format!(
                "Invalid command mode input: {:?}",
                key_event
            ))),
        }
    }
}

impl CommandModeRegister {
    /// Supported commands:
    /// :q: Quit.
    /// :h: Help.
    /// :1234: Go to position 1234 on the same contig.
    /// :12:1234: Go to position 1234 on contig 12.
    pub fn parse(&self) -> Result<Vec<Message>, TGVError> {
        if self.buffer.input == "q" {
            return Ok(vec![Message::Quit]);
        }

        if self.buffer.input == "h" {
            return Err(TGVError::RegisterError(
                "TODO: help screen is not implemented".to_string(),
            ));
        }

        if let Ok((_, true)) = restore_default_options(&self.buffer.input) {
            // TODO: this results in resetting twice now.
            return Ok(vec![Message::SetAlignmentChange(vec![])]);
        }

        if let Ok((_, true)) = view_as_pair(&self.buffer.input) {
            return Ok(vec![Message::SetAlignmentChange(vec![
                AlignmentDisplayOption::ViewAsPairs,
            ])]);
        }

        if let Ok((remaining, options)) = parse_display_options(&self.buffer.input) {
            if remaining.is_empty() {
                return Ok(vec![Message::SetAlignmentChange(options)]);
            }
        }

        let split = self.buffer.input.split(":").collect::<Vec<&str>>();

        match split.len() {
            1 => match split[0].parse::<usize>() {
                Ok(n) => Ok(vec![Message::GotoCoordinate(n)]),
                Err(_) => Ok(vec![Message::GoToGene(split[0].to_string())]),
            },
            2 => match split[1].parse::<usize>() {
                Ok(n) => Ok(vec![Message::GotoContigNameCoordinate(
                    split[0].to_string(),
                    n,
                )]),
                Err(_) => Err(TGVError::RegisterError(format!(
                    "Invalid command mode input: {}",
                    self.buffer.input
                ))),
            },
            _ => Err(TGVError::RegisterError(format!(
                "Invalid command mode input: {}",
                self.buffer.input
            ))),
        }
    }
}

/// Highest level parser
fn parse_display_options(input: &str) -> IResult<&str, Vec<AlignmentDisplayOption>> {
    many0(alt((parse_filter, parse_sort))).parse(input)
}

fn restore_default_options(input: &str) -> IResult<&str, bool> {
    let (input, parsed) = delimited(
        multispace0,
        alt((tag_no_case("clear"), tag_no_case("default"))),
        multispace0,
    )
    .parse(input)?;

    Ok((input, (input.is_empty() && !parsed.is_empty())))
}

fn view_as_pair(input: &str) -> IResult<&str, bool> {
    let (input, parsed) =
        delimited(multispace0, tag_no_case("paired"), multispace0).parse(input)?;

    Ok((input, (input.is_empty() && !parsed.is_empty())))
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
        Some(Some(position)) => Ok((input, AlignmentSort::BaseAt(position))),
        _ => Ok((input, AlignmentSort::BaseAtCurrentPosition)),
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
    let (input, desc_opt) = opt(alt((tag_no_case("DESC"), tag_no_case("ASC")))).parse(input)?;

    match desc_opt {
        Some(desc) => {
            if desc.to_ascii_lowercase() == *"desc" {
                Ok((input, basic_sort.reverse()))
            } else {
                Ok((input, basic_sort))
            }
        }
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

fn parse_filter(input: &str) -> IResult<&str, AlignmentDisplayOption> {
    delimited(
        preceded(
            multispace0,
            alt((tag_no_case("FILTER"), tag_no_case("WHERE"))),
        ),
        node_filter,
        multispace0,
    )
    .parse(input)
    .map(|(input, filter)| (input, AlignmentDisplayOption::Filter(filter)))
}

fn parse_sort(input: &str) -> IResult<&str, AlignmentDisplayOption> {
    delimited(
        preceded(
            multispace0,
            alt((tag_no_case("SORT"), tag_no_case("ORDER BY"))),
        ),
        parse_sort_expression,
        multispace0,
    )
    .parse(input)
    .map(|(input, filter)| (input, AlignmentDisplayOption::Sort(filter)))
}

fn node_base_filter(input: &str) -> IResult<&str, AlignmentFilter> {
    let (input, (position, base)) = preceded(
        tag_no_case("BASE"),
        separated_pair(
            parse_optional_parenthesis,
            delimited(multispace0, tag("="), multispace0),
            alt((
                tag_no_case("A"),
                tag_no_case("T"),
                tag_no_case("C"),
                tag_no_case("G"),
                tag_no_case("N"),
                tag_no_case("SOFTCLIP"),
            )),
        ),
    )
    .parse(input)?;

    let is_softclip = base.to_lowercase() == "softclip";

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

#[cfg(test)]
mod tests {

    use super::*;
    use crate::message::Message;
    use rstest::rstest;

    #[rstest]
    // Test empty strings
    #[case("", AlignmentSort::Default)]
    #[case("   ", AlignmentSort::Default)]
    #[case("BASE", AlignmentSort::BaseAtCurrentPosition)]
    #[case("base", AlignmentSort::BaseAtCurrentPosition)]
    #[case("BASE()", AlignmentSort::BaseAtCurrentPosition)]
    #[case("base()", AlignmentSort::BaseAtCurrentPosition)]
    #[case("BASE(2)", AlignmentSort::BaseAt(2))]
    #[case("base(10)", AlignmentSort::BaseAt(10))]
    // Test STRAND variants
    #[case("STRAND", AlignmentSort::StrandAtCurrentBase)]
    #[case("strand", AlignmentSort::StrandAtCurrentBase)]
    #[case("STRAND()", AlignmentSort::StrandAtCurrentBase)]
    #[case("strand()", AlignmentSort::StrandAtCurrentBase)]
    #[case("STRAND(5)", AlignmentSort::StrandAt(5))]
    // Test simple keywords
    #[case("START", AlignmentSort::Start)]
    #[case("MAPQ", AlignmentSort::MappingQuality)]
    #[case("readname", AlignmentSort::ReadName)]
    // Test with DESC/DEC
    #[case(
        "BASE(2) DESC",
        AlignmentSort::Reverse(Box::new(AlignmentSort::BaseAt(2)))
    )]
    #[case(
        "BASE desc",
        AlignmentSort::Reverse(Box::new(AlignmentSort::BaseAtCurrentPosition))
    )]
    #[case(
        "STRAND desc",
        AlignmentSort::Reverse(Box::new(AlignmentSort::StrandAtCurrentBase))
    )]
    // Test comma-separated (Then)
    #[case(
        "BASE(2), START",
        AlignmentSort::Then(Box::new(AlignmentSort::BaseAt(2)), Box::new(AlignmentSort::Start))
    )]
    #[case(
        "BASE, STRAND(3)",
        AlignmentSort::Then(
            Box::new(AlignmentSort::BaseAtCurrentPosition),
            Box::new(AlignmentSort::StrandAt(3))
        )
    )]
    // Test complex combination
    #[case(
        "BASE(2) DESC, MAPQ",
        AlignmentSort::Then(
            Box::new(AlignmentSort::Reverse(Box::new(AlignmentSort::BaseAt(2)))),
            Box::new(AlignmentSort::MappingQuality)
        )
    )]
    // Test with extra whitespace
    #[case(
        "  BASE(2)  ,  START  ",
        AlignmentSort::Then(Box::new(AlignmentSort::BaseAt(2)), Box::new(AlignmentSort::Start))
    )]
    fn test_parse_alignment_sort(#[case] input: &str, #[case] expected: AlignmentSort) {
        let (remaining, sort) = parse_sort_expression(input).unwrap();
        assert!(remaining.is_empty());
        assert_eq!(sort, expected);
        // TODO: no remaining characters
    }

    #[rstest]
    #[case("BASE() DEC")]
    fn test_parse_alignment_sort_errors(#[case] input: &str) {
        match parse_sort_expression(input) {
            Ok((input, sort)) => {
                assert!(!input.is_empty())
            }
            Err(_) => {
                // Ok
            }
        }
    }

    #[rstest]
    #[case("BASE=A", AlignmentFilter::BaseAtCurrentPosition('A'))]
    #[case("BASE(123)=A", AlignmentFilter::Base(123, 'A'))]
    #[case("BASE=softclip", AlignmentFilter::BaseAtCurrentPositionSoftClip)]
    #[case("BASE(123)=softclip", AlignmentFilter::BaseSoftclip(123))]
    #[case("BASE(123) = A", AlignmentFilter::Base(123, 'A'))]
    fn test_parse_alignment_filter(#[case] input: &str, #[case] expected: AlignmentFilter) {
        let (remaining, filter) = node_filter(input).unwrap();

        assert!(remaining.is_empty());
        assert_eq!(filter, expected);
    }

    #[rstest]
    #[case("  BASE=DD  ")]
    fn test_parse_alignment_filter_error(#[case] input: &str) {
        match parse_sort_expression(input) {
            Ok((input, sort)) => {
                assert!(!input.is_empty())
            }
            Err(_) => {
                // Ok
            }
        }
    }

    #[rstest]
    #[case("q", Ok(vec![Message::Quit]))]
    #[case("1234", Ok(vec![Message::GotoCoordinate(1234)]))]
    #[case("chr1:1000", Ok(vec![Message::GotoContigNameCoordinate(
        "chr1".to_string(),
        1000,
    )]))]
    #[case("17:7572659", Ok(vec![Message::GotoContigNameCoordinate(
        "17".to_string(),
        7572659,
    )]))]
    #[case("TP53", Ok(vec![Message::GoToGene("TP53".to_string())]))]
    #[case("invalid:command:format", Err(TGVError::RegisterError("Invalid command mode input: invalid:command:format".to_string())))]
    #[case("chr1:invalid", Err(TGVError::RegisterError("Invalid command mode input: chr1:invalid".to_string())))]
    fn test_command_parse(#[case] input: &str, #[case] expected: Result<Vec<Message>, TGVError>) {
        let register = CommandModeRegister {
            buffer: CommandBuffer {
                input: input.to_string(),
                cursor_position: input.len(),
            },
        };
        match (&register.parse(), &expected) {
            (Ok(result), Ok(expected)) => assert_eq!(result, expected),
            (Err(e), Err(expected)) => {} // OK
            _ => panic!(
                "Test failed.  result: {:?}, expected: {:?}",
                register.parse(),
                expected
            ),
        }
    }
}

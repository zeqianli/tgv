//! Parsing for command mode (`:`) and search mode (`/`) input.

use crate::{
    error::TGVError,
    message::{AlignmentDisplayOption, Message, Movement, Zoom},
};

/// Parse command mode input. Supported commands:
/// - `:q`: Quit.
/// - `:w [name|path]`: Save the session. Without an argument, save to the active session.
/// - `:wq [name|path]`: Save the session and quit.
/// - `:paired`: Show alignments as read pairs.
/// - `:clear`, `:default`: Restore the default alignment display.
///
/// The front end handles commands that only change its own view, such as listing contigs.
pub fn parse(input: &str) -> Result<Vec<Message>, TGVError> {
    let input = input.trim();
    if input == "q" {
        return Ok(vec![Message::Quit]);
    }

    if let Some(message) = parse_session_command(input, "w", Message::SaveSession) {
        return Ok(vec![message]);
    }

    if let Some(message) = parse_session_command(input, "wq", Message::SaveAndQuit) {
        return Ok(vec![message]);
    }

    if input.eq_ignore_ascii_case("clear") || input.eq_ignore_ascii_case("default") {
        return Ok(vec![Message::SetAlignmentOption(vec![])]);
    }

    if input.eq_ignore_ascii_case("paired") {
        return Ok(vec![Message::SetAlignmentOption(vec![
            AlignmentDisplayOption::ViewAsPairs,
        ])]);
    }

    Err(TGVError::RegisterError(format!("Unknown command: {input}")))
}

/// Parse search mode input into messages. Supported targets:
/// - `/1234`: Go to position 1234 on the current contig.
/// - `/chr1:1234`: Go to position 1234 on contig `chr1`.
/// - `/chr1:1000-2000`, `/1000-2000`: Show a 1-based, inclusive region, centered and zoomed to
///   fit.
/// - `/TP53`: Go to a gene.
///
/// Positions may use comma separators, such as `chr1:1,000,000`.
pub fn parse_search(input: &str) -> Result<Vec<Message>, TGVError> {
    let input = input.trim();
    let invalid = || TGVError::RegisterError(format!("Invalid search: {input}"));
    if input.is_empty() {
        return Err(invalid());
    }

    // Contig names may contain colons, such as HLA alleles, so the position follows the last one.
    let (contig, locus) = match input.rsplit_once(':') {
        Some((contig, locus)) => (Some(contig), locus),
        None => (None, input),
    };
    let target = match parse_locus(locus) {
        Some(target) => target,
        None if contig.is_none() => {
            return Ok(vec![Message::Move(Movement::Gene(input.to_string()))]);
        }
        None => return Err(invalid()),
    };
    let (position, bases) = match target {
        SearchLocus::Position(position) => (position, None),
        SearchLocus::Region { start, end } => {
            if start == 0 || start > end {
                return Err(invalid());
            }
            (start + (end - start) / 2, Some(end - start + 1))
        }
    };
    let movement = match contig {
        Some(contig) => Movement::ContigNamePosition(contig.to_string(), position),
        None => Movement::Position(position),
    };
    let mut messages = vec![Message::Move(movement)];
    if let Some(bases) = bases {
        messages.push(Zoom::Fit { bases }.into());
    }
    Ok(messages)
}

enum SearchLocus {
    Position(u64),
    /// 1-based, inclusive.
    Region {
        start: u64,
        end: u64,
    },
}

/// Parse `1234` or `1000-2000`, allowing comma separators.
fn parse_locus(locus: &str) -> Option<SearchLocus> {
    let number = |text: &str| text.trim().replace(',', "").parse::<u64>().ok();
    match locus.split_once('-') {
        Some((start, end)) => Some(SearchLocus::Region {
            start: number(start)?,
            end: number(end)?,
        }),
        None => number(locus).map(SearchLocus::Position),
    }
}

fn parse_session_command(
    input: &str,
    command: &str,
    make_message: impl FnOnce(Option<String>) -> Message,
) -> Option<Message> {
    if input == command {
        return Some(make_message(None));
    }

    input.strip_prefix(command).and_then(|remaining| {
        let session_name = remaining.strip_prefix(' ')?.trim();
        Some(make_message(
            (!session_name.is_empty()).then(|| session_name.to_string()),
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case("q", Some(vec![Message::Quit]))]
    #[case("w", Some(vec![Message::SaveSession(None)]))]
    #[case("w session-name", Some(vec![Message::SaveSession(Some("session-name".to_string()))]))]
    #[case("w /tmp/test.toml", Some(vec![Message::SaveSession(Some("/tmp/test.toml".to_string()))]))]
    #[case("wq", Some(vec![Message::SaveAndQuit(None)]))]
    #[case("wq session-name", Some(vec![Message::SaveAndQuit(Some("session-name".to_string()))]))]
    #[case("paired", Some(vec![Message::SetAlignmentOption(vec![AlignmentDisplayOption::ViewAsPairs])]))]
    #[case("CLEAR", Some(vec![Message::SetAlignmentOption(vec![])]))]
    #[case("1234", None)]
    #[case("chr1:1000", None)]
    #[case("TP53", None)]
    #[case("sort base", None)]
    #[case("wfoo", None)]
    fn test_command_parse(#[case] input: &str, #[case] expected: Option<Vec<Message>>) {
        assert_eq!(parse(input).ok(), expected);
    }

    #[rstest]
    #[case("1234", Some(vec![Movement::Position(1234).into()]))]
    #[case("1,234", Some(vec![Movement::Position(1234).into()]))]
    #[case("chr1:1000", Some(vec![Movement::ContigNamePosition("chr1".to_string(), 1000).into()]))]
    #[case("17:7572659", Some(vec![Movement::ContigNamePosition("17".to_string(), 7572659).into()]))]
    #[case("chr1:1,000-2,000", Some(vec![
        Movement::ContigNamePosition("chr1".to_string(), 1500).into(),
        Zoom::Fit { bases: 1001 }.into(),
    ]))]
    #[case("100-199", Some(vec![Movement::Position(149).into(), Zoom::Fit { bases: 100 }.into()]))]
    #[case("HLA-A*01:01:01:01:100", Some(vec![
        Movement::ContigNamePosition("HLA-A*01:01:01:01".to_string(), 100).into(),
    ]))]
    #[case("TP53", Some(vec![Movement::Gene("TP53".to_string()).into()]))]
    #[case("HLA-A", Some(vec![Movement::Gene("HLA-A".to_string()).into()]))]
    #[case("", None)]
    #[case("chr1:invalid", None)]
    #[case("chr1:200-100", None)]
    #[case("chr1:0-100", None)]
    fn test_search_parse(#[case] input: &str, #[case] expected: Option<Vec<Message>>) {
        assert_eq!(parse_search(input).ok(), expected);
    }
}

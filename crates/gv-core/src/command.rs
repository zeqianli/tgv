//! Parsing for command mode (`:`) and search mode (`/`) input.

use crate::{
    error::TGVError,
    message::{AlignmentDisplayOption, Message, Movement},
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

/// Parse search mode input into a movement. Supported targets:
/// - `/1234`: Go to position 1234 on the current contig.
/// - `/chr1:1234`: Go to position 1234 on contig `chr1`.
/// - `/TP53`: Go to a gene.
pub fn parse_search(input: &str) -> Result<Vec<Message>, TGVError> {
    let input = input.trim();
    let invalid = || TGVError::RegisterError(format!("Invalid search: {input}"));
    if input.is_empty() {
        return Err(invalid());
    }

    let movement = match input.split_once(':') {
        Some((contig, position)) => Movement::ContigNamePosition(
            contig.to_string(),
            position.parse::<u64>().map_err(|_| invalid())?,
        ),
        None => match input.parse::<u64>() {
            Ok(position) => Movement::Position(position),
            Err(_) => Movement::Gene(input.to_string()),
        },
    };
    Ok(vec![Message::Move(movement)])
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
    #[case("1234", Some(Movement::Position(1234)))]
    #[case("chr1:1000", Some(Movement::ContigNamePosition("chr1".to_string(), 1000)))]
    #[case("17:7572659", Some(Movement::ContigNamePosition("17".to_string(), 7572659)))]
    #[case("TP53", Some(Movement::Gene("TP53".to_string())))]
    #[case("", None)]
    #[case("chr1:invalid", None)]
    #[case("invalid:command:format", None)]
    fn test_search_parse(#[case] input: &str, #[case] expected: Option<Movement>) {
        assert_eq!(
            parse_search(input).ok(),
            expected.map(|movement| vec![Message::Move(movement)])
        );
    }
}

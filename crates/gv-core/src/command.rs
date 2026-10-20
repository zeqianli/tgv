//! Parsing for command mode (`:`) and search mode (`/`) input.

use crate::{
    error::TGVError,
    message::{AlignmentDisplayOption, Message, Movement, Zoom},
};

/// Parse command mode input. Supported commands:
/// - `:q`: Quit, unless a track has unsaved changes. `:q!` quits anyway.
/// - `:w [path]`: Write the focused BED track to its file, or to the path.
/// - `:wa`: Write every modified BED track.
/// - `:wq [path]`, `:wqa`, `:xa`: Write, then quit.
/// - `:ws [name|path]`: Save the session. Without an argument, save to the active session.
/// - `:wsq [name|path]`: Save the session and quit.
/// - `:bd`: Remove the focused track, unless it has unsaved changes. `:bd!` removes it anyway.
/// - `:e PATH…`, `:open PATH…`: Add file tracks. Paths use shell quoting, and `~` expands to the
///   home directory.
/// - `:paired`: Show alignments as read pairs.
/// - `:clear`, `:default`: Restore the default alignment display.
///
/// The front end handles commands that only change its own view, such as listing contigs.
pub fn parse(input: &str) -> Result<Vec<Message>, TGVError> {
    let input = input.trim();
    match input {
        "q" => return Ok(vec![Message::Quit]),
        "q!" => return Ok(vec![Message::ForceQuit]),
        "wa" => return Ok(vec![Message::WriteAll]),
        "wqa" | "xa" => return Ok(vec![Message::WriteAllAndQuit]),
        "bd" => return Ok(vec![Message::CloseTrack { force: false }]),
        "bd!" => return Ok(vec![Message::CloseTrack { force: true }]),
        _ => {}
    }

    let expanded = |path: Option<String>| path.map(|path| shellexpand::tilde(&path).into_owned());
    if let Some(message) =
        parse_session_command(input, "w", |path| Message::WriteTrack(expanded(path)))
    {
        return Ok(vec![message]);
    }

    if let Some(message) =
        parse_session_command(input, "wq", |path| Message::WriteAndQuit(expanded(path)))
    {
        return Ok(vec![message]);
    }

    if let Some(message) = parse_session_command(input, "ws", Message::SaveSession) {
        return Ok(vec![message]);
    }

    if let Some(message) = parse_session_command(input, "wsq", Message::SaveAndQuit) {
        return Ok(vec![message]);
    }

    if let Some(message) = parse_open_command(input) {
        return message.map(|message| vec![message]);
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

fn parse_open_command(input: &str) -> Option<Result<Message, TGVError>> {
    let (command, arguments) = input.split_once(char::is_whitespace).unwrap_or((input, ""));
    if command != "e" && command != "open" {
        return None;
    }

    let paths = match shlex::split(arguments) {
        Some(paths) if !paths.is_empty() => paths,
        Some(_) => {
            return Some(Err(TGVError::RegisterError(format!(
                "Name the files to open: :{command} PATH…"
            ))));
        }
        None => {
            return Some(Err(TGVError::RegisterError(format!(
                "Unmatched quote in: {arguments}"
            ))));
        }
    };
    Some(Ok(Message::OpenFiles(
        paths
            .iter()
            .map(|path| shellexpand::tilde(path).into_owned())
            .collect(),
    )))
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
    #[case("q!", Some(vec![Message::ForceQuit]))]
    #[case("w", Some(vec![Message::WriteTrack(None)]))]
    #[case("w /tmp/peaks.bed", Some(vec![Message::WriteTrack(Some("/tmp/peaks.bed".to_string()))]))]
    #[case("wa", Some(vec![Message::WriteAll]))]
    #[case("wq", Some(vec![Message::WriteAndQuit(None)]))]
    #[case("wqa", Some(vec![Message::WriteAllAndQuit]))]
    #[case("xa", Some(vec![Message::WriteAllAndQuit]))]
    #[case("ws", Some(vec![Message::SaveSession(None)]))]
    #[case("ws session-name", Some(vec![Message::SaveSession(Some("session-name".to_string()))]))]
    #[case("ws /tmp/test.toml", Some(vec![Message::SaveSession(Some("/tmp/test.toml".to_string()))]))]
    #[case("wsq", Some(vec![Message::SaveAndQuit(None)]))]
    #[case("wsq session-name", Some(vec![Message::SaveAndQuit(Some("session-name".to_string()))]))]
    #[case("bd", Some(vec![Message::CloseTrack { force: false }]))]
    #[case("bd!", Some(vec![Message::CloseTrack { force: true }]))]
    #[case("paired", Some(vec![Message::SetAlignmentOption(vec![AlignmentDisplayOption::ViewAsPairs])]))]
    #[case("CLEAR", Some(vec![Message::SetAlignmentOption(vec![])]))]
    #[case("1234", None)]
    #[case("chr1:1000", None)]
    #[case("TP53", None)]
    #[case("sort base", None)]
    #[case("wfoo", None)]
    #[case("e a.bam", Some(vec![Message::OpenFiles(vec!["a.bam".to_string()])]))]
    #[case("open a.bam  b.vcf", Some(vec![Message::OpenFiles(vec!["a.bam".to_string(), "b.vcf".to_string()])]))]
    #[case("e 'my file.bed'", Some(vec![Message::OpenFiles(vec!["my file.bed".to_string()])]))]
    #[case(r"e my\ file.bed", Some(vec![Message::OpenFiles(vec!["my file.bed".to_string()])]))]
    #[case("e", None)]
    #[case("open", None)]
    #[case("e 'a.bam", None)]
    #[case("efoo", None)]
    #[case("opena.bam", None)]
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

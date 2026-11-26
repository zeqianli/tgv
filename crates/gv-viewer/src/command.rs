use crate::message::{Action, NavigationAction, SessionAction, TrackAction};
use gv_core::message::{AlignmentDisplayOption, Movement, Zoom};

pub fn parse(input: &str) -> Result<Vec<Action>, crate::Error> {
    let input = input.trim();
    match input {
        "q" => return Ok(vec![Action::Session(SessionAction::Quit)]),
        "q!" => return Ok(vec![Action::Session(SessionAction::ForceQuit)]),
        "wa" => return Ok(vec![Action::Session(SessionAction::WriteAll)]),
        "wqa" | "xa" => return Ok(vec![Action::Session(SessionAction::WriteAllAndQuit)]),
        "new" => return Ok(vec![Action::Track(TrackAction::NewBedTrack)]),
        _ => {}
    }

    let expanded = |path: Option<String>| path.map(|path| shellexpand::tilde(&path).into_owned());
    if let Some(message) = parse_session_command(input, "w", |path| {
        Action::Session(SessionAction::WriteTrack(expanded(path)))
    }) {
        return Ok(vec![message]);
    }

    if let Some(message) = parse_session_command(input, "wq", |path| {
        Action::Session(SessionAction::WriteAndQuit(expanded(path)))
    }) {
        return Ok(vec![message]);
    }

    if let Some(message) = parse_session_command(input, "ws", |path| {
        Action::Session(SessionAction::SaveSession(path))
    }) {
        return Ok(vec![message]);
    }

    if let Some(message) = parse_session_command(input, "wsq", |path| {
        Action::Session(SessionAction::SaveAndQuit(path))
    }) {
        return Ok(vec![message]);
    }

    if let Some(message) = parse_open_command(input) {
        return message.map(|message| vec![message]);
    }

    if input.eq_ignore_ascii_case("clear") || input.eq_ignore_ascii_case("default") {
        return Ok(vec![Action::Track(TrackAction::SetAlignmentOption(vec![]))]);
    }

    if input.eq_ignore_ascii_case("paired") {
        return Ok(vec![Action::Track(TrackAction::SetAlignmentOption(vec![
            AlignmentDisplayOption::ViewAsPairs,
        ]))]);
    }

    Err(crate::Error::UnknownCommand(input.to_string()))
}

pub fn parse_search(input: &str) -> Result<Vec<Action>, crate::Error> {
    let input = input.trim();
    if let Some((accession, description)) = gv_core::hgvs::split(input) {
        return Ok(vec![Action::Navigate(NavigationAction::Hgvs {
            accession: accession.to_string(),
            description: description.to_string(),
        })]);
    }
    let invalid = || crate::Error::InvalidSearch(input.to_string());
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
            return Ok(vec![Action::Navigate(NavigationAction::Move(
                Movement::Gene(input.to_string()),
            ))]);
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
    let mut messages = vec![Action::Navigate(NavigationAction::Move(movement))];
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

fn parse_open_command(input: &str) -> Option<Result<Action, crate::Error>> {
    let (command, arguments) = input.split_once(char::is_whitespace).unwrap_or((input, ""));
    if command != "e" && command != "open" {
        return None;
    }

    let paths = match shlex::split(arguments) {
        Some(paths) if !paths.is_empty() => paths,
        Some(_) => {
            return Some(Err(crate::Error::MissingPaths {
                command: command.to_string(),
            }));
        }
        None => {
            return Some(Err(crate::Error::UnmatchedQuote(arguments.to_string())));
        }
    };
    Some(Ok(Action::Track(TrackAction::OpenFiles(
        paths
            .iter()
            .map(|path| shellexpand::tilde(path).into_owned())
            .collect(),
    ))))
}

fn parse_session_command(
    input: &str,
    command: &str,
    make_message: impl FnOnce(Option<String>) -> Action,
) -> Option<Action> {
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
    #[case("q", Some(vec![Action::Session(SessionAction::Quit)]))]
    #[case("q!", Some(vec![Action::Session(SessionAction::ForceQuit)]))]
    #[case("w", Some(vec![Action::Session(SessionAction::WriteTrack(None))]))]
    #[case("w /tmp/peaks.bed", Some(vec![Action::Session(SessionAction::WriteTrack(Some("/tmp/peaks.bed".to_string())))]))]
    #[case("wa", Some(vec![Action::Session(SessionAction::WriteAll)]))]
    #[case("wq", Some(vec![Action::Session(SessionAction::WriteAndQuit(None))]))]
    #[case("wqa", Some(vec![Action::Session(SessionAction::WriteAllAndQuit)]))]
    #[case("xa", Some(vec![Action::Session(SessionAction::WriteAllAndQuit)]))]
    #[case("ws", Some(vec![Action::Session(SessionAction::SaveSession(None))]))]
    #[case("ws session-name", Some(vec![Action::Session(SessionAction::SaveSession(Some("session-name".to_string())))]))]
    #[case("ws /tmp/test.toml", Some(vec![Action::Session(SessionAction::SaveSession(Some("/tmp/test.toml".to_string())))]))]
    #[case("wsq", Some(vec![Action::Session(SessionAction::SaveAndQuit(None))]))]
    #[case("wsq session-name", Some(vec![Action::Session(SessionAction::SaveAndQuit(Some("session-name".to_string())))]))]
    #[case("new", Some(vec![Action::Track(TrackAction::NewBedTrack)]))]
    #[case("paired", Some(vec![Action::Track(TrackAction::SetAlignmentOption(vec![AlignmentDisplayOption::ViewAsPairs]))]))]
    #[case("CLEAR", Some(vec![Action::Track(TrackAction::SetAlignmentOption(vec![]))]))]
    #[case("1234", None)]
    #[case("chr1:1000", None)]
    #[case("TP53", None)]
    #[case("sort base", None)]
    #[case("wfoo", None)]
    #[case("e a.bam", Some(vec![Action::Track(TrackAction::OpenFiles(vec!["a.bam".to_string()]))]))]
    #[case("open a.bam  b.vcf", Some(vec![Action::Track(TrackAction::OpenFiles(vec!["a.bam".to_string(), "b.vcf".to_string()]))]))]
    #[case("e 'my file.bed'", Some(vec![Action::Track(TrackAction::OpenFiles(vec!["my file.bed".to_string()]))]))]
    #[case(r"e my\ file.bed", Some(vec![Action::Track(TrackAction::OpenFiles(vec!["my file.bed".to_string()]))]))]
    #[case("e", None)]
    #[case("open", None)]
    #[case("e 'a.bam", None)]
    #[case("efoo", None)]
    #[case("opena.bam", None)]
    fn test_command_parse(#[case] input: &str, #[case] expected: Option<Vec<Action>>) {
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
    #[case("NM_004985.5:c.35G>A", Some(vec![Action::Navigate(NavigationAction::Hgvs {
        accession: "NM_004985.5".to_string(),
        description: "c.35G>A".to_string(),
    })]))]
    #[case(" KRAS:p.12 ", Some(vec![Action::Navigate(NavigationAction::Hgvs {
        accession: "KRAS".to_string(),
        description: "p.12".to_string(),
    })]))]
    #[case("chr1:invalid", None)]
    #[case("chr1:200-100", None)]
    #[case("chr1:0-100", None)]
    fn test_search_parse(#[case] input: &str, #[case] expected: Option<Vec<Action>>) {
        assert_eq!(parse_search(input).ok(), expected);
    }
}

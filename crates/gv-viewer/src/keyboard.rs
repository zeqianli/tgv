use crate::message::{InputAction, NavigationAction};

use crate::{
    keys::{Key, KeyCode, Mode, PendingKeys},
    message::{Action, Movement},
    viewer::Scene,
};
use gv_core::prelude::*;
use itertools::Itertools;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum InputMode {
    Editing(Mode),
    Command(Mode),
    Search(Mode),
    ContigList(Mode),
}

impl From<Mode> for InputMode {
    fn from(mode: Mode) -> Self {
        Self::Editing(mode)
    }
}

#[derive(Debug, Clone)]
pub struct KeyboardState {
    pub mode: InputMode,
    pub pending: PendingKeys,
    pub command: String,
    pub command_cursor: usize,

    pub contig_list_cursor: usize,
}

impl Default for KeyboardState {
    fn default() -> Self {
        Self {
            mode: InputMode::Editing(Mode::Normal),
            pending: PendingKeys::default(),
            command: "".to_string(),
            command_cursor: 0,

            contig_list_cursor: 0,
        }
    }
}

impl KeyboardState {
    pub fn shown_mode(&self) -> Mode {
        match self.mode {
            InputMode::Editing(mode)
            | InputMode::Command(mode)
            | InputMode::Search(mode)
            | InputMode::ContigList(mode) => mode,
        }
    }

    pub fn leave_insert_mode(&mut self, track: TrackId) {
        if self.shown_mode() != Mode::Insert(track) {
            return;
        }
        self.mode = match self.mode {
            InputMode::Editing(_) => {
                self.pending.clear();
                InputMode::Editing(Mode::Normal)
            }
            InputMode::Command(_) => InputMode::Command(Mode::Normal),
            InputMode::Search(_) => InputMode::Search(Mode::Normal),
            InputMode::ContigList(_) => InputMode::ContigList(Mode::Normal),
        };
    }

    pub fn clear(&mut self) {
        self.pending.clear();
        self.command.clear();

        self.command_cursor = 0;
        self.contig_list_cursor = 0;
    }
}

impl KeyboardState {
    pub fn handle_paste(&mut self, text: &str) -> Vec<Action> {
        match self.mode {
            InputMode::Command(_) | InputMode::Search(_) => {
                let text: String = text.chars().filter(|c| !matches!(c, '\n' | '\r')).collect();
                self.command.insert_str(self.command_cursor, &text);
                self.command_cursor += text.len();
                vec![Action::Input(InputAction::CommandChanged)]
            }
            InputMode::ContigList(_) | InputMode::Editing(_) => Vec::new(),
        }
    }

    fn handle_contig_list(
        &mut self,
        key_event: Key,
        state: &State,
    ) -> Result<Vec<Action>, crate::Error> {
        match key_event.code {
            KeyCode::Enter if state.contig_header.contigs.is_empty() => Ok(vec![]),
            KeyCode::Enter => Ok(vec![
                Action::Input(InputAction::SwitchInputMode(self.shown_mode().into())),
                Action::Input(InputAction::SwitchScene(Scene::Main)),
                Movement::ContigIndex(self.contig_list_cursor).into(),
            ]),

            KeyCode::Esc => Ok(vec![
                Action::Input(InputAction::SwitchInputMode(self.shown_mode().into())),
                Action::Input(InputAction::SwitchScene(Scene::Main)),
            ]),

            KeyCode::Char('j') | KeyCode::Down => {
                let index = usize::min(
                    self.contig_list_cursor.saturating_add(1),
                    state.contig_header.contigs.len().saturating_sub(1),
                );
                Ok(vec![Action::Navigate(NavigationAction::SelectContig(
                    index,
                ))])
            }
            KeyCode::Char('k') | KeyCode::Up => Ok(vec![Action::Navigate(
                NavigationAction::SelectContig(self.contig_list_cursor.saturating_sub(1)),
            )]),

            KeyCode::Char('}') => {
                let index = usize::min(
                    self.contig_list_cursor.saturating_add(30),
                    state.contig_header.contigs.len().saturating_sub(1),
                );
                Ok(vec![Action::Navigate(NavigationAction::SelectContig(
                    index,
                ))])
            }

            KeyCode::Char('{') => Ok(vec![Action::Navigate(NavigationAction::SelectContig(
                self.contig_list_cursor.saturating_sub(30),
            ))]),
            _ => Ok(vec![]),
        }
    }

    /// Edit the command line, shared by command and search mode. Enter submits the line to
    /// `submit`.
    fn handle_command_line(
        &mut self,
        key_event: Key,
        submit: fn(&str) -> Result<Vec<Action>, crate::Error>,
    ) -> Result<Vec<Action>, crate::Error> {
        match key_event.code {
            KeyCode::Esc => Ok(vec![
                Action::Input(InputAction::ClearAllKeyRegisters),
                Action::Input(InputAction::SwitchInputMode(self.shown_mode().into())),
            ]),

            KeyCode::Enter => match self.command.trim() {
                "ls" | "contigs" if matches!(self.mode, InputMode::Command(_)) => Ok(vec![
                    Action::Input(InputAction::ClearAllKeyRegisters),
                    Action::Input(InputAction::SwitchScene(Scene::ContigList)),
                    Action::Input(InputAction::SwitchInputMode(InputMode::ContigList(
                        self.shown_mode(),
                    ))),
                ]),
                _ => Ok(submit(self.command.as_str())
                    .unwrap_or_else(|e| vec![Action::message(e.to_string())])
                    .into_iter()
                    .chain(vec![
                        Action::Input(InputAction::ClearAllKeyRegisters),
                        Action::Input(InputAction::SwitchInputMode(self.shown_mode().into())),
                    ])
                    .collect_vec()),
            },
            KeyCode::Char(c) => {
                self.command.insert(self.command_cursor, c);
                self.command_cursor += 1;
                Ok(vec![Action::Input(InputAction::CommandChanged)])
            }
            KeyCode::Backspace => {
                if self.command_cursor > 0 {
                    self.command.remove(self.command_cursor - 1);
                    self.command_cursor -= 1;
                }
                Ok(vec![Action::Input(InputAction::CommandChanged)])
            }
            KeyCode::Left => {
                self.command_cursor = self.command_cursor.saturating_sub(1);
                Ok(vec![Action::Input(InputAction::CommandChanged)])
            }
            KeyCode::Right => {
                self.command_cursor = self
                    .command_cursor
                    .saturating_add(1)
                    .clamp(0, self.command.len());
                Ok(vec![Action::Input(InputAction::CommandChanged)])
            }
            _ => Err(crate::Error::InvalidCommandKey(key_event)),
        }
    }

    pub fn handle_key_event(
        &mut self,
        key_event: Key,
        state: &State,
    ) -> Result<Vec<Action>, crate::Error> {
        Ok(match self.mode {
            InputMode::Editing(mode) => self.pending.push(key_event, mode),
            InputMode::Command(_) => self.handle_command_line(key_event, crate::command::parse),
            InputMode::Search(_) => {
                self.handle_command_line(key_event, crate::command::parse_search)
            }
            InputMode::ContigList(_) => self.handle_contig_list(key_event, state),
            // KeyRegisterType::ContigListCommand => {
            //     self.contig_list_command.handle_key_event(key_event)
            // }
        }
        .unwrap_or_else(|e| {
            vec![
                Action::Input(InputAction::ClearAllKeyRegisters),
                Action::message(format!("{}", e)),
            ]
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::SessionAction;
    use gv_core::contig_header::Contig;
    use rstest::rstest;

    fn state(contigs: usize) -> State {
        let mut header = ContigHeader::new(Reference::NoReference);
        header.contigs = (1..=contigs)
            .map(|index| Contig::new(&format!("chr{index}"), Some(100)))
            .collect();
        State::new(Reference::NoReference, header).unwrap()
    }

    fn keyboard(mode: InputMode, command: &str, command_cursor: usize) -> KeyboardState {
        KeyboardState {
            mode,
            command: command.to_string(),
            command_cursor,
            ..KeyboardState::default()
        }
    }

    fn press(keyboard: &mut KeyboardState, state: &State, codes: &[KeyCode]) -> Vec<Action> {
        let mut actions = Vec::new();
        for code in codes {
            actions = keyboard.handle_key_event(Key::from(*code), state).unwrap();
        }
        actions
    }

    fn type_text(text: &str) -> Vec<KeyCode> {
        text.chars().map(KeyCode::Char).collect()
    }

    fn changed() -> Action {
        Action::Input(InputAction::CommandChanged)
    }

    fn clear_all() -> Action {
        Action::Input(InputAction::ClearAllKeyRegisters)
    }

    fn switch(mode: InputMode) -> Action {
        Action::Input(InputAction::SwitchInputMode(mode))
    }

    fn select(index: usize) -> Action {
        Action::Navigate(NavigationAction::SelectContig(index))
    }

    #[rstest]
    #[case::editing(InputMode::Editing(Mode::Insert(1)), Mode::Insert(1))]
    #[case::command(InputMode::Command(Mode::Normal), Mode::Normal)]
    #[case::search(InputMode::Search(Mode::Insert(2)), Mode::Insert(2))]
    #[case::contig_list(InputMode::ContigList(Mode::Normal), Mode::Normal)]
    fn shown_mode(#[case] mode: InputMode, #[case] expected: Mode) {
        assert_eq!(keyboard(mode, "", 0).shown_mode(), expected);
    }

    #[rstest]
    #[case::editing(InputMode::Editing(Mode::Insert(1)), InputMode::Editing(Mode::Normal))]
    #[case::command(InputMode::Command(Mode::Insert(1)), InputMode::Command(Mode::Normal))]
    #[case::search(InputMode::Search(Mode::Insert(1)), InputMode::Search(Mode::Normal))]
    #[case::contig_list(
        InputMode::ContigList(Mode::Insert(1)),
        InputMode::ContigList(Mode::Normal)
    )]
    #[case::other_track(
        InputMode::Editing(Mode::Insert(2)),
        InputMode::Editing(Mode::Insert(2))
    )]
    #[case::normal_mode(InputMode::Editing(Mode::Normal), InputMode::Editing(Mode::Normal))]
    fn leave_insert_mode(#[case] mode: InputMode, #[case] expected: InputMode) {
        let mut keyboard = keyboard(mode, "", 0);
        keyboard.leave_insert_mode(1);
        assert_eq!(keyboard.mode, expected);
    }

    #[test]
    fn leave_insert_mode_clears_pending_keys() {
        let mut keyboard = keyboard(InputMode::Editing(Mode::Insert(1)), "", 0);
        press(&mut keyboard, &state(0), &[KeyCode::Char('3')]);
        assert!(!keyboard.pending.is_empty());
        keyboard.leave_insert_mode(1);
        assert!(keyboard.pending.is_empty());
    }

    #[test]
    fn clear_resets_the_typed_state() {
        let mut keyboard = keyboard(InputMode::Command(Mode::Normal), "abc", 2);
        keyboard.contig_list_cursor = 4;
        keyboard.clear();
        assert_eq!(keyboard.command, "");
        assert_eq!(keyboard.command_cursor, 0);
        assert_eq!(keyboard.contig_list_cursor, 0);
        assert_eq!(keyboard.mode, InputMode::Command(Mode::Normal));
    }

    #[rstest]
    #[case::typing("", 0, type_text("abc"), "abc", 3)]
    #[case::insert_in_the_middle("ac", 1, type_text("b"), "abc", 2)]
    #[case::backspace("abc", 3, vec![KeyCode::Backspace], "ab", 2)]
    #[case::backspace_in_the_middle("abc", 2, vec![KeyCode::Backspace], "ac", 1)]
    #[case::backspace_at_the_start("abc", 0, vec![KeyCode::Backspace], "abc", 0)]
    #[case::left("abc", 3, vec![KeyCode::Left, KeyCode::Left], "abc", 1)]
    #[case::left_at_the_start("abc", 0, vec![KeyCode::Left], "abc", 0)]
    #[case::right("abc", 1, vec![KeyCode::Right], "abc", 2)]
    #[case::right_at_the_end("abc", 3, vec![KeyCode::Right], "abc", 3)]
    fn command_line_editing(
        #[case] command: &str,
        #[case] cursor: usize,
        #[case] codes: Vec<KeyCode>,
        #[case] expected_command: &str,
        #[case] expected_cursor: usize,
    ) {
        for mode in [
            InputMode::Command(Mode::Normal),
            InputMode::Search(Mode::Normal),
        ] {
            let mut keyboard = keyboard(mode, command, cursor);
            assert_eq!(press(&mut keyboard, &state(0), &codes), vec![changed()]);
            assert_eq!(keyboard.command, expected_command);
            assert_eq!(keyboard.command_cursor, expected_cursor);
        }
    }

    #[test]
    fn an_invalid_command_line_key_clears_and_reports() {
        let mut keyboard = keyboard(InputMode::Command(Mode::Normal), "ab", 2);
        let actions = press(&mut keyboard, &state(0), &[KeyCode::Tab]);
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0], clear_all());
        assert!(matches!(actions[1], Action::Message(_)));
    }

    #[rstest]
    #[case::quit(
        InputMode::Command(Mode::Normal),
        "q",
        vec![
            Action::Session(SessionAction::Quit),
            clear_all(),
            switch(InputMode::Editing(Mode::Normal)),
        ]
    )]
    #[case::keeps_insert_mode(
        InputMode::Command(Mode::Insert(1)),
        "q",
        vec![
            Action::Session(SessionAction::Quit),
            clear_all(),
            switch(InputMode::Editing(Mode::Insert(1))),
        ]
    )]
    #[case::contig_list(
        InputMode::Command(Mode::Normal),
        " ls ",
        vec![
            clear_all(),
            Action::Input(InputAction::SwitchScene(Scene::ContigList)),
            switch(InputMode::ContigList(Mode::Normal)),
        ]
    )]
    #[case::contigs(
        InputMode::Command(Mode::Insert(1)),
        "contigs",
        vec![
            clear_all(),
            Action::Input(InputAction::SwitchScene(Scene::ContigList)),
            switch(InputMode::ContigList(Mode::Insert(1))),
        ]
    )]
    #[case::search_for_ls(
        InputMode::Search(Mode::Normal),
        "ls",
        vec![
            Action::Navigate(NavigationAction::Move(Movement::Gene("ls".to_string()))),
            clear_all(),
            switch(InputMode::Editing(Mode::Normal)),
        ]
    )]
    fn enter_submits_the_command_line(
        #[case] mode: InputMode,
        #[case] command: &str,
        #[case] expected: Vec<Action>,
    ) {
        let mut keyboard = keyboard(mode, command, command.len());
        assert_eq!(press(&mut keyboard, &state(0), &[KeyCode::Enter]), expected);
    }

    #[test]
    fn enter_reports_an_unknown_command() {
        let mut keyboard = keyboard(InputMode::Command(Mode::Normal), "nonsense", 8);
        let actions = press(&mut keyboard, &state(0), &[KeyCode::Enter]);
        assert_eq!(actions.len(), 3);
        assert!(matches!(&actions[0], Action::Message(message) if message.contains("nonsense")));
        assert_eq!(
            actions[1..],
            [clear_all(), switch(InputMode::Editing(Mode::Normal))]
        );
    }

    #[rstest]
    #[case::command(InputMode::Command(Mode::Normal), InputMode::Editing(Mode::Normal))]
    #[case::search(
        InputMode::Search(Mode::Insert(1)),
        InputMode::Editing(Mode::Insert(1))
    )]
    fn esc_cancels_the_command_line(#[case] mode: InputMode, #[case] expected: InputMode) {
        let mut keyboard = keyboard(mode, "abc", 3);
        assert_eq!(
            press(&mut keyboard, &state(0), &[KeyCode::Esc]),
            vec![clear_all(), switch(expected)]
        );
    }

    #[rstest]
    #[case::command(InputMode::Command(Mode::Normal), "ac", 1, "x\ny\r", "axyc", 3, vec![changed()])]
    #[case::search(InputMode::Search(Mode::Normal), "", 0, "chr1:100", "chr1:100", 8, vec![changed()])]
    #[case::editing(InputMode::Editing(Mode::Normal), "", 0, "abc", "", 0, vec![])]
    #[case::contig_list(InputMode::ContigList(Mode::Normal), "", 0, "abc", "", 0, vec![])]
    fn handle_paste(
        #[case] mode: InputMode,
        #[case] command: &str,
        #[case] cursor: usize,
        #[case] text: &str,
        #[case] expected_command: &str,
        #[case] expected_cursor: usize,
        #[case] expected: Vec<Action>,
    ) {
        let mut keyboard = keyboard(mode, command, cursor);
        assert_eq!(keyboard.handle_paste(text), expected);
        assert_eq!(keyboard.command, expected_command);
        assert_eq!(keyboard.command_cursor, expected_cursor);
    }

    #[rstest]
    #[case::down(0, KeyCode::Char('j'), vec![select(1)])]
    #[case::down_arrow(0, KeyCode::Down, vec![select(1)])]
    #[case::down_at_the_last(4, KeyCode::Char('j'), vec![select(4)])]
    #[case::up(2, KeyCode::Char('k'), vec![select(1)])]
    #[case::up_arrow(2, KeyCode::Up, vec![select(1)])]
    #[case::up_at_the_first(0, KeyCode::Char('k'), vec![select(0)])]
    #[case::page_down(1, KeyCode::Char('}'), vec![select(4)])]
    #[case::page_up(3, KeyCode::Char('{'), vec![select(0)])]
    #[case::other_key(1, KeyCode::Char('x'), vec![])]
    #[case::enter(
        2,
        KeyCode::Enter,
        vec![
            switch(InputMode::Editing(Mode::Insert(1))),
            Action::Input(InputAction::SwitchScene(Scene::Main)),
            Movement::ContigIndex(2).into(),
        ]
    )]
    #[case::esc(
        2,
        KeyCode::Esc,
        vec![
            switch(InputMode::Editing(Mode::Insert(1))),
            Action::Input(InputAction::SwitchScene(Scene::Main)),
        ]
    )]
    fn contig_list(#[case] cursor: usize, #[case] code: KeyCode, #[case] expected: Vec<Action>) {
        let mut keyboard = keyboard(InputMode::ContigList(Mode::Insert(1)), "", 0);
        keyboard.contig_list_cursor = cursor;
        assert_eq!(press(&mut keyboard, &state(5), &[code]), expected);
    }

    #[rstest]
    #[case::enter(KeyCode::Enter, vec![])]
    #[case::down(KeyCode::Char('j'), vec![select(0)])]
    #[case::page_down(KeyCode::Char('}'), vec![select(0)])]
    fn contig_list_without_contigs(#[case] code: KeyCode, #[case] expected: Vec<Action>) {
        let mut keyboard = keyboard(InputMode::ContigList(Mode::Normal), "", 0);
        assert_eq!(press(&mut keyboard, &state(0), &[code]), expected);
    }
}

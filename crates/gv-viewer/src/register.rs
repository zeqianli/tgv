//! The input modes, and the command line that command and search mode share.

use crate::message::{InputAction, NavigationAction};

use crate::{
    keys::{Key, KeyCode, Mode, PendingKeys},
    message::{Action, Movement},
    viewer::Scene,
};
use gv_core::alignment::is_url;
use gv_core::prelude::*;
use itertools::Itertools;
use std::path::Path;

/// The active input mode and the editing mode to resume after a temporary mode closes.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum KeyRegisterType {
    Editing(Mode),
    Command(Mode),
    Search(Mode),
    ContigList(Mode),
}

impl From<Mode> for KeyRegisterType {
    fn from(mode: Mode) -> Self {
        Self::Editing(mode)
    }
}

#[derive(Debug, Clone)]
pub struct Registers {
    pub current: KeyRegisterType,
    /// The normal or insert mode command being typed.
    pub pending: PendingKeys,
    pub command: String,
    pub command_cursor: usize,

    /// Index of the current focused contig.
    /// Indexes in the contig list view is identical to the contig header.
    pub contig_list_cursor: usize,
}

impl Default for Registers {
    fn default() -> Self {
        Self {
            current: KeyRegisterType::Editing(Mode::Normal),
            pending: PendingKeys::default(),
            command: "".to_string(),
            command_cursor: 0,

            contig_list_cursor: 0,
        }
    }
}

impl Registers {
    /// The mode the screen shows: normal or insert mode. Command, search, and contig list input
    /// show the mode they return to.
    pub fn shown_mode(&self) -> Mode {
        match self.current {
            KeyRegisterType::Editing(mode)
            | KeyRegisterType::Command(mode)
            | KeyRegisterType::Search(mode)
            | KeyRegisterType::ContigList(mode) => mode,
        }
    }

    /// Leaves insert mode for a track that no longer has the focus or was removed.
    pub fn leave_insert_mode(&mut self, track: TrackId) {
        if self.shown_mode() != Mode::Insert(track) {
            return;
        }
        self.current = match self.current {
            KeyRegisterType::Editing(_) => {
                self.pending.clear();
                KeyRegisterType::Editing(Mode::Normal)
            }
            KeyRegisterType::Command(_) => KeyRegisterType::Command(Mode::Normal),
            KeyRegisterType::Search(_) => KeyRegisterType::Search(Mode::Normal),
            KeyRegisterType::ContigList(_) => KeyRegisterType::ContigList(Mode::Normal),
        };
    }

    pub fn clear(&mut self) {
        self.pending.clear();
        self.command.clear();

        self.command_cursor = 0;
        self.contig_list_cursor = 0;
    }
}

impl Registers {
    /// Handles a bracketed paste. Terminals paste dropped files as shell-escaped paths, so in
    /// normal mode a paste of existing files opens them. On the command line, the text is
    /// inserted at the cursor.
    pub fn handle_paste(&mut self, text: &str) -> Vec<Action> {
        match self.current {
            KeyRegisterType::Editing(Mode::Normal) => {
                let paths = shlex::split(text.trim()).unwrap_or_default();
                if paths.is_empty() {
                    return vec![Action::message(format!("Not a file: {}", text.trim()))];
                }
                match paths
                    .iter()
                    .find(|path| !is_url(path) && !Path::new(path).is_file())
                {
                    Some(missing) => vec![Action::message(format!("Not a file: {missing}"))],
                    None => vec![Action::Track(crate::message::TrackAction::OpenFiles(paths))],
                }
            }
            KeyRegisterType::Command(_) | KeyRegisterType::Search(_) => {
                let text: String = text.chars().filter(|c| !matches!(c, '\n' | '\r')).collect();
                self.command.insert_str(self.command_cursor, &text);
                self.command_cursor += text.len();
                vec![Action::Input(InputAction::CommandChanged)]
            }
            KeyRegisterType::ContigList(_) | KeyRegisterType::Editing(Mode::Insert(_)) => {
                Vec::new()
            }
        }
    }

    /// Move the selected contig up or down.
    fn handle_contig_list(
        &mut self,
        key_event: Key,
        state: &State,
    ) -> Result<Vec<Action>, TGVError> {
        match key_event.code {
            KeyCode::Enter if state.contig_header.contigs.is_empty() => Ok(vec![]),
            KeyCode::Enter => Ok(vec![
                Action::Input(InputAction::SwitchKeyRegister(self.shown_mode().into())),
                Action::Input(InputAction::SwitchScene(Scene::Main)),
                Movement::ContigIndex(self.contig_list_cursor).into(),
            ]),

            KeyCode::Esc => Ok(vec![
                Action::Input(InputAction::SwitchKeyRegister(self.shown_mode().into())),
                Action::Input(InputAction::SwitchScene(Scene::Main)),
            ]),
            // FEAT: command mode in contig list
            // - search and filter contig by regex patterns
            // Implementing this needs lots of extra state tracking and messaging types.
            // Note sure how useful this is.
            //
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
        submit: fn(&str) -> Result<Vec<Action>, TGVError>,
    ) -> Result<Vec<Action>, TGVError> {
        match key_event.code {
            KeyCode::Esc => Ok(vec![
                Action::Input(InputAction::ClearAllKeyRegisters),
                Action::Input(InputAction::SwitchKeyRegister(self.shown_mode().into())),
            ]),

            KeyCode::Enter => match self.command.trim() {
                "ls" | "contigs" if matches!(self.current, KeyRegisterType::Command(_)) => {
                    Ok(vec![
                        Action::Input(InputAction::ClearAllKeyRegisters),
                        Action::Input(InputAction::SwitchScene(Scene::ContigList)),
                        Action::Input(InputAction::SwitchKeyRegister(KeyRegisterType::ContigList(
                            self.shown_mode(),
                        ))),
                    ])
                }
                _ => Ok(submit(self.command.as_str())
                    .unwrap_or_else(|e| vec![Action::message(e.to_string())])
                    .into_iter()
                    .chain(vec![
                        Action::Input(InputAction::ClearAllKeyRegisters),
                        Action::Input(InputAction::SwitchKeyRegister(self.shown_mode().into())),
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
            _ => Err(TGVError::RegisterError(format!(
                "Invalid command mode input: {:?}",
                key_event
            ))),
        }
    }

    pub fn handle_key_event(
        &mut self,
        key_event: Key,
        state: &State,
    ) -> Result<Vec<Action>, TGVError> {
        Ok(match self.current {
            KeyRegisterType::Editing(mode) => self.pending.push(key_event, mode),
            KeyRegisterType::Command(_) => {
                self.handle_command_line(key_event, |input| crate::command::parse(input))
            }
            KeyRegisterType::Search(_) => {
                self.handle_command_line(key_event, |input| crate::command::parse_search(input))
            }
            KeyRegisterType::ContigList(_) => self.handle_contig_list(key_event, state),
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

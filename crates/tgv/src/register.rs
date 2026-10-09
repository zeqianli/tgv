use crate::{
    app::Scene,
    message::{Action, Movement, UpdateLayoutAction},
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use gv_core::alignment::is_url;
use gv_core::normal::update_by_char;
use gv_core::prelude::*;
use itertools::Itertools;
use std::path::Path;
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum KeyRegisterType {
    Normal,
    Command,
    /// Navigation input after `/`. It shares the command line with command mode.
    Search,
    ContigList,
    // ContigListCommand,
}

pub struct Registers {
    pub current: KeyRegisterType,
    pub normal: String,
    pub command: String,
    pub command_cursor: usize,

    /// Index of the current focused contig.
    /// Indexes in the contig list view is identical to the contig header.
    pub contig_list_cursor: usize,
}

impl Default for Registers {
    fn default() -> Self {
        Self {
            current: KeyRegisterType::Normal,
            normal: "".to_string(),
            command: "".to_string(),
            command_cursor: 0,

            contig_list_cursor: 0,
        }
    }
}

impl Registers {
    pub fn clear(&mut self) {
        self.normal.clear();
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
            KeyRegisterType::Normal => {
                let paths = shlex::split(text.trim()).unwrap_or_default();
                if paths.is_empty() {
                    return vec![Action::message(format!("Not a file: {}", text.trim()))];
                }
                match paths
                    .iter()
                    .find(|path| !is_url(path) && !Path::new(path).is_file())
                {
                    Some(missing) => vec![Action::message(format!("Not a file: {missing}"))],
                    None => vec![Action::Core(gv_core::message::Message::OpenFiles(paths))],
                }
            }
            KeyRegisterType::Command | KeyRegisterType::Search => {
                let text: String = text.chars().filter(|c| !matches!(c, '\n' | '\r')).collect();
                self.command.insert_str(self.command_cursor, &text);
                self.command_cursor += text.len();
                vec![Action::CommandChanged]
            }
            KeyRegisterType::ContigList => Vec::new(),
        }
    }

    /// Move the selected contig up or down.
    fn handle_contig_list(
        &mut self,
        key_event: KeyEvent,
        state: &State,
    ) -> Result<Vec<Action>, TGVError> {
        match key_event.code {
            KeyCode::Enter if state.contig_header.contigs.is_empty() => Ok(vec![]),
            KeyCode::Enter => Ok(vec![
                Action::SwitchKeyRegister(KeyRegisterType::Normal),
                Action::SwitchScene(Scene::Main),
                Movement::ContigIndex(self.contig_list_cursor).into(),
            ]),

            KeyCode::Esc => Ok(vec![
                Action::SwitchKeyRegister(KeyRegisterType::Normal),
                Action::SwitchScene(Scene::Main),
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
                Ok(vec![Action::SelectContig(index)])
            }
            KeyCode::Char('k') | KeyCode::Up => Ok(vec![Action::SelectContig(
                self.contig_list_cursor.saturating_sub(1),
            )]),

            KeyCode::Char('}') => {
                let index = usize::min(
                    self.contig_list_cursor.saturating_add(30),
                    state.contig_header.contigs.len().saturating_sub(1),
                );
                Ok(vec![Action::SelectContig(index)])
            }

            KeyCode::Char('{') => Ok(vec![Action::SelectContig(
                self.contig_list_cursor.saturating_sub(30),
            )]),
            _ => Ok(vec![]),
        }
    }

    /// Edit the command line, shared by command and search mode. Enter submits the line to
    /// `submit`.
    fn handle_command_line(
        &mut self,
        key_event: KeyEvent,
        submit: fn(&str) -> Result<Vec<Action>, TGVError>,
    ) -> Result<Vec<Action>, TGVError> {
        match key_event.code {
            KeyCode::Esc => Ok(vec![
                Action::ClearAllKeyRegisters,
                Action::SwitchKeyRegister(KeyRegisterType::Normal),
            ]),

            KeyCode::Enter => match self.command.trim() {
                "ls" | "contigs" if self.current == KeyRegisterType::Command => Ok(vec![
                    Action::ClearAllKeyRegisters,
                    Action::SwitchScene(Scene::ContigList),
                    Action::SwitchKeyRegister(KeyRegisterType::ContigList),
                ]),
                _ => Ok(submit(self.command.as_str())
                    .unwrap_or_else(|e| {
                        vec![Action::Core(gv_core::message::Message::Message(format!(
                            "{}",
                            e
                        )))]
                    })
                    .into_iter()
                    .chain(vec![
                        Action::ClearAllKeyRegisters,
                        Action::SwitchKeyRegister(KeyRegisterType::Normal),
                    ])
                    .collect_vec()),
            },
            KeyCode::Char(c) => {
                self.command.insert(self.command_cursor, c);
                self.command_cursor += 1;
                Ok(vec![Action::CommandChanged])
            }
            KeyCode::Backspace => {
                if self.command_cursor > 0 {
                    self.command.remove(self.command_cursor - 1);
                    self.command_cursor -= 1;
                }
                Ok(vec![Action::CommandChanged])
            }
            KeyCode::Left => {
                self.command_cursor = self.command_cursor.saturating_sub(1);
                Ok(vec![Action::CommandChanged])
            }
            KeyCode::Right => {
                self.command_cursor = self
                    .command_cursor
                    .saturating_add(1)
                    .clamp(0, self.command.len());
                Ok(vec![Action::CommandChanged])
            }
            _ => Err(TGVError::RegisterError(format!(
                "Invalid command mode input: {:?}",
                key_event
            ))),
        }
    }

    fn handle_normal(&mut self, key_event: KeyEvent) -> Result<Vec<Action>, TGVError> {
        match key_event.code {
            KeyCode::Char('s') if self.normal.is_empty() => Ok(vec![Action::UpdateLayout(
                UpdateLayoutAction::ToggleSidebar,
            )]),
            KeyCode::Char(':') => Ok(vec![
                Action::ClearAllKeyRegisters,
                Action::SwitchKeyRegister(KeyRegisterType::Command),
            ]),
            KeyCode::Char('/') => Ok(vec![
                Action::ClearAllKeyRegisters,
                Action::SwitchKeyRegister(KeyRegisterType::Search),
            ]),
            // Like undo and redo in vim, `u` and `Ctrl-r` move through the jump history.
            KeyCode::Char('r') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                self.clear();
                Ok(vec![Action::JumpForward])
            }
            KeyCode::Char('u') if self.normal.is_empty() => Ok(vec![Action::JumpBack]),
            KeyCode::Char(char) => Ok(update_by_char(&mut self.normal, char)?
                .into_iter()
                .map(|m| m.into())
                .collect_vec()),
            KeyCode::Left => Ok(update_by_char(&mut self.normal, 'h')?
                .into_iter()
                .map(|m| m.into())
                .collect_vec()),
            KeyCode::Up => Ok(update_by_char(&mut self.normal, 'k')?
                .into_iter()
                .map(|m| m.into())
                .collect_vec()),
            KeyCode::Down => Ok(update_by_char(&mut self.normal, 'j')?
                .into_iter()
                .map(|m| m.into())
                .collect_vec()),
            KeyCode::Right => Ok(update_by_char(&mut self.normal, 'l')?
                .into_iter()
                .map(|m| m.into())
                .collect_vec()),

            _ => {
                self.clear();
                Err(TGVError::RegisterError(format!(
                    "Invalid normal mode input: {:?}",
                    key_event
                )))
            }
        }
    }

    pub fn handle_key_event(
        &mut self,
        key_event: KeyEvent,
        state: &State,
    ) -> Result<Vec<Action>, TGVError> {
        Ok(match self.current {
            KeyRegisterType::Normal => self.handle_normal(key_event),
            KeyRegisterType::Command => self.handle_command_line(key_event, |input| {
                Ok(gv_core::command::parse(input)?
                    .into_iter()
                    .map(Action::Core)
                    .collect())
            }),
            KeyRegisterType::Search => self.handle_command_line(key_event, |input| {
                Ok(gv_core::command::parse_search(input)?
                    .into_iter()
                    .map(Action::Core)
                    .collect())
            }),
            KeyRegisterType::ContigList => self.handle_contig_list(key_event, state),
            // KeyRegisterType::ContigListCommand => {
            //     self.contig_list_command.handle_key_event(key_event)
            // }
        }
        .unwrap_or_else(|e| {
            vec![
                Action::ClearAllKeyRegisters,
                Action::message(format!("{}", e)),
            ]
        }))
    }
}

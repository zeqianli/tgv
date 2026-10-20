use crate::{
    app::Scene,
    message::{Action, FocusMove, Movement, Scroll, UpdateLayoutAction},
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use gv_core::alignment::is_url;
use gv_core::bed::BedEditKind;
use gv_core::message::Message;
use gv_core::normal::update_by_char;
use gv_core::prelude::*;
use itertools::Itertools;
use std::path::Path;
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum KeyRegisterType {
    Normal,
    /// Moves the focus between tracks, and creates, removes, and undoes edits on them.
    Track,
    /// Edits a BED track at the cursor. The track is part of the mode, so insert mode always
    /// has one.
    Insert {
        track: TrackId,
    },
    Command,
    /// Navigation input after `/`. It shares the command line with command mode.
    Search,
    ContigList,
    // ContigListCommand,
}

pub struct Registers {
    pub current: KeyRegisterType,
    /// The mode that command, search, and contig list input return to: normal or track mode.
    pub return_to: KeyRegisterType,
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
            return_to: KeyRegisterType::Normal,
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
            KeyRegisterType::Normal | KeyRegisterType::Track => {
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
            KeyRegisterType::ContigList | KeyRegisterType::Insert { .. } => Vec::new(),
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
                Action::SwitchKeyRegister(self.return_to.clone()),
                Action::SwitchScene(Scene::Main),
                Movement::ContigIndex(self.contig_list_cursor).into(),
            ]),

            KeyCode::Esc => Ok(vec![
                Action::SwitchKeyRegister(self.return_to.clone()),
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
                Action::SwitchKeyRegister(self.return_to.clone()),
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
                        Action::SwitchKeyRegister(self.return_to.clone()),
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
            // As in vim, the first Esc only cancels a pending count or `g`.
            KeyCode::Esc if !self.normal.is_empty() => {
                self.normal.clear();
                Ok(vec![])
            }
            KeyCode::Esc => Ok(vec![Action::SwitchKeyRegister(KeyRegisterType::Track)]),
            KeyCode::Char(char @ ('i' | 'a')) => {
                self.normal.clear();
                Ok(vec![Action::EnterInsertMode {
                    append: char == 'a',
                }])
            }
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

    /// Track mode: the normal mode keys, with `j/k/gg/gG` moving the focus between tracks.
    fn handle_track(&mut self, key_event: KeyEvent) -> Result<Vec<Action>, TGVError> {
        let char = match key_event.code {
            KeyCode::Char(char) => char,
            KeyCode::Left => 'h',
            KeyCode::Up => 'k',
            KeyCode::Down => 'j',
            KeyCode::Right => 'l',
            // Track mode is the outermost layer, so Esc only cancels pending input.
            KeyCode::Esc => {
                self.normal.clear();
                return Ok(vec![]);
            }
            KeyCode::Enter => {
                self.normal.clear();
                return Ok(vec![Action::SwitchKeyRegister(KeyRegisterType::Normal)]);
            }
            _ => {
                self.clear();
                return Err(TGVError::RegisterError(format!(
                    "Invalid track mode input: {key_event:?}"
                )));
            }
        };
        match char {
            'r' if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                self.normal.clear();
                Ok(vec![Action::Redo])
            }
            'u' if self.normal.is_empty() => Ok(vec![Action::Undo]),
            's' | ':' | '/' if self.normal.is_empty() => self.handle_normal(key_event),
            'i' | 'a' => {
                self.normal.clear();
                Ok(vec![Action::EnterInsertMode {
                    append: char == 'a',
                }])
            }
            'o' | 'O' => {
                self.normal.clear();
                Ok(vec![Action::NewBedTrack { below: char == 'o' }])
            }
            'd' if self.normal == "d" => {
                self.normal.clear();
                Ok(vec![Message::CloseTrack { force: false }.into()])
            }
            'd' if self.normal.is_empty() => {
                self.normal.push('d');
                Ok(vec![])
            }
            // Reserved for moving tracks up and down.
            'J' | 'K' => {
                self.normal.clear();
                Ok(vec![])
            }
            char => Ok(update_by_char(&mut self.normal, char)?
                .into_iter()
                .map(|message| match message {
                    Message::Scroll(Scroll::Down(n)) => Action::MoveFocus(FocusMove::Next(n)),
                    Message::Scroll(Scroll::Up(n)) => Action::MoveFocus(FocusMove::Previous(n)),
                    Message::Scroll(Scroll::Top) => Action::MoveFocus(FocusMove::First),
                    Message::Scroll(Scroll::Bottom) => Action::MoveFocus(FocusMove::Last),
                    message => message.into(),
                })
                .collect_vec()),
        }
    }

    /// Insert mode: paint and erase BED features at the cursor, and move and zoom as in normal
    /// mode.
    fn handle_insert(
        &mut self,
        track: TrackId,
        key_event: KeyEvent,
    ) -> Result<Vec<Action>, TGVError> {
        let edit = |edit| Action::EditBed { track, edit };
        let char = match key_event.code {
            KeyCode::Esc => {
                self.normal.clear();
                return Ok(vec![Action::SwitchKeyRegister(KeyRegisterType::Normal)]);
            }
            KeyCode::Char(' ') => {
                self.normal.clear();
                return Ok(vec![edit(BedEditKind::Add), Movement::Right(1).into()]);
            }
            KeyCode::Backspace => {
                self.normal.clear();
                return Ok(vec![Movement::Left(1).into(), edit(BedEditKind::Remove)]);
            }
            KeyCode::Delete => {
                self.normal.clear();
                return Ok(vec![edit(BedEditKind::Remove)]);
            }
            KeyCode::Char(char) => char,
            KeyCode::Left => 'h',
            KeyCode::Right => 'l',
            // Insert mode stays on one track.
            KeyCode::Up | KeyCode::Down => {
                self.normal.clear();
                return Ok(vec![]);
            }
            _ => {
                self.clear();
                return Err(TGVError::RegisterError(format!(
                    "Invalid insert mode input: {key_event:?}"
                )));
            }
        };
        if matches!(char, 'j' | 'k' | 'J' | 'K') {
            self.normal.clear();
            return Ok(vec![]);
        }
        Ok(update_by_char(&mut self.normal, char)?
            .into_iter()
            .filter(|message| !matches!(message, Message::Scroll(_)))
            .map(Action::from)
            .collect_vec())
    }

    pub fn handle_key_event(
        &mut self,
        key_event: KeyEvent,
        state: &State,
    ) -> Result<Vec<Action>, TGVError> {
        Ok(match self.current {
            KeyRegisterType::Normal => self.handle_normal(key_event),
            KeyRegisterType::Track => self.handle_track(key_event),
            KeyRegisterType::Insert { track } => self.handle_insert(track, key_event),
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

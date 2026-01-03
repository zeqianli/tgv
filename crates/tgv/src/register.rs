use crate::{app::Scene, message::Message};
use crossterm::event::{KeyCode, KeyEvent};
use gv_core::normal::update_by_char;
use gv_core::{error::TGVError, state::State};

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum KeyRegisterType {
    Normal,
    Command,
    Help,
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
    pub fn update(&mut self, state: &State) -> Result<(), TGVError> {
        if self.current != KeyRegisterType::ContigList {
            self.contig_list.cursor_position = state.contig_index();
        }
        Ok(())
    }

    pub fn clear(&mut self) {
        self.normal.clear();
        self.command.clear();

        self.command_cursor = 0;
        self.contig_list_cursor = 0;
        //self.contig_list_command.buffer.clear();
    }
}

impl Registers {
    fn handle_help(&mut self, key_event: KeyEvent) -> Result<Vec<Message>, TGVError> {
        match key_event.code {
            KeyCode::Esc => Ok(vec![
                Message::SwitchScene(Scene::Main),
                Message::SwitchKeyRegister(KeyRegisterType::Normal),
            ]), // TODO: when handling this, should switch register too.
            // This ensures that switching scene and switching register are always together.
            _ => Ok(vec![]),
        }
    }

    /// Move the selected contig up or down.
    fn handle_contig_list(
        &mut self,
        key_event: KeyEvent,
        state: &State,
    ) -> Result<Vec<Message>, TGVError> {
        match key_event.code {
            KeyCode::Enter => Ok(vec![
                Message::SwitchKeyRegister(KeyRegisterType::Normal),
                Message::SwitchScene(Scene::Main),
                Message::GotoContigIndex(self.cursor_position),
            ]),

            KeyCode::Esc => Ok(vec![
                Message::SwitchKeyRegister(KeyRegisterType::Normal),
                Message::SwitchScene(Scene::Main),
            ]),
            // FEAT: command mode in contig list
            // - search and filter contig by regex patterns
            // Implementing this needs lots of extra state tracking and messaging types.
            // Note sure how useful this is.
            //
            KeyCode::Char('j') | KeyCode::Down => {
                self.contig_list_cursor = usize::min(
                    self.contig_list_cursor.saturating_add(1),
                    state.contig_header.contigs.len() - 1,
                );

                Ok(vec![])
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.contig_list_cursor = self.contig_list_cursor.saturating_sub(1);
                Ok(vec![])
            }

            KeyCode::Char('}') => {
                self.contig_list_cursor = usize::min(
                    self.contig_list_cursor.saturating_add(30),
                    state.contig_header.contigs.len() - 1,
                );
                Ok(vec![])
            }

            KeyCode::Char('{') => {
                self.contig_list_cursor = self.contig_list_cursor.saturating_sub(30);
                Ok(vec![])
            }
            _ => Ok(vec![]),
        }
    }

    fn handle_command(&mut self, key_event: KeyEvent) -> Result<Vec<Message>, TGVError> {
        match key_event.code {
            KeyCode::Esc => Ok(vec![
                Message::ClearAllKeyRegisters,
                Message::SwitchKeyRegister(KeyRegisterType::Normal),
            ]),

            KeyCode::Enter => match self.buffer.input.as_ref() {
                "h" => Ok(vec![
                    Message::ClearAllKeyRegisters,
                    Message::SwitchScene(Scene::Help),
                    Message::SwitchKeyRegister(KeyRegisterType::Help),
                ]),
                "ls" | "contigs" => Ok(vec![
                    Message::ClearAllKeyRegisters,
                    Message::SwitchScene(Scene::ContigList),
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
                self.input.insert(self.cursor_position, c);
                self.cursor_position += 1;
                Ok(vec![])
            }
            KeyCode::Backspace => {
                if self.cursor_position > 0 {
                    self.input.remove(self.cursor_position - 1);
                    self.cursor_position -= 1;
                }
                Ok(vec![])
            }
            KeyCode::Left => {
                self.cursor_position = self.cursor_position.saturating_sub(1);
                Ok(vec![])
            }
            KeyCode::Right => {
                self.cursor_position = self
                    .cursor_position
                    .saturating_add(1)
                    .clamp(0, self.input.len());
                Ok(vec![])
            }
            _ => Err(TGVError::RegisterError(format!(
                "Invalid command mode input: {:?}",
                key_event
            ))),
        }
    }

    fn handle_normal_key_event(&mut self, key_event: KeyEvent) -> Result<Vec<Message>, TGVError> {
        match key_event.code {
            KeyCode::Char(':') => Ok(vec![
                Message::ClearAllKeyRegisters,
                Message::SwitchKeyRegister(KeyRegisterType::Command),
            ]),
            KeyCode::Char(char) => update_by_char(self.normal, char),
            KeyCode::Left => update_by_char(self.normal, 'h'),
            KeyCode::Up => update_by_char(self.normal, 'k'),
            KeyCode::Down => update_by_char(self.normal, 'j'),
            KeyCode::Right => update_by_char(self.normal, 'l'),

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
    ) -> Result<Vec<Message>, TGVError> {
        Ok(match self.current {
            KeyRegisterType::Normal => self.handle_normal(key_event),
            KeyRegisterType::Command => self.handle_command(key_event),
            KeyRegisterType::Help => self.handle_help(key_event),
            KeyRegisterType::ContigList => self.handle_contig_list(key_event, state),
            // KeyRegisterType::ContigListCommand => {
            //     self.contig_list_command.handle_key_event(key_event)
            // }
        }
        .unwrap_or_else(|e| {
            vec![
                Message::ClearAllKeyRegisters,
                Message::Message(format!("{}", e)),
            ]
        }))
    }
}

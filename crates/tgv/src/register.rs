pub mod command;
mod contig_list;
mod help;
mod mouse;
mod normal;
use crate::{error::TGVError, message::Message, states::State};

pub use crate::{
    register::{
        command::CommandModeRegister, contig_list::ContigListModeRegister, help::HelpModeRegister,
        mouse::NormalMouseRegister, normal::NormalModeRegister,
    },
    repository::Repository,
};

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum KeyRegisterType {
    Normal,
    Command,
    Help,
    ContigList,
    // ContigListCommand,
}

/// Register stores inputs and translates key event to StateMessages.
pub trait KeyRegister {
    fn handle_key_event(
        &mut self,
        event: KeyEvent,
        state: &State,
    ) -> Result<Vec<Message>, TGVError>;
}

pub trait MouseRegister {
    fn handle_mouse_event(
        &mut self,
        state: &State,
        repository: &Repository,
        event: MouseEvent,
    ) -> Result<Vec<Message>, TGVError>;
}

pub struct Registers {
    pub current: KeyRegisterType,
    pub normal: NormalModeRegister,
    pub command: CommandModeRegister,
    pub help: HelpModeRegister,
    pub contig_list: ContigListModeRegister,
    //pub contig_list_command: ContigListCommandModeRegister,
    pub mouse_register: NormalMouseRegister,
}

impl Default for Registers {
    fn default() -> Self {
        Ok(Self {
            current: KeyRegisterType::Normal,
            normal: NormalModeRegister::default(),
            command: CommandModeRegister::default(),
            help: HelpModeRegister::default(),
            contig_list: ContigListModeRegister::default(),
            //contig_list_command: ContigListCommandModeRegister::default(),
            mouse_register: NormalMouseRegister::default(),
        })
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
        self.command.buffer.clear();
        //self.contig_list_command.buffer.clear();
    }
}

impl KeyRegister for Registers {
    fn handle_key_event(
        &mut self,
        key_event: KeyEvent,
        state: &State,
    ) -> Result<Vec<Message>, TGVError> {
        Ok(match self.current {
            KeyRegisterType::Normal => self.normal.handle_key_event(key_event, state),
            KeyRegisterType::Command => self.command.handle_key_event(key_event, state),
            KeyRegisterType::Help => self.help.handle_key_event(key_event, state),
            KeyRegisterType::ContigList => self.contig_list.handle_key_event(key_event, state),
            // KeyRegisterType::ContigListCommand => {
            //     self.contig_list_command.handle_key_event(key_event, state)
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

fn handle_key_event(
    &mut self,
    key_event: KeyEvent,
    state: &State,
) -> Result<Vec<Message>, TGVError> {
    match key_event.code {
        KeyCode::Char(':') => Ok(vec![
            Message::ClearAllKeyRegisters,
            Message::SwitchKeyRegister(KeyRegisterType::Command),
        ]),
        KeyCode::Char(char) => self.update_by_char(char),
        KeyCode::Left => self.update_by_char('h'),
        KeyCode::Up => self.update_by_char('k'),
        KeyCode::Down => self.update_by_char('j'),
        KeyCode::Right => self.update_by_char('l'),

        _ => {
            self.clear();
            Err(TGVError::RegisterError(format!(
                "Invalid normal mode input: {:?}",
                key_event
            )))
        }
    }
}

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

#[derive(Debug, Default)]
pub struct ContigListModeRegister {
    /// index of contigs in the contig header
    pub cursor_position: usize,
}

impl KeyRegister for ContigListModeRegister {
    /// Move the selected contig up or down.
    fn handle_key_event(
        &mut self,
        key_event: KeyEvent,
        state: &State,
    ) -> Result<Vec<Message>, TGVError> {
        match key_event.code {
            KeyCode::Enter => Ok(vec![
                Message::ClearAllKeyRegisters,
                Message::SwitchKeyRegister(KeyRegisterType::Normal),
                Message::SwitchScene(Scene::Main),
                Message::GotoContigIndex(self.cursor_position),
            ]),

            KeyCode::Esc => Ok(vec![
                Message::ClearAllKeyRegisters,
                Message::SwitchKeyRegister(KeyRegisterType::Normal),
                Message::SwitchScene(Scene::Main),
            ]),
            // FEAT: command mode in contig list
            // - search and filter contig by regex patterns
            // Implementing this needs lots of extra state tracking and messaging types.
            // Note sure how useful this is.
            //
            // KeyCode::Char(':') | KeyCode::Char('/') => Ok(vec![Message::SwitchKeyRegister(
            //     KeyRegisterType::ContigListCommand,
            // )]),
            KeyCode::Char('j') | KeyCode::Down => {
                self.cursor_position = usize::min(
                    self.cursor_position.saturating_add(1),
                    state.contig_header.contigs.len() - 1,
                );

                Ok(vec![])
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.cursor_position = self.cursor_position.saturating_sub(1);
                Ok(vec![])
            }

            KeyCode::Char('}') => {
                self.cursor_position = usize::min(
                    self.cursor_position.saturating_add(30),
                    state.contig_header.contigs.len() - 1,
                );
                Ok(vec![])
            }

            KeyCode::Char('{') => {
                self.cursor_position = self.cursor_position.saturating_sub(30);
                Ok(vec![])
            }
            _ => Ok(vec![]),
        }
    }
}

#[derive(Default, Debug)]
pub struct ContigListCommandModeRegister {
    pub buffer: CommandBuffer,
}

/// [Not implemented yet]
/// - search and filter contig by regex patterns
// Implementing this needs lots of extra state tracking and messaging types.
// Note sure how useful this is.
impl KeyRegister for ContigListCommandModeRegister {
    fn handle_key_event(
        &mut self,
        key_event: KeyEvent,
        state: &State,
    ) -> Result<Vec<Message>, TGVError> {
        match key_event.code {
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

#[derive(Default, Debug)]
pub struct HelpModeRegister {}

impl KeyRegister for HelpModeRegister {
    fn handle_key_event(
        &mut self,
        key_event: KeyEvent,
        state: &State,
    ) -> Result<Vec<Message>, TGVError> {
        match key_event.code {
            KeyCode::Esc => Ok(vec![
                Message::SwitchScene(Scene::Main),
                Message::ClearAllKeyRegisters,
                Message::SwitchKeyRegister(KeyRegisterType::Normal),
            ]),
            _ => Ok(vec![]),
        }
    }
}

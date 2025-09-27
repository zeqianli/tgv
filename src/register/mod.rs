mod command;
mod contig_list;
mod help;
mod mouse;
mod normal;
use crate::{error::TGVError, message::StateMessage, states::State};
use crossterm::event::{KeyCode, KeyEvent, MouseEvent};

pub use crate::{
    register::{
        command::CommandModeRegister, contig_list::ContigListModeRegister, help::HelpModeRegister,
        mouse::NormalMouseRegister, normal::NormalModeRegister,
    },
    rendering::Scene,
    repository::Repository,
};

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum RegisterType {
    Normal,
    Command,
    Help,
    ContigList,
}

/// Register stores inputs and translates key event to StateMessages.
pub trait KeyRegister {
    fn handle_key_event(
        &mut self,
        event: KeyEvent,
        state: &State,
    ) -> Result<Vec<StateMessage>, TGVError>;
}

pub trait MouseRegister {
    fn handle_mouse_event(
        &mut self,
        state: &State,
        repository: &Repository,
        event: MouseEvent,
    ) -> Result<Vec<StateMessage>, TGVError>;
}

pub struct Registers {
    pub current: RegisterType,
    pub normal: NormalModeRegister,
    pub command: CommandModeRegister,
    pub help: HelpModeRegister,
    pub contig_list: ContigListModeRegister,
    pub mouse_register: NormalMouseRegister,
}

impl Registers {
    pub fn new(state: &State) -> Result<Self, TGVError> {
        Ok(Self {
            current: RegisterType::Normal,
            normal: NormalModeRegister::default(),
            command: CommandModeRegister::default(),
            help: HelpModeRegister::default(),
            contig_list: ContigListModeRegister::default(),
            mouse_register: NormalMouseRegister::new(&state.layout.root),
        })
    }

    pub fn update(&mut self, state: &State) -> Result<(), TGVError> {
        if self.current != RegisterType::ContigList {
            self.contig_list.cursor_position = state.contig_index();
        }
        Ok(())
    }

    fn clear(&mut self) {
        self.normal.clear();
        self.command.clear();
    }
}

impl KeyRegister for Registers {
    fn handle_key_event(
        &mut self,
        key_event: KeyEvent,
        state: &State,
    ) -> Result<Vec<StateMessage>, TGVError> {
        match (key_event.code, self.current.clone()) {
            (KeyCode::Char(':'), RegisterType::Normal) => {
                self.clear();
                self.current = RegisterType::Command;

                return Ok(vec![]);
            }
            (KeyCode::Esc, RegisterType::Command) => {
                self.current = RegisterType::Normal;
                self.clear();
                return Ok(vec![]);
            }
            (KeyCode::Esc, RegisterType::Help) => {
                self.current = RegisterType::Normal;
                self.clear();
                return Ok(vec![StateMessage::SetDisplayMode(Scene::Main)]);
            }

            (KeyCode::Enter, RegisterType::Command) => {
                if self.command.input == "h" {
                    self.current = RegisterType::Help;
                    self.clear();
                    return Ok(vec![StateMessage::SetDisplayMode(Scene::Help)]);
                }

                if self.command.input == "ls" || self.command.input == "contigs" {
                    self.current = RegisterType::ContigList;
                    self.clear();
                    return Ok(vec![StateMessage::SetDisplayMode(Scene::ContigList)]);
                }
                let output = self
                    .command
                    .parse()
                    .unwrap_or_else(|e| vec![StateMessage::Message(format!("{}", e))]);
                self.current = RegisterType::Normal;
                self.command.clear();
                return Ok(output);
            }
            (KeyCode::Enter, RegisterType::ContigList) => {
                self.current = RegisterType::Normal;
                self.clear();

                return Ok(vec![
                    StateMessage::SetDisplayMode(Scene::Main),
                    StateMessage::GotoContigIndex(self.contig_list.cursor_position),
                ]);
            }

            (KeyCode::Esc, RegisterType::ContigList) => {
                self.current = RegisterType::Normal;
                self.clear();
                return Ok(vec![StateMessage::SetDisplayMode(Scene::Main)]);
            }

            _ => {}
        }

        Ok(match self.current {
            RegisterType::Normal => self.normal.handle_key_event(key_event, state),
            RegisterType::Command => self.command.handle_key_event(key_event, state),
            RegisterType::Help => self.help.handle_key_event(key_event, state),
            RegisterType::ContigList => self.contig_list.handle_key_event(key_event, state),
        }
        .unwrap_or_else(|e| {
            self.clear();
            vec![StateMessage::Message(format!("{}", e))]
        }))
    }
}

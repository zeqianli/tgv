pub mod command;
mod contig_list;
mod help;
mod mouse;
mod normal;
use crate::{
    error::TGVError, message::Message,
    states::State,
};
use crossterm::event::{KeyEvent, MouseEvent};

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

impl Registers {
    pub fn new(state: &State) -> Result<Self, TGVError> {
        Ok(Self {
            current: KeyRegisterType::Normal,
            normal: NormalModeRegister::default(),
            command: CommandModeRegister::default(),
            help: HelpModeRegister::default(),
            contig_list: ContigListModeRegister::default(),
            //contig_list_command: ContigListCommandModeRegister::default(),
            mouse_register: NormalMouseRegister::new(&state.layout.root),
        })
    }

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

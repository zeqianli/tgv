use crate::{
    error::TGVError,
    message::Message,
    register::{command::CommandBuffer, CommandModeRegister, KeyRegister, KeyRegisterType},
    rendering::Scene,
    states::State,
};
use crossterm::event::{KeyCode, KeyEvent};

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
            KeyCode::Enter => {
                return Ok(vec![
                    Message::ClearAllKeyRegisters,
                    Message::SwitchKeyRegister(KeyRegisterType::Normal),
                    Message::SwitchScene(Scene::Main),
                    Message::GotoContigIndex(self.cursor_position),
                ]);
            }

            KeyCode::Esc => {
                return Ok(vec![
                    Message::ClearAllKeyRegisters,
                    Message::SwitchKeyRegister(KeyRegisterType::Normal),
                    Message::SwitchScene(Scene::Main),
                ]);
            }
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
                self.cursor_position = self.cursor_position.saturating_sub(1);
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

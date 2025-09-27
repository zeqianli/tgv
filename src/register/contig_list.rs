use crate::{error::TGVError, message::StateMessage, register::KeyRegister, states::State};
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
    ) -> Result<Vec<StateMessage>, TGVError> {
        match key_event.code {
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
    pub input: String,
    pub cursor_position: usize,
}

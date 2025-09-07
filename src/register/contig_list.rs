use crate::{error::TGVError, message::StateMessage, register::Register, states::State};
use crossterm::event::{KeyCode, KeyEvent};

pub struct ContigListModeRegister {
    /// index of contigs in the contig header
    pub cursor_position: usize,
}

impl ContigListModeRegister {
    pub fn new(cursor_position: usize) -> Self {
        Self { cursor_position }
    }
}

impl Register for ContigListModeRegister {
    /// Move the selected contig up or down.
    fn update_key_event(
        &mut self,
        key_event: KeyEvent,
        state: &State,
    ) -> Result<Vec<StateMessage>, TGVError> {
        match key_event.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.cursor_position = self.cursor_position.saturating_add(1);
                let total_n_contigs = state.contig_header.contigs.len();
                if self.cursor_position >= total_n_contigs && total_n_contigs > 0 {
                    self.cursor_position = total_n_contigs - 1;
                }
                Ok(vec![])
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.cursor_position = self.cursor_position.saturating_sub(1);
                Ok(vec![])
            }
            _ => Ok(vec![]),
        }
    }
}

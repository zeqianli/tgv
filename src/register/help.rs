use crate::{
    display_mode::DisplayMode, error::TGVError, message::StateMessage, register::Register,
    states::State,
};
use crossterm::event::{KeyCode, KeyEvent};

pub struct HelpModeRegister {}

impl Default for HelpModeRegister {
    fn default() -> Self {
        Self::new()
    }
}

impl HelpModeRegister {
    pub fn new() -> Self {
        Self {}
    }
}

impl Register for HelpModeRegister {
    fn update_key_event(
        &mut self,
        key_event: KeyEvent,
        state: &State,
    ) -> Result<Vec<StateMessage>, TGVError> {
        match key_event.code {
            KeyCode::Esc => Ok(vec![StateMessage::SetDisplayMode(DisplayMode::Main)]),
            _ => Ok(vec![]),
        }
    }
}

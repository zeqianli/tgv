use crate::{
    error::TGVError, message::StateMessage, register::DisplayMode, register::KeyRegister,
    states::State,
};
use crossterm::event::{KeyCode, KeyEvent};

#[derive(Default, Debug)]
pub struct HelpModeRegister {}

impl KeyRegister for HelpModeRegister {
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

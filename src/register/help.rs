use crate::{
    error::TGVError, message::StateMessage, register::KeyRegister, rendering::Scene, states::State,
};
use crossterm::event::{KeyCode, KeyEvent};

#[derive(Default, Debug)]
pub struct HelpModeRegister {}

impl KeyRegister for HelpModeRegister {
    fn handle_key_event(
        &mut self,
        key_event: KeyEvent,
        state: &State,
    ) -> Result<Vec<StateMessage>, TGVError> {
        match key_event.code {
            KeyCode::Esc => Ok(vec![StateMessage::SetDisplayMode(Scene::Main)]),
            _ => Ok(vec![]),
        }
    }
}

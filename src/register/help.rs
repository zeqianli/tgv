use crate::{
    error::TGVError,
    message::Message,
    register::{KeyRegister, KeyRegisterType},
    rendering::Scene,
    states::State,
};
use crossterm::event::{KeyCode, KeyEvent};

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

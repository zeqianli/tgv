use crate::{
    error::TGVError,
    message::Message,
    register::{KeyRegister, KeyRegisterType},
    states::State,
};
use crossterm::event::{KeyCode, KeyEvent};

#[derive(Clone, Debug, Default)]
pub struct NormalModeRegister {
    input: String,
}

impl NormalModeRegister {
    pub fn add_char(&mut self, c: char) {
        self.input.push(c);
    }

    pub fn clear(&mut self) {
        self.input = String::new();
    }
}

/// Normal mode command handling
impl NormalModeRegister {
    const SMALL_HORIZONTAL_STEP: usize = 1;
    const LARGE_HORIZONTAL_STEP: usize = 30;
    const SMALL_VERTICAL_STEP: usize = 1;
    const LARGE_VERTICAL_STEP: usize = 30;

    const ZOOM_STEP: usize = 2;

    /// Translate key input to a state message. This does not mute states. States are muted downstream by handling state messages.
    pub fn update_by_char(&mut self, char: char) -> Result<Vec<Message>, TGVError> {
        // Add to registers
        match char {
            '1' | '2' | '3' | '4' | '5' | '6' | '7' | '8' | '9' => {
                if self.input.is_empty() || self.input.parse::<usize>().is_ok() {
                    self.add_char(char);
                    return Ok(vec![]); // Don't clear the register
                }
            }
            '0' => match self.input.len() {
                0 => return Err(TGVError::RegisterError("Empty input".to_string())),
                _ => {
                    if self.input.parse::<usize>().is_ok() {
                        self.add_char('0');
                        return Ok(vec![]); // Don't clear the register
                    }
                }
            },

            'g' => {
                if self.input.is_empty() || self.input.parse::<usize>().is_ok() {
                    self.add_char('g');
                    return Ok(vec![]); // Don't clear the register
                } else {
                    self.add_char('g');
                }
            }
            _ => {
                self.add_char(char); // proceed to interpretation
            }
        };

        let input = self.input.clone();

        self.clear();
        Self::parse_input(input)
    }

    fn parse_input(input: String) -> Result<Vec<Message>, TGVError> {
        let mut n_movement_chars = "".to_string();
        for char in input.chars() {
            match char {
                '0' | '1' | '2' | '3' | '4' | '5' | '6' | '7' | '8' | '9' => {
                    n_movement_chars += &char.to_string();
                }
                _ => {
                    break;
                }
            }
        }

        let suffix: String = input.chars().skip(n_movement_chars.len()).collect();

        let n_movements = if n_movement_chars.is_empty() {
            1
        } else {
            n_movement_chars.parse::<usize>()?
        };

        match suffix.as_str() {
            "ge" => Ok(vec![Message::GotoPreviousExonsEnd(n_movements)]),
            "gE" => Ok(vec![Message::GotoPreviousGenesEnd(n_movements)]),
            "gg" => Ok(vec![Message::GotoY(0)]),
            "G" => Ok(vec![Message::GotoYBottom]),
            "w" => Ok(vec![Message::GotoNextExonsStart(n_movements)]),
            "b" => Ok(vec![Message::GotoPreviousExonsStart(n_movements)]),
            "e" => Ok(vec![Message::GotoNextExonsEnd(n_movements)]),
            "W" => Ok(vec![Message::GotoNextGenesStart(n_movements)]),
            "B" => Ok(vec![Message::GotoPreviousGenesStart(n_movements)]),
            "E" => Ok(vec![Message::GotoNextGenesEnd(n_movements)]),
            "h" => Ok(vec![Message::MoveLeft(
                n_movements * Self::SMALL_HORIZONTAL_STEP,
            )]),
            "l" => Ok(vec![Message::MoveRight(
                n_movements * Self::SMALL_HORIZONTAL_STEP,
            )]),
            "j" => Ok(vec![Message::MoveDown(
                n_movements * Self::SMALL_VERTICAL_STEP,
            )]),
            "k" => Ok(vec![Message::MoveUp(
                n_movements * Self::SMALL_VERTICAL_STEP,
            )]),

            "y" => Ok(vec![Message::MoveLeft(
                Self::LARGE_HORIZONTAL_STEP * n_movements,
            )]),
            "p" => Ok(vec![Message::MoveRight(
                Self::LARGE_HORIZONTAL_STEP * n_movements,
            )]),

            "z" => Ok(vec![Message::ZoomIn(Self::ZOOM_STEP * n_movements)]),
            "o" => Ok(vec![Message::ZoomOut(Self::ZOOM_STEP * n_movements)]),
            "{" => Ok(vec![Message::MoveUp(
                Self::LARGE_VERTICAL_STEP * n_movements,
            )]),
            "}" => Ok(vec![Message::MoveDown(
                Self::LARGE_VERTICAL_STEP * n_movements,
            )]),
            _ => Err(TGVError::RegisterError(format!(
                "Invalid normal mode input: {}",
                input
            ))),
        }
    }
}

impl KeyRegister for NormalModeRegister {
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
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::message::Message;
    use rstest::rstest;

    #[rstest]
    #[case("",'g', Ok(vec![]))]
    #[case("g",'g', Ok(vec![Message::GotoY(0)]))]
    #[case("",'G', Ok(vec![Message::GotoYBottom]))]
    #[case("",'1', Ok(vec![]))]
    #[case("g",'1', Err(TGVError::RegisterError("Invalid input: g".to_string())))]
    #[case("", 'w', Ok(vec![Message::GotoNextExonsStart(1)]))]
    #[case("", 'b', Ok(vec![Message::GotoPreviousExonsStart(1)]))]
    #[case("", 'e', Ok(vec![Message::GotoNextExonsEnd(1)]))]
    #[case("", 'h', Ok(vec![Message::MoveLeft(1)]))]
    #[case("", 'l', Ok(vec![Message::MoveRight(1)]))]
    #[case("", 'j', Ok(vec![Message::MoveDown(1)]))]
    #[case("", 'k', Ok(vec![Message::MoveUp(1)]))]
    #[case("", 'z', Ok(vec![Message::ZoomIn(2)]))]
    #[case("", 'o', Ok(vec![Message::ZoomOut(2)]))]
    #[case("", '{', Ok(vec![Message::MoveUp(30)]))]
    #[case("", '}', Ok(vec![Message::MoveDown(30)]))]
    #[case("g", 'e', Ok(vec![Message::GotoPreviousExonsEnd(1)]))]
    #[case("g", 'E', Ok(vec![Message::GotoPreviousGenesEnd(1)]))]
    #[case("3", 'w', Ok(vec![Message::GotoNextExonsStart(3)]))]
    #[case("5", 'l', Ok(vec![Message::MoveRight(5)]))]
    #[case("10", 'z', Ok(vec![Message::ZoomIn(20)]))]
    #[case("", 'x', Err(TGVError::RegisterError("Invalid normal mode input: x".to_string())))]
    #[case("g", 'x', Err(TGVError::RegisterError("Invalid normal mode input: gx".to_string())))]
    #[case("3", 'x', Err(TGVError::RegisterError("Invalid normal mode input: 3x".to_string())))]
    #[case("3g", 'x', Err(TGVError::RegisterError("Invalid normal mode input: 3gx".to_string())))]
    fn test_normal_mode_translate(
        #[case] existing_buffer: &str,
        #[case] key: char,
        #[case] expected: Result<Vec<Message>, TGVError>,
    ) {
        let mut register = NormalModeRegister {
            input: existing_buffer.to_string(),
        };

        // Test the translation

        let result = register.update_by_char(key);
        match (&result, &expected) {
            (Ok(result), Ok(expected)) => assert_eq!(result, expected),
            (Err(e), Err(expected)) => {} // OK
            _ => panic!(
                "Test failed.  result: {:?}, expected: {:?}",
                result, expected
            ),
        }
    }
}

use crate::error::TGVError;
use crate::message::{Message, Movement, Scroll, Zoom};

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
const SMALL_HORIZONTAL_STEP: u64 = 1;
const LARGE_HORIZONTAL_STEP: u64 = 30;
const SMALL_VERTICAL_STEP: usize = 1;
const LARGE_VERTICAL_STEP: usize = 30;

const ZOOM_STEP: u64 = 2;

/// Translate key input to a state message. This does not mute states. States are muted downstream by handling state messages.
pub fn update_by_char(current: &mut String, char: char) -> Result<Vec<Message>, TGVError> {
    // Add to registers
    match char {
        '1' | '2' | '3' | '4' | '5' | '6' | '7' | '8' | '9' => {
            if current.is_empty() || current.parse::<usize>().is_ok() {
                current.push(char);
                return Ok(vec![]); // Don't clear the register
            }
        }
        '0' => match current.len() {
            0 => return Err(TGVError::RegisterError("Empty current".to_string())),
            _ => {
                if current.parse::<usize>().is_ok() {
                    current.push('0');
                    return Ok(vec![]); // Don't clear the register
                }
            }
        },

        'g' => {
            if current.is_empty() || current.parse::<usize>().is_ok() {
                current.push('g');
                return Ok(vec![]); // Don't clear the register
            } else {
                current.push('g');
            }
        }
        _ => {
            current.push(char); // proceed to interpretation
        }
    };
    let input = current.clone();
    current.clear();
    parse_input(input)
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
        "ge" => Ok(vec![Message::Move(Movement::PreviousExonsEnd(n_movements))]),
        "gE" => Ok(vec![Message::Move(Movement::PreviousGenesEnd(n_movements))]),
        //"gg" => Ok(vec![Message::Move(Movement::Y(0))]),
        //"G" => Ok(vec![Message::Move(Movement::YBottom]),
        "w)" => Ok(vec![Message::Move(Movement::NextExonsStart(n_movements))]),
        "b" => Ok(vec![Message::Move(Movement::PreviousExonsStart(
            n_movements,
        ))]),
        "e" => Ok(vec![Message::Move(Movement::NextExonsEnd(n_movements))]),
        "W" => Ok(vec![Message::Move(Movement::NextGenesStart(n_movements))]),
        "B" => Ok(vec![Message::Move(Movement::PreviousGenesStart(
            n_movements,
        ))]),
        "E" => Ok(vec![Message::Move(Movement::NextGenesEnd(n_movements))]),
        "h" => Ok(vec![Message::Move(Movement::Left(
            n_movements as u64 * SMALL_HORIZONTAL_STEP,
        ))]),
        "l" => Ok(vec![Message::Move(Movement::Right(
            n_movements as u64 * SMALL_HORIZONTAL_STEP,
        ))]),
        "j" => Ok(vec![Message::Scroll(Scroll::Down(
            n_movements * SMALL_VERTICAL_STEP,
        ))]),
        "k" => Ok(vec![Message::Scroll(Scroll::Up(
            n_movements * SMALL_VERTICAL_STEP,
        ))]),

        "y" => Ok(vec![Message::Move(Movement::Left(
            LARGE_HORIZONTAL_STEP * n_movements as u64,
        ))]),
        "p" => Ok(vec![Message::Move(Movement::Right(
            LARGE_HORIZONTAL_STEP * n_movements as u64,
        ))]),

        "z" => Ok(vec![Message::Zoom(Zoom::In(
            ZOOM_STEP * n_movements as u64,
        ))]),
        "o" => Ok(vec![Message::Zoom(Zoom::Out(
            ZOOM_STEP * n_movements as u64,
        ))]),
        "{" => Ok(vec![Message::Scroll(Scroll::Up(
            LARGE_VERTICAL_STEP * n_movements,
        ))]),
        "}" => Ok(vec![Message::Scroll(Scroll::Down(
            LARGE_VERTICAL_STEP * n_movements,
        ))]),
        _ => Err(TGVError::RegisterError(format!(
            "Invalid normal mode input: {}",
            input
        ))),
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::message::Message;
    use rstest::rstest;

    // #[rstest]
    // #[case("",'g', Ok(vec![]))]
    // #[case("g",'g', Ok(vec![Message::GotoY(0)]))]
    // #[case("",'G', Ok(vec![Message::GotoYBottom]))]
    // #[case("",'1', Ok(vec![]))]
    // #[case("g",'1', Err(TGVError::RegisterError("Invalid input: g".to_string())))]
    // #[case("", 'w', Ok(vec![Message::GotoNextExonsStart(1)]))]
    // #[case("", 'b', Ok(vec![Message::GotoPreviousExonsStart(1)]))]
    // #[case("", 'e', Ok(vec![Message::GotoNextExonsEnd(1)]))]
    // #[case("", 'h', Ok(vec![Message::MoveLeft(1)]))]
    // #[case("", 'l', Ok(vec![Message::MoveRight(1)]))]
    // #[case("", 'j', Ok(vec![Message::MoveDown(1)]))]
    // #[case("", 'k', Ok(vec![Message::MoveUp(1)]))]
    // #[case("", 'z', Ok(vec![Message::ZoomIn(2)]))]
    // #[case("", 'o', Ok(vec![Message::ZoomOut(2)]))]
    // #[case("", '{', Ok(vec![Message::MoveUp(30)]))]
    // #[case("", '}', Ok(vec![Message::MoveDown(30)]))]
    // #[case("g", 'e', Ok(vec![Message::GotoPreviousExonsEnd(1)]))]
    // #[case("g", 'E', Ok(vec![Message::GotoPreviousGenesEnd(1)]))]
    // #[case("3", 'w', Ok(vec![Message::GotoNextExonsStart(3)]))]
    // #[case("5", 'l', Ok(vec![Message::MoveRight(5)]))]
    // #[case("10", 'z', Ok(vec![Message::ZoomIn(20)]))]
    // #[case("", 'x', Err(TGVError::RegisterError("Invalid normal mode input: x".to_string())))]
    // #[case("g", 'x', Err(TGVError::RegisterError("Invalid normal mode input: gx".to_string())))]
    // #[case("3", 'x', Err(TGVError::RegisterError("Invalid normal mode input: 3x".to_string())))]
    // #[case("3g", 'x', Err(TGVError::RegisterError("Invalid normal mode input: 3gx".to_string())))]
    // fn test_normal_mode_translate(
    //     #[case] existing_buffer: &str,
    //     #[case] key: char,
    //     #[case] expected: Result<Vec<Message>, TGVError>,
    // ) {
    //     let mut register = NormalModeRegister {
    //         input: existing_buffer.to_string(),
    //     };

    //     // Test the translation

    //     let result = register.update_by_char(key);
    //     match (&result, &expected) {
    //         (Ok(result), Ok(expected)) => assert_eq!(result, expected),
    //         (Err(e), Err(expected)) => {} // OK
    //         _ => panic!(
    //             "Test failed.  result: {:?}, expected: {:?}",
    //             result, expected
    //         ),
    //     }
    // }
}

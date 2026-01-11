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
        "ge" => Ok(vec![Message::from(Movement::PreviousExonsEnd(n_movements))]),
        "gE" => Ok(vec![Message::from(Movement::PreviousGenesEnd(n_movements))]),
        "gg" => Ok(vec![Message::from(Scroll::Position(0))]),
        "gG" => Ok(vec![Message::from(Scroll::Bottom)]),
        "w" => Ok(vec![Message::from(Movement::NextExonsStart(n_movements))]),
        "b" => Ok(vec![Message::from(Movement::PreviousExonsStart(
            n_movements,
        ))]),
        "e" => Ok(vec![Message::from(Movement::NextExonsEnd(n_movements))]),
        "W" => Ok(vec![Message::from(Movement::NextGenesStart(n_movements))]),
        "B" => Ok(vec![Message::from(Movement::PreviousGenesStart(
            n_movements,
        ))]),
        "E" => Ok(vec![Message::from(Movement::NextGenesEnd(n_movements))]),
        "h" => Ok(vec![Message::from(Movement::Left(
            n_movements as u64 * SMALL_HORIZONTAL_STEP,
        ))]),
        "l" => Ok(vec![Message::from(Movement::Right(
            n_movements as u64 * SMALL_HORIZONTAL_STEP,
        ))]),
        "j" => Ok(vec![Message::from(Scroll::Down(
            n_movements * SMALL_VERTICAL_STEP,
        ))]),
        "k" => Ok(vec![Message::from(Scroll::Up(
            n_movements * SMALL_VERTICAL_STEP,
        ))]),

        "y" => Ok(vec![Message::from(Movement::Left(
            LARGE_HORIZONTAL_STEP * n_movements as u64,
        ))]),
        "p" => Ok(vec![Message::from(Movement::Right(
            LARGE_HORIZONTAL_STEP * n_movements as u64,
        ))]),

        "z" => Ok(vec![Message::from(Zoom::In(
            ZOOM_STEP * n_movements as u64,
        ))]),
        "o" => Ok(vec![Message::from(Zoom::Out(
            ZOOM_STEP * n_movements as u64,
        ))]),
        "{" => Ok(vec![Message::from(Scroll::Up(
            LARGE_VERTICAL_STEP * n_movements,
        ))]),
        "}" => Ok(vec![Message::from(Scroll::Down(
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
    use crate::message::{Movement, Scroll, Zoom};
    use rstest::rstest;

    #[rstest]
    #[case("",'g', Ok(vec![]))]
    #[case("g",'g', Ok(vec![Scroll::Position(0).into()]))]
    #[case("g",'G', Ok(vec![Scroll::Bottom.into()]))]
    #[case("",'1', Ok(vec![]))]
    #[case("g",'1', Err(TGVError::RegisterError("Invalid input: g".to_string())))]
    #[case("", 'w', Ok(vec![Movement::NextExonsStart(1).into()]))]
    #[case("", 'b', Ok(vec![Movement::PreviousExonsStart(1).into()]))]
    #[case("", 'e', Ok(vec![Movement::NextExonsEnd(1).into()]))]
    #[case("", 'h', Ok(vec![Movement::Left(1).into()]))]
    #[case("", 'l', Ok(vec![Movement::Right(1).into()]))]
    #[case("", 'j', Ok(vec![Scroll::Down(1).into()]))]
    #[case("", 'k', Ok(vec![Scroll::Up(1).into()]))]
    #[case("", 'z', Ok(vec![Zoom::In(2).into()]))]
    #[case("", 'o', Ok(vec![Zoom::Out(2).into()]))]
    #[case("", '{', Ok(vec![Scroll::Up(30).into()]))]
    #[case("", '}', Ok(vec![Scroll::Down(30).into()]))]
    #[case("g", 'e', Ok(vec![Movement::PreviousExonsEnd(1).into()]))]
    #[case("g", 'E', Ok(vec![Movement::PreviousGenesEnd(1).into()]))]
    #[case("3", 'w', Ok(vec![Movement::NextExonsStart(3).into()]))]
    #[case("5", 'l', Ok(vec![Movement::Right(5).into()]))]
    #[case("10", 'z', Ok(vec![Zoom::In(20).into()]))]
    #[case("", 'x', Err(TGVError::RegisterError("Invalid normal mode input: x".to_string())))]
    #[case("g", 'x', Err(TGVError::RegisterError("Invalid normal mode input: gx".to_string())))]
    #[case("3", 'x', Err(TGVError::RegisterError("Invalid normal mode input: 3x".to_string())))]
    #[case("3g", 'x', Err(TGVError::RegisterError("Invalid normal mode input: 3gx".to_string())))]
    fn test_normal_mode_translate(
        #[case] existing_buffer: &str,
        #[case] key: char,
        #[case] expected: Result<Vec<Message>, TGVError>,
    ) {
        // Test the translation
        let mut buffer = existing_buffer.to_string();

        let result = update_by_char(&mut buffer, key);
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

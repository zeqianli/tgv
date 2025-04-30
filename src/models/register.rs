use crate::{error::TGVError, models::{message::StateMessage, mode::InputMode}, states::State};
use crossterm::event::{Event, KeyEvent, KeyCode};



/// Register stores inputs and translates key event to StateMessages. 
pub trait Register {


    /// Update with a new event. 
    /// If applicable, return 
    /// If this event triggers an error, returns Error.
    fn update(&mut self, event: Event) -> Result<Vec<StateMessage>, TGVError>;

}

pub enum RegisterEnum {
    Normal(NormalModeRegister),
    Command(CommandModeRegister)
}

impl RegisterEnum {
    pub fn new(mode: InputMode) -> Result<Self, TGVError> {
        match mode {
            InputMode::Normal | InputMode::Help => Ok(RegisterEnum::Normal(NormalModeRegister::new())),
            InputMode::Command => Ok(RegisterEnum::Command(CommandModeRegister::new()))
        }
    }
}

#[derive(Clone)]
pub struct NormalModeRegister {
    input: String,
}

impl NormalModeRegister {
    pub fn new() -> Self {
        Self {
            input: String::new(),
        }
    }

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

    const ZOOM_STEP: usize = 2;

    const VALID_MOVEMENT_SUFFIXES: [&str; 18] = [
        "ge", // previous exon end
        "gE", // previous exon start,g1
        "w",  // next exon start
        "b",  // previous exon start
        "e",  // next exon end
        "W",  // next gene start
        "B",  // previous gene start
        "E",  // next gene end
        "h",  // left
        "l",  // right
        "j",  // down
        "k",  // up
        "y",  // large left
        "p",  // large right
        "z",  // zoom out
        "o",  // zoom in
        "{",  // previous contig
        "}",  // next contig
    ];

    /// Translate key input to a state message. This does not mute states. States are muted downstream by handling state messages.
    pub fn update_by_char(&mut self, char: char) -> Result<Vec<StateMessage>, TGVError> { 

        // TODO: clear logic is not right here. 
        // Add to registers
        let messages = match char {
            '1' | '2' | '3' | '4' | '5' | '6' | '7' | '8' | '9' => {
                if self.input.is_empty() || self.input.parse::<usize>().is_ok() {
                    vec![StateMessage::AddCharToNormalModeRegisters(char)]
                } else {
                    return Err(TGVError::RegisterError(format!("Invalid input: {}", self.input)))
                }
            },
            '0' => match self.input.len() {
                0 => return Err(TGVError::RegisterError("Empty input".to_string())),
                _ => {
                    if self.input.parse::<usize>().is_ok() {
                        vec![StateMessage::AddCharToNormalModeRegisters('0')]
                    } else {
                        return Err(TGVError::RegisterError(format!("Invalid input: {}", self.input)));
                    }
                }
            },

            'g' => {
                if self.input.is_empty() || self.input.parse::<usize>().is_ok() {
                    self.add_char('g');
                    return Ok(vec![]) // Don't clear the register
                } else {
                    return Err(TGVError::RegisterError(format!("Invalid input: {}", self.input)))
                }
            }

            c => {
                let string = self.input.clone() + &c.to_string();

                let mut suffix: Option<String> = None;

                for suf in NormalModeRegister::VALID_MOVEMENT_SUFFIXES.iter() {
                    if string.ends_with(suf) {
                        suffix = Some(suf.to_string());
                        break;
                    }
                }

                if suffix.is_none() {
                    return Err(TGVError::RegisterError(format!("Invalid normal mode input: {}", string)));
                }

                let suffix = suffix.unwrap();

                let n_movements: usize;

                if suffix.len() == string.len() {
                    n_movements = 1;
                } else {
                    match string[0..string.len() - suffix.len()].parse::<usize>() {
                        Ok(n) => n_movements = n,
                        Err(_) => return Err(TGVError::RegisterError(format!("Invalid normal mode input: {}", string))),
                    }
                }

                match suffix.as_str() {
                    "ge" => vec![StateMessage::GotoPreviousExonsEnd(n_movements),],
                    "gE" => vec![StateMessage::GotoPreviousGenesEnd(n_movements),],
                    "w" => vec![StateMessage::GotoNextExonsStart(n_movements),],
                    "b" => vec![StateMessage::GotoPreviousExonsStart(n_movements),],
                    "e" => vec![StateMessage::GotoNextExonsEnd(n_movements),],
                    "W" => vec![StateMessage::GotoNextGenesStart(n_movements),],
                    "B" => vec![StateMessage::GotoPreviousGenesStart(n_movements),],
                    "E" => vec![StateMessage::GotoNextGenesEnd(n_movements),],
                    "h" => vec![StateMessage::MoveLeft(n_movements * Self::SMALL_HORIZONTAL_STEP),],
                    "l" => vec![StateMessage::MoveRight(n_movements * Self::SMALL_HORIZONTAL_STEP),],
                    "j" => vec![StateMessage::MoveDown(n_movements * Self::SMALL_VERTICAL_STEP),],
                    "k" => vec![StateMessage::MoveUp(n_movements * Self::SMALL_VERTICAL_STEP),],

                    "y" => vec![StateMessage::MoveLeft(Self::LARGE_HORIZONTAL_STEP * n_movements),],
                    "p" => vec![StateMessage::MoveRight(Self::LARGE_HORIZONTAL_STEP * n_movements),],

                    "z" => vec![StateMessage::ZoomIn(Self::ZOOM_STEP * n_movements),],
                    "o" => vec![StateMessage::ZoomOut(Self::ZOOM_STEP * n_movements),],
                    "{" => vec![StateMessage::GotoPreviousContig(n_movements),],
                    "}" => vec![StateMessage::GotoNextContig(n_movements),],
                    _ => return Err(TGVError::RegisterError(format!("Invalid normal mode input: {}", string))),
                }
            }
        };

        // If reaches here, clear the register
        self.clear();
        Ok(messages)
    }
}


impl Register for NormalModeRegister {
    fn update(&mut self, event: Event) -> Result<Vec<StateMessage>, TGVError> {
        match event {
            Event::Key(key_event) => {
                match key_event.code {
                    KeyCode::Char(':') => {
                        self.clear();
                        return Ok(vec![StateMessage::SwitchMode(InputMode::Normal)])
                    }
                    KeyCode::Char(char) => return self.update_by_char(char),
                    _ => {
                        self.clear();
                        return Err(TGVError::RegisterError(format!(
                            "Invalid input: {:?}",
                            key_event
                        )))

                    }
                }
            }
            _ => Ok(vec![])

        }
    }
}

pub struct CommandModeRegister {
    input: String,
    cursor_position: usize,
}

impl Default for CommandModeRegister {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandModeRegister {
    pub fn new() -> Self {
        Self {
            input: String::new(),
            cursor_position: 0,
        }
    }

    pub fn input(&self) -> String {
        self.input.clone()
    }

    pub fn cursor_position(&self) -> usize {
        self.cursor_position
    }

    pub fn clear(&mut self) {
        self.input = String::new();
        self.cursor_position = 0;
    }

    pub fn add_char(&mut self, c: char) {
        self.input.insert(self.cursor_position, c);
        self.cursor_position += 1;
    }

    pub fn backspace(&mut self) {
        if self.cursor_position > 0 {
            self.input.remove(self.cursor_position - 1);
            self.cursor_position -= 1;
        }
    }

    pub fn move_cursor_left(&mut self, by: usize) {
        self.cursor_position = self.cursor_position.saturating_sub(by);
    }

    pub fn move_cursor_right(&mut self, by: usize) {
        self.cursor_position = self
            .cursor_position
            .saturating_add(by)
            .clamp(0, self.input.len());
    }
}

impl Register for CommandModeRegister {
    pub fn udpate(&self, event: Event) -> Result<Vec<StateMessage>, TGVError> {

        // TODO
        match c {
            KeyCode::Char(c) => Ok(vec![StateMessage::AddCharToCommandModeRegisters(c)]),
            KeyCode::Backspace => Ok(vec![StateMessage::BackspaceCommandModeRegisters]),
            KeyCode::Left => Ok(vec![StateMessage::MoveCursorLeft(1)]),
            KeyCode::Right => Ok(vec![StateMessage::MoveCursorRight(1)]),
            _ => Err("Invalid input".to_string()),
        }
    }
}

impl CommandModeRegister {

    /// Supported commands:
    /// :q: Quit.
    /// :h: Help.
    /// :1234: Go to position 1234 on the same contig.
    /// :12:1234: Go to position 1234 on contig 12.
    pub fn parse(&self) -> Result<Vec<StateMessage>, String> {
        if self.input == "q" {
            return Ok(vec![StateMessage::Quit]);
        }

        if self.input == "h" {
            return Ok(vec![StateMessage::SwitchMode(InputMode::Help)]);
        }

        let split = self.input.split(":").collect::<Vec<&str>>();

        match split.len() {
            1 => match split[0].parse::<usize>() {
                Ok(n) => Ok(vec![StateMessage::GotoCoordinate(n)]),
                Err(_) => Ok(vec![StateMessage::GoToGene(split[0].to_string())]),
            },
            2 => match split[1].parse::<usize>() {
                Ok(n) => Ok(vec![StateMessage::GotoContigCoordinate(
                    split[0].to_string(),
                    n,
                )]),
                Err(_) => Err(format!("Invalid command mode input: {}", self.input)),
            },
            _ => Err(format!("Invalid command mode input: {}", self.input)),
        }
    }
}

// TODO: HelpMode
// Old code:
/* 

fn translate_key_event(&self, key_event: KeyEvent) -> Vec<StateMessage> {
    let messages = match self.input_mode {
        InputMode::Normal => {
            match key_event.code {
                // Switch mode
                KeyCode::Char(':') => vec![
                    StateMessage::SwitchMode(InputMode::Command),
                    StateMessage::ClearNormalModeRegisters,
                ],
                _ => match self.normal_mode_register.translate(key_event.code) {
                    Ok(messages) => messages,
                    Err(error_message) => vec![
                        StateMessage::NormalModeRegisterError(error_message),
                        StateMessage::ClearNormalModeRegisters,
                    ],
                },
            }
        }
        InputMode::Command => match key_event.code {
            KeyCode::Esc => vec![
                StateMessage::ClearCommandModeRegisters,
                StateMessage::SwitchMode(InputMode::Normal),
            ],
            KeyCode::Enter => {
                let mut messages = vec![
                    StateMessage::ClearCommandModeRegisters,
                    StateMessage::SwitchMode(InputMode::Normal),
                ];
                messages.extend(match self.command_mode_register.parse() {
                    Ok(parsed_messages) => parsed_messages,
                    Err(error_message) => {
                        vec![StateMessage::CommandModeRegisterError(error_message)]
                    }
                });
                messages
            }
            _ => match self.command_mode_register.translate(key_event.code) {
                Ok(messages) => messages,
                Err(error_message) => {
                    vec![StateMessage::CommandModeRegisterError(error_message)]
                }
            },
        },
        InputMode::Help => match key_event.code {
            KeyCode::Esc => vec![StateMessage::SwitchMode(InputMode::Normal)],
            _ => vec![],
        },
    };

    // Check that if there is a message that requires the reference genome, make sure it is provided.
    // Otherwise, pass on an error message.
    for message in messages.iter() {
        if message.requires_reference() && self.settings.reference.is_none() {
            return vec![StateMessage::Error(
                TGVError::StateError("Reference is not provided".to_string()).to_string(),
            )];
        }
    }

    messages
}
*/



#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::message::StateMessage;
    use rstest::rstest;

    #[rstest]
    #[case("q", Ok(vec![StateMessage::Quit]))]
    #[case("1234", Ok(vec![StateMessage::GotoCoordinate(1234)]))]
    #[case("chr1:1000", Ok(vec![StateMessage::GotoContigCoordinate("chr1".to_string(), 1000)]))]
    #[case("17:7572659", Ok(vec![StateMessage::GotoContigCoordinate("17".to_string(), 7572659)]))]
    #[case("TP53", Ok(vec![StateMessage::GoToGene("TP53".to_string())]))]
    #[case("invalid:command:format", Err("Invalid command mode input: invalid:command:format".to_string()))]
    #[case("chr1:invalid", Err("Invalid command mode input: chr1:invalid".to_string()))]
    fn test_command_parse(
        #[case] input: &str,
        #[case] expected: Result<Vec<StateMessage>, String>,
    ) {
        let register = CommandModeRegister {
            input: input.to_string(),
            cursor_position: input.len(),
        };
        assert_eq!(register.parse(), expected);
    }

    #[rstest]
    #[case("",KeyCode::Char('g'), Ok(vec![StateMessage::AddCharToNormalModeRegisters('g')]))]
    #[case("g",KeyCode::Char('g'), Err("Invalid input: g".to_string()))]
    #[case("",KeyCode::Char('1'), Ok(vec![StateMessage::AddCharToNormalModeRegisters('1')]))]
    #[case("g",KeyCode::Char('1'), Err("Invalid input: g".to_string()))]
    #[case("", KeyCode::Char('w'), Ok(vec![StateMessage::GotoNextExonsStart(1), StateMessage::ClearNormalModeRegisters]))]
    #[case("", KeyCode::Char('b'), Ok(vec![StateMessage::GotoPreviousExonsStart(1), StateMessage::ClearNormalModeRegisters]))]
    #[case("", KeyCode::Char('e'), Ok(vec![StateMessage::GotoNextExonsEnd(1), StateMessage::ClearNormalModeRegisters]))]
    #[case("", KeyCode::Char('h'), Ok(vec![StateMessage::MoveLeft(1), StateMessage::ClearNormalModeRegisters]))]
    #[case("", KeyCode::Char('l'), Ok(vec![StateMessage::MoveRight(1), StateMessage::ClearNormalModeRegisters]))]
    #[case("", KeyCode::Char('j'), Ok(vec![StateMessage::MoveDown(1), StateMessage::ClearNormalModeRegisters]))]
    #[case("", KeyCode::Char('k'), Ok(vec![StateMessage::MoveUp(1), StateMessage::ClearNormalModeRegisters]))]
    #[case("", KeyCode::Char('z'), Ok(vec![StateMessage::ZoomIn(2), StateMessage::ClearNormalModeRegisters]))]
    #[case("", KeyCode::Char('o'), Ok(vec![StateMessage::ZoomOut(2), StateMessage::ClearNormalModeRegisters]))]
    #[case("", KeyCode::Char('{'), Ok(vec![StateMessage::GotoPreviousContig(1), StateMessage::ClearNormalModeRegisters]))]
    #[case("", KeyCode::Char('}'), Ok(vec![StateMessage::GotoNextContig(1), StateMessage::ClearNormalModeRegisters]))]
    #[case("g", KeyCode::Char('e'), Ok(vec![StateMessage::GotoPreviousExonsEnd(1), StateMessage::ClearNormalModeRegisters]))]
    #[case("g", KeyCode::Char('E'), Ok(vec![StateMessage::GotoPreviousGenesEnd(1), StateMessage::ClearNormalModeRegisters]))]
    #[case("3", KeyCode::Char('w'), Ok(vec![StateMessage::GotoNextExonsStart(3), StateMessage::ClearNormalModeRegisters]))]
    #[case("5", KeyCode::Char('l'), Ok(vec![StateMessage::MoveRight(5), StateMessage::ClearNormalModeRegisters]))]
    #[case("10", KeyCode::Char('z'), Ok(vec![StateMessage::ZoomIn(20), StateMessage::ClearNormalModeRegisters]))]
    #[case("", KeyCode::Char('x'), Err("Invalid normal mode input: x".to_string()))]
    #[case("g", KeyCode::Char('x'), Err("Invalid normal mode input: gx".to_string()))]
    #[case("3", KeyCode::Char('x'), Err("Invalid normal mode input: 3x".to_string()))]
    #[case("3g", KeyCode::Char('x'), Err("Invalid normal mode input: 3gx".to_string()))]
    fn test_normal_mode_translate(
        #[case] existing_buffer: &str,
        #[case] key: KeyCode,
        #[case] expected: Result<Vec<StateMessage>, String>,
    ) {
        let register = NormalModeRegister {
            input: existing_buffer.to_string(),
        };

        // Test the translation
        let result = register.translate(key);
        assert_eq!(result, expected);
    }
}

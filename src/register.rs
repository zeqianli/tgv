use crate::{display_mode::DisplayMode, error::TGVError, message::StateMessage, states::State};
use crossterm::event::{KeyCode, KeyEvent};

use strum::Display;

#[derive(Debug, Clone, Eq, PartialEq, Display)]
pub enum RegisterType {
    Normal,
    Command,
    Help,
    ContigList,
}

/// Register stores inputs and translates key event to StateMessages.
pub trait Register {
    /// Update with a new event.
    /// If applicable, return
    /// If this event triggers an error, returns Error.
    fn update_key_event(
        &mut self,
        event: KeyEvent,
        state: &State,
    ) -> Result<Vec<StateMessage>, TGVError>;
}

pub struct Registers {
    pub current: RegisterType,
    pub normal: NormalModeRegister,
    pub command: CommandModeRegister,
    pub help: HelpModeRegister,
    pub contig_list: ContigListModeRegister,
}

impl Registers {
    pub fn new() -> Result<Self, TGVError> {
        Ok(Self {
            current: RegisterType::Normal,
            normal: NormalModeRegister::new(),
            command: CommandModeRegister::new(),
            help: HelpModeRegister::new(),
            contig_list: ContigListModeRegister::new(0),
        })
    }

    pub fn update_state(&mut self, state: &State) -> Result<(), TGVError> {
        if self.current != RegisterType::ContigList {
            self.contig_list.cursor_position = state
                .contigs
                .get_index(&state.contig()?)
                .ok_or(TGVError::RegisterError("No contigs".to_string()))?;
        }
        Ok(())
    }

    fn clear(&mut self) {
        self.normal.clear();
        self.command.clear();
    }
}

// pub enum RegisterEnum {
//     Normal(NormalModeRegister),
//     Command(CommandModeRegister),
// }

impl Register for Registers {
    fn update_key_event(
        &mut self,
        key_event: KeyEvent,
        state: &State,
    ) -> Result<Vec<StateMessage>, TGVError> {
        match (key_event.code, self.current.clone()) {
            (KeyCode::Char(':'), RegisterType::Normal) => {
                self.clear();
                self.current = RegisterType::Command;

                return Ok(vec![]);
            }
            (KeyCode::Esc, RegisterType::Command) => {
                self.current = RegisterType::Normal;
                self.clear();
                return Ok(vec![]);
            }
            (KeyCode::Esc, RegisterType::Help) => {
                self.current = RegisterType::Normal;
                self.clear();
                return Ok(vec![StateMessage::SetDisplayMode(DisplayMode::Main)]);
            }

            (KeyCode::Enter, RegisterType::Command) => {
                if self.command.input() == "h" {
                    self.current = RegisterType::Help;
                    self.clear();
                    return Ok(vec![StateMessage::SetDisplayMode(DisplayMode::Help)]);
                }

                if self.command.input() == "ls" || self.command.input() == "contigs" {
                    self.current = RegisterType::ContigList;
                    self.clear();
                    return Ok(vec![StateMessage::SetDisplayMode(DisplayMode::ContigList)]);
                }
                let output = self
                    .command
                    .parse()
                    .unwrap_or_else(|e| vec![StateMessage::Message(format!("{}", e))]);
                self.current = RegisterType::Normal;
                self.command.clear();
                return Ok(output);
            }
            (KeyCode::Enter, RegisterType::ContigList) => {
                self.current = RegisterType::Normal;
                self.clear();

                return Ok(vec![
                    StateMessage::SetDisplayMode(DisplayMode::Main),
                    StateMessage::GotoContigIndex(self.contig_list.cursor_position),
                ]);
            }

            (KeyCode::Esc, RegisterType::ContigList) => {
                self.current = RegisterType::Normal;
                self.clear();
                return Ok(vec![StateMessage::SetDisplayMode(DisplayMode::Main)]);
            }

            _ => {}
        }

        Ok(match self.current {
            RegisterType::Normal => self.normal.update_key_event(key_event, state),
            RegisterType::Command => self.command.update_key_event(key_event, state),
            RegisterType::Help => self.help.update_key_event(key_event, state),
            RegisterType::ContigList => self.contig_list.update_key_event(key_event, state),
        }
        .unwrap_or_else(|e| {
            self.clear();
            vec![StateMessage::Message(format!("{}", e))]
        }))
    }
}

#[derive(Clone)]
pub struct NormalModeRegister {
    input: String,
}

impl Default for NormalModeRegister {
    fn default() -> Self {
        Self::new()
    }
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
                    self.add_char(char);
                    return Ok(vec![]); // Don't clear the register
                } else {
                    return Err(TGVError::RegisterError(format!(
                        "Invalid input: {}",
                        self.input
                    )));
                }
            }
            '0' => match self.input.len() {
                0 => return Err(TGVError::RegisterError("Empty input".to_string())),
                _ => {
                    if self.input.parse::<usize>().is_ok() {
                        self.add_char('0');
                        return Ok(vec![]); // Don't clear the register
                    } else {
                        return Err(TGVError::RegisterError(format!(
                            "Invalid input: {}",
                            self.input
                        )));
                    }
                }
            },

            'g' => {
                if self.input.is_empty() || self.input.parse::<usize>().is_ok() {
                    self.add_char('g');
                    return Ok(vec![]); // Don't clear the register
                } else {
                    return Err(TGVError::RegisterError(format!(
                        "Invalid input: {}",
                        self.input
                    )));
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
                    return Err(TGVError::RegisterError(format!(
                        "Invalid normal mode input: {}",
                        string
                    )));
                }

                let suffix = suffix.unwrap();

                let n_movements: usize;

                if suffix.len() == string.len() {
                    n_movements = 1;
                } else {
                    match string[0..string.len() - suffix.len()].parse::<usize>() {
                        Ok(n) => n_movements = n,
                        Err(_) => {
                            return Err(TGVError::RegisterError(format!(
                                "Invalid normal mode input: {}",
                                string
                            )))
                        }
                    }
                }

                match suffix.as_str() {
                    "ge" => vec![StateMessage::GotoPreviousExonsEnd(n_movements)],
                    "gE" => vec![StateMessage::GotoPreviousGenesEnd(n_movements)],
                    "w" => vec![StateMessage::GotoNextExonsStart(n_movements)],
                    "b" => vec![StateMessage::GotoPreviousExonsStart(n_movements)],
                    "e" => vec![StateMessage::GotoNextExonsEnd(n_movements)],
                    "W" => vec![StateMessage::GotoNextGenesStart(n_movements)],
                    "B" => vec![StateMessage::GotoPreviousGenesStart(n_movements)],
                    "E" => vec![StateMessage::GotoNextGenesEnd(n_movements)],
                    "h" => vec![StateMessage::MoveLeft(
                        n_movements * Self::SMALL_HORIZONTAL_STEP,
                    )],
                    "l" => vec![StateMessage::MoveRight(
                        n_movements * Self::SMALL_HORIZONTAL_STEP,
                    )],
                    "j" => vec![StateMessage::MoveDown(
                        n_movements * Self::SMALL_VERTICAL_STEP,
                    )],
                    "k" => vec![StateMessage::MoveUp(
                        n_movements * Self::SMALL_VERTICAL_STEP,
                    )],

                    "y" => vec![StateMessage::MoveLeft(
                        Self::LARGE_HORIZONTAL_STEP * n_movements,
                    )],
                    "p" => vec![StateMessage::MoveRight(
                        Self::LARGE_HORIZONTAL_STEP * n_movements,
                    )],

                    "z" => vec![StateMessage::ZoomIn(Self::ZOOM_STEP * n_movements)],
                    "o" => vec![StateMessage::ZoomOut(Self::ZOOM_STEP * n_movements)],
                    "{" => vec![StateMessage::GotoPreviousContig(n_movements)],
                    "}" => vec![StateMessage::GotoNextContig(n_movements)],
                    _ => {
                        return Err(TGVError::RegisterError(format!(
                            "Invalid normal mode input: {}",
                            string
                        )))
                    }
                }
            }
        };

        // If reaches here, clear the register
        self.clear();
        Ok(messages)
    }
}

impl Register for NormalModeRegister {
    fn update_key_event(
        &mut self,
        key_event: KeyEvent,
        state: &State,
    ) -> Result<Vec<StateMessage>, TGVError> {
        match key_event.code {
            KeyCode::Char(char) => self.update_by_char(char),
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
    fn update_key_event(
        &mut self,
        key_event: KeyEvent,
        state: &State,
    ) -> Result<Vec<StateMessage>, TGVError> {
        // TODO
        match key_event.code {
            KeyCode::Char(c) => {
                self.add_char(c);
                Ok(vec![])
            }
            KeyCode::Backspace => {
                self.backspace();
                Ok(vec![])
            }
            KeyCode::Left => {
                self.move_cursor_left(1);
                Ok(vec![])
            }
            KeyCode::Right => {
                self.move_cursor_right(1);
                Ok(vec![])
            }
            _ => Err(TGVError::RegisterError(format!(
                "Invalid command mode input: {:?}",
                key_event
            ))),
        }
    }
}

impl CommandModeRegister {
    /// Supported commands:
    /// :q: Quit.
    /// :h: Help.
    /// :1234: Go to position 1234 on the same contig.
    /// :12:1234: Go to position 1234 on contig 12.
    pub fn parse(&self) -> Result<Vec<StateMessage>, TGVError> {
        if self.input == "q" {
            return Ok(vec![StateMessage::Quit]);
        }

        if self.input == "h" {
            return Err(TGVError::RegisterError(
                "TODO: help screen is not implemented".to_string(),
            ));
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
                Err(_) => Err(TGVError::RegisterError(format!(
                    "Invalid command mode input: {}",
                    self.input
                ))),
            },
            _ => Err(TGVError::RegisterError(format!(
                "Invalid command mode input: {}",
                self.input
            ))),
        }
    }
}

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
                let total_n_contigs = state.contigs.all_data().len();
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

#[cfg(test)]
mod tests {

    use super::*;
    use crate::message::StateMessage;
    use rstest::rstest;

    #[rstest]
    #[case("q", Ok(vec![StateMessage::Quit]))]
    #[case("1234", Ok(vec![StateMessage::GotoCoordinate(1234)]))]
    #[case("chr1:1000", Ok(vec![StateMessage::GotoContigCoordinate(
        "chr1".to_string(),
        1000,
    )]))]
    #[case("17:7572659", Ok(vec![StateMessage::GotoContigCoordinate(
        "17".to_string(),
        7572659,
    )]))]
    #[case("TP53", Ok(vec![StateMessage::GoToGene("TP53".to_string())]))]
    #[case("invalid:command:format", Err(TGVError::RegisterError("Invalid command mode input: invalid:command:format".to_string())))]
    #[case("chr1:invalid", Err(TGVError::RegisterError("Invalid command mode input: chr1:invalid".to_string())))]
    fn test_command_parse(
        #[case] input: &str,
        #[case] expected: Result<Vec<StateMessage>, TGVError>,
    ) {
        let register = CommandModeRegister {
            input: input.to_string(),
            cursor_position: input.len(),
        };
        match (&register.parse(), &expected) {
            (Ok(result), Ok(expected)) => assert_eq!(result, expected),
            (Err(e), Err(expected)) => {} // OK
            _ => panic!(
                "Test failed.  result: {:?}, expected: {:?}",
                register.parse(),
                expected
            ),
        }
    }

    #[rstest]
    #[case("",'g', Ok(vec![]))]
    #[case("g",'g', Err(TGVError::RegisterError("Invalid input: g".to_string())))]
    #[case("",'1', Ok(vec![]))]
    #[case("g",'1', Err(TGVError::RegisterError("Invalid input: g".to_string())))]
    #[case("", 'w', Ok(vec![StateMessage::GotoNextExonsStart(1)]))]
    #[case("", 'b', Ok(vec![StateMessage::GotoPreviousExonsStart(1)]))]
    #[case("", 'e', Ok(vec![StateMessage::GotoNextExonsEnd(1)]))]
    #[case("", 'h', Ok(vec![StateMessage::MoveLeft(1)]))]
    #[case("", 'l', Ok(vec![StateMessage::MoveRight(1)]))]
    #[case("", 'j', Ok(vec![StateMessage::MoveDown(1)]))]
    #[case("", 'k', Ok(vec![StateMessage::MoveUp(1)]))]
    #[case("", 'z', Ok(vec![StateMessage::ZoomIn(2)]))]
    #[case("", 'o', Ok(vec![StateMessage::ZoomOut(2)]))]
    #[case("", '{', Ok(vec![StateMessage::GotoPreviousContig(1)]))]
    #[case("", '}', Ok(vec![StateMessage::GotoNextContig(1)]))]
    #[case("g", 'e', Ok(vec![StateMessage::GotoPreviousExonsEnd(1)]))]
    #[case("g", 'E', Ok(vec![StateMessage::GotoPreviousGenesEnd(1)]))]
    #[case("3", 'w', Ok(vec![StateMessage::GotoNextExonsStart(3)]))]
    #[case("5", 'l', Ok(vec![StateMessage::MoveRight(5)]))]
    #[case("10", 'z', Ok(vec![StateMessage::ZoomIn(20)]))]
    #[case("", 'x', Err(TGVError::RegisterError("Invalid normal mode input: x".to_string())))]
    #[case("g", 'x', Err(TGVError::RegisterError("Invalid normal mode input: gx".to_string())))]
    #[case("3", 'x', Err(TGVError::RegisterError("Invalid normal mode input: 3x".to_string())))]
    #[case("3g", 'x', Err(TGVError::RegisterError("Invalid normal mode input: 3gx".to_string())))]
    fn test_normal_mode_translate(
        #[case] existing_buffer: &str,
        #[case] key: char,
        #[case] expected: Result<Vec<StateMessage>, TGVError>,
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

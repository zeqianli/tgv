//! The vim key grammar of normal and insert mode.
//!
//! The structure follows vim's own: [`PendingKeys`] holds a command while it is typed, like
//! vim's `cmdarg_T` and `oparg_T`. That is a count, a prefix (`g` or `Ctrl-w`), and a pending
//! operator with the count typed before it. Motions only describe where the cursor goes. `App`
//! resolves them against the focused track, and applies an operator to the range between the
//! cursor and the motion's target, as vim's `do_pending_operator` does.
//!
//! Insert mode types no text, so it takes vim's normal-mode editing grammar: counts, `x`, `X`,
//! and `d{motion}`. `Space` is the only typing key, and it paints.

use crate::message::{
    Action, ColumnSpan, CursorAfter, Direction, FocusMove, Motion, Operator, UpdateLayoutAction,
};
use crate::register::KeyRegisterType;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use gv_core::bed::BedEditKind;
use gv_core::message::{Scroll, WordMotion, Zoom};
use gv_core::prelude::*;

const LARGE_HORIZONTAL_STEP: u64 = 30;
const LARGE_VERTICAL_STEP: usize = 30;
const ZOOM_STEP: u64 = 2;

/// The grammar a key is read with. Insert mode edits one BED track.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Mode {
    Normal,
    Insert(TrackId),
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum Prefix {
    G,
    CtrlW,
}

/// A command while it is typed.
#[derive(Debug, Default)]
pub struct PendingKeys {
    count: Option<usize>,
    prefix: Option<Prefix>,
    /// The operator waiting for its motion, with the count typed before it.
    operator: Option<(Operator, Option<usize>)>,
    /// The keys typed so far, for error messages, such as `3g`.
    typed: String,
}

impl PendingKeys {
    pub fn is_empty(&self) -> bool {
        self.typed.is_empty()
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Adds a key. Returns the actions of a completed command, or nothing while the command is
    /// still being typed. A completed or invalid command clears the pending keys.
    pub fn push(&mut self, key: KeyEvent, mode: Mode) -> Result<Vec<Action>, TGVError> {
        self.typed.push_str(&key_name(key));
        let result = self.interpret(key, mode);
        if !matches!(result, Ok(None)) {
            self.clear();
        }
        result.map(Option::unwrap_or_default)
    }

    /// The count typed before the operator, times the count typed after it.
    fn total_count(&self) -> usize {
        let operator_count = self.operator.and_then(|(_, count)| count);
        operator_count.unwrap_or(1) * self.count.unwrap_or(1)
    }

    fn invalid(&self, mode: Mode) -> TGVError {
        let mode = match mode {
            Mode::Normal => "normal",
            Mode::Insert(_) => "insert",
        };
        TGVError::RegisterError(format!("Invalid {mode} mode input: {}", self.typed))
    }

    fn interpret(&mut self, key: KeyEvent, mode: Mode) -> Result<Option<Vec<Action>>, TGVError> {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        let code = match key.code {
            KeyCode::Left => KeyCode::Char('h'),
            KeyCode::Right => KeyCode::Char('l'),
            KeyCode::Up => KeyCode::Char('k'),
            KeyCode::Down => KeyCode::Char('j'),
            code => code,
        };

        if code == KeyCode::Esc {
            // As in vim, the first Esc only cancels pending input.
            let pending = self.typed.len() > key_name(key).len();
            return Ok(Some(match mode {
                Mode::Insert(_) if !pending => {
                    vec![Action::SwitchKeyRegister(KeyRegisterType::Normal)]
                }
                _ => Vec::new(),
            }));
        }

        let count = self.total_count();
        match self.prefix.take() {
            Some(Prefix::CtrlW) => {
                let movement = match code {
                    KeyCode::Char('j') => FocusMove::Next(count),
                    KeyCode::Char('k') => FocusMove::Previous(count),
                    KeyCode::Char('w') => FocusMove::NextWrap,
                    KeyCode::Char('W') => FocusMove::PreviousWrap,
                    KeyCode::Char('t') => FocusMove::First,
                    KeyCode::Char('b') => FocusMove::Last,
                    _ => return Err(self.invalid(mode)),
                };
                return Ok(Some(vec![Action::MoveFocus(movement)]));
            }
            Some(Prefix::G) => {
                let motion = match code {
                    KeyCode::Char('e') => WordMotion::PreviousEnd,
                    KeyCode::Char('E') => WordMotion::PreviousEnd,
                    KeyCode::Char('g') | KeyCode::Char('G') if self.operator.is_none() => {
                        return Ok(Some(match mode {
                            Mode::Normal if code == KeyCode::Char('g') => {
                                vec![Scroll::Top.into()]
                            }
                            Mode::Normal => vec![Scroll::Bottom.into()],
                            // Insert mode stays on one track.
                            Mode::Insert(_) => Vec::new(),
                        }));
                    }
                    _ => return Err(self.invalid(mode)),
                };
                let big = code == KeyCode::Char('E');
                return self.motion(mode, Motion::Word { motion, big, count });
            }
            None => {}
        }

        if control {
            return match code {
                KeyCode::Char('j') => Ok(Some(vec![Action::MoveFocus(FocusMove::Next(count))])),
                KeyCode::Char('k') => Ok(Some(vec![Action::MoveFocus(FocusMove::Previous(count))])),
                KeyCode::Char('w') => {
                    self.prefix = Some(Prefix::CtrlW);
                    Ok(None)
                }
                // Like undo and redo in vim, `u` and `Ctrl-r` move through the jump history in
                // normal mode, and through the edit history in insert mode.
                KeyCode::Char('r') if self.operator.is_none() => Ok(Some(vec![match mode {
                    Mode::Normal => Action::JumpForward,
                    Mode::Insert(track) => Action::Redo(track),
                }])),
                _ => Err(self.invalid(mode)),
            };
        }

        if let KeyCode::Char(digit @ '0'..='9') = code
            && (digit != '0' || self.count.is_some())
        {
            let digit = digit.to_digit(10).expect("the key is a digit") as usize;
            self.count = Some(
                self.count
                    .unwrap_or(0)
                    .saturating_mul(10)
                    .saturating_add(digit),
            );
            return Ok(None);
        }

        let columns = count as u64;
        let motion = match code {
            KeyCode::Char('g') => {
                self.prefix = Some(Prefix::G);
                return Ok(None);
            }
            KeyCode::Char('h') => Some(Motion::Columns {
                direction: Direction::Left,
                count: columns,
            }),
            KeyCode::Char('l') => Some(Motion::Columns {
                direction: Direction::Right,
                count: columns,
            }),
            KeyCode::Char('H') => Some(Motion::Columns {
                direction: Direction::Left,
                count: columns * LARGE_HORIZONTAL_STEP,
            }),
            KeyCode::Char('L') => Some(Motion::Columns {
                direction: Direction::Right,
                count: columns * LARGE_HORIZONTAL_STEP,
            }),
            KeyCode::Char(char @ ('w' | 'e' | 'b' | 'W' | 'E' | 'B')) => Some(Motion::Word {
                motion: match char.to_ascii_lowercase() {
                    'w' => WordMotion::NextStart,
                    'e' => WordMotion::NextEnd,
                    _ => WordMotion::PreviousStart,
                },
                big: char.is_ascii_uppercase(),
                count,
            }),
            _ => None,
        };
        if let Some(motion) = motion {
            return self.motion(mode, motion);
        }
        if self.operator.is_some() {
            return Err(self.invalid(mode));
        }

        let without_count = self.count.is_none();
        let actions = match (code, mode) {
            (KeyCode::Char('j' | 'k' | 'J' | 'K'), Mode::Insert(_)) => Vec::new(),
            (KeyCode::Char('j'), Mode::Normal) => vec![Scroll::Down(count).into()],
            (KeyCode::Char('k'), Mode::Normal) => vec![Scroll::Up(count).into()],
            (KeyCode::Char('J'), Mode::Normal) => {
                vec![Scroll::Down(count * LARGE_VERTICAL_STEP).into()]
            }
            (KeyCode::Char('K'), Mode::Normal) => {
                vec![Scroll::Up(count * LARGE_VERTICAL_STEP).into()]
            }
            (KeyCode::Char('z'), _) => vec![Action::Core(Zoom::In(ZOOM_STEP * columns).into())],
            (KeyCode::Char('o'), _) => vec![Action::Core(Zoom::Out(ZOOM_STEP * columns).into())],
            (KeyCode::Char('s'), _) if without_count => {
                vec![Action::UpdateLayout(UpdateLayoutAction::ToggleSidebar)]
            }
            (KeyCode::Char(':'), _) => vec![
                Action::ClearAllKeyRegisters,
                Action::SwitchKeyRegister(KeyRegisterType::Command),
            ],
            (KeyCode::Char('/'), _) => vec![
                Action::ClearAllKeyRegisters,
                Action::SwitchKeyRegister(KeyRegisterType::Search),
            ],
            (KeyCode::Char('u'), Mode::Normal) if without_count => vec![Action::JumpBack],
            (KeyCode::Char(char @ ('i' | 'a')), Mode::Normal) => {
                vec![Action::EnterInsertMode {
                    append: char == 'a',
                }]
            }
            // Insert mode is already on.
            (KeyCode::Char('i' | 'a'), Mode::Insert(_)) => Vec::new(),
            (KeyCode::Char('u'), Mode::Insert(track)) if without_count => {
                vec![Action::Undo(track)]
            }
            (KeyCode::Char('d'), Mode::Insert(_)) => {
                self.operator = Some((Operator::Erase, self.count.take()));
                return Ok(None);
            }
            (KeyCode::Char(' '), Mode::Insert(track)) => vec![Action::EditColumns {
                track,
                edit: BedEditKind::Add,
                span: ColumnSpan::Right(columns),
                cursor: CursorAfter::PastSpan,
                typing: true,
            }],
            (KeyCode::Backspace, Mode::Insert(track)) => vec![Action::EditColumns {
                track,
                // Shift-Backspace paints to the left, the mirror of Backspace.
                edit: if key.modifiers.contains(KeyModifiers::SHIFT) {
                    BedEditKind::Add
                } else {
                    BedEditKind::Remove
                },
                span: ColumnSpan::Left(columns),
                cursor: CursorAfter::SpanStart,
                typing: true,
            }],
            (KeyCode::Char('x') | KeyCode::Delete, Mode::Insert(track)) => {
                vec![Action::EditColumns {
                    track,
                    edit: BedEditKind::Remove,
                    span: ColumnSpan::Right(columns),
                    cursor: CursorAfter::SpanStart,
                    typing: false,
                }]
            }
            (KeyCode::Char('X'), Mode::Insert(track)) => vec![Action::EditColumns {
                track,
                edit: BedEditKind::Remove,
                span: ColumnSpan::Left(columns),
                cursor: CursorAfter::SpanStart,
                typing: false,
            }],
            _ => return Err(self.invalid(mode)),
        };
        Ok(Some(actions))
    }

    /// A motion moves the cursor, or completes the pending operator.
    fn motion(&mut self, mode: Mode, motion: Motion) -> Result<Option<Vec<Action>>, TGVError> {
        match (self.operator, mode) {
            (Some((operator, _)), Mode::Insert(track)) => Ok(Some(vec![Action::Operate {
                track,
                operator,
                motion,
            }])),
            (Some(_), Mode::Normal) => Err(self.invalid(mode)),
            (None, _) => Ok(Some(vec![Action::Motion(motion)])),
        }
    }
}

/// How a key shows in messages: `w`, `^w`, or `<BS>`.
fn key_name(key: KeyEvent) -> String {
    match key.code {
        KeyCode::Char(char) if key.modifiers.contains(KeyModifiers::CONTROL) => format!("^{char}"),
        KeyCode::Char(' ') => "<Space>".to_string(),
        KeyCode::Char(char) => char.to_string(),
        KeyCode::Backspace if key.modifiers.contains(KeyModifiers::SHIFT) => "<S-BS>".to_string(),
        KeyCode::Backspace => "<BS>".to_string(),
        KeyCode::Delete => "<Del>".to_string(),
        KeyCode::Esc => "<Esc>".to_string(),
        KeyCode::Left => "<Left>".to_string(),
        KeyCode::Right => "<Right>".to_string(),
        KeyCode::Up => "<Up>".to_string(),
        KeyCode::Down => "<Down>".to_string(),
        code => format!("<{code}>"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn key(name: &str) -> KeyEvent {
        let (modifiers, rest) = match name.strip_prefix('^') {
            Some(rest) if !rest.is_empty() => (KeyModifiers::CONTROL, rest),
            _ => (KeyModifiers::NONE, name),
        };
        let code = match rest {
            "<Space>" => KeyCode::Char(' '),
            "<BS>" => KeyCode::Backspace,
            "<S-BS>" => return KeyEvent::new(KeyCode::Backspace, KeyModifiers::SHIFT),
            "<Del>" => KeyCode::Delete,
            "<Esc>" => KeyCode::Esc,
            "<Left>" => KeyCode::Left,
            rest => KeyCode::Char(rest.chars().next().unwrap()),
        };
        KeyEvent::new(code, modifiers)
    }

    /// Types the keys, separated by spaces, and returns the last key's result.
    fn type_keys(keys: &str, mode: Mode) -> Result<Vec<Action>, String> {
        let mut pending = PendingKeys::default();
        let mut result = Ok(Vec::new());
        for name in keys.split(' ') {
            result = pending
                .push(key(name), mode)
                .map_err(|error| error.to_string());
        }
        result
    }

    fn columns(direction: Direction, count: u64) -> Action {
        Action::Motion(Motion::Columns { direction, count })
    }

    fn word(motion: WordMotion, big: bool, count: usize) -> Motion {
        Motion::Word { motion, big, count }
    }

    fn invalid(mode: &str, typed: &str) -> Result<Vec<Action>, String> {
        Err(format!(
            "Register error: Invalid {mode} mode input: {typed}"
        ))
    }

    #[rstest]
    #[case("g", Ok(vec![]))]
    #[case("g g", Ok(vec![Scroll::Top.into()]))]
    #[case("g G", Ok(vec![Scroll::Bottom.into()]))]
    #[case("1", Ok(vec![]))]
    #[case("g 1", invalid("normal", "g1"))]
    #[case("w", Ok(vec![Action::Motion(word(WordMotion::NextStart, false, 1))]))]
    #[case("b", Ok(vec![Action::Motion(word(WordMotion::PreviousStart, false, 1))]))]
    #[case("e", Ok(vec![Action::Motion(word(WordMotion::NextEnd, false, 1))]))]
    #[case("h", Ok(vec![columns(Direction::Left, 1)]))]
    #[case("l", Ok(vec![columns(Direction::Right, 1)]))]
    #[case("j", Ok(vec![Scroll::Down(1).into()]))]
    #[case("k", Ok(vec![Scroll::Up(1).into()]))]
    #[case("z", Ok(vec![Action::Core(Zoom::In(2).into())]))]
    #[case("o", Ok(vec![Action::Core(Zoom::Out(2).into())]))]
    #[case("g e", Ok(vec![Action::Motion(word(WordMotion::PreviousEnd, false, 1))]))]
    #[case("g E", Ok(vec![Action::Motion(word(WordMotion::PreviousEnd, true, 1))]))]
    #[case("3 w", Ok(vec![Action::Motion(word(WordMotion::NextStart, false, 3))]))]
    #[case("5 l", Ok(vec![columns(Direction::Right, 5)]))]
    #[case("1 0 z", Ok(vec![Action::Core(Zoom::In(20).into())]))]
    #[case("x", invalid("normal", "x"))]
    #[case("g x", invalid("normal", "gx"))]
    #[case("3 x", invalid("normal", "3x"))]
    #[case("3 g x", invalid("normal", "3gx"))]
    #[case("y", invalid("normal", "y"))]
    #[case("p", invalid("normal", "p"))]
    #[case("{", invalid("normal", "{"))]
    #[case("}", invalid("normal", "}"))]
    fn test_normal_mode(#[case] keys: &str, #[case] expected: Result<Vec<Action>, String>) {
        assert_eq!(type_keys(keys, Mode::Normal), expected);
    }
}

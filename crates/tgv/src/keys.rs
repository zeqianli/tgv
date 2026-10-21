//! Decodes terminal key events into the keys of the shared vim grammar in `gv_viewer::keys`.

use crossterm::event::{KeyCode as TerminalKeyCode, KeyEvent, KeyModifiers as TerminalModifiers};
use gv_viewer::keys::{Key, KeyCode, KeyModifiers};

/// The key a terminal key event stands for, or `None` for keys the grammar doesn't read, such
/// as function keys.
pub fn key_from_event(event: KeyEvent) -> Option<Key> {
    let code = match event.code {
        TerminalKeyCode::Char(char) => KeyCode::Char(char),
        TerminalKeyCode::Backspace => KeyCode::Backspace,
        TerminalKeyCode::Delete => KeyCode::Delete,
        TerminalKeyCode::Enter => KeyCode::Enter,
        TerminalKeyCode::Esc => KeyCode::Esc,
        TerminalKeyCode::Left => KeyCode::Left,
        TerminalKeyCode::Right => KeyCode::Right,
        TerminalKeyCode::Up => KeyCode::Up,
        TerminalKeyCode::Down => KeyCode::Down,
        TerminalKeyCode::Tab => KeyCode::Tab,
        TerminalKeyCode::Home => KeyCode::Home,
        TerminalKeyCode::End => KeyCode::End,
        TerminalKeyCode::PageUp => KeyCode::PageUp,
        TerminalKeyCode::PageDown => KeyCode::PageDown,
        _ => return None,
    };
    Some(Key::new(
        code,
        KeyModifiers {
            shift: event.modifiers.contains(TerminalModifiers::SHIFT),
            control: event.modifiers.contains(TerminalModifiers::CONTROL),
            alt: event.modifiers.contains(TerminalModifiers::ALT),
        },
    ))
}

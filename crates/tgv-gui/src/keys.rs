//! Decodes window keystrokes into the keys of the shared vim grammar in `gv_viewer::keys`.

use gv_viewer::keys::{Key, KeyCode, KeyModifiers};

/// The key a keystroke stands for, or `None` for keys the grammar doesn't read, such as
/// function keys and Command shortcuts, which the window handles itself.
pub fn key_from_keystroke(keystroke: &gpui::Keystroke) -> Option<Key> {
    let modifiers = keystroke.modifiers;
    if modifiers.platform {
        return None;
    }
    let code = match keystroke.key.as_str() {
        "space" => KeyCode::Char(' '),
        "backspace" => KeyCode::Backspace,
        "delete" => KeyCode::Delete,
        "enter" => KeyCode::Enter,
        "escape" => KeyCode::Esc,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "tab" => KeyCode::Tab,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        key => {
            // The typed character includes Shift, such as `:` or `G`. Control and Option
            // change it, so those use the key itself, as in `^r`.
            let typed = keystroke
                .key_char
                .as_deref()
                .filter(|_| !modifiers.control && !modifiers.alt);
            let mut chars = typed.unwrap_or(key).chars();
            let (Some(char), None) = (chars.next(), chars.next()) else {
                return None;
            };
            if typed.is_none() && modifiers.shift {
                KeyCode::Char(char.to_ascii_uppercase())
            } else {
                KeyCode::Char(char)
            }
        }
    };
    Some(Key::new(
        code,
        KeyModifiers {
            shift: modifiers.shift,
            control: modifiers.control,
            alt: modifiers.alt,
        },
    ))
}

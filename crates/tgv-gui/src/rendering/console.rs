use super::prelude::*;
use crate::rendering::{TrackFocus, colors::CURSOR_COLOR};
use gv_viewer::keyboard::{InputMode, KeyboardState};

/// Render the command line for command (`:`) and search (`/`) mode.
pub fn render_console(
    area: &Rect,
    keyboard_state: &KeyboardState,
    window: &mut Window,
    cx: &mut App,
) -> Result<(), crate::Error> {
    let y = area.y + (area.height - LINE_HEIGHT) / 2.0;
    let prompt = if matches!(keyboard_state.mode, InputMode::Search(_)) {
        '/'
    } else {
        ':'
    };
    let before_cursor: String = keyboard_state
        .command
        .chars()
        .take(keyboard_state.command_cursor)
        .collect();
    let x = area.x + 4.0;
    let cursor_x = x + TextContext::default()
        .text(&format!("{prompt}{before_cursor}"))
        .width(window);
    let cursor_char = keyboard_state
        .command
        .chars()
        .nth(keyboard_state.command_cursor)
        .unwrap_or(' ');
    let cursor_width = TextContext::default()
        .text(&cursor_char.to_string())
        .width(window)
        .max(6.0);
    fill_rect(
        Rect::new(cursor_x, y, cursor_width, LINE_HEIGHT),
        to_hsla(CURSOR_COLOR),
        window,
    );
    TextContext::default()
        .text(&format!("{prompt}{}", keyboard_state.command))
        .color(TEXT)
        .paint(x, y, LINE_HEIGHT, window, cx)?;
    Ok(())
}

/// Shows the mode and the focused track in insert mode, as vim does:
/// `-- INSERT -- peaks.bed [+]`. Normal mode shows nothing, except that with the sidebar hidden
/// the focused track's name shows on the right, like vim's ruler, so the focus is never hidden.
pub fn render_mode_line(
    area: &Rect,
    keyboard_state: &KeyboardState,
    focus: &TrackFocus,
    sidebar_hidden: bool,
    palette: &Palette,
    window: &mut Window,
    cx: &mut App,
) -> Result<(), crate::Error> {
    let y = area.y + (area.height - LINE_HEIGHT) / 2.0;
    let name = if focus.modified {
        format!("{} [+]", focus.name)
    } else {
        focus.name.clone()
    };
    match keyboard_state.mode {
        InputMode::Editing(gv_viewer::keys::Mode::Insert(_)) => {
            TextContext::default()
                .text(&format!("-- INSERT -- {name}"))
                .color(palette.MODE_INSERT)
                .bold()
                .paint(area.x + 4.0, y, LINE_HEIGHT, window, cx)?;
        }
        _ if sidebar_hidden => {
            let width = TextContext::default().text(&name).width(window);
            TextContext::default().text(&name).color(MUTED_TEXT).paint(
                area.right() - width - 4.0,
                y,
                LINE_HEIGHT,
                window,
                cx,
            )?;
        }
        _ => {}
    }
    Ok(())
}

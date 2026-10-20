use crate::{
    register::{KeyRegisterType, Registers},
    rendering::TrackFocus,
};
use gv_core::prelude::*;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};

/// Render the command line for command (`:`) and search (`/`) mode.
const MIN_AREA_WIDTH: u16 = 2;
const MIN_AREA_HEIGHT: u16 = 1;
pub fn render_console(area: &Rect, buf: &mut Buffer, buffer: &Registers) -> Result<(), TGVError> {
    if area.width < MIN_AREA_WIDTH || area.height < MIN_AREA_HEIGHT {
        return Ok(());
    }

    let cursor_char = buffer
        .command
        .chars()
        .nth(buffer.command_cursor)
        .unwrap_or(' ');
    let cursor_char_position = 1usize.saturating_add(buffer.command_cursor);
    let cursor_char_style = Style::default().bg(Color::Red);

    buf.set_stringn(
        area.x,
        area.y,
        format!(
            "{}{}",
            if buffer.current == KeyRegisterType::Search {
                '/'
            } else {
                ':'
            },
            buffer.command
        ),
        area.width as usize,
        Style::default(),
    );
    if cursor_char_position < area.width as usize {
        buf.set_stringn(
            area.x + cursor_char_position as u16,
            area.y,
            cursor_char.to_string(),
            1,
            cursor_char_style,
        );
    }
    Ok(())
}

/// Shows the mode and the focused track in track and insert mode, as vim does:
/// `-- INSERT -- peaks.bed [+]`. Normal mode shows nothing.
pub fn render_mode_line(area: &Rect, buf: &mut Buffer, registers: &Registers, focus: &TrackFocus) {
    let mode = match registers.current {
        KeyRegisterType::Track => "-- TRACK --",
        KeyRegisterType::Insert { .. } => "-- INSERT --",
        _ => return,
    };
    let text = match &focus.name {
        Some(name) if focus.modified => format!("{mode} {name} [+]"),
        Some(name) => format!("{mode} {name}"),
        None => format!("{mode} (no tracks)"),
    };
    buf.set_stringn(
        area.x,
        area.y,
        text,
        area.width as usize,
        Style::default().add_modifier(Modifier::BOLD),
    );
}

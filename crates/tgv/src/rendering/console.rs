use crate::register::Registers;
use gv_core::error::TGVError;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
};

/// Render the command mode console.
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
        format!(":{}", buffer.command),
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

use crate::{error::TGVError, register::command::CommandBuffer};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
};

/// Render the command mode console.
const MIN_AREA_WIDTH: u16 = 2;
const MIN_AREA_HEIGHT: u16 = 1;
pub fn render_console(
    area: &Rect,
    buf: &mut Buffer,
    buffer: &CommandBuffer,
) -> Result<(), TGVError> {
    if area.width < MIN_AREA_WIDTH || area.height < MIN_AREA_HEIGHT {
        return Ok(());
    }

    let cursor_char = buffer
        .input
        .chars()
        .nth(buffer.cursor_position)
        .unwrap_or(' ');
    let cursor_char_position = area.x + 1 + buffer.cursor_position as u16;
    let cursor_char_style = Style::default().bg(Color::Red);

    buf.set_stringn(area.x, area.y, ":", area.width as usize, Style::default());
    buf.set_stringn(
        area.x + 1,
        area.y,
        &buffer.input,
        area.width as usize - 1,
        Style::default(),
    );
    buf.set_stringn(
        area.x + cursor_char_position,
        area.y,
        cursor_char.to_string(),
        area.width as usize - cursor_char_position as usize,
        cursor_char_style,
    );
    Ok(())
}

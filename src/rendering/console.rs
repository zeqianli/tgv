use crate::{error::TGVError, register::CommandModeRegister};
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
    register: &CommandModeRegister,
) -> Result<(), TGVError> {
    if area.width < MIN_AREA_WIDTH || area.height < MIN_AREA_HEIGHT {
        return Ok(());
    }

    let input = register.input();
    let cursor_position = register.cursor_position();

    let cursor_char = if cursor_position >= input.len() {
        ' '
    } else {
        input.chars().nth(cursor_position).unwrap()
    };
    let cursor_char_position = area.x + 1 + cursor_position as u16;
    let cursor_char_style = Style::default().bg(Color::Red);

    buf.set_stringn(area.x, area.y, ":", area.width as usize, Style::default());
    buf.set_stringn(
        area.x + 1,
        area.y,
        input,
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

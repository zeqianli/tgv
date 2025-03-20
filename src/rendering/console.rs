use crate::models::register::CommandModeRegister;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
};

/// Render the command mode console.
pub fn render_console(area: &Rect, buf: &mut Buffer, command_mode_register: &CommandModeRegister) {
    let input = command_mode_register.input();
    let cursor_position = command_mode_register.cursor_position();

    let cursor_char;
    if cursor_position >= input.len() {
        cursor_char = ' '
    } else {
        cursor_char = input.chars().nth(cursor_position).unwrap();
    }
    let cursor_char_position = area.x + 1 + cursor_position as u16;
    let cursor_char_style = Style::default().bg(Color::Red);

    buf.set_string(area.x, area.y, ":", Style::default());
    buf.set_string(area.x + 1, area.y, input, Style::default());
    buf.set_string(
        cursor_char_position,
        area.y,
        cursor_char.to_string(),
        cursor_char_style,
    );
}

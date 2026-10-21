use crate::{
    register::{KeyRegisterType, Registers},
    rendering::{TrackFocus, colors::Palette},
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

/// Shows the mode and the focused track in insert mode, as vim does:
/// `-- INSERT -- peaks.bed [+]`. Normal mode shows nothing, except that with the sidebar hidden
/// the focused track's name shows on the right, like vim's ruler, so the focus is never hidden.
pub fn render_mode_line(
    area: &Rect,
    buf: &mut Buffer,
    registers: &Registers,
    focus: &TrackFocus,
    sidebar_hidden: bool,
    palette: &Palette,
) {
    let name = if focus.modified {
        format!("{} [+]", focus.name)
    } else {
        focus.name.clone()
    };
    match registers.current {
        KeyRegisterType::Insert { .. } => buf.set_stringn(
            area.x,
            area.y,
            format!("-- INSERT -- {name}"),
            area.width as usize,
            Style::default()
                .fg(palette.MODE_INSERT)
                .add_modifier(Modifier::BOLD),
        ),
        _ if sidebar_hidden => {
            let width = name.chars().count().min(area.width as usize);
            buf.set_stringn(
                area.right() - width as u16,
                area.y,
                name,
                width,
                Style::default().fg(Color::Gray),
            )
        }
        _ => return,
    };
}

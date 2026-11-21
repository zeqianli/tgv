use crate::{
    layout::Rect,
    rendering::{
        LINE_HEIGHT, TextContext, TrackFocus,
        colors::{MUTED_TEXT, Palette, Rgb, TEXT, ToHsla},
        fill_rect,
    },
};
use gpui::{App, Window};
use gv_viewer::register::{KeyRegisterType, Registers};

/// The command line's cursor.
const CURSOR_COLOR: Rgb = Rgb::from_u32(0xcd3131);

/// Render the command line for command (`:`) and search (`/`) mode.
pub fn render_console(
    area: &Rect,
    registers: &Registers,
    window: &mut Window,
    cx: &mut App,
) -> Result<(), crate::Error> {
    let y = area.y + (area.height - LINE_HEIGHT) / 2.0;
    let prompt = if matches!(registers.current, KeyRegisterType::Search(_)) {
        '/'
    } else {
        ':'
    };
    let before_cursor: String = registers
        .command
        .chars()
        .take(registers.command_cursor)
        .collect();
    let x = area.x + 4.0;
    let cursor_x = x + TextContext::default()
        .text(&format!("{prompt}{before_cursor}"))
        .width(window);
    let cursor_char = registers
        .command
        .chars()
        .nth(registers.command_cursor)
        .unwrap_or(' ');
    let cursor_width = TextContext::default()
        .text(&cursor_char.to_string())
        .width(window)
        .max(6.0);
    fill_rect(
        Rect::new(cursor_x, y, cursor_width, LINE_HEIGHT),
        CURSOR_COLOR.hsla(),
        window,
    );
    TextContext::default()
        .text(&format!("{prompt}{}", registers.command))
        .color(TEXT)
        .paint(x, y, LINE_HEIGHT, window, cx)?;
    Ok(())
}

/// Shows the mode and the focused track in insert mode, as vim does:
/// `-- INSERT -- peaks.bed [+]`. Normal mode shows nothing, except that with the sidebar hidden
/// the focused track's name shows on the right, like vim's ruler, so the focus is never hidden.
pub fn render_mode_line(
    area: &Rect,
    registers: &Registers,
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
    match registers.current {
        KeyRegisterType::Editing(gv_viewer::keys::Mode::Insert(_)) => {
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

use crate::{
    layout::Rect,
    rendering::{
        LINE_HEIGHT, Painter, TrackFocus,
        colors::{MUTED_TEXT, Palette, Rgb, TEXT, ToHsla},
    },
};
use gv_viewer::register::{KeyRegisterType, Registers};

/// The command line's cursor.
const CURSOR_COLOR: Rgb = Rgb::from_u32(0xcd3131);

/// Render the command line for command (`:`) and search (`/`) mode.
pub fn render_console(painter: &mut Painter, area: &Rect, registers: &Registers) {
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
    let cursor_x = x + painter.text_width(&format!("{prompt}{before_cursor}"));
    let cursor_char = registers
        .command
        .chars()
        .nth(registers.command_cursor)
        .unwrap_or(' ');
    let cursor_width = painter.text_width(&cursor_char.to_string()).max(6.0);
    painter.fill(
        Rect::new(cursor_x, y, cursor_width, LINE_HEIGHT),
        CURSOR_COLOR.hsla(),
    );
    painter.text(x, y, &format!("{prompt}{}", registers.command), TEXT);
}

/// Shows the mode and the focused track in insert mode, as vim does:
/// `-- INSERT -- peaks.bed [+]`. Normal mode shows nothing, except that with the sidebar hidden
/// the focused track's name shows on the right, like vim's ruler, so the focus is never hidden.
pub fn render_mode_line(
    painter: &mut Painter,
    area: &Rect,
    registers: &Registers,
    focus: &TrackFocus,
    sidebar_hidden: bool,
    palette: &Palette,
) {
    let y = area.y + (area.height - LINE_HEIGHT) / 2.0;
    let name = if focus.modified {
        format!("{} [+]", focus.name)
    } else {
        focus.name.clone()
    };
    match registers.current {
        KeyRegisterType::Editing(gv_viewer::keys::Mode::Insert(_)) => {
            painter.bold_text(
                area.x + 4.0,
                y,
                &format!("-- INSERT -- {name}"),
                palette.MODE_INSERT,
            );
        }
        _ if sidebar_hidden => {
            let width = painter.text_width(&name);
            painter.text(area.right() - width - 4.0, y, &name, MUTED_TEXT);
        }
        _ => {}
    }
}

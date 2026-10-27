use crate::{
    layout::Rect,
    rendering::{
        LINE_HEIGHT, Painter, Status,
        colors::{MUTED_TEXT, TEXT},
    },
};
use gv_core::prelude::*;

/// Shows the data under the pointer, that the window waits for data, or the messages.
pub fn render_status_bar(painter: &mut Painter, area: &Rect, state: &State, status: &Status) {
    let y = area.y + (area.height - LINE_HEIGHT) / 2.0;
    let x = area.x + 4.0;
    if let Some(hover) = status.hover {
        painter.text(x, y, hover, TEXT);
    } else if status.waiting {
        painter.text(x, y, "Loading…", MUTED_TEXT);
    } else if !state.messages.is_empty() {
        painter.text(x, y, &state.messages.join("  "), TEXT);
    }
}

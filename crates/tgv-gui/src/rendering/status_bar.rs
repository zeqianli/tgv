use crate::{
    layout::Rect,
    rendering::{
        LINE_HEIGHT, Status, TextContext,
        colors::{MUTED_TEXT, TEXT},
    },
};
use gpui::{App, Window};
use gv_core::prelude::*;

/// Shows the data under the pointer, that the window waits for data, or the messages.
pub fn render_status_bar(
    area: &Rect,
    state: &State,
    status: &Status,
    window: &mut Window,
    cx: &mut App,
) -> Result<(), crate::Error> {
    let y = area.y + (area.height - LINE_HEIGHT) / 2.0;
    let x = area.x + 4.0;
    if let Some(hover) = status.hover {
        TextContext::default()
            .text(hover)
            .color(TEXT)
            .paint(x, y, LINE_HEIGHT, window, cx)?;
    } else if status.waiting {
        TextContext::default()
            .text("Loading…")
            .color(MUTED_TEXT)
            .paint(x, y, LINE_HEIGHT, window, cx)?;
    } else if !state.messages.is_empty() {
        TextContext::default()
            .text(&state.messages.join("  "))
            .color(TEXT)
            .paint(x, y, LINE_HEIGHT, window, cx)?;
    }
    Ok(())
}

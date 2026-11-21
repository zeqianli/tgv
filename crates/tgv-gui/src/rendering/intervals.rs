use crate::{
    layout::{PixelView, Rect},
    rendering::{
        LINE_HEIGHT, TextContext, colors::MUTED_TEXT, colors::Rgb, colors::to_hsla, fill_rect,
    },
};
use gpui::{App, Window};
use gv_core::variant::Variant;

/// Simple rendering of intervals, with alternating colors.
/// The upstream code is responsible to pass only relevant intervals to this function.
pub fn render_simple_intervals(
    area: &Rect,
    intervals: &[Variant],
    view: &PixelView,
    colors: &[Rgb],
    window: &mut Window,
) {
    for (index, interval) in intervals.iter().enumerate() {
        if let Some((x, width)) = view.span(interval.start, interval.end, area) {
            fill_rect(
                Rect::new(x, area.y + 2.0, width, area.height - 4.0),
                to_hsla(colors[index % colors.len()]),
                window,
            );
        }
    }
}

/// Says that the track isn't loaded for this view, because the view is zoomed out too far.
pub fn render_zoom_in_notice(
    area: &Rect,
    features: &str,
    window: &mut Window,
    cx: &mut App,
) -> Result<(), crate::Error> {
    TextContext::default()
        .text(&format!("Zoom in to view {features}"))
        .color(MUTED_TEXT)
        .paint(
            area.x + 4.0,
            area.y + (area.height.min(LINE_HEIGHT * 2.0) - LINE_HEIGHT) / 2.0,
            LINE_HEIGHT,
            window,
            cx,
        )?;
    Ok(())
}

use crate::layout::CellView;
use crate::layout::{AlignmentView, OnScreenCoordinate};
use gv_core::variant::Variant;

use ratatui::{buffer::Buffer, layout::Rect, style::Color, style::Style};

/// Simple rendering of intervals
/// The upstream code is responsible to pass only relevant intervals to this function.
pub fn render_simple_intervals(
    area: &Rect,
    buf: &mut Buffer,
    intervals: &[Variant],
    alignment_view: &AlignmentView,
    colors: &[Color],
    first_color_index: usize,
) {
    // TODO:
    // A better solution for overlapping intervals.

    let mut i_color = first_color_index;
    for interval in intervals {
        let onscreen_x = alignment_view.onscreen_x_coordinate(interval.start, area);
        let onscreen_y = alignment_view.onscreen_x_coordinate(interval.end, area);
        if let Some((x, length)) =
            OnScreenCoordinate::onscreen_start_and_length(&onscreen_x, &onscreen_y, area)
        {
            buf.set_string(
                area.x + x,
                area.y,
                " ".repeat(length as usize),
                Style::default().bg(colors[i_color]),
            );
        }
        i_color += 1;
        if i_color == colors.len() {
            i_color = 0;
        }
    }
}

/// Says that the track isn't loaded for this view, because the view is zoomed out too far.
pub fn render_zoom_in_notice(area: &Rect, buf: &mut Buffer, features: &str) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    buf.set_stringn(
        area.x,
        area.y,
        format!("zoom in to view {features}"),
        area.width as usize,
        Style::default().fg(Color::DarkGray),
    );
}

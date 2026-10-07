use crate::layout::{AlignmentView, OnScreenCoordinate};
use gv_core::intervals::IntervalSchema;
use gv_core::prelude::*;
use polars::prelude::DataFrame;

use ratatui::{buffer::Buffer, layout::Rect, style::Color, style::Style};

/// Simple rendering of intervals
/// The upstream code is responsible to pass only relevant intervals to this function.
pub fn render_simple_intervals(
    area: &Rect,
    buf: &mut Buffer,
    intervals: &DataFrame,
    alignment_view: &AlignmentView,
    colors: &[Color],
    first_color_index: usize,
) -> Result<(), TGVError> {
    // TODO:
    // A better solution for overlapping intervals.

    let mut i_color = first_color_index;
    let starts = intervals.column(IntervalSchema::START)?.u64()?;
    let ends = intervals.column(IntervalSchema::END)?.u64()?;
    for (start, end) in starts.into_no_null_iter().zip(ends.into_no_null_iter()) {
        let onscreen_x = alignment_view.onscreen_x_coordinate(start, area);
        let onscreen_y = alignment_view.onscreen_x_coordinate(end, area);
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

    Ok(())
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

use crate::{
    layout::{PixelView, Rect},
    rendering::{LINE_HEIGHT, Painter, colors::MUTED_TEXT, colors::Rgb, colors::ToHsla},
};
use gv_core::{intervals::IntervalSchema, prelude::*};
use polars::prelude::DataFrame;

/// Simple rendering of intervals, with alternating colors.
/// The upstream code is responsible to pass only relevant intervals to this function.
pub fn render_simple_intervals(
    painter: &mut Painter,
    area: &Rect,
    intervals: &DataFrame,
    view: &PixelView,
    colors: &[Rgb],
) -> Result<(), TGVError> {
    let starts = intervals.column(IntervalSchema::START)?.u64()?;
    let ends = intervals.column(IntervalSchema::END)?.u64()?;
    for (index, (start, end)) in starts
        .into_no_null_iter()
        .zip(ends.into_no_null_iter())
        .enumerate()
    {
        if let Some((x, width)) = view.span(start, end, area) {
            painter.fill(
                Rect::new(x, area.y + 2.0, width, area.height - 4.0),
                colors[index % colors.len()].hsla(),
            );
        }
    }
    Ok(())
}

/// Says that the track isn't loaded for this view, because the view is zoomed out too far.
pub fn render_zoom_in_notice(painter: &mut Painter, area: &Rect, features: &str) {
    painter.text(
        area.x + 4.0,
        area.y + (area.height.min(LINE_HEIGHT * 2.0) - LINE_HEIGHT) / 2.0,
        &format!("Zoom in to view {features}"),
        MUTED_TEXT,
    );
}

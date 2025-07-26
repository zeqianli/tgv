use crate::error::TGVError;

use crate::intervals::GenomeInterval;
use crate::{
    states::State,
    window::{OnScreenCoordinate, ViewingWindow},
};
use crossterm::style;
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::Color,
    style::Style,
};

/// Simple rendering of intervals
/// The upstream code is responsible to pass only relevant intervals to this function.
pub fn render_simple_intervals<T: GenomeInterval>(
    area: &Rect,
    buf: &mut Buffer,
    intervals: Vec<&T>,
    state: &State,
    color: Color,
) -> Result<(), TGVError> {
    // Cytoband

    // panic!("Current cytoband: {:?}", state.current_cytoband());
    for interval in intervals.iter() {
        let viewing_window = state.viewing_window()?;

        let onscreen_x = viewing_window.onscreen_x_coordinate(interval.start(), area);
        let onscreen_y = viewing_window.onscreen_x_coordinate(interval.end(), area);
        if let Some((x, length)) =
            OnScreenCoordinate::onscreen_start_and_length(&onscreen_x, &onscreen_y, area)
        {
            buf.set_string(
                area.x + x as u16,
                area.y,
                " ".repeat(length),
                Style::default().bg(color),
            );
        }
    }

    Ok(())
}

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
    colors: Vec<Color>, // alternate
    first_color_index: usize,
) -> Result<(), TGVError> {
    // Cytoband

    // panic!("Current cytoband: {:?}", state.current_cytoband());
    let mut i_color = first_color_index;
    for interval in intervals {
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

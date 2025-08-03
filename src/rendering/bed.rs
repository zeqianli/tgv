use crate::error::TGVError;

use crate::bed::BEDIntervals;
use crate::intervals::GenomeInterval;
use crate::{
    rendering::{self, intervals::render_simple_intervals},
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

pub fn render_bed(
    area: &Rect,
    buf: &mut Buffer,
    bed: &BEDIntervals,
    state: &State,
) -> Result<(), TGVError> {
    //panic!("{:?}", bed);
    let intervals = bed.intervals.overlapping(&state.viewing_region()?)?;
    if intervals.len() > 0 {
        let first_color_index = intervals[0].index % 2;
        render_simple_intervals(
            area,
            buf,
            intervals,
            state,
            vec![rendering::colors::BED1, rendering::colors::BED2],
            first_color_index,
        )?;
    }

    Ok(())
}

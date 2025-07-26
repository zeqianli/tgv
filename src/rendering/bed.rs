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
    render_simple_intervals(
        area,
        buf,
        bed.intervals.overlapping(&state.viewing_region()?)?,
        state,
        rendering::colors::BED,
    )
}

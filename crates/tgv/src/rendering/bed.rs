use crate::{
    layout::AlignmentView,
    rendering::{colors::Palette, intervals::render_simple_intervals},
};
use gv_core::{alignment::Alignment, bed::BEDRepository, error::TGVError, state::State};
use ratatui::{buffer::Buffer, layout::Rect};

pub fn render_bed(
    area: &Rect,
    buf: &mut Buffer,
    state: &State,
    alignment_view: &AlignmentView,
    pallete: &Palette,
) -> Result<(), TGVError> {
    let intervals = state
        .bed_intervals
        .overlapping(&alignment_view.region(area))?; // FIXME: wasteful calculation here
    if !intervals.is_empty() {
        let first_color_index = intervals[0].index % 2;
        render_simple_intervals(
            area,
            buf,
            intervals,
            state,
            vec![pallete.BED1, pallete.BED2],
            first_color_index,
        )?;
    }

    Ok(())
}

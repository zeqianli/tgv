use gv_core::{
    bed::BEDIntervals,
    error::TGVError,
    rendering::{colors::Palette, intervals::render_simple_intervals},
    state::State,
};
use ratatui::{buffer::Buffer, layout::Rect};

pub fn render_bed(
    area: &Rect,
    buf: &mut Buffer,
    bed: &BEDIntervals,
    state: &State,
    pallete: &Palette,
) -> Result<(), TGVError> {
    let intervals = bed.intervals.overlapping(&state.viewing_region())?;
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

use crate::{
    layout::AlignmentView,
    rendering::{colors::Palette, intervals::render_simple_intervals},
};
use gv_core::{
    bed::BedTrack,
    error::TGVError,
    intervals::GenomeInterval,
};
use ratatui::{buffer::Buffer, layout::Rect};

pub fn render_bed(
    area: &Rect,
    buf: &mut Buffer,
    bed_intervals: &BedTrack,
    alignment_view: &AlignmentView,
    pallete: &Palette,
) -> Result<(), TGVError> {
    let region = alignment_view.region(area);
    let intervals =
        bed_intervals.overlapping(region.contig_index(), region.start(), region.end())?; // Optimize?
    if !intervals.is_empty() {
        let first_color_index = intervals[0].index % 2;
        render_simple_intervals(
            area,
            buf,
            intervals,
            alignment_view,
            vec![pallete.BED1, pallete.BED2],
            first_color_index,
        )?;
    }

    Ok(())
}

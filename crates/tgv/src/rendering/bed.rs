//! BED and bigBed rendering through the shared interval display.

use crate::{
    layout::{AlignmentView, IntervalMode},
    rendering::{
        colors::Palette,
        intervals::{
            IntervalLabels, IntervalStyle, render_packed_intervals, render_zoom_in_notice,
        },
    },
};
use gv_core::{bed::BedTable, intervals::IntervalDisplayMode, prelude::*};
use ratatui::{buffer::Buffer, layout::Rect};

pub fn render_bed(
    area: &Rect,
    buf: &mut Buffer,
    bed: &BedTable,
    alignment_view: &AlignmentView,
    palette: &Palette,
    mode: IntervalDisplayMode,
) -> Result<(), TGVError> {
    let region = alignment_view.region(area);
    if !bed.has_complete_data(&region) {
        render_zoom_in_notice(area, buf, "BED features");
        return Ok(());
    }
    let rows = bed.query(region.contig_index(), region.start(), region.end())?;
    render_packed_intervals(
        area,
        buf,
        &rows,
        bed.lanes.count,
        alignment_view,
        IntervalStyle {
            mode,
            colors: [palette.BED1, palette.BED2],
            first_lane: match IntervalMode::from(mode) {
                IntervalMode::Expanded => bed
                    .lanes
                    .first_visible(IntervalMode::from(mode).visible_lanes(area.height)),
                IntervalMode::Collapsed => 0,
            },
            labels: IntervalLabels::Bed,
        },
    )
}

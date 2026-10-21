//! VCF and BCF rendering through the shared interval display.

use crate::{
    layout::AlignmentView,
    rendering::{
        colors::Palette,
        intervals::{
            IntervalLabels, IntervalStyle, render_packed_intervals, render_zoom_in_notice,
        },
    },
};
use gv_core::{
    intervals::{IntervalDisplayMode, lane_count},
    prelude::*,
    variant::VariantTable,
};
use ratatui::{buffer::Buffer, layout::Rect};

pub fn render_variants(
    area: &Rect,
    buf: &mut Buffer,
    variants: &VariantTable,
    alignment_view: &AlignmentView,
    palette: &Palette,
    mode: IntervalDisplayMode,
) -> Result<(), TGVError> {
    let region = alignment_view.region(area);
    if !variants.has_complete_data(&region) {
        render_zoom_in_notice(area, buf, "variants");
        return Ok(());
    }
    let rows = variants.query(region.contig_index(), region.start(), region.end())?;
    render_packed_intervals(
        area,
        buf,
        &rows,
        lane_count(&variants.data)?,
        alignment_view,
        IntervalStyle {
            mode,
            colors: [palette.VCF1, palette.VCF2],
            first_lane: 0,
            labels: IntervalLabels::Variant,
        },
    )
}

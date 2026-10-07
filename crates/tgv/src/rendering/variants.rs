use crate::{
    layout::AlignmentView,
    rendering::{
        colors::Palette,
        intervals::{render_simple_intervals, render_zoom_in_notice},
    },
};
use gv_core::{prelude::*, variant::VariantTable};
use ratatui::{buffer::Buffer, layout::Rect};

pub fn render_variants(
    area: &Rect,
    buf: &mut Buffer,
    variants: &VariantTable,
    alignment_view: &AlignmentView,
    palette: &Palette,
) -> Result<(), TGVError> {
    let region = alignment_view.region(area);
    if !variants.has_complete_data(&region) {
        render_zoom_in_notice(area, buf, "variants");
        return Ok(());
    }
    let rows = variants.query(region.contig_index(), region.start(), region.end())?;
    // Colors alternate by on-screen order, because row IDs of indexed files change between
    // loads.
    render_simple_intervals(
        area,
        buf,
        &rows,
        alignment_view,
        &[palette.VCF1, palette.VCF2],
        0,
    )
}

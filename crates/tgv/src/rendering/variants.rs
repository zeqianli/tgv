use crate::rendering::colors::ToColor;
use crate::{
    layout::AlignmentView,
    rendering::{
        colors::Palette,
        intervals::{render_simple_intervals, render_zoom_in_notice},
    },
};
use gv_core::variant::VariantTable;
use ratatui::{buffer::Buffer, layout::Rect};

pub fn render_variants(
    area: &Rect,
    buf: &mut Buffer,
    variants: &VariantTable,
    alignment_view: &AlignmentView,
    palette: &Palette,
) -> Result<(), crate::Error> {
    let region = alignment_view.region(f64::from(area.width));
    if !variants.has_complete_data(&region) {
        render_zoom_in_notice(area, buf, "variants");
        return Ok(());
    }
    let variants = variants.variants(region.contig_index(), region.start(), region.end())?;
    // Colors alternate by on-screen order, because row IDs of indexed files change between
    // loads.
    render_simple_intervals(
        area,
        buf,
        &variants,
        alignment_view,
        &[palette.VCF1.color(), palette.VCF2.color()],
        0,
    );
    Ok(())
}

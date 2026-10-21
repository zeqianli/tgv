use crate::{
    layout::{PixelView, Rect},
    rendering::{
        Painter,
        colors::Palette,
        intervals::{render_simple_intervals, render_zoom_in_notice},
    },
};
use gv_core::{prelude::*, variant::VariantTable};

pub fn render_variants(
    painter: &mut Painter,
    area: &Rect,
    variants: &VariantTable,
    view: &PixelView,
    palette: &Palette,
) -> Result<(), TGVError> {
    let region = view.region(area);
    if !variants.has_complete_data(&region) {
        render_zoom_in_notice(painter, area, "variants");
        return Ok(());
    }
    let rows = variants.query(region.contig_index(), region.start(), region.end())?;
    // Colors alternate by on-screen order, because row IDs of indexed files change between
    // loads.
    render_simple_intervals(painter, area, &rows, view, &[palette.VCF1, palette.VCF2])
}

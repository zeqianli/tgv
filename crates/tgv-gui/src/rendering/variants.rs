use crate::{
    layout::{PixelView, Rect},
    rendering::{
        colors::Palette,
        intervals::{render_simple_intervals, render_zoom_in_notice},
    },
};
use gpui::{App, Window};
use gv_core::{prelude::*, variant::VariantTable};

pub fn render_variants(
    area: &Rect,
    variants: &VariantTable,
    view: &PixelView,
    palette: &Palette,
    window: &mut Window,
    cx: &mut App,
) -> Result<(), crate::Error> {
    let region = view.region(area);
    if !variants.has_complete_data(&region) {
        render_zoom_in_notice(area, "variants", window, cx)?;
        return Ok(());
    }
    let rows = variants.query(region.contig_index(), region.start(), region.end())?;
    // Colors alternate by on-screen order, because row IDs of indexed files change between
    // loads.
    render_simple_intervals(area, &rows, view, &[palette.VCF1, palette.VCF2], window)
}

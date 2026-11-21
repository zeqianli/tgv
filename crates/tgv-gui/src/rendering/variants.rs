use super::prelude::*;
use gv_core::variant::VariantTable;

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
    let variants = variants.variants(region.contig_index(), region.start(), region.end())?;
    // Colors alternate by on-screen order, because row IDs of indexed files change between
    // loads.
    let colors = [palette.VCF1, palette.VCF2];
    for (index, variant) in variants.iter().enumerate() {
        if let Some((x, width)) = view.span(variant.start, variant.end, area) {
            fill_rect(
                Rect::new(x, area.y + 2.0, width, area.height - 4.0),
                to_hsla(colors[index % colors.len()]),
                window,
            );
        }
    }
    Ok(())
}

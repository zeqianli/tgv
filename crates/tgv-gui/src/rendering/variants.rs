use super::prelude::*;
use crate::layout::FEATURE_ROW_HEIGHT;
use gv_core::variant::VariantTable;
use gv_viewer::viewport::TrackView;

pub fn render_variants(
    area: &Rect,
    variants: &VariantTable,
    view: &PixelView,
    track_view: TrackView,
    palette: &Palette,
    window: &mut Window,
    cx: &mut App,
) -> Result<(), crate::Error> {
    let region = view.region(area);
    if !variants.has_complete_data(&region) {
        render_zoom_in_notice(area, "variants", window, cx)?;
        return Ok(());
    }
    let rows = track_view.visible_rows((area.height / FEATURE_ROW_HEIGHT).ceil() as usize);
    for variant in variants.variants(
        region.contig_index(),
        region.start(),
        region.end(),
        rows.clone(),
    )? {
        if let Some((x, width)) = view.span(variant.start, variant.end, area) {
            let row_top = area.y + (variant.y - rows.start) as f32 * FEATURE_ROW_HEIGHT;
            fill_rect(
                Rect::new(x, row_top + 2.0, width, FEATURE_ROW_HEIGHT - 4.0),
                to_hsla(palette.VARIANT),
                window,
            );
        }
    }
    Ok(())
}

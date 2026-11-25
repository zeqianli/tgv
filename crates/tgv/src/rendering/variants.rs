use crate::layout::{AlignmentView, CellView, OnScreenCoordinate};
use crate::rendering::colors::to_color;
use crate::rendering::{colors::Palette, render_zoom_in_notice};
use gv_core::variant::VariantTable;
use gv_viewer::viewport::TrackView;
use ratatui::{buffer::Buffer, layout::Rect, style::Style};

pub fn render_variants(
    area: &Rect,
    buf: &mut Buffer,
    variants: &VariantTable,
    alignment_view: &AlignmentView,
    track_view: TrackView,
    palette: &Palette,
) -> Result<(), crate::Error> {
    let region = alignment_view.region(f64::from(area.width));
    if !variants.has_complete_data(&region) {
        render_zoom_in_notice(area, buf, "variants");
        return Ok(());
    }
    let rows = track_view.visible_rows(area.height as usize);
    let style = Style::default().bg(to_color(palette.VARIANT));
    for variant in variants.variants(
        region.contig_index(),
        region.start(),
        region.end(),
        rows.clone(),
    )? {
        let onscreen_start = alignment_view.onscreen_x_coordinate(variant.start, area);
        let onscreen_end = alignment_view.onscreen_x_coordinate(variant.end, area);
        if let Some((x, length)) =
            OnScreenCoordinate::onscreen_start_and_length(&onscreen_start, &onscreen_end, area)
        {
            buf.set_string(
                area.x + x,
                area.y + (variant.y - rows.start) as u16,
                " ".repeat(length as usize),
                style,
            );
        }
    }
    Ok(())
}

use crate::layout::{AlignmentView, CellView, OnScreenCoordinate};
use crate::rendering::colors::to_color;
use crate::rendering::{colors::Palette, render_zoom_in_notice};
use gv_core::variant::VariantTable;
use ratatui::{buffer::Buffer, layout::Rect, style::Style};

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
    let colors = [to_color(palette.VCF1), to_color(palette.VCF2)];
    for (index, variant) in variants.iter().enumerate() {
        let onscreen_start = alignment_view.onscreen_x_coordinate(variant.start, area);
        let onscreen_end = alignment_view.onscreen_x_coordinate(variant.end, area);
        if let Some((x, length)) =
            OnScreenCoordinate::onscreen_start_and_length(&onscreen_start, &onscreen_end, area)
        {
            buf.set_string(
                area.x + x,
                area.y,
                " ".repeat(length as usize),
                Style::default().bg(colors[index % colors.len()]),
            );
        }
    }
    Ok(())
}

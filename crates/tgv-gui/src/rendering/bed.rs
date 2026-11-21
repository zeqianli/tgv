use crate::{
    layout::{PixelView, Rect},
    rendering::{
        LINE_HEIGHT, TextContext,
        colors::{Palette, TEXT, ToHsla},
        fill_rect,
        intervals::render_zoom_in_notice,
    },
};
use gpui::{App, Window};
use gv_core::{
    bed::{BedSchema, BedTable},
    prelude::*,
};

/// The space between strand chevrons in a feature.
const CHEVRON_SPACING: f32 = 24.0;

/// Draws each feature as a bar, with strand chevrons and its name when the name fits.
pub fn render_bed(
    area: &Rect,
    bed: &BedTable,
    view: &PixelView,
    palette: &Palette,
    window: &mut Window,
    cx: &mut App,
) -> Result<(), TGVError> {
    let region = view.region(area);
    if !bed.has_complete_data(&region) {
        render_zoom_in_notice(area, "BED features", window, cx)?;
        return Ok(());
    }
    let rows = bed.query(region.contig_index(), region.start(), region.end())?;
    let starts = rows.column(BedSchema::START)?.u64()?;
    let ends = rows.column(BedSchema::END)?.u64()?;
    let names = rows.column(BedSchema::NAME)?.str()?;
    let strands = rows.column(BedSchema::STRAND)?.str()?;
    let colors = [palette.BED1, palette.BED2];
    let text_y = area.y + (area.height - LINE_HEIGHT) / 2.0;
    for row in 0..rows.height() {
        let Some((x, width)) = view.span(
            starts.get(row).expect("BED starts are non-null"),
            ends.get(row).expect("BED ends are non-null"),
            area,
        ) else {
            continue;
        };
        // Colors alternate by on-screen order, because row IDs of indexed files change between
        // loads.
        fill_rect(
            Rect::new(x, area.y + 2.0, width, area.height - 4.0),
            colors[row % 2].hsla(),
            window,
        );
        let name = names.get(row).filter(|name| !name.is_empty());
        let name_width = name.map_or(0.0, |name| TextContext::default().text(name).width(window));
        if let Some(name) = name
            && name_width + 8.0 <= width
        {
            TextContext::default().text(name).color(TEXT).paint(
                x + (width - name_width) / 2.0,
                text_y,
                LINE_HEIGHT,
                window,
                cx,
            )?;
        } else if let Some(chevron) = match strands.get(row) {
            Some("+") => Some("›"),
            Some("-") => Some("‹"),
            _ => None,
        } {
            let mut chevron_x = x + CHEVRON_SPACING / 2.0;
            while chevron_x + 4.0 < x + width {
                TextContext::default().text(chevron).color(TEXT).paint(
                    chevron_x - 2.0,
                    text_y,
                    LINE_HEIGHT,
                    window,
                    cx,
                )?;
                chevron_x += CHEVRON_SPACING;
            }
        }
    }
    Ok(())
}

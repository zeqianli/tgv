use super::prelude::*;
use crate::rendering::intervals::render_zoom_in_notice;
use gv_core::{bed::BedTable, strand::Strand};

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
) -> Result<(), crate::Error> {
    let region = view.region(area);
    if !bed.has_complete_data(&region) {
        render_zoom_in_notice(area, "BED features", window, cx)?;
        return Ok(());
    }
    let colors = [palette.BED1, palette.BED2];
    let text_y = area.y + (area.height - LINE_HEIGHT) / 2.0;
    for (row, feature) in bed
        .features(region.contig_index(), region.start(), region.end())?
        .iter()
        .enumerate()
    {
        let Some((x, width)) = view.span(feature.start, feature.end, area) else {
            continue;
        };
        // Colors alternate by on-screen order, because row IDs of indexed files change between
        // loads.
        fill_rect(
            Rect::new(x, area.y + 2.0, width, area.height - 4.0),
            to_hsla(colors[row % 2]),
            window,
        );
        let name = feature.name.as_deref();
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
        } else if let Some(chevron) = match feature.strand {
            Some(Strand::Forward) => Some("›"),
            Some(Strand::Reverse) => Some("‹"),
            None => None,
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

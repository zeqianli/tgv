use crate::{
    layout::{PixelView, Rect},
    rendering::{
        LINE_HEIGHT, Painter,
        colors::{Palette, TEXT, ToHsla},
        intervals::render_zoom_in_notice,
    },
};
use gv_core::{
    bed::{BedSchema, BedTable},
    prelude::*,
};

/// The space between strand chevrons in a feature.
const CHEVRON_SPACING: f32 = 24.0;

/// Draws each feature as a bar, with strand chevrons and its name when the name fits.
pub fn render_bed(
    painter: &mut Painter,
    area: &Rect,
    bed: &BedTable,
    view: &PixelView,
    palette: &Palette,
) -> Result<(), TGVError> {
    let region = view.region(area);
    if !bed.has_complete_data(&region) {
        render_zoom_in_notice(painter, area, "BED features");
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
        painter.fill(
            Rect::new(x, area.y + 2.0, width, area.height - 4.0),
            colors[row % 2].hsla(),
        );
        let name = names.get(row).filter(|name| !name.is_empty());
        let name_width = name.map_or(0.0, |name| painter.text_width(name));
        if let Some(name) = name
            && name_width + 8.0 <= width
        {
            painter.text(x + (width - name_width) / 2.0, text_y, name, TEXT);
        } else if let Some(chevron) = match strands.get(row) {
            Some("+") => Some("›"),
            Some("-") => Some("‹"),
            _ => None,
        } {
            let mut chevron_x = x + CHEVRON_SPACING / 2.0;
            while chevron_x + 4.0 < x + width {
                painter.text(chevron_x - 2.0, text_y, chevron, TEXT);
                chevron_x += CHEVRON_SPACING;
            }
        }
    }
    Ok(())
}

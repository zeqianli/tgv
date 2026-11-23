use super::prelude::*;
use crate::layout::FEATURE_ROW_HEIGHT;
use gv_core::{bed::BedTable, strand::Strand};
use gv_viewer::viewport::TrackView;

/// The space between strand chevrons in a feature.
const CHEVRON_SPACING: f32 = 24.0;

/// Draws each feature as a bar, with strand chevrons and its name when the name fits.
pub fn render_bed(
    area: &Rect,
    bed: &BedTable,
    view: &PixelView,
    track_view: TrackView,
    palette: &Palette,
    window: &mut Window,
    cx: &mut App,
) -> Result<(), crate::Error> {
    let region = view.region(area);
    if !bed.has_complete_data(&region) {
        render_zoom_in_notice(area, "BED features", window, cx)?;
        return Ok(());
    }
    let rows = track_view.visible_rows((area.height / FEATURE_ROW_HEIGHT).ceil() as usize);
    for feature in bed.features(
        region.contig_index(),
        region.start(),
        region.end(),
        rows.clone(),
    )? {
        let Some((x, width)) = view.span(feature.start, feature.end, area) else {
            continue;
        };
        let row_top = area.y + (feature.y - rows.start) as f32 * FEATURE_ROW_HEIGHT;
        let text_y = row_top + (FEATURE_ROW_HEIGHT - LINE_HEIGHT) / 2.0;
        fill_rect(
            Rect::new(x, row_top + 2.0, width, FEATURE_ROW_HEIGHT - 4.0),
            to_hsla(palette.BED),
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

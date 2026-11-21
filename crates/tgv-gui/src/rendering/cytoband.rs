use crate::{
    layout::{PixelView, Rect},
    rendering::{
        LINE_HEIGHT, TextContext,
        colors::{Palette, TEXT, to_hsla},
        fill_rect,
    },
};
use gpui::{App, Window};
use gv_core::{cytoband::Stain, prelude::*};
use gv_viewer::prepare::coordinate::get_abbreviated_length_string;

/// Pixels right of the cytoband for the contig length label.
pub(crate) const CYTOBAND_TEXT_RIGHT_SPACING: f32 = 64.0;
const BAR_HEIGHT: f32 = 12.0;

/// Draws the contig's ideogram, with a box around the part the view shows.
pub fn render_cytobands(
    area: &Rect,
    state: &State,
    view: &PixelView,
    palette: &Palette,
    window: &mut Window,
    cx: &mut App,
) -> Result<(), crate::Error> {
    if area.width <= CYTOBAND_TEXT_RIGHT_SPACING + 1.0 {
        return Ok(());
    }
    let bar = Rect::new(
        area.x,
        area.y + (area.height - BAR_HEIGHT) / 2.0,
        area.width - CYTOBAND_TEXT_RIGHT_SPACING,
        BAR_HEIGHT,
    );
    let Some(contig_length) = state.contig_length(&view.viewport.focus)? else {
        return Ok(());
    };

    // Right label
    TextContext::default()
        .text(&get_abbreviated_length_string(contig_length))
        .color(TEXT)
        .paint(
            bar.right() + 8.0,
            area.y + (area.height - LINE_HEIGHT) / 2.0,
            LINE_HEIGHT,
            window,
            cx,
        )?;

    let bands = state.cytobands.bands(view.viewport.focus.contig_index)?;
    if !bands.is_empty() {
        let total_length = bands.iter().map(|band| band.end).max().unwrap_or(0).max(1) as f32;
        for band in &bands {
            let left = bar.x + (band.start - 1) as f32 / total_length * bar.width;
            let right = bar.x + band.end as f32 / total_length * bar.width;
            // The centromere is drawn pinched, as ideograms show it.
            let rect = if band.stain == Stain::Acen {
                Rect::new(left, bar.y + 3.0, right - left, bar.height - 6.0)
            } else {
                Rect::new(left, bar.y, right - left, bar.height)
            };
            fill_rect(
                rect,
                to_hsla(palette.cytoband_color(band.stain.clone())),
                window,
            );
        }
    } else {
        fill_rect(bar, to_hsla(palette.OTHER_COLOR), window);
    }

    // Box the current viewing window.
    let (left, right) = view.visible(area);
    let scale = |position: u64| bar.x + position as f32 / contig_length as f32 * bar.width;
    let (left, right) = (
        scale(left.saturating_sub(1)),
        scale(right).max(scale(left) + 2.0),
    );
    let color = to_hsla(palette.HIGHLIGHT_COLOR);
    let (top, bottom) = (bar.y - 3.0, bar.bottom() + 3.0);
    fill_rect(Rect::new(left, top, right - left, 2.0), color, window);
    fill_rect(
        Rect::new(left, bottom - 2.0, right - left, 2.0),
        color,
        window,
    );
    fill_rect(Rect::new(left, top, 2.0, bottom - top), color, window);
    fill_rect(
        Rect::new(right - 2.0, top, 2.0, bottom - top),
        color,
        window,
    );
    Ok(())
}

use crate::{
    layout::{PixelView, Rect},
    rendering::{
        LINE_HEIGHT, TextContext,
        colors::{Palette, TEXT, ToHsla},
        fill_rect,
    },
};
use gpui::{App, Window};
use gv_core::{
    cytoband::{CytobandSchema, Stain},
    prelude::*,
};
use gv_viewer::prepare::coordinate::get_abbreviated_length_string;
use polars::prelude::ChunkAgg;

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
) -> Result<(), gv_core::Error> {
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

    let bands = state
        .cytobands
        .query(view.viewport.focus.contig_index, 1, u64::MAX)?;
    if bands.height() > 0 {
        let starts = bands.column(CytobandSchema::START)?.u64()?;
        let ends = bands.column(CytobandSchema::END)?.u64()?;
        let stains = bands.column(CytobandSchema::STAIN)?.str()?;
        let total_length = ends.max().unwrap_or(0).max(1) as f32;
        for ((start, end), stain) in starts
            .into_no_null_iter()
            .zip(ends.into_no_null_iter())
            .zip(stains.iter().map(Option::unwrap_or_default))
        {
            let stain = Stain::try_from(stain)?;
            let left = bar.x + (start - 1) as f32 / total_length * bar.width;
            let right = bar.x + end as f32 / total_length * bar.width;
            // The centromere is drawn pinched, as ideograms show it.
            let band = if stain == Stain::Acen {
                Rect::new(left, bar.y + 3.0, right - left, bar.height - 6.0)
            } else {
                Rect::new(left, bar.y, right - left, bar.height)
            };
            fill_rect(band, palette.cytoband_color(stain).hsla(), window);
        }
    } else {
        fill_rect(bar, palette.OTHER_COLOR.hsla(), window);
    }

    // Box the current viewing window.
    let (left, right) = view.visible(area);
    let scale = |position: u64| bar.x + position as f32 / contig_length as f32 * bar.width;
    let (left, right) = (
        scale(left.saturating_sub(1)),
        scale(right).max(scale(left) + 2.0),
    );
    let color = palette.HIGHLIGHT_COLOR.hsla();
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

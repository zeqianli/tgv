use crate::{
    layout::{PixelView, Rect},
    rendering::{
        LINE_HEIGHT, TextContext,
        colors::{MUTED_TEXT, TEXT, to_hsla},
        fill_rect,
    },
};
use gpui::{App, Window};
use gv_core::prelude::*;
use gv_viewer::prepare::coordinate::{
    calculate_intermarker_distance, get_abbreviated_coordinate_text,
};

/// The least space between two ticks of the ruler.
const MIN_SPACING_BETWEEN_MARKERS: f64 = 90.0;
const TICK_HEIGHT: f32 = 6.0;

/// Draws the ruler: labeled ticks at round coordinates.
pub fn render_coordinates(
    area: &Rect,
    view: &PixelView,
    state: &State,
    window: &mut Window,
    cx: &mut App,
) -> Result<(), crate::Error> {
    let contig_length = state.contig_length(&view.viewport.focus)?;
    let bases = (view.viewport.zoom.bases_per_pixel * MIN_SPACING_BETWEEN_MARKERS).ceil() as u64;
    let (intermarker_distance, power) = calculate_intermarker_distance(bases.max(1));
    let (left, right) = view.visible(area);
    let right = contig_length.map_or(right, |length| right.min(length));

    fill_rect(
        Rect::new(area.x, area.bottom() - 1.0, area.width, 1.0),
        to_hsla(MUTED_TEXT),
        window,
    );
    let mut pivot = (left / intermarker_distance + 1) * intermarker_distance;
    while pivot <= right {
        // Ticks mark the middle of their base.
        let x = view.x(pivot, area) + view.pixels_per_base() / 2.0;
        fill_rect(
            Rect::new(x.round(), area.bottom() - TICK_HEIGHT, 1.0, TICK_HEIGHT),
            to_hsla(MUTED_TEXT),
            window,
        );
        let text = get_abbreviated_coordinate_text(pivot, power);
        let width = TextContext::default().text(&text).width(window);
        TextContext::default().text(&text).color(TEXT).paint(
            x - width / 2.0,
            area.y + 2.0,
            LINE_HEIGHT,
            window,
            cx,
        )?;
        pivot += intermarker_distance;
    }
    Ok(())
}

use crate::{
    layout::{PixelView, Rect},
    rendering::{
        Painter,
        colors::{MUTED_TEXT, TEXT, ToHsla},
    },
};
use gv_core::prelude::*;
use gv_viewer::prepare::coordinate::{
    calculate_intermarker_distance, get_abbreviated_coordinate_text,
};

/// The least space between two ticks of the ruler.
const MIN_SPACING_BETWEEN_MARKERS: f64 = 90.0;
const TICK_HEIGHT: f32 = 6.0;

/// Draws the ruler: labeled ticks at round coordinates.
pub fn render_coordinates(
    painter: &mut Painter,
    area: &Rect,
    view: &PixelView,
    state: &State,
) -> Result<(), TGVError> {
    let contig_length = state.contig_length(&view.viewport.focus)?;
    let bases = (view.viewport.zoom.bases_per_pixel * MIN_SPACING_BETWEEN_MARKERS).ceil() as u64;
    let (intermarker_distance, power) = calculate_intermarker_distance(bases.max(1));
    let (left, right) = view.visible(area);
    let right = contig_length.map_or(right, |length| right.min(length));

    painter.fill(
        Rect::new(area.x, area.bottom() - 1.0, area.width, 1.0),
        MUTED_TEXT.hsla(),
    );
    let mut pivot = (left / intermarker_distance + 1) * intermarker_distance;
    while pivot <= right {
        // Ticks mark the middle of their base.
        let x = view.x(pivot, area) + view.pixels_per_base() / 2.0;
        painter.fill(
            Rect::new(x.round(), area.bottom() - TICK_HEIGHT, 1.0, TICK_HEIGHT),
            MUTED_TEXT.hsla(),
        );
        let text = get_abbreviated_coordinate_text(pivot, power);
        let width = painter.text_width(&text);
        painter.text(x - width / 2.0, area.y + 2.0, &text, TEXT);
        pivot += intermarker_distance;
    }
    Ok(())
}

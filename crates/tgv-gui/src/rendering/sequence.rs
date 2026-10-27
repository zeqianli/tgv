use crate::{
    layout::{PixelView, Rect},
    rendering::{
        Painter,
        colors::{Palette, ToHsla},
    },
};
use gv_core::prelude::*;

/// Bases at least this wide show their letters.
pub const MIN_PIXELS_PER_LETTER: f32 = 7.0;

/// Draws the reference bases: a colored box for each base, with its letter when it fits. When
/// a pixel covers several bases, it shows the first.
pub fn render_sequence(
    painter: &mut Painter,
    area: &Rect,
    state: &State,
    view: &PixelView,
    palette: &Palette,
) -> Result<(), TGVError> {
    let ppb = view.pixels_per_base();
    let (left, right) = view.visible(area);
    if ppb >= 1.0 {
        for position in left..=right {
            let Some(base) = state.sequence.base_at(position) else {
                continue;
            };
            let Some((x, width)) = view.span(position, position, area) else {
                continue;
            };
            let rect = Rect::new(x, area.y, width, area.height);
            painter.fill(rect, palette.base_color(base).hsla());
            if ppb >= MIN_PIXELS_PER_LETTER {
                painter.letter(rect, base as char, palette.SEQUENCE_FOREGROUND_COLOR);
            }
        }
    } else {
        let mut x = area.x;
        while x < area.right() {
            if let Some(base) = state.sequence.base_at(view.position_at(x, area)) {
                painter.fill(
                    Rect::new(x, area.y, 1.0, area.height),
                    palette.base_color(base).hsla(),
                );
            }
            x += 1.0;
        }
    }
    Ok(())
}

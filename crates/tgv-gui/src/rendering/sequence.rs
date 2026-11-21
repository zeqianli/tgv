use crate::{
    layout::{PixelView, Rect},
    rendering::{
        colors::{Palette, to_hsla},
        fill_rect, paint_letter,
    },
};
use gpui::{App, Window};
use gv_core::prelude::*;

/// Bases at least this wide show their letters.
pub const MIN_PIXELS_PER_LETTER: f32 = 7.0;

/// Draws the reference bases: a colored box for each base, with its letter when it fits. When
/// a pixel covers several bases, it shows the first.
pub fn render_sequence(
    area: &Rect,
    state: &State,
    view: &PixelView,
    palette: &Palette,
    window: &mut Window,
    cx: &mut App,
) -> Result<(), crate::Error> {
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
            fill_rect(rect, to_hsla(palette.base_color(base)), window);
            if ppb >= MIN_PIXELS_PER_LETTER {
                paint_letter(
                    rect,
                    base as char,
                    palette.SEQUENCE_FOREGROUND_COLOR,
                    window,
                    cx,
                )?;
            }
        }
    } else {
        let mut x = area.x;
        while x < area.right() {
            if let Some(base) = state.sequence.base_at(view.position_at(x, area)) {
                fill_rect(
                    Rect::new(x, area.y, 1.0, area.height),
                    to_hsla(palette.base_color(base)),
                    window,
                );
            }
            x += 1.0;
        }
    }
    Ok(())
}

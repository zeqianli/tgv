use crate::{
    alignment::{AlignedRead, Alignment},
    error::TGVError,
    rendering::colors::Palette,
    window::{OnScreenCoordinate, ViewingWindow},
};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
};
use rust_htslib::bam::record::Cigar;

/// Render an alignment on the alignment area.
pub fn render_alignment(
    area: &Rect,
    buf: &mut Buffer,
    window: &ViewingWindow,
    alignment: &Alignment,
    background_color: &Color,
    pallete: &Palette,
) -> Result<(), TGVError> {
    // This iterates through all cached reads and re-calculates coordinates for each movement.
    // Consider improvement.
    for read in alignment.reads.iter() {
        if let Some(contexts) =
            get_read_rendering_info(read, window, area, background_color, pallete)
        {
            for context in contexts {
                buf.set_string(
                    area.x + context.x,
                    area.y + context.y,
                    context.string,
                    context.style,
                )
            }
        }
    }
    Ok(())
}

enum Modifier {
    Forward,

    Reverse,

    Insertion(usize),
}

struct RenderingContext {
    x: u16,
    y: u16,
    string: String,
    style: Style,
}

impl RenderingContext {
    fn forward_arrow(&self) -> Self {
        return Self {
            x: self.x + self.string.len() as u16 - 1,
            y: self.y,
            string: "►".to_string(),
            style: self.style.clone(),
        };
    }

    fn reverse_arrow(&self) -> Self {
        return Self {
            x: self.x,
            y: self.y,
            string: "◄".to_string(),
            style: self.style.clone(),
        };
    }

    fn insertion(&self, background_color: &Color, pallete: &Palette) -> Self {
        return Self {
            x: self.x,
            y: self.y,
            string: "▌".to_string(),
            style: Style::default().fg(pallete.INSERTION_COLOR),
        };
    }
}

/// Get rendering needs for an aligned read.
/// Returns: x, y,
fn get_read_rendering_info(
    read: &AlignedRead,
    viewing_window: &ViewingWindow,
    area: &Rect,
    background_color: &Color,
    pallete: &Palette,
) -> Option<Vec<RenderingContext>> {
}

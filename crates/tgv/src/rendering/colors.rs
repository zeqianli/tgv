//! Terminal colors for the shared palette in `gv_viewer::colors`.

pub use gv_viewer::colors::{DARK_THEME, Palette, Rgb};
use ratatui::style::{Color, Modifier, Style};

/// Converts a palette color to a terminal color.
pub trait ToColor {
    fn color(self) -> Color;
}

impl ToColor for Rgb {
    fn color(self) -> Color {
        Color::Rgb(self.r, self.g, self.b)
    }
}

/// Style for a mismatched base. Low-quality bases are dimmed, so likely sequencing errors
/// stand out less.
pub fn mismatch_style(palette: &Palette, base: u8, quality: Option<u8>) -> Style {
    let style = Style::default().fg(palette.mismatch_color(base).color());
    if Palette::is_mismatch_shaded(quality) {
        style.add_modifier(Modifier::DIM)
    } else {
        style
    }
}

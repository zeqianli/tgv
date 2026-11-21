//! Terminal colors for the shared palette in `gv_viewer::colors`.

pub use gv_viewer::colors::{DARK_THEME, Palette, Rgb};
use ratatui::style::{Color, Modifier, Style};

pub fn to_color(color: Rgb) -> Color {
    Color::Rgb(color.r, color.g, color.b)
}

pub fn mismatch_style(palette: &Palette, base: u8, quality: Option<u8>) -> Style {
    let style = Style::default().fg(to_color(palette.mismatch_color(base)));
    if Palette::is_mismatch_shaded(quality) {
        style.add_modifier(Modifier::DIM)
    } else {
        style
    }
}

use gpui::Hsla;
pub use gv_viewer::colors::{
    CURSOR_COLOR, DARK_THEME, MUTED_TEXT, Palette, READ_BASE_LETTER, Rgb, SEPARATOR, TEXT,
};

pub fn to_hsla(color: Rgb) -> Hsla {
    gpui::rgb((u32::from(color.r) << 16) | (u32::from(color.g) << 8) | u32::from(color.b)).into()
}

pub fn to_hsla_with_alpha(color: Rgb, alpha: f32) -> Hsla {
    let mut color = to_hsla(color);
    color.a = alpha;
    color
}

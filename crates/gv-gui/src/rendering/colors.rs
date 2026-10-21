//! Window colors for the shared palette in `gv_viewer::colors`.

use gpui::Hsla;
pub use gv_viewer::colors::{DARK_THEME, Palette, Rgb};

/// Converts a palette color to a window color.
pub trait ToHsla {
    fn hsla(self) -> Hsla;

    /// The color with an opacity from 0 to 1.
    fn alpha(self, alpha: f32) -> Hsla;
}

impl ToHsla for Rgb {
    fn hsla(self) -> Hsla {
        gpui::rgb((u32::from(self.r) << 16) | (u32::from(self.g) << 8) | u32::from(self.b)).into()
    }

    fn alpha(self, alpha: f32) -> Hsla {
        let mut color = self.hsla();
        color.a = alpha;
        color
    }
}

/// Text that is less important than the data, such as sidebar notes.
pub const MUTED_TEXT: Rgb = Rgb::from_u32(0x9ca3af);

/// The window's main text color.
pub const TEXT: Rgb = Rgb::from_u32(0xe5e5e5);

/// Lines between sidebar sections.
pub const SEPARATOR: Rgb = Rgb::from_u32(0x3f3f46);

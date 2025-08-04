use ratatui::style::palette::tailwind;
/// Colors profile
///
///
///
use ratatui::style::Color;

// Background
pub struct Palette {
    /// Track alternating colors
    pub background_1: Color,
    pub background_2: Color,
}

pub const DARK_THEME: Palette = Palette {
    background_1: Color::from_u32(0x1e1e1e),
    background_2: Color::from_u32(0x323232),
};

// Alignment
pub const MATCH_COLOR: Color = tailwind::GRAY.c500;
#[allow(dead_code)]
pub const MISMATCH_COLOR: Color = Color::Rgb(251, 198, 207);
pub const SOFTCLIP_A: Color = Color::LightRed;
pub const SOFTCLIP_C: Color = Color::LightGreen;
pub const SOFTCLIP_G: Color = Color::LightBlue;
pub const SOFTCLIP_T: Color = Color::LightYellow;
pub const SOFTCLIP_N: Color = Color::LightMagenta;

// Cytoband
pub const HIGHLIGHT_COLOR: Color = tailwind::RED.c800;
pub const CYTOBAND_DEFAULT_COLOR: Color = tailwind::GRAY.c300;
// const GNEG_COLOR: Color = tailwind::GREEN.c100;
pub const GPOS25_COLOR: Color = tailwind::GREEN.c200;
pub const GPOS50_COLOR: Color = tailwind::GREEN.c500;
pub const GPOS75_COLOR: Color = tailwind::GREEN.c700;
pub const GPOS100_COLOR: Color = tailwind::GREEN.c900;

pub const ACEN_COLOR: Color = tailwind::RED.c300;
pub const GVAR_COLOR: Color = CYTOBAND_DEFAULT_COLOR;
pub const STALK_COLOR: Color = CYTOBAND_DEFAULT_COLOR;
pub const OTHER_COLOR: Color = CYTOBAND_DEFAULT_COLOR;

// Sequence
pub const SEQUENCE_FOREGROUND_COLOR: Color = tailwind::GRAY.c900;
pub const BASE_A: Color = tailwind::RED.c300;
pub const BASE_C: Color = tailwind::GREEN.c300;
pub const BASE_G: Color = tailwind::BLUE.c300;
pub const BASE_T: Color = tailwind::YELLOW.c300;
pub const BASE_N: Color = tailwind::GRAY.c300;

// Intervals
pub const VCF1: Color = tailwind::VIOLET.c900;
pub const VCF2: Color = tailwind::VIOLET.c400;
pub const BED1: Color = tailwind::INDIGO.c900;
pub const BED2: Color = tailwind::INDIGO.c400;

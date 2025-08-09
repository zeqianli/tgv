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

    // Alignment
    pub MATCH_COLOR: Color,
    pub MISMATCH_COLOR: Color,
    pub SOFTCLIP_A: Color,
    pub SOFTCLIP_C: Color,
    pub SOFTCLIP_G: Color,
    pub SOFTCLIP_T: Color,
    pub SOFTCLIP_N: Color,

    // Cytoband
    pub HIGHLIGHT_COLOR: Color,
    // pub CYTOBAND_DEFAULT_COLOR: Color,
    //  GNEG_COLOR: Color = tailwind::GREEN.c100;
    pub GPOS25_COLOR: Color,
    pub GPOS50_COLOR: Color,
    pub GPOS75_COLOR: Color,
    pub GPOS100_COLOR: Color,

    pub ACEN_COLOR: Color,
    pub GVAR_COLOR: Color,
    pub STALK_COLOR: Color,
    pub OTHER_COLOR: Color,

    // Sequence
    pub SEQUENCE_FOREGROUND_COLOR: Color,
    pub BASE_A: Color,
    pub BASE_C: Color,
    pub BASE_G: Color,
    pub BASE_T: Color,
    pub BASE_N: Color,

    // Intervals
    pub VCF1: Color,
    pub VCF2: Color,
    pub BED1: Color,
    pub BED2: Color,
}

impl Palette {
    pub fn softclip_color(&self, base: char) -> Color {
        match base {
            'A' => self.SOFTCLIP_A,
            'C' => self.SOFTCLIP_C,
            'G' => self.SOFTCLIP_G,
            'T' => self.SOFTCLIP_T,
            'N' => self.SOFTCLIP_N,
            _ => self.SOFTCLIP_N,
        }
    }
}

pub const DARK_THEME: Palette = Palette {
    background_1: Color::from_u32(0x1e1e1e),
    background_2: Color::from_u32(0x323232),

    // Alignment
    MATCH_COLOR: tailwind::GRAY.c500,
    MISMATCH_COLOR: Color::Rgb(251, 198, 207),
    SOFTCLIP_A: Color::LightRed,
    SOFTCLIP_C: Color::LightGreen,
    SOFTCLIP_G: Color::LightBlue,
    SOFTCLIP_T: Color::LightYellow,
    SOFTCLIP_N: Color::LightMagenta,

    // Cytoband
    HIGHLIGHT_COLOR: tailwind::RED.c800,
    //  GNEG_COLOR: Color = tailwind::GREEN.c100;
    GPOS25_COLOR: tailwind::GREEN.c200,
    GPOS50_COLOR: tailwind::GREEN.c500,
    GPOS75_COLOR: tailwind::GREEN.c700,
    GPOS100_COLOR: tailwind::GREEN.c900,

    ACEN_COLOR: tailwind::RED.c300,
    GVAR_COLOR: tailwind::GRAY.c300,
    STALK_COLOR: tailwind::GRAY.c300,
    OTHER_COLOR: tailwind::GRAY.c300,

    // Sequence
    SEQUENCE_FOREGROUND_COLOR: tailwind::GRAY.c900,
    BASE_A: tailwind::RED.c300,
    BASE_C: tailwind::GREEN.c300,
    BASE_G: tailwind::BLUE.c300,
    BASE_T: tailwind::YELLOW.c300,
    BASE_N: tailwind::GRAY.c300,

    // Intervals
    VCF1: tailwind::VIOLET.c900,
    VCF2: tailwind::VIOLET.c400,
    BED1: tailwind::INDIGO.c900,
    BED2: tailwind::INDIGO.c400,
};

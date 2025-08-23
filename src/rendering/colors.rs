use ratatui::style::palette::tailwind;
/// Colors profile
///
///
///
use ratatui::style::Color;

// Background
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(non_snake_case)]
pub struct Palette {
    /// Track alternating colors
    pub background_1: Color,
    pub background_2: Color,

    // Alignment
    pub MATCH_COLOR: Color,
    pub MISMATCH_COLOR: Color,
    pub DELETION_COLOR: Color,
    pub REFSKIP_COLOR: Color,
    pub INSERTION_COLOR: Color,
    pub SOFTCLIP_A: Color,
    pub SOFTCLIP_C: Color,
    pub SOFTCLIP_G: Color,
    pub SOFTCLIP_T: Color,
    pub SOFTCLIP_N: Color,
    pub MISMATCH_A: Color,
    pub MISMATCH_C: Color,
    pub MISMATCH_G: Color,
    pub MISMATCH_T: Color,
    pub MISMATCH_N: Color,

    // Coverage
    pub COVERAGE_ALT: Color,
    pub COVERAGE_A: Color,
    pub COVERAGE_T: Color,
    pub COVERAGE_C: Color,
    pub COVERAGE_G: Color,
    pub COVERAGE_N: Color,
    pub COVERAGE_TOTAL: Color,
    pub COVERAGE_SOFTCLIP: Color,

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

    // Gene track
    pub EXON_BACKGROUND_COLOR: Color,
    pub EXON_FOREGROUND_COLOR: Color,
    pub GENE_BACKGROUND_COLOR: Color,
    pub NON_CDS_EXON_BACKGROUND_COLOR: Color,
    pub INTRON_FOREGROUND_COLOR: Color,
}

impl Palette {
    pub fn softclip_color(&self, base: u8) -> Color {
        match base {
            b'A' => self.SOFTCLIP_A,
            b'C' => self.SOFTCLIP_C,
            b'G' => self.SOFTCLIP_G,
            b'T' => self.SOFTCLIP_T,
            b'N' => self.SOFTCLIP_N,
            _ => self.SOFTCLIP_N,
        }
    }

    pub fn base_color(&self, base: u8) -> Color {
        match base {
            b'A' | b'a' => self.BASE_A,
            b'C' | b'c' => self.BASE_C,
            b'G' | b'g' => self.BASE_G,
            b'T' | b't' => self.BASE_T,
            _ => self.BASE_N,
        }
    }

    pub fn mismatch_color(&self, base: u8) -> Color {
        match base {
            b'A' | b'a' => self.MISMATCH_A,
            b'C' | b'c' => self.MISMATCH_C,
            b'G' | b'g' => self.MISMATCH_G,
            b'T' | b't' => self.MISMATCH_T,
            _ => self.MISMATCH_N,
        }
    }
}

pub const DARK_THEME: Palette = Palette {
    background_1: Color::from_u32(0x1e1e1e),
    background_2: Color::from_u32(0x323232),

    // Alignment
    MATCH_COLOR: tailwind::GRAY.c500,
    MISMATCH_COLOR: Color::Rgb(251, 198, 207),
    DELETION_COLOR: Color::Red,
    REFSKIP_COLOR: Color::Red,
    INSERTION_COLOR: Color::Magenta,

    SOFTCLIP_A: Color::LightRed,
    SOFTCLIP_C: Color::LightGreen,
    SOFTCLIP_G: Color::LightBlue,
    SOFTCLIP_T: Color::LightYellow,
    SOFTCLIP_N: Color::LightMagenta,

    MISMATCH_A: Color::LightRed,
    MISMATCH_C: Color::LightGreen,
    MISMATCH_G: Color::LightBlue,
    MISMATCH_T: Color::LightYellow,
    MISMATCH_N: Color::LightMagenta,

    COVERAGE_ALT: Color::Red,
    COVERAGE_A: Color::LightRed,
    COVERAGE_T: Color::LightYellow,
    COVERAGE_C: Color::LightGreen,
    COVERAGE_G: Color::LightBlue,
    COVERAGE_N: Color::LightMagenta,
    COVERAGE_TOTAL: Color::Gray,
    COVERAGE_SOFTCLIP: Color::Cyan, // TODO

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

    // Gene track
    EXON_BACKGROUND_COLOR: tailwind::BLUE.c800,
    EXON_FOREGROUND_COLOR: tailwind::WHITE,
    GENE_BACKGROUND_COLOR: tailwind::BLUE.c600,
    NON_CDS_EXON_BACKGROUND_COLOR: tailwind::BLUE.c500,
    INTRON_FOREGROUND_COLOR: tailwind::BLUE.c300,
};

//! The color palette that both front ends draw with.
//!
//! Colors are plain RGB, so the palette doesn't depend on a terminal or window library. Each
//! front end converts them to its own color type when it draws.

use gv_core::cytoband::Stain;
use noodles::sam::record::data::field::value::base_modifications::group::{
    Modification, modification,
};

/// A color as red, green, and blue components.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// A color from `0xRRGGBB`.
    pub const fn from_u32(value: u32) -> Self {
        Self {
            r: (value >> 16) as u8,
            g: (value >> 8) as u8,
            b: value as u8,
        }
    }
}

/// Tailwind and terminal colors that the dark theme uses, by name.
mod named {
    use super::Rgb;

    pub const WHITE: Rgb = Rgb::from_u32(0xffffff);

    // The default dark colors of common terminals, for the colors that were named terminal
    // colors before the palette became RGB.
    pub const RED: Rgb = Rgb::from_u32(0xcd3131);
    pub const LIGHT_RED: Rgb = Rgb::from_u32(0xf14c4c);
    pub const LIGHT_GREEN: Rgb = Rgb::from_u32(0x23d18b);
    pub const LIGHT_BLUE: Rgb = Rgb::from_u32(0x3b8eea);
    pub const LIGHT_YELLOW: Rgb = Rgb::from_u32(0xf5f543);
    pub const LIGHT_MAGENTA: Rgb = Rgb::from_u32(0xd670d6);
    pub const MAGENTA: Rgb = Rgb::from_u32(0xbc3fbc);
    pub const GRAY: Rgb = Rgb::from_u32(0xe5e5e5);
    pub const CYAN: Rgb = Rgb::from_u32(0x11a8cd);

    pub const GRAY_300: Rgb = Rgb::from_u32(0xd1d5db);
    pub const GRAY_500: Rgb = Rgb::from_u32(0x6b7280);
    pub const GRAY_700: Rgb = Rgb::from_u32(0x374151);
    pub const GRAY_900: Rgb = Rgb::from_u32(0x111827);
    pub const RED_300: Rgb = Rgb::from_u32(0xfca5a5);
    pub const RED_800: Rgb = Rgb::from_u32(0x991b1b);
    pub const GREEN_200: Rgb = Rgb::from_u32(0xbbf7d0);
    pub const GREEN_300: Rgb = Rgb::from_u32(0x86efac);
    pub const GREEN_400: Rgb = Rgb::from_u32(0x4ade80);
    pub const GREEN_500: Rgb = Rgb::from_u32(0x22c55e);
    pub const GREEN_700: Rgb = Rgb::from_u32(0x15803d);
    pub const GREEN_900: Rgb = Rgb::from_u32(0x14532d);
    pub const BLUE_300: Rgb = Rgb::from_u32(0x93c5fd);
    pub const BLUE_500: Rgb = Rgb::from_u32(0x3b82f6);
    pub const BLUE_600: Rgb = Rgb::from_u32(0x2563eb);
    pub const BLUE_700: Rgb = Rgb::from_u32(0x1d4ed8);
    pub const BLUE_800: Rgb = Rgb::from_u32(0x1e40af);
    pub const YELLOW_300: Rgb = Rgb::from_u32(0xfde047);
    pub const YELLOW_700: Rgb = Rgb::from_u32(0xa16207);
    pub const VIOLET_400: Rgb = Rgb::from_u32(0xa78bfa);
    pub const VIOLET_900: Rgb = Rgb::from_u32(0x4c1d95);
    pub const INDIGO_400: Rgb = Rgb::from_u32(0x818cf8);
    pub const INDIGO_900: Rgb = Rgb::from_u32(0x312e81);
    pub const AMBER_700: Rgb = Rgb::from_u32(0xb45309);
    pub const SLATE_400: Rgb = Rgb::from_u32(0x94a3b8);
    pub const SLATE_700: Rgb = Rgb::from_u32(0x334155);
    pub const SLATE_800: Rgb = Rgb::from_u32(0x1e293b);
    pub const SKY_300: Rgb = Rgb::from_u32(0x7dd3fc);
    pub const SKY_700: Rgb = Rgb::from_u32(0x0369a1);
    pub const ORANGE_600: Rgb = Rgb::from_u32(0xea580c);
    pub const TEAL_500: Rgb = Rgb::from_u32(0x14b8a6);
    pub const PURPLE_600: Rgb = Rgb::from_u32(0x9333ea);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(non_snake_case)]
pub struct Palette {
    /// Track alternating colors
    pub background: Rgb,

    // Alignment
    pub MATCH_COLOR: Rgb,
    pub MATCH_FG_COLOR: Rgb,
    /// The body of a read with mapping quality 0.
    pub ZERO_MAPQ_COLOR: Rgb,

    pub MISMATCH_COLOR: Rgb,
    pub DELETION_COLOR: Rgb,
    pub PAIRGAP_COLOR: Rgb,
    pub PAIR_OVERLAP_COLOR: Rgb,
    pub REFSKIP_COLOR: Rgb,
    pub INSERTION_COLOR: Rgb,
    pub SOFTCLIP_A: Rgb,
    pub SOFTCLIP_C: Rgb,
    pub SOFTCLIP_G: Rgb,
    pub SOFTCLIP_T: Rgb,
    pub SOFTCLIP_N: Rgb,
    pub MISMATCH_A: Rgb,
    pub MISMATCH_C: Rgb,
    pub MISMATCH_G: Rgb,
    pub MISMATCH_T: Rgb,
    pub MISMATCH_N: Rgb,

    // Coverage
    pub COVERAGE_ALT: Rgb,
    pub COVERAGE_A: Rgb,
    pub COVERAGE_T: Rgb,
    pub COVERAGE_C: Rgb,
    pub COVERAGE_G: Rgb,
    pub COVERAGE_N: Rgb,
    pub COVERAGE_TOTAL: Rgb,
    pub COVERAGE_SOFTCLIP: Rgb,

    // Cytoband
    pub HIGHLIGHT_COLOR: Rgb,
    pub GPOS25_COLOR: Rgb,
    pub GPOS50_COLOR: Rgb,
    pub GPOS75_COLOR: Rgb,
    pub GPOS100_COLOR: Rgb,

    pub ACEN_COLOR: Rgb,
    pub GVAR_COLOR: Rgb,
    pub STALK_COLOR: Rgb,
    pub OTHER_COLOR: Rgb,

    // Sequence
    pub SEQUENCE_FOREGROUND_COLOR: Rgb,
    pub BASE_A: Rgb,
    pub BASE_C: Rgb,
    pub BASE_G: Rgb,
    pub BASE_T: Rgb,
    pub BASE_N: Rgb,

    // Intervals
    pub VCF1: Rgb,
    pub VCF2: Rgb,
    pub BED1: Rgb,
    pub BED2: Rgb,

    /// Intervals that an agent highlights, behind the coordinate ruler.
    pub HIGHLIGHT: Rgb,

    // Context menu and sidebar
    pub MENU_BACKGROUND: Rgb,
    pub MENU_HOVER_BACKGROUND: Rgb,
    pub MENU_FOREGROUND: Rgb,
    pub MENU_DISABLED_FOREGROUND: Rgb,
    /// Alignment display options listed in the sidebar.
    pub SIDEBAR_OPTION_COLOR: Rgb,
    /// The focused track's sidebar section in normal mode.
    pub FOCUS_NORMAL: Rgb,
    /// The focused track's sidebar section in insert mode, and the mode line.
    pub FOCUS_INSERT: Rgb,
    /// The insert mode line.
    pub MODE_INSERT: Rgb,
    /// Where dropped files go, in the window.
    pub DROP_TARGET: Rgb,

    // Gene track
    pub EXON_BACKGROUND_COLOR: Rgb,
    pub EXON_FOREGROUND_COLOR: Rgb,
    pub GENE_BACKGROUND_COLOR: Rgb,
    pub NON_CDS_EXON_BACKGROUND_COLOR: Rgb,
    pub INTRON_FOREGROUND_COLOR: Rgb,

    // Base modifications (MM/ML tags)
    /// 5mC high probability (>70%): warm orange
    pub MOD_5MC_HIGH: Rgb,
    /// 5mC medium probability (30-70%): muted yellow
    pub MOD_5MC_MED: Rgb,
    /// 5mC low probability (<30%): cool blue
    pub MOD_5MC_LOW: Rgb,
    /// 5hmC: teal/cyan
    pub MOD_5HMC: Rgb,
    /// 6mA: purple
    pub MOD_6MA: Rgb,
}

impl Palette {
    /// Mismatched bases with a Phred quality below this are dimmed. IGV shades bases below 20
    /// by default.
    const SHADED_MISMATCH_QUALITY: u8 = 20;

    pub fn softclip_color(&self, base: u8) -> Rgb {
        match base {
            b'A' => self.SOFTCLIP_A,
            b'C' => self.SOFTCLIP_C,
            b'G' => self.SOFTCLIP_G,
            b'T' => self.SOFTCLIP_T,
            b'N' => self.SOFTCLIP_N,
            _ => self.SOFTCLIP_N,
        }
    }

    pub fn base_color(&self, base: u8) -> Rgb {
        match base {
            b'A' | b'a' => self.BASE_A,
            b'C' | b'c' => self.BASE_C,
            b'G' | b'g' => self.BASE_G,
            b'T' | b't' => self.BASE_T,
            _ => self.BASE_N,
        }
    }

    pub fn mismatch_color(&self, base: u8) -> Rgb {
        match base {
            b'A' | b'a' => self.MISMATCH_A,
            b'C' | b'c' => self.MISMATCH_C,
            b'G' | b'g' => self.MISMATCH_G,
            b'T' | b't' => self.MISMATCH_T,
            _ => self.MISMATCH_N,
        }
    }

    /// Whether a mismatched base is dimmed. Low-quality bases are dimmed, so likely sequencing
    /// errors stand out less. A missing quality, 255 in BAM, is not shaded.
    pub fn is_mismatch_shaded(quality: Option<u8>) -> bool {
        matches!(quality, Some(quality) if quality < Self::SHADED_MISMATCH_QUALITY)
    }

    /// Returns the background color for a base modification given its type and
    /// probability (0-255 from the ML tag, where 255 = fully modified).
    pub fn modification_color(&self, modification: &Modification, probability: u8) -> Rgb {
        // FIXME: colors for other modifications
        match *modification {
            modification::FIVE_METHYLCYTOSINE => {
                if probability >= 179 {
                    self.MOD_5MC_HIGH
                } else if probability >= 77 {
                    self.MOD_5MC_MED
                } else {
                    self.MOD_5MC_LOW
                }
            }
            modification::FIVE_HYDROXYMETHYLCYTOSINE => self.MOD_5HMC,
            modification::SIX_METHYLADENINE => self.MOD_6MA,
            _ => self.MOD_5MC_MED,
        }
    }

    /// Returns the color associated with the stain type.
    pub fn cytoband_color(&self, stain: Stain) -> Rgb {
        // FIXME: This function is AI code. I haven't verified the correctness.
        match stain {
            Stain::Gneg => Rgb::from_u32(0xffffff),
            Stain::Gpos(p) => {
                let start_r = 240.0;
                let start_g = 253.0;
                let start_b = 244.0;
                let end_r = 5.0;
                let end_g = 46.0;
                let end_b = 22.0;

                let t = p as f32 / 100.0;

                let r = (start_r * (1.0 - t) + end_r * t).round() as u8;
                let g = (start_g * (1.0 - t) + end_g * t).round() as u8;
                let b = (start_b * (1.0 - t) + end_b * t).round() as u8;

                Rgb::new(r, g, b)
            }
            Stain::Acen => Rgb::from_u32(0xdc2626),
            Stain::Gvar => Rgb::from_u32(0x60a5fa),
            Stain::Stalk => Rgb::from_u32(0xc026d3),
            Stain::Other(_) => Rgb::from_u32(0x4b5563),
        }
    }
}

pub const DARK_THEME: Palette = Palette {
    // FIXME: use standard ATCG colors, same as IGV
    background: Rgb::from_u32(0x1e1e1e),

    // Alignment
    MATCH_COLOR: named::GRAY_500,
    MATCH_FG_COLOR: named::WHITE,
    ZERO_MAPQ_COLOR: named::GRAY_700,
    MISMATCH_COLOR: Rgb::new(251, 198, 207),
    DELETION_COLOR: named::RED,
    PAIRGAP_COLOR: named::LIGHT_RED,
    PAIR_OVERLAP_COLOR: named::GRAY_900,
    REFSKIP_COLOR: named::RED,
    INSERTION_COLOR: named::MAGENTA,

    SOFTCLIP_A: named::LIGHT_RED,
    SOFTCLIP_C: named::LIGHT_GREEN,
    SOFTCLIP_G: named::LIGHT_BLUE,
    SOFTCLIP_T: named::LIGHT_YELLOW,
    SOFTCLIP_N: named::LIGHT_MAGENTA,

    MISMATCH_A: named::LIGHT_RED,
    MISMATCH_C: named::LIGHT_GREEN,
    MISMATCH_G: named::LIGHT_BLUE,
    MISMATCH_T: named::LIGHT_YELLOW,
    MISMATCH_N: named::LIGHT_MAGENTA,

    COVERAGE_ALT: named::RED,
    COVERAGE_A: named::LIGHT_RED,
    COVERAGE_T: named::LIGHT_YELLOW,
    COVERAGE_C: named::LIGHT_GREEN,
    COVERAGE_G: named::LIGHT_BLUE,
    COVERAGE_N: named::LIGHT_MAGENTA,
    COVERAGE_TOTAL: named::GRAY,
    COVERAGE_SOFTCLIP: named::CYAN, // TODO

    // Cytoband
    HIGHLIGHT_COLOR: named::RED_800,
    GPOS25_COLOR: named::GREEN_200,
    GPOS50_COLOR: named::GREEN_500,
    GPOS75_COLOR: named::GREEN_700,
    GPOS100_COLOR: named::GREEN_900,

    ACEN_COLOR: named::RED_300,
    GVAR_COLOR: named::GRAY_300,
    STALK_COLOR: named::GRAY_300,
    OTHER_COLOR: named::GRAY_300,

    // Sequence
    SEQUENCE_FOREGROUND_COLOR: named::GRAY_900,
    BASE_A: named::RED_300,
    BASE_C: named::GREEN_300,
    BASE_G: named::BLUE_300,
    BASE_T: named::YELLOW_300,
    BASE_N: named::GRAY_300,

    // Intervals
    VCF1: named::VIOLET_900,
    VCF2: named::VIOLET_400,
    BED1: named::INDIGO_900,
    BED2: named::INDIGO_400,

    HIGHLIGHT: named::AMBER_700,

    MENU_BACKGROUND: named::SLATE_700,
    MENU_HOVER_BACKGROUND: named::SKY_700,
    MENU_FOREGROUND: named::WHITE,
    MENU_DISABLED_FOREGROUND: named::SLATE_400,
    SIDEBAR_OPTION_COLOR: named::SKY_300,
    FOCUS_NORMAL: named::SLATE_800,
    FOCUS_INSERT: named::GREEN_900,
    MODE_INSERT: named::GREEN_400,
    DROP_TARGET: named::SKY_300,

    // Gene track
    EXON_BACKGROUND_COLOR: named::BLUE_800,
    EXON_FOREGROUND_COLOR: named::WHITE,
    GENE_BACKGROUND_COLOR: named::BLUE_600,
    NON_CDS_EXON_BACKGROUND_COLOR: named::BLUE_500,
    INTRON_FOREGROUND_COLOR: named::BLUE_300,

    // Base modifications
    MOD_5MC_HIGH: named::ORANGE_600, // >70% — warm orange (highly methylated)
    MOD_5MC_MED: named::YELLOW_700,  // 30-70% — muted yellow (ambiguous)
    MOD_5MC_LOW: named::BLUE_700,    // <30% — cool blue (unmethylated)
    MOD_5HMC: named::TEAL_500,       // 5hmC — teal
    MOD_6MA: named::PURPLE_600,      // 6mA — purple
};

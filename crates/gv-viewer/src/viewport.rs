//! What part of the genome a viewer shows, independent of screen resolution.
//!
//! A [`Viewport`] holds the focus, the zoom, and each alignment track's vertical scroll. The
//! zoom is a [`Scale`]: how many bases one screen unit covers. The terminal UI's unit is a
//! column and its scale is a whole number of bases ([`ColumnScale`]). The window's unit is a
//! pixel and its scale is continuous ([`PixelScale`]). The viewer reasons in bases, and each
//! front end says how wide its track area is in its own units.

use gv_core::error::TGVError;
use gv_core::intervals::{Focus, Region};
use gv_core::message::{Scroll, Zoom};
use std::fmt::Debug;

/// How many bases a screen unit covers, and what can be shown at that zoom.
pub trait Scale: Copy + Debug + Default + PartialEq + Send + 'static {
    /// The bases on each side of the focus in a track area `width` units wide.
    fn half_width(&self, width: f64) -> u64;

    /// Applies a zoom to a track area `width` units wide. A factor of 0 is an error.
    fn zoom(self, zoom: &Zoom, width: f64) -> Result<Self, TGVError>;

    /// Zooms in far enough that a track area `width` units wide doesn't show more than
    /// `contig_length` bases.
    fn clamp(self, width: f64, contig_length: u64) -> Self;

    /// The bases that one column of a key motion, such as `h` or `l`, moves.
    fn column_bases(&self) -> u64;

    /// The bases of the column that holds `position`, 1-based and inclusive, in a track area
    /// whose first base is `left`. Insert mode paints and erases whole columns.
    fn column_at(&self, left: u64, position: u64) -> (u64, u64);

    /// The scale for insert mode, where every column must be whole bases.
    fn for_insert_mode(self) -> Self;

    /// Whether reads are drawn and loaded.
    fn shows_alignments(&self) -> bool;

    /// Whether the reference sequence is drawn and loaded.
    fn shows_sequence(&self) -> bool;

    /// Whether indexed variant and BED files are loaded. They can be too large to read for a
    /// zoomed-out view.
    fn shows_indexed_features(&self) -> bool;

    /// Bases per screen unit, as session files store it and agents see it.
    fn bases_per_unit(&self) -> f64;

    /// The scale for a zoom from a session file. Front ends with whole-number scales round it.
    fn from_bases_per_unit(bases: f64) -> Self;
}

/// The terminal UI's scale: a whole number of bases per column.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ColumnScale(pub u64);

impl ColumnScale {
    pub const MAX_TO_DISPLAY_ALIGNMENTS: u64 = 32;
    pub const MAX_TO_DISPLAY_SEQUENCES: u64 = 2;

    /// Indexed variant and BED files can be too large to read for a zoomed-out view. At this
    /// zoom, an 80-column track area shows 8 kb, and the viewer cache loads about 80 kb.
    pub const MAX_TO_DISPLAY_INDEXED_FEATURES: u64 = 100;
}

impl Default for ColumnScale {
    fn default() -> Self {
        Self(1)
    }
}

impl Scale for ColumnScale {
    fn half_width(&self, width: f64) -> u64 {
        (width as u64 * self.0) / 2
    }

    fn zoom(self, zoom: &Zoom, width: f64) -> Result<Self, TGVError> {
        Ok(Self(match *zoom {
            Zoom::In(0) => {
                return Err(TGVError::ValueError(
                    "Zoom in factor cannot be 0".to_string(),
                ));
            }
            Zoom::Out(0) => {
                return Err(TGVError::ValueError(
                    "Zoom out factor cannot be 0".to_string(),
                ));
            }
            Zoom::In(factor) => u64::max(1, self.0 / factor),
            // Bounded later by `clamp`.
            Zoom::Out(factor) => self.0.saturating_mul(factor),
            Zoom::Fit { bases } => bases.div_ceil((width as u64).max(1)).max(1),
        }))
    }

    fn clamp(self, width: f64, contig_length: u64) -> Self {
        Self(u64::min(self.0, contig_length / width as u64).max(1))
    }

    fn column_bases(&self) -> u64 {
        self.0
    }

    fn column_at(&self, left: u64, position: u64) -> (u64, u64) {
        let start = left + (position - left) / self.0 * self.0;
        (start, start + self.0 - 1)
    }

    fn for_insert_mode(self) -> Self {
        // A column is always whole bases.
        self
    }

    fn shows_alignments(&self) -> bool {
        self.0 <= Self::MAX_TO_DISPLAY_ALIGNMENTS
    }

    fn shows_sequence(&self) -> bool {
        self.0 <= Self::MAX_TO_DISPLAY_SEQUENCES
    }

    fn shows_indexed_features(&self) -> bool {
        self.0 <= Self::MAX_TO_DISPLAY_INDEXED_FEATURES
    }

    fn bases_per_unit(&self) -> f64 {
        self.0 as f64
    }

    fn from_bases_per_unit(bases: f64) -> Self {
        Self((bases.round() as u64).max(1))
    }
}

/// The window's scale: a continuous number of bases per pixel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PixelScale {
    pub bases_per_pixel: f64,
}

impl PixelScale {
    /// Pixels that one column of a key motion moves, so `h` and `l` move about as far across
    /// the window as one terminal column moves across a terminal.
    pub const KEY_STEP_PIXELS: f64 = 8.0;

    /// The closest zoom: a base is this many pixels wide.
    pub const MAX_PIXELS_PER_BASE: f64 = 40.0;

    /// The limits are about the TUI's, for an 8-pixel-wide terminal cell, so a window shows
    /// about the same kinds of data as a terminal of the same size. The sequence shows further
    /// out than in the TUI, because mismatches are computed against it.
    pub const MAX_TO_DISPLAY_ALIGNMENTS: f64 = 4.0;
    pub const MAX_TO_DISPLAY_SEQUENCES: f64 = 2.0;
    pub const MAX_TO_DISPLAY_INDEXED_FEATURES: f64 = 12.5;

    pub fn pixels_per_base(&self) -> f64 {
        1.0 / self.bases_per_pixel
    }
}

impl Default for PixelScale {
    fn default() -> Self {
        Self {
            bases_per_pixel: 1.0 / Self::KEY_STEP_PIXELS,
        }
    }
}

impl Scale for PixelScale {
    fn half_width(&self, width: f64) -> u64 {
        (width * self.bases_per_pixel / 2.0).round() as u64
    }

    fn zoom(self, zoom: &Zoom, width: f64) -> Result<Self, TGVError> {
        let bases_per_pixel = match *zoom {
            Zoom::In(0) => {
                return Err(TGVError::ValueError(
                    "Zoom in factor cannot be 0".to_string(),
                ));
            }
            Zoom::Out(0) => {
                return Err(TGVError::ValueError(
                    "Zoom out factor cannot be 0".to_string(),
                ));
            }
            Zoom::In(factor) => self.bases_per_pixel / factor as f64,
            Zoom::Out(factor) => self.bases_per_pixel * factor as f64,
            Zoom::Fit { bases } => bases as f64 / width.max(1.0),
        };
        Ok(Self {
            bases_per_pixel: bases_per_pixel.max(1.0 / Self::MAX_PIXELS_PER_BASE),
        })
    }

    fn clamp(self, width: f64, contig_length: u64) -> Self {
        Self {
            bases_per_pixel: self
                .bases_per_pixel
                .min(contig_length as f64 / width.max(1.0))
                .max(1.0 / Self::MAX_PIXELS_PER_BASE),
        }
    }

    fn column_bases(&self) -> u64 {
        ((self.bases_per_pixel * Self::KEY_STEP_PIXELS).round() as u64).max(1)
    }

    fn column_at(&self, _left: u64, position: u64) -> (u64, u64) {
        // Columns start at the cursor rather than at a grid, because pixels are much finer
        // than a column.
        (position, position + self.column_bases() - 1)
    }

    fn for_insert_mode(self) -> Self {
        // One column is then one base, so every edit paints or erases whole bases.
        Self {
            bases_per_pixel: self.bases_per_pixel.min(1.0 / Self::KEY_STEP_PIXELS),
        }
    }

    fn shows_alignments(&self) -> bool {
        self.bases_per_pixel <= Self::MAX_TO_DISPLAY_ALIGNMENTS
    }

    fn shows_sequence(&self) -> bool {
        self.bases_per_pixel <= Self::MAX_TO_DISPLAY_SEQUENCES
    }

    fn shows_indexed_features(&self) -> bool {
        self.bases_per_pixel <= Self::MAX_TO_DISPLAY_INDEXED_FEATURES
    }

    fn bases_per_unit(&self) -> f64 {
        self.bases_per_pixel
    }

    fn from_bases_per_unit(bases: f64) -> Self {
        Self {
            bases_per_pixel: bases.max(1.0 / Self::MAX_PIXELS_PER_BASE),
        }
    }
}

/// The focus, the zoom, and each alignment track's vertical scroll.
#[derive(Clone, Debug, PartialEq)]
pub struct Viewport<Z: Scale> {
    pub focus: Focus,
    pub zoom: Z,
    /// The first row shown in each alignment track, by alignment index.
    pub y: Vec<usize>,
}

impl<Z: Scale> Viewport<Z> {
    pub fn new(focus: Focus, zoom: Z, alignment_count: usize) -> Self {
        Self {
            focus,
            zoom,
            y: vec![0; alignment_count],
        }
    }

    pub fn scroll(&mut self, index: usize, scroll: Scroll, depth: usize) {
        match scroll {
            Scroll::Up(n) => self.y[index] = self.y[index].saturating_sub(n),
            Scroll::Down(n) => self.y[index] = self.y[index].saturating_add(n).min(depth),
            Scroll::Top => self.y[index] = 0,
            Scroll::Bottom => self.y[index] = depth.saturating_sub(1),
        }
    }

    /// The region shown in a track area `width` units wide.
    pub fn region(&self, width: f64) -> Region {
        Region {
            focus: self.focus.clone(),
            half_width: self.zoom.half_width(width),
        }
    }

    /// Applies a zoom, then keeps the view within the contig.
    pub fn zoom(
        &mut self,
        zoom: Zoom,
        width: f64,
        contig_length: Option<u64>,
    ) -> Result<(), TGVError> {
        self.zoom = self.zoom.zoom(&zoom, width)?;
        self.self_correct(width, contig_length);
        Ok(())
    }

    /// Set the top track # of the viewing window.
    /// 0-based.
    pub fn set_y(&mut self, index: usize, y: usize, depth: usize) {
        self.y[index] = usize::min(y, depth.saturating_sub(1))
    }

    /// Top track # of the viewing window.
    /// 0-based, inclusive.
    pub fn top(&self, index: usize) -> usize {
        self.y[index]
    }

    /// Moves the view within the contig: the zoom can't show more than the contig, the right
    /// edge can't pass the contig's end, and the left edge can't pass its start.
    pub fn self_correct(&mut self, width: f64, contig_length: Option<u64>) {
        if width <= 0.0 {
            return;
        }
        if let Some(contig_length) = contig_length {
            self.zoom = self.zoom.clamp(width, contig_length);

            let right = self.region(width).end();
            if right > contig_length {
                self.focus.position = self.focus.position.saturating_sub(right - contig_length);
            }
        }

        // TODO: consider loosening this, so the contig start can be centered.
        self.focus.position = self.focus.position.max(1 + self.zoom.half_width(width));
    }
}

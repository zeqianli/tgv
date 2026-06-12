use crate::settings::Settings;
use gv_core::{
    alignment::Alignment,
    error::TGVError,
    intervals::{Focus, GenomeInterval, Region},
    message::{Scroll, Zoom},
    repository::RepositoryFileIndex,
};
use ratatui::layout::{Constraint, Direction, Layout, Rect};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AreaType {
    Cytoband,
    Coordinate,
    Coverage(usize),
    Alignment(usize),
    Sequence,
    GeneTrack,
    Console,
    Error,
    Variant(usize),
    Bed(usize),
}

impl AreaType {
    fn constraint(&self) -> Constraint {
        match self {
            AreaType::Cytoband => Constraint::Length(2),
            AreaType::Coordinate => Constraint::Length(2),
            AreaType::Coverage(_) => Constraint::Length(6),
            AreaType::Alignment(_) => Constraint::Fill(1),
            AreaType::Sequence => Constraint::Length(1),
            AreaType::GeneTrack => Constraint::Length(2),
            AreaType::Console => Constraint::Length(2),
            AreaType::Error => Constraint::Length(2),
            AreaType::Variant(_) => Constraint::Length(1),
            AreaType::Bed(_) => Constraint::Length(1),
        }
    }
}

pub struct AlignmentView {
    pub focus: Focus,
    pub zoom: u64,
    pub y: usize,
}

/// States for the alignment view
impl AlignmentView {
    pub const MAX_ZOOM_TO_DISPLAY_ALIGNMENTS: u64 = 32;
    pub const MAX_ZOOM_TO_DISPLAY_SEQUENCES: u64 = 2;

    pub fn new(focus: Focus) -> Self {
        AlignmentView {
            focus,
            zoom: 1,
            y: 0,
        }
    }
    const ALIGNMENT_CACHE_RATIO: u64 = 3;

    pub fn alignment_cache_region(&self, region: Region) -> Region {
        Region {
            focus: region.focus,
            half_width: region.half_width * Self::ALIGNMENT_CACHE_RATIO,
        }
    }

    const SEQUENCE_CACHE_RATIO: u64 = 6;

    pub fn sequence_cache_region(&self, region: Region) -> Region {
        Region {
            focus: region.focus,
            half_width: region.half_width * Self::SEQUENCE_CACHE_RATIO,
        }
    }

    const TRACK_CACHE_RATIO: u64 = 10;

    pub fn track_cache_region(&self, region: Region) -> Region {
        Region {
            focus: region.focus,
            half_width: region.half_width * Self::TRACK_CACHE_RATIO,
        }
    }

    pub fn scroll(&mut self, scroll: Scroll, alignment: &Alignment) {
        match scroll {
            Scroll::Up(n) => self.y = self.y.saturating_sub(n),
            Scroll::Down(n) => self.y = usize::min(self.y.saturating_add(n), alignment.depth()),
            Scroll::Position(y) => self.y = y,
            Scroll::Bottom => self.y = alignment.depth().saturating_sub(1),
        }
    }

    pub fn region(&self, area: &Rect) -> Region {
        Region {
            focus: self.focus.clone(),
            half_width: (area.width as u64 * self.zoom) / 2,
        }
    }

    /// FIXME: cost of this is pretty high. Lots of useless calculation here.
    pub fn left(&self, area: &Rect) -> u64 {
        self.region(area).start()
    }

    /// FIXME: cost of this is pretty high. Lots of useless calculation here.
    pub fn right(&self, area: &Rect) -> u64 {
        self.region(area).end()
    }

    pub fn zoom(
        &mut self,
        zoom: Zoom,
        area: &Rect,
        contig_length: Option<u64>,
    ) -> Result<(), TGVError> {
        self.zoom = match zoom {
            Zoom::In(r) => {
                if r == 0 {
                    return Err(TGVError::ValueError(
                        "Zoom in factor cannot be 0".to_string(),
                    ));
                };
                u64::max(1, self.zoom / r)
            }
            Zoom::Out(r) => {
                if r == 0 {
                    return Err(TGVError::ValueError(
                        "Zoom out factor cannot be 0".to_string(),
                    ));
                }

                self.zoom * r // will be bounded and self-corrected later
            }
        };

        self.self_correct(area, contig_length);
        Ok(())
    }

    /// Set the top track # of the viewing window.
    /// 0-based.
    pub fn set_y(&mut self, y: usize, depth: usize) {
        self.y = usize::min(y, depth.saturating_sub(1))
    }

    /// Check if the viewing window overlaps with [left, right].
    /// 1-based, inclusive.
    pub fn overlaps_x_interval(&self, left: u64, right: u64, area: &Rect) -> bool {
        // FIXME: can reduce some useless calculation here.
        left <= self.right(area) && right >= self.left(area)
    }

    /// Top track # of the viewing window.
    /// 0-based, inclusive.
    pub fn top(&self) -> usize {
        self.y
    }

    /// Bottom track # of the viewing window.
    /// 0-based, exclusive.
    pub fn bottom(&self, area: &Rect) -> usize {
        self.y + area.height as usize
    }

    /// Move the viewing window be within the contig range.
    pub fn self_correct(&mut self, area: &Rect, contig_length: Option<u64>) {
        if let Some(contig_length) = contig_length {
            // 1. Zoom: cannot be large than contig_length / area.width
            self.zoom = u64::min(self.zoom, contig_length / area.width as u64);

            // 2. Right: cannot be larger than contig_length
            let right = self.region(area).end();
            if right > contig_length {
                self.focus.position = self.focus.position.saturating_sub(right - contig_length);
            }
        }

        // left end must be >=1. TODO: consider loosen this?
        self.focus.position = self
            .focus
            .position
            .max(1 + (area.width as u64 * self.zoom) / 2);
    }

    /// Height of the viewing window.
    // pub fn height(&self, area: &Rect) -> usize {
    //     area.height as usize
    // }

    /// Check if the viewing window overlaps with [top, bottom).
    /// y: 0-based.
    pub fn overlaps_y(&self, y: usize, area: &Rect) -> bool {
        (self.top()..self.bottom(area)).contains(&y)
    }

    /// Returns the onscreen x coordinate in the area. Example:
    /// Bases displayed in the window: 1 2 | 3 4 5 6 7 8 | 9 10
    /// Zoom = 2, window has 3 pixels
    /// 1/2 -> Left(0)
    /// 3/4 -> OnScreen(0)
    /// 5/6 -> OnScreen(1)
    /// 7/8 -> OnScreen(2)
    /// 9/10 -> Right(1)
    ///
    /// x: 1-based
    pub fn onscreen_x_coordinate(&self, x: u64, area: &Rect) -> OnScreenCoordinate {
        // TODO: for now, we assume that left and right area equals to the alignment area. Fix this in the future if we need x axis layouts.
        let self_left = self.left(area);
        let self_right = self.right(area);

        if x < self_left {
            OnScreenCoordinate::Left(usize::max(((self_left - x) / self.zoom) as usize, 1))
        } else if x > self_right {
            OnScreenCoordinate::Right(usize::max(((x - self_right) / self.zoom) as usize, 1))
        } else {
            OnScreenCoordinate::OnScreen(((x - self_left) / self.zoom) as usize)
        }
    }

    /// Given an onscreen x position, return the genome coordinate range (1-based, inclusive) at that x location.
    pub fn coordinates_of_onscreen_x(&self, x: u16, area: &Rect) -> Option<(u64, u64)> {
        if x < area.left() || x >= area.right() {
            return None;
        }

        let left = self.left(area) + (x - area.left()) as u64 * self.zoom;

        Some((left, left + self.zoom - 1))
    }

    /// Given an onscreen x position, return the genome coordinate range (1-based, inclusive) at that x location.
    pub fn coordinate_of_onscreen_y(&self, y: u16, area: &Rect) -> Option<usize> {
        if y < area.top() || y >= area.bottom() {
            return None;
        }

        Some(self.top() + (y - area.top()) as usize)
    }

    /// Returns the onscreen y coordinate in the area. Example
    /// y: 0-based.
    pub fn onscreen_y_coordinate(&self, y: usize, area: &Rect) -> OnScreenCoordinate {
        let self_top = self.top();
        let self_bottom = self.bottom(area);

        if y < self_top {
            OnScreenCoordinate::Left(self_top - y)
        } else if y >= self_bottom {
            OnScreenCoordinate::Right(y - self_bottom) // Note that this is different from the x coordinate. TODO: think about this.
        } else {
            OnScreenCoordinate::OnScreen(y - self_top)
        }
    }
}

/// Main page layout
pub struct MainLayout {
    pub tracks: Vec<AreaType>,

    pub main_area: Rect,

    pub areas: Vec<(AreaType, Rect)>,
}

impl MainLayout {
    pub fn new(settings: &Settings, repository_file_indexes: &[RepositoryFileIndex]) -> Self {
        let mut tracks = vec![];
        if settings.core.reference.needs_track() {
            tracks.push(AreaType::Cytoband);
        }

        if settings.core.reference.needs_sequence() || settings.core.reference.needs_track() {
            tracks.push(AreaType::Coordinate);
        }

        for repository_file_index in repository_file_indexes {
            match repository_file_index {
                RepositoryFileIndex::Alignment(index) => {
                    tracks.push(AreaType::Coverage(*index));
                    tracks.push(AreaType::Alignment(*index));
                }
                RepositoryFileIndex::Variant(index) => tracks.push(AreaType::Variant(*index)),
                RepositoryFileIndex::Bed(index) => tracks.push(AreaType::Bed(*index)),
            }
        }

        if settings.core.reference.needs_sequence() {
            tracks.push(AreaType::Sequence);
        }
        if settings.core.reference.needs_track() {
            tracks.push(AreaType::GeneTrack);
        }

        tracks.push(AreaType::Console);
        tracks.push(AreaType::Error);

        MainLayout {
            tracks: tracks,
            main_area: Rect::default(),
            areas: Vec::new(),
        }
    }
    /// Update the area. If the area size changed, terminal refresh is needed.
    pub fn set_area(&mut self, area: Rect) -> bool {
        if area.width != self.main_area.width || area.height != self.main_area.height {
            self.main_area = area;

            let constraints = self
                .tracks
                .iter()
                .map(AreaType::constraint)
                .collect::<Vec<_>>();
            let areas = Layout::default()
                .direction(Direction::Vertical)
                .constraints(constraints)
                .split(area);

            self.areas = self
                .tracks
                .iter()
                .copied()
                .zip(areas.iter().copied())
                .collect();
            true
        } else {
            false
        }
    }

    pub fn get_area_type_at_position(&self, x: u16, y: u16) -> Option<&(AreaType, Rect)> {
        self.areas.iter().find(|(_area_type, area)| {
            x >= area.x && x < area.right() && y >= area.y && y < area.bottom()
        })
    }
}

pub enum OnScreenCoordinate {
    /// Coordinate on left side of the screen.
    /// The last pixel is 1.
    Left(usize),

    /// Coordinate on screen.
    /// First pixel is 0.
    OnScreen(usize),

    /// Coordinate on right side of the screen.
    /// The first pixel is 1.
    Right(usize),
}

impl OnScreenCoordinate {
    pub fn width(
        left: &OnScreenCoordinate,  // inclusive
        right: &OnScreenCoordinate, // inclusive
        area: &Rect,
    ) -> usize {
        match (left, right) {
            (OnScreenCoordinate::OnScreen(a), OnScreenCoordinate::OnScreen(b))
            | (OnScreenCoordinate::Left(a), OnScreenCoordinate::Left(b))
            | (OnScreenCoordinate::Right(a), OnScreenCoordinate::Right(b)) => a.abs_diff(*b) + 1,

            (OnScreenCoordinate::Left(a), OnScreenCoordinate::OnScreen(b))
            | (OnScreenCoordinate::OnScreen(a), OnScreenCoordinate::Left(b)) => b + a + 1,

            (OnScreenCoordinate::Left(a), OnScreenCoordinate::Right(b))
            | (OnScreenCoordinate::Right(a), OnScreenCoordinate::Left(b)) => {
                a + b + area.width as usize
            }

            (OnScreenCoordinate::OnScreen(a), OnScreenCoordinate::Right(b)) => {
                area.width as usize - a + b
            }
            (OnScreenCoordinate::Right(a), OnScreenCoordinate::OnScreen(b)) => {
                area.width as usize - b + a
            }
        }
    }

    pub fn get(&self) -> usize {
        match self {
            OnScreenCoordinate::Left(a) => *a,
            OnScreenCoordinate::OnScreen(a) => *a,
            OnScreenCoordinate::Right(a) => *a,
        }
    }

    pub fn onscreen_start_and_length(
        left: &OnScreenCoordinate,  // inclusive
        right: &OnScreenCoordinate, // inclusive
        area: &Rect,
    ) -> Option<(u16, u16)> {
        match (left, right) {
            (OnScreenCoordinate::Left(_a), OnScreenCoordinate::Left(_b)) => None,

            (OnScreenCoordinate::Left(_a), OnScreenCoordinate::OnScreen(b)) => {
                Some((0, (b + 1) as u16))
            }

            (OnScreenCoordinate::Left(_a), OnScreenCoordinate::Right(_b)) => Some((0, area.width)),

            (OnScreenCoordinate::OnScreen(_a), OnScreenCoordinate::Left(_b)) => None,

            (OnScreenCoordinate::OnScreen(a), OnScreenCoordinate::OnScreen(b)) => {
                if a > b {
                    return None;
                }
                Some((*a as u16, (b - a + 1) as u16))
            }

            (OnScreenCoordinate::OnScreen(a), OnScreenCoordinate::Right(_b)) => {
                Some((*a as u16, (area.width - *a as u16)))
            }
            (OnScreenCoordinate::Right(_a), OnScreenCoordinate::Left(_b)) => None,

            (OnScreenCoordinate::Right(_a), OnScreenCoordinate::OnScreen(_b)) => None,

            (OnScreenCoordinate::Right(_a), OnScreenCoordinate::Right(_b)) => None,
        }
    }
}

pub fn linear_scale(
    original_x: u64,
    original_length: u64,
    new_start: u16,
    new_end: u16,
) -> Result<u16, TGVError> {
    if original_length == 0 {
        return Err(TGVError::ValueError(
            "Trying to linear scale with original_length = 0 when rendering cytoband".to_string(),
        ));
    }
    Ok(new_start
        + (original_x as f64 / (original_length) as f64 * (new_end - new_start) as f64) as u16)
}

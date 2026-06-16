use crate::settings::Settings;
use gv_core::{
    alignment::Alignment,
    error::TGVError,
    intervals::{Focus, GenomeInterval, Region},
    message::{Scroll, Zoom},
    repository::RepositoryFileIndex,
};
use ratatui::layout::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AreaType {
    Cytoband,
    Coordinate,
    Coverage(usize),
    Alignment(usize),
    AlignmentDivider { upper: usize, lower: usize },
    Sequence,
    GeneTrack,
    Console,
    Error,
    Variant(usize),
    Bed(usize),
}

impl AreaType {
    fn desired_height(&self) -> Option<u16> {
        match self {
            AreaType::Cytoband => Some(2),
            AreaType::Coordinate => Some(2),
            AreaType::Coverage(_) => Some(MainLayout::COVERAGE_HEIGHT),
            AreaType::Alignment(_) => None,
            AreaType::AlignmentDivider { .. } => Some(1),
            AreaType::Sequence => Some(1),
            AreaType::GeneTrack => Some(2),
            AreaType::Console => Some(2),
            AreaType::Error => Some(2),
            AreaType::Variant(_) => Some(1),
            AreaType::Bed(_) => Some(1),
        }
    }
}

pub struct AlignmentView {
    pub focus: Focus,
    pub zoom: u64,
    pub y: Vec<usize>,
}

/// States for the alignment view
impl AlignmentView {
    pub const MAX_ZOOM_TO_DISPLAY_ALIGNMENTS: u64 = 32;
    pub const MAX_ZOOM_TO_DISPLAY_SEQUENCES: u64 = 2;

    pub fn new(focus: Focus) -> Self {
        AlignmentView {
            focus,
            zoom: 1,
            y: Vec::new(),
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

    pub fn scroll(&mut self, scroll: Scroll, alignments: &[Alignment]) {
        match scroll {
            Scroll::Up { index, n } => {
                if alignments.get(index).is_some() {
                    self.ensure_y(index);
                    self.y[index] = self.y[index].saturating_sub(n);
                }
            }
            Scroll::Down { index, n } => {
                if let Some(alignment) = alignments.get(index) {
                    self.ensure_y(index);
                    self.y[index] = usize::min(self.y[index].saturating_add(n), alignment.depth());
                }
            }
            Scroll::Position(y) => {
                if !alignments.is_empty() {
                    self.ensure_y(alignments.len() - 1);
                    self.y.iter_mut().for_each(|alignment_y| *alignment_y = y);
                }
            }
            Scroll::Bottom => {
                if !alignments.is_empty() {
                    self.ensure_y(alignments.len() - 1);
                    for (index, alignment) in alignments.iter().enumerate() {
                        self.y[index] = alignment.depth().saturating_sub(1);
                    }
                }
            }
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
    pub fn set_y(&mut self, index: usize, y: usize, depth: usize) {
        self.ensure_y(index);
        self.y[index] = usize::min(y, depth.saturating_sub(1))
    }

    /// Check if the viewing window overlaps with [left, right].
    /// 1-based, inclusive.
    pub fn overlaps_x_interval(&self, left: u64, right: u64, area: &Rect) -> bool {
        // FIXME: can reduce some useless calculation here.
        left <= self.right(area) && right >= self.left(area)
    }

    /// Top track # of the viewing window.
    /// 0-based, inclusive.
    pub fn top(&self, index: usize) -> usize {
        self.y.get(index).copied().unwrap_or_default()
    }

    /// Bottom track # of the viewing window.
    /// 0-based, exclusive.
    pub fn bottom(&self, index: usize, area: &Rect) -> usize {
        self.top(index) + area.height as usize
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
    pub fn overlaps_y(&self, index: usize, y: usize, area: &Rect) -> bool {
        (self.top(index)..self.bottom(index, area)).contains(&y)
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
    pub fn coordinate_of_onscreen_y(&self, index: usize, y: u16, area: &Rect) -> Option<usize> {
        if y < area.top() || y >= area.bottom() {
            return None;
        }

        Some(self.top(index) + (y - area.top()) as usize)
    }

    /// Returns the onscreen y coordinate in the area. Example
    /// y: 0-based.
    pub fn onscreen_y_coordinate(&self, index: usize, y: usize, area: &Rect) -> OnScreenCoordinate {
        let self_top = self.top(index);
        let self_bottom = self.bottom(index, area);

        if y < self_top {
            OnScreenCoordinate::Left(self_top - y)
        } else if y >= self_bottom {
            OnScreenCoordinate::Right(y - self_bottom) // Note that this is different from the x coordinate. TODO: think about this.
        } else {
            OnScreenCoordinate::OnScreen(y - self_top)
        }
    }

    fn ensure_y(&mut self, index: usize) {
        if self.y.len() <= index {
            self.y.resize(index + 1, 0);
        }
    }
}

/// Main page layout
pub struct MainLayout {
    pub tracks: Vec<AreaType>,

    pub main_area: Rect,

    pub areas: Vec<(AreaType, Rect)>,

    alignment_heights: Vec<u16>,
}

impl MainLayout {
    const ALIGNMENT_MIN_HEIGHT: u16 = 1;
    const COVERAGE_HEIGHT: u16 = 6;

    pub fn new(settings: &Settings, repository_file_indexes: &[RepositoryFileIndex]) -> Self {
        let mut tracks = vec![];
        if settings.core.reference.needs_track() {
            tracks.push(AreaType::Cytoband);
        }

        if settings.core.reference.needs_sequence() || settings.core.reference.needs_track() {
            tracks.push(AreaType::Coordinate);
        }

        let mut last_alignment_index = None;
        for repository_file_index in repository_file_indexes {
            match repository_file_index {
                RepositoryFileIndex::Alignment(index) => {
                    if let Some(upper) = last_alignment_index {
                        tracks.push(AreaType::AlignmentDivider {
                            upper,
                            lower: *index,
                        });
                    }
                    tracks.push(AreaType::Coverage(*index));
                    tracks.push(AreaType::Alignment(*index));
                    last_alignment_index = Some(*index);
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
            tracks,
            main_area: Rect::default(),
            areas: Vec::new(),
            alignment_heights: Vec::new(),
        }
    }

    /// Update the area. If the area size changed, terminal refresh is needed.
    pub fn set_area(&mut self, area: Rect) -> bool {
        if area.width != self.main_area.width || area.height != self.main_area.height {
            self.main_area = area;
            self.recalculate_areas();
            true
        } else {
            false
        }
    }

    pub fn resize_alignment_pair(&mut self, upper: usize, lower: usize, delta_rows: i32) {
        if delta_rows == 0 {
            return;
        }

        self.capture_current_alignment_heights();
        self.ensure_alignment_height(usize::max(upper, lower));

        let minimum_height = if self.can_fit_alignment_minimums() {
            Self::ALIGNMENT_MIN_HEIGHT
        } else {
            0
        };
        let upper_height = self.alignment_heights[upper];
        let lower_height = self.alignment_heights[lower];
        let actual_delta = if delta_rows > 0 {
            delta_rows.min((lower_height.saturating_sub(minimum_height)) as i32)
        } else {
            delta_rows.max(-((upper_height.saturating_sub(minimum_height)) as i32))
        };

        if actual_delta == 0 {
            return;
        }

        if actual_delta > 0 {
            let actual_delta = actual_delta as u16;
            self.alignment_heights[upper] = upper_height.saturating_add(actual_delta);
            self.alignment_heights[lower] = lower_height.saturating_sub(actual_delta);
        } else {
            let actual_delta = (-actual_delta) as u16;
            self.alignment_heights[upper] = upper_height.saturating_sub(actual_delta);
            self.alignment_heights[lower] = lower_height.saturating_add(actual_delta);
        }

        self.recalculate_areas();
    }

    fn recalculate_areas(&mut self) {
        let alignment_heights = self.resolved_alignment_heights();
        let mut y = self.main_area.y;
        let mut remaining_height = self.main_area.height;

        self.areas = self
            .tracks
            .iter()
            .map(|track| {
                let desired_height = match track {
                    AreaType::Alignment(index) => {
                        alignment_heights.get(*index).copied().unwrap_or(0)
                    }
                    _ => track.desired_height().unwrap_or_default(),
                };
                let height = u16::min(desired_height, remaining_height);
                let rect = Rect::new(self.main_area.x, y, self.main_area.width, height);
                y = y.saturating_add(height);
                remaining_height = remaining_height.saturating_sub(height);
                (*track, rect)
            })
            .collect();
    }

    fn resolved_alignment_heights(&mut self) -> Vec<u16> {
        let alignment_count = self.alignment_count();
        if alignment_count == 0 {
            return Vec::new();
        }

        self.ensure_alignment_height(alignment_count - 1);
        let fixed_height = self.fixed_desired_height();
        let available_height = self.main_area.height.saturating_sub(fixed_height);

        if available_height < alignment_count as u16 * Self::ALIGNMENT_MIN_HEIGHT {
            return (0..alignment_count)
                .scan(available_height, |remaining_height, _| {
                    let height = u16::min(Self::ALIGNMENT_MIN_HEIGHT, *remaining_height);
                    *remaining_height = remaining_height.saturating_sub(height);
                    Some(height)
                })
                .collect();
        }

        let mut heights = Vec::with_capacity(alignment_count);
        let mut remaining_height = available_height;
        for index in 0..alignment_count {
            let remaining_alignments = alignment_count - index - 1;
            let reserved_height = remaining_alignments as u16 * Self::ALIGNMENT_MIN_HEIGHT;
            let maximum_height = remaining_height.saturating_sub(reserved_height);
            let height = self.alignment_heights[index]
                .max(Self::ALIGNMENT_MIN_HEIGHT)
                .min(maximum_height);
            heights.push(height);
            remaining_height = remaining_height.saturating_sub(height);
        }

        if remaining_height > 0 {
            let shared_extra_height = remaining_height / alignment_count as u16;
            let mut extra_remainder = remaining_height % alignment_count as u16;
            for height in &mut heights {
                *height = height.saturating_add(shared_extra_height);
                if extra_remainder > 0 {
                    *height = height.saturating_add(1);
                    extra_remainder -= 1;
                }
            }
        }

        heights
    }

    fn capture_current_alignment_heights(&mut self) {
        let alignment_count = self.alignment_count();
        if alignment_count == 0 {
            return;
        }

        self.ensure_alignment_height(alignment_count - 1);
        for (area_type, area) in &self.areas {
            if let AreaType::Alignment(index) = area_type {
                self.alignment_heights[*index] = area.height;
            }
        }
    }

    fn can_fit_alignment_minimums(&self) -> bool {
        let alignment_count = self.alignment_count() as u16;
        self.main_area
            .height
            .saturating_sub(self.fixed_desired_height())
            >= alignment_count.saturating_mul(Self::ALIGNMENT_MIN_HEIGHT)
    }

    fn fixed_desired_height(&self) -> u16 {
        self.tracks
            .iter()
            .filter_map(AreaType::desired_height)
            .fold(0, u16::saturating_add)
    }

    fn alignment_count(&self) -> usize {
        self.tracks
            .iter()
            .filter_map(|area_type| match area_type {
                AreaType::Alignment(index) => Some(*index + 1),
                _ => None,
            })
            .max()
            .unwrap_or_default()
    }

    fn ensure_alignment_height(&mut self, index: usize) {
        if self.alignment_heights.len() <= index {
            self.alignment_heights
                .resize(index + 1, Self::ALIGNMENT_MIN_HEIGHT);
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

#[cfg(test)]
mod tests {
    use super::*;
    use gv_core::reference::Reference;
    use rstest::rstest;

    fn settings_without_reference() -> Settings {
        let mut settings = Settings::default();
        settings.core.reference = Reference::NoReference;
        settings
    }

    fn alignment_layout(alignment_count: usize, height: u16) -> MainLayout {
        let settings = settings_without_reference();
        let repository_file_indexes = (0..alignment_count)
            .map(RepositoryFileIndex::Alignment)
            .collect::<Vec<_>>();
        let mut layout = MainLayout::new(&settings, &repository_file_indexes);
        layout.set_area(Rect::new(0, 0, 80, height));
        layout
    }

    fn area_height(layout: &MainLayout, expected_area_type: AreaType) -> u16 {
        layout
            .areas
            .iter()
            .find_map(|(area_type, area)| (*area_type == expected_area_type).then_some(area.height))
            .expect("area exists")
    }

    fn alignment_with_depth(depth: usize) -> Alignment {
        let mut alignment = Alignment::default();
        alignment.ys_index.resize(depth, Vec::new());
        alignment
    }

    #[rstest]
    #[case(vec![], vec![AreaType::Console, AreaType::Error])]
    #[case(
        vec![RepositoryFileIndex::Alignment(0)],
        vec![
            AreaType::Coverage(0),
            AreaType::Alignment(0),
            AreaType::Console,
            AreaType::Error,
        ]
    )]
    #[case(
        vec![
            RepositoryFileIndex::Alignment(0),
            RepositoryFileIndex::Alignment(1),
            RepositoryFileIndex::Alignment(2),
        ],
        vec![
            AreaType::Coverage(0),
            AreaType::Alignment(0),
            AreaType::AlignmentDivider { upper: 0, lower: 1 },
            AreaType::Coverage(1),
            AreaType::Alignment(1),
            AreaType::AlignmentDivider { upper: 1, lower: 2 },
            AreaType::Coverage(2),
            AreaType::Alignment(2),
            AreaType::Console,
            AreaType::Error,
        ]
    )]
    #[case(
        vec![
            RepositoryFileIndex::Variant(0),
            RepositoryFileIndex::Alignment(0),
            RepositoryFileIndex::Bed(0),
            RepositoryFileIndex::Alignment(1),
        ],
        vec![
            AreaType::Variant(0),
            AreaType::Coverage(0),
            AreaType::Alignment(0),
            AreaType::Bed(0),
            AreaType::AlignmentDivider { upper: 0, lower: 1 },
            AreaType::Coverage(1),
            AreaType::Alignment(1),
            AreaType::Console,
            AreaType::Error,
        ]
    )]
    fn layout_adds_alignment_dividers_between_alignment_groups(
        #[case] repository_file_indexes: Vec<RepositoryFileIndex>,
        #[case] expected_tracks: Vec<AreaType>,
    ) {
        let settings = settings_without_reference();

        let layout = MainLayout::new(&settings, &repository_file_indexes);
        assert_eq!(layout.tracks, expected_tracks);
    }

    #[test]
    fn alignment_view_scrolls_only_the_requested_alignment() {
        let alignments = vec![alignment_with_depth(10), alignment_with_depth(10)];
        let mut alignment_view = AlignmentView::new(Focus::default());

        alignment_view.scroll(Scroll::Down { index: 1, n: 3 }, &alignments);
        assert_eq!(alignment_view.top(0), 0);
        assert_eq!(alignment_view.top(1), 3);

        alignment_view.scroll(Scroll::Up { index: 1, n: 1 }, &alignments);
        assert_eq!(alignment_view.top(0), 0);
        assert_eq!(alignment_view.top(1), 2);

        alignment_view.scroll(Scroll::Down { index: 0, n: 4 }, &alignments);
        assert_eq!(alignment_view.top(0), 4);
        assert_eq!(alignment_view.top(1), 2);
    }

    #[rstest]
    #[case(1, 1)]
    #[case(2, 1)]
    fn resizing_alignment_divider_moves_height_between_adjacent_alignments(
        #[case] initial_delta: i16,
        #[case] second_delta: i16,
    ) {
        let mut layout = alignment_layout(2, 24);
        let initial_upper_height = area_height(&layout, AreaType::Alignment(0));
        let initial_lower_height = area_height(&layout, AreaType::Alignment(1));
        let initial_first_coverage_height = area_height(&layout, AreaType::Coverage(0));
        let initial_second_coverage_height = area_height(&layout, AreaType::Coverage(1));

        layout.resize_alignment_pair(0, 1, initial_delta as i32);
        assert_eq!(
            area_height(&layout, AreaType::Alignment(0)),
            initial_upper_height + initial_delta as u16
        );
        assert_eq!(
            area_height(&layout, AreaType::Alignment(1)),
            initial_lower_height - initial_delta as u16
        );
        assert_eq!(
            area_height(&layout, AreaType::Coverage(0)),
            initial_first_coverage_height
        );
        assert_eq!(
            area_height(&layout, AreaType::Coverage(1)),
            initial_second_coverage_height
        );

        layout.resize_alignment_pair(0, 1, -(second_delta as i32));
        assert_eq!(
            area_height(&layout, AreaType::Alignment(0)),
            initial_upper_height + initial_delta as u16 - second_delta as u16
        );
        assert_eq!(
            area_height(&layout, AreaType::Alignment(1)),
            initial_lower_height - initial_delta as u16 + second_delta as u16
        );
    }

    #[rstest]
    #[case(99, 6, 1)]
    #[case(-99, 1, 6)]
    fn resizing_alignment_divider_clamps_to_minimum_alignment_height(
        #[case] delta: i16,
        #[case] expected_upper_height: u16,
        #[case] expected_lower_height: u16,
    ) {
        let mut layout = alignment_layout(2, 24);

        layout.resize_alignment_pair(0, 1, delta as i32);

        assert_eq!(
            area_height(&layout, AreaType::Alignment(0)),
            expected_upper_height
        );
        assert_eq!(
            area_height(&layout, AreaType::Alignment(1)),
            expected_lower_height
        );
    }

    #[test]
    fn small_windows_allocate_layout_top_first() {
        let layout = alignment_layout(3, 16);

        assert_eq!(area_height(&layout, AreaType::Coverage(0)), 6);
        assert_eq!(area_height(&layout, AreaType::Alignment(0)), 0);
        assert_eq!(
            area_height(&layout, AreaType::AlignmentDivider { upper: 0, lower: 1 }),
            1
        );
        assert_eq!(area_height(&layout, AreaType::Coverage(1)), 6);
        assert_eq!(area_height(&layout, AreaType::Alignment(1)), 0);
        assert_eq!(
            area_height(&layout, AreaType::AlignmentDivider { upper: 1, lower: 2 }),
            1
        );
        assert_eq!(area_height(&layout, AreaType::Coverage(2)), 2);
        assert_eq!(area_height(&layout, AreaType::Alignment(2)), 0);
        assert_eq!(area_height(&layout, AreaType::Console), 0);
        assert_eq!(area_height(&layout, AreaType::Error), 0);
    }
}

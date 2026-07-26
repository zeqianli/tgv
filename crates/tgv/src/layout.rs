use gv_core::{
    alignment::Alignment,
    error::TGVError,
    intervals::{Focus, GenomeInterval, Region},
    message::{Scroll, Zoom},
    reference::Reference,
    repository::{Repository, RepositoryFileIndex},
};
use ratatui::layout::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainLayoutArea {
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
    Fill,
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

    pub fn new(focus: Focus, alignment_count: usize) -> Self {
        AlignmentView {
            focus,
            zoom: 1,
            y: vec![0; alignment_count],
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
                if !alignments.is_empty() {
                    self.y[index] = self.y[index].saturating_sub(n);
                }
            }
            Scroll::Down { index, n } => {
                if !alignments.is_empty() {
                    self.y[index] =
                        usize::min(self.y[index].saturating_add(n), alignments[index].depth());
                }
            }
            Scroll::Position(y) => {
                if !alignments.is_empty() {
                    self.y.iter_mut().for_each(|alignment_y| *alignment_y = y);
                }
            }
            Scroll::Bottom => {
                if !alignments.is_empty() {
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
        self.y[index]
    }

    /// Bottom track # of the viewing window.
    /// 0-based, exclusive.
    pub fn bottom(&self, index: usize, area: &Rect) -> usize {
        self.top(index) + area.height as usize
    }

    /// Move the viewing window be within the contig range.
    pub fn self_correct(&mut self, area: &Rect, contig_length: Option<u64>) {
        if area.width == 0 {
            return;
        }

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
}

#[derive(Debug, Clone, Copy)]
enum SidebarState {
    Expanded { requested_width: u16 },
    Collapsed { requested_width: u16 },
}

pub struct ResolvedMainLayout {
    pub terminal_area: Rect,
    pub sidebar_area: Rect,
    pub sidebar_divider_area: Rect,
    pub main_area: Rect,
    pub scrollbar_area: Rect,
    pub scrollbar_thumb_area: Rect,
    /// Each entry contains the identity, full render rectangle, source rectangle, and destination rectangle.
    pub track_rects: Vec<(MainLayoutArea, Rect, Rect, Rect)>,
    /// Each entry contains the repository identity, full label rectangle, source rectangle, and destination rectangle.
    pub file_rects: Vec<(RepositoryFileIndex, Rect, Rect, Rect)>,
}

/// Persistent interaction state for the main page layout.
pub struct MainLayout {
    sidebar: SidebarState,
    alignment_heights: Vec<u16>,
    requested_scroll_offset: usize,
}

impl MainLayout {
    const ALIGNMENT_MIN_HEIGHT: u16 = 1;
    const COVERAGE_HEIGHT: u16 = 6;
    pub const SIDEBAR_DEFAULT_WIDTH: u16 = 20;
    pub const SIDEBAR_MIN_WIDTH: u16 = 12;
    const SIDEBAR_DIVIDER_WIDTH: u16 = 1;
    const SCROLLBAR_WIDTH: u16 = 1;
    const MAIN_MIN_WIDTH: u16 = 1;

    fn desired_height(area: MainLayoutArea) -> Option<u16> {
        match area {
            MainLayoutArea::Cytoband => Some(2),
            MainLayoutArea::Coordinate => Some(2),
            MainLayoutArea::Coverage(_) => Some(Self::COVERAGE_HEIGHT),
            MainLayoutArea::Alignment(_) => None,
            MainLayoutArea::AlignmentDivider { .. } => Some(1),
            MainLayoutArea::Sequence => Some(1),
            MainLayoutArea::GeneTrack => Some(2),
            MainLayoutArea::Console => Some(2),
            MainLayoutArea::Error => Some(2),
            MainLayoutArea::Variant(_) => Some(1),
            MainLayoutArea::Bed(_) => Some(1),
            MainLayoutArea::Fill => None,
        }
    }

    pub fn new(alignment_count: usize) -> Self {
        Self {
            sidebar: SidebarState::Expanded {
                requested_width: Self::SIDEBAR_DEFAULT_WIDTH,
            },
            alignment_heights: vec![Self::ALIGNMENT_MIN_HEIGHT; alignment_count],
            requested_scroll_offset: 0,
        }
    }

    pub fn toggle_sidebar(&mut self) {
        self.sidebar = match self.sidebar {
            SidebarState::Expanded { requested_width } => {
                SidebarState::Collapsed { requested_width }
            }
            SidebarState::Collapsed { requested_width } => {
                SidebarState::Expanded { requested_width }
            }
        };
    }

    pub fn resize_sidebar_to(&mut self, column: u16, terminal_area: Rect) {
        let SidebarState::Expanded { requested_width } = &mut self.sidebar else {
            return;
        };

        let desired_width = column
            .saturating_sub(terminal_area.x)
            .max(Self::SIDEBAR_MIN_WIDTH);
        let maximum_width = terminal_area
            .width
            .saturating_sub(
                Self::SCROLLBAR_WIDTH + Self::MAIN_MIN_WIDTH + Self::SIDEBAR_DIVIDER_WIDTH,
            )
            .max(Self::SIDEBAR_MIN_WIDTH);
        *requested_width = desired_width.min(maximum_width);
    }

    pub fn resize_alignment_pair(
        &mut self,
        upper: usize,
        lower: usize,
        delta_rows: i32,
        resolved: &ResolvedMainLayout,
    ) {
        if delta_rows == 0 {
            return;
        }

        let mut alignment_heights = vec![Self::ALIGNMENT_MIN_HEIGHT; self.alignment_heights.len()];
        for (area_type, full_rect, _, _) in &resolved.track_rects {
            if let MainLayoutArea::Alignment(index) = area_type {
                alignment_heights[*index] = full_rect.height;
            }
        }
        let previous_alignment_heights = alignment_heights.clone();

        let minimum_height = Self::ALIGNMENT_MIN_HEIGHT;
        let upper_height = alignment_heights[upper];
        let lower_height = alignment_heights[lower];
        let actual_delta = if delta_rows > 0 {
            delta_rows.min((lower_height.saturating_sub(minimum_height)) as i32)
        } else {
            delta_rows.max(-((upper_height.saturating_sub(minimum_height)) as i32))
        };

        if actual_delta == 0 {
            log::trace!(
                "Alignment divider resize was clamped to zero: upper={} lower={} requested_delta_rows={} heights={:?} minimum_height={}",
                upper,
                lower,
                delta_rows,
                previous_alignment_heights,
                minimum_height,
            );
            return;
        }

        if actual_delta > 0 {
            let actual_delta = actual_delta as u16;
            alignment_heights[upper] = upper_height.saturating_add(actual_delta);
            alignment_heights[lower] = lower_height.saturating_sub(actual_delta);
        } else {
            let actual_delta = (-actual_delta) as u16;
            alignment_heights[upper] = upper_height.saturating_sub(actual_delta);
            alignment_heights[lower] = lower_height.saturating_add(actual_delta);
        }

        self.alignment_heights = alignment_heights;
        log::debug!(
            "Alignment divider resized: upper={} lower={} requested_delta_rows={} actual_delta_rows={} previous_alignment_heights={:?} new_alignment_heights={:?}",
            upper,
            lower,
            delta_rows,
            actual_delta,
            previous_alignment_heights,
            self.alignment_heights,
        );
    }

    pub fn page_canvas_toward(&mut self, row: u16, resolved: &ResolvedMainLayout) {
        if resolved.scrollbar_area.width == 0 || resolved.scrollbar_area.height == 0 {
            return;
        }
        if row < resolved.scrollbar_thumb_area.top() {
            self.requested_scroll_offset =
                Self::scroll_offset(resolved).saturating_sub(resolved.main_area.height as usize);
        } else if row >= resolved.scrollbar_thumb_area.bottom() {
            let scroll_limit = Self::scroll_limit(resolved);
            self.requested_scroll_offset = Self::scroll_offset(resolved)
                .saturating_add(resolved.main_area.height as usize)
                .min(scroll_limit);
        }
    }

    pub fn drag_scrollbar_thumb(
        &mut self,
        row: u16,
        grab_offset: u16,
        resolved: &ResolvedMainLayout,
    ) {
        if resolved.scrollbar_area.width == 0 || resolved.scrollbar_area.height == 0 {
            return;
        }
        let travel = resolved
            .scrollbar_area
            .height
            .saturating_sub(resolved.scrollbar_thumb_area.height);
        let scroll_limit = Self::scroll_limit(resolved);
        if travel == 0 || scroll_limit == 0 {
            self.requested_scroll_offset = 0;
            return;
        }

        let target_start = row
            .saturating_sub(resolved.scrollbar_area.y)
            .saturating_sub(grab_offset)
            .min(travel);
        self.requested_scroll_offset = (target_start as usize)
            .saturating_mul(scroll_limit)
            .saturating_add(travel as usize / 2)
            / travel as usize;
    }

    pub fn resolve(
        &self,
        terminal_area: Rect,
        reference: &Reference,
        repository: &Repository,
    ) -> ResolvedMainLayout {
        let scrollbar_width = terminal_area.width.min(Self::SCROLLBAR_WIDTH);
        let scrollbar_area = Rect::new(
            terminal_area.right().saturating_sub(scrollbar_width),
            terminal_area.y,
            scrollbar_width,
            terminal_area.height,
        );

        let width_before_scrollbar = terminal_area.width.saturating_sub(scrollbar_width);
        let maximum_sidebar_width = width_before_scrollbar
            .saturating_sub(Self::MAIN_MIN_WIDTH + Self::SIDEBAR_DIVIDER_WIDTH);
        let effective_sidebar_width = match self.sidebar {
            SidebarState::Expanded { requested_width } => {
                requested_width.min(maximum_sidebar_width)
            }
            SidebarState::Collapsed { .. } => 0,
        };
        let divider_width = u16::from(effective_sidebar_width > 0).min(Self::SIDEBAR_DIVIDER_WIDTH);

        let sidebar_area = Rect::new(
            terminal_area.x,
            terminal_area.y,
            effective_sidebar_width,
            terminal_area.height,
        );
        let sidebar_divider_area = Rect::new(
            sidebar_area.right(),
            terminal_area.y,
            divider_width,
            terminal_area.height,
        );
        let main_area = Rect::new(
            sidebar_divider_area.right(),
            terminal_area.y,
            width_before_scrollbar
                .saturating_sub(effective_sidebar_width)
                .saturating_sub(divider_width),
            terminal_area.height,
        );

        let tracks = Self::build_tracks(reference, &repository.file_indexes);
        let fixed_height = tracks
            .iter()
            .filter_map(|area| Self::desired_height(*area))
            .fold(0, u16::saturating_add);
        let alignment_heights = Self::resolve_alignment_heights(
            &self.alignment_heights,
            main_area.height,
            fixed_height,
        );

        let fill_height = if alignment_heights.is_empty() {
            main_area.height.saturating_sub(fixed_height)
        } else {
            0
        };

        let mut y = 0usize;
        let content_areas = tracks
            .iter()
            .map(|track| {
                let height = match track {
                    MainLayoutArea::Alignment(index) => alignment_heights[*index],
                    MainLayoutArea::Fill => fill_height,
                    _ => Self::desired_height(*track).unwrap_or_default(),
                };
                let content_area = (*track, y, height);
                y = y.saturating_add(height as usize);
                content_area
            })
            .collect::<Vec<_>>();
        let content_height = y;
        let maximum_scroll_offset = content_height.saturating_sub(main_area.height as usize);
        let scroll_offset = self.requested_scroll_offset.min(maximum_scroll_offset);
        let viewport_start = scroll_offset;
        let viewport_end = viewport_start.saturating_add(main_area.height as usize);

        let track_rects = content_areas
            .iter()
            .map(|(area_type, content_y, height)| {
                let content_end = content_y.saturating_add(*height as usize);
                let visible_start = (*content_y).max(viewport_start);
                let visible_end = content_end.min(viewport_end);
                let visible_height = visible_end.saturating_sub(visible_start) as u16;
                let screen_y = if visible_height == 0 {
                    main_area.bottom()
                } else {
                    main_area
                        .y
                        .saturating_add(visible_start.saturating_sub(viewport_start) as u16)
                };

                let top_clip = visible_start
                    .saturating_sub(*content_y)
                    .min(*height as usize) as u16;
                (
                    *area_type,
                    Rect::new(0, 0, main_area.width, *height),
                    Rect::new(0, top_clip, main_area.width, visible_height),
                    Rect::new(main_area.x, screen_y, main_area.width, visible_height),
                )
            })
            .collect::<Vec<_>>();

        let file_rects = repository
            .file_indexes
            .iter()
            .filter_map(|repository_index| {
                let (first_type, last_type) = match repository_index {
                    RepositoryFileIndex::Alignment(index) => (
                        MainLayoutArea::Coverage(*index),
                        MainLayoutArea::Alignment(*index),
                    ),
                    RepositoryFileIndex::Variant(index) => (
                        MainLayoutArea::Variant(*index),
                        MainLayoutArea::Variant(*index),
                    ),
                    RepositoryFileIndex::Bed(index) => {
                        (MainLayoutArea::Bed(*index), MainLayoutArea::Bed(*index))
                    }
                };
                let first = content_areas
                    .iter()
                    .find(|(area_type, _, _)| *area_type == first_type)?;
                let last = content_areas
                    .iter()
                    .find(|(area_type, _, _)| *area_type == last_type)?;
                let label_start = first.1;
                let label_end = last.1.saturating_add(last.2 as usize);
                let visible_start = label_start.max(viewport_start);
                let visible_end = label_end.min(viewport_end);
                let visible_height = visible_end.saturating_sub(visible_start) as u16;
                let full_height = label_end.saturating_sub(label_start) as u16;
                let top_clip = visible_start.saturating_sub(label_start) as u16;
                (visible_height > 0).then(|| {
                    (
                        *repository_index,
                        Rect::new(0, 0, sidebar_area.width, full_height),
                        Rect::new(0, top_clip, sidebar_area.width, visible_height),
                        Rect::new(
                        sidebar_area.x,
                        main_area
                            .y
                            .saturating_add(visible_start.saturating_sub(viewport_start) as u16),
                        sidebar_area.width,
                        visible_height,
                    ),
                    )
                })
            })
            .collect::<Vec<_>>();

        let track_length = scrollbar_area.height;
        let viewport_length = main_area.height as usize;
        let thumb_length = if scrollbar_area.width == 0 || track_length == 0 {
            0
        } else if content_height <= viewport_length || content_height == 0 {
            track_length
        } else {
            ((track_length as usize).saturating_mul(viewport_length) / content_height)
                .max(1)
                .min(track_length as usize) as u16
        };
        let travel = track_length.saturating_sub(thumb_length);
        let thumb_start = if maximum_scroll_offset == 0 {
            0
        } else {
            (scroll_offset.saturating_mul(travel as usize) / maximum_scroll_offset) as u16
        };
        let scrollbar_thumb_area = Rect::new(
            scrollbar_area.x,
            scrollbar_area.y.saturating_add(thumb_start),
            scrollbar_area.width,
            thumb_length,
        );

        ResolvedMainLayout {
            terminal_area,
            sidebar_area,
            sidebar_divider_area,
            main_area,
            scrollbar_area,
            scrollbar_thumb_area,
            track_rects,
            file_rects,
        }
    }
}

impl MainLayout {
    fn scroll_offset(resolved: &ResolvedMainLayout) -> usize {
        let mut content_y = 0usize;
        for (_, full_rect, source_rect, destination_rect) in &resolved.track_rects {
            if destination_rect.height > 0 {
                return content_y.saturating_add(source_rect.y as usize);
            }
            content_y = content_y.saturating_add(full_rect.height as usize);
        }
        0
    }

    fn scroll_limit(resolved: &ResolvedMainLayout) -> usize {
        resolved
            .track_rects
            .iter()
            .map(|(_, full_rect, _, _)| full_rect.height as usize)
            .fold(0usize, usize::saturating_add)
            .saturating_sub(resolved.main_area.height as usize)
    }

    fn build_tracks(
        reference: &Reference,
        repository_file_indexes: &[RepositoryFileIndex],
    ) -> Vec<MainLayoutArea> {
        let mut tracks = Vec::new();
        if reference.needs_track() {
            tracks.push(MainLayoutArea::Cytoband);
        }
        if reference.needs_sequence() || reference.needs_track() {
            tracks.push(MainLayoutArea::Coordinate);
        }

        let mut last_alignment_index = None;
        for repository_file_index in repository_file_indexes {
            match repository_file_index {
                RepositoryFileIndex::Alignment(index) => {
                    if let Some(upper) = last_alignment_index {
                        tracks.push(MainLayoutArea::AlignmentDivider {
                            upper,
                            lower: *index,
                        });
                    }
                    tracks.push(MainLayoutArea::Coverage(*index));
                    tracks.push(MainLayoutArea::Alignment(*index));
                    last_alignment_index = Some(*index);
                }
                RepositoryFileIndex::Variant(index) => tracks.push(MainLayoutArea::Variant(*index)),
                RepositoryFileIndex::Bed(index) => tracks.push(MainLayoutArea::Bed(*index)),
            }
        }

        if last_alignment_index.is_none() {
            tracks.push(MainLayoutArea::Fill);
        }
        if reference.needs_sequence() {
            tracks.push(MainLayoutArea::Sequence);
        }
        if reference.needs_track() {
            tracks.push(MainLayoutArea::GeneTrack);
        }
        tracks.push(MainLayoutArea::Console);
        tracks.push(MainLayoutArea::Error);
        tracks
    }

    fn resolve_alignment_heights(
        requested_heights: &[u16],
        main_height: u16,
        fixed_height: u16,
    ) -> Vec<u16> {
        let alignment_count = requested_heights.len();
        if alignment_count == 0 {
            return Vec::new();
        }

        let available_height = main_height.saturating_sub(fixed_height);
        let minimum_total_height = u16::try_from(alignment_count)
            .unwrap_or(u16::MAX)
            .saturating_mul(Self::ALIGNMENT_MIN_HEIGHT);
        if available_height < minimum_total_height {
            return vec![Self::ALIGNMENT_MIN_HEIGHT; alignment_count];
        }

        let mut heights = Vec::with_capacity(alignment_count);
        let mut remaining_height = available_height;
        for (index, requested_height) in requested_heights.iter().enumerate() {
            let remaining_alignments = alignment_count - index - 1;
            let reserved_height = u16::try_from(remaining_alignments)
                .unwrap_or(u16::MAX)
                .saturating_mul(Self::ALIGNMENT_MIN_HEIGHT);
            let maximum_height = remaining_height.saturating_sub(reserved_height);
            let height = (*requested_height)
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
    use super::MainLayoutArea as AreaType;
    use super::*;
    use gv_core::reference::Reference;
    use rstest::rstest;

    fn repository(file_indexes: Vec<RepositoryFileIndex>) -> Repository {
        Repository {
            file_indexes,
            alignment_repositories: Vec::new(),
            variant_repositories: Vec::new(),
            bed_repositories: Vec::new(),
            track_service: None,
            sequence_service: None,
        }
    }

    fn resolve(
        layout: &MainLayout,
        repository: &Repository,
        reference: &Reference,
        height: u16,
    ) -> ResolvedMainLayout {
        layout.resolve(Rect::new(0, 0, 80, height), reference, repository)
    }

    fn alignment_layout(
        alignment_count: usize,
        height: u16,
    ) -> (MainLayout, Repository, ResolvedMainLayout) {
        let file_indexes = (0..alignment_count)
            .map(RepositoryFileIndex::Alignment)
            .collect::<Vec<_>>();
        let repository = repository(file_indexes);
        let layout = MainLayout::new(alignment_count);
        let resolved = resolve(&layout, &repository, &Reference::NoReference, height);
        (layout, repository, resolved)
    }

    fn area_height(layout: &ResolvedMainLayout, expected_area_type: AreaType) -> u16 {
        layout
            .track_rects
            .iter()
            .find_map(|(area_type, full_rect, _, _)| {
                (*area_type == expected_area_type).then_some(full_rect.height)
            })
            .expect("area exists")
    }

    fn alignment_with_depth(depth: usize) -> Alignment {
        let mut alignment = Alignment::default();
        alignment.ys_index.resize(depth, Vec::new());
        alignment
    }

    #[rstest]
    #[case(
        vec![],
        vec![AreaType::Fill, AreaType::Console, AreaType::Error]
    )]
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
        let alignment_count = repository_file_indexes
            .iter()
            .filter(|file_index| matches!(file_index, RepositoryFileIndex::Alignment(_)))
            .count();
        let repository = repository(repository_file_indexes);
        let layout = MainLayout::new(alignment_count);
        let resolved = resolve(&layout, &repository, &Reference::NoReference, 24);
        assert_eq!(
            resolved
                .track_rects
                .iter()
                .map(|(area_type, _, _, _)| *area_type)
                .collect::<Vec<_>>(),
            expected_tracks
        );
    }

    #[test]
    fn alignment_view_scrolls_only_the_requested_alignment() {
        let alignments = vec![alignment_with_depth(10), alignment_with_depth(10)];
        let mut alignment_view = AlignmentView::new(Focus::default(), alignments.len());

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
        let (mut layout, repository, mut resolved) = alignment_layout(2, 24);
        let initial_upper_height = area_height(&resolved, AreaType::Alignment(0));
        let initial_lower_height = area_height(&resolved, AreaType::Alignment(1));
        let initial_first_coverage_height = area_height(&resolved, AreaType::Coverage(0));
        let initial_second_coverage_height = area_height(&resolved, AreaType::Coverage(1));

        layout.resize_alignment_pair(0, 1, initial_delta as i32, &resolved);
        resolved = resolve(&layout, &repository, &Reference::NoReference, 24);
        assert_eq!(
            area_height(&resolved, AreaType::Alignment(0)),
            initial_upper_height + initial_delta as u16
        );
        assert_eq!(
            area_height(&resolved, AreaType::Alignment(1)),
            initial_lower_height - initial_delta as u16
        );
        assert_eq!(
            area_height(&resolved, AreaType::Coverage(0)),
            initial_first_coverage_height
        );
        assert_eq!(
            area_height(&resolved, AreaType::Coverage(1)),
            initial_second_coverage_height
        );

        layout.resize_alignment_pair(0, 1, -(second_delta as i32), &resolved);
        resolved = resolve(&layout, &repository, &Reference::NoReference, 24);
        assert_eq!(
            area_height(&resolved, AreaType::Alignment(0)),
            initial_upper_height + initial_delta as u16 - second_delta as u16
        );
        assert_eq!(
            area_height(&resolved, AreaType::Alignment(1)),
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
        let (mut layout, repository, mut resolved) = alignment_layout(2, 24);

        layout.resize_alignment_pair(0, 1, delta as i32, &resolved);
        resolved = resolve(&layout, &repository, &Reference::NoReference, 24);

        assert_eq!(
            area_height(&resolved, AreaType::Alignment(0)),
            expected_upper_height
        );
        assert_eq!(
            area_height(&resolved, AreaType::Alignment(1)),
            expected_lower_height
        );
    }

    #[test]
    fn small_windows_preserve_minimum_track_heights_in_an_overflow_canvas() {
        let (mut layout, repository, mut resolved) = alignment_layout(3, 16);

        assert_eq!(area_height(&resolved, AreaType::Coverage(0)), 6);
        assert_eq!(area_height(&resolved, AreaType::Alignment(0)), 1);
        assert_eq!(
            area_height(&resolved, AreaType::AlignmentDivider { upper: 0, lower: 1 }),
            1
        );
        assert_eq!(area_height(&resolved, AreaType::Coverage(1)), 6);
        assert_eq!(area_height(&resolved, AreaType::Alignment(1)), 1);
        assert_eq!(
            area_height(&resolved, AreaType::AlignmentDivider { upper: 1, lower: 2 }),
            1
        );
        assert_eq!(area_height(&resolved, AreaType::Coverage(2)), 6);
        assert_eq!(area_height(&resolved, AreaType::Alignment(2)), 1);
        assert_eq!(area_height(&resolved, AreaType::Console), 2);
        assert_eq!(area_height(&resolved, AreaType::Error), 2);
        assert_eq!(MainLayout::scroll_limit(&resolved), 11);

        layout.page_canvas_toward(
            resolved.scrollbar_area.bottom().saturating_sub(1),
            &resolved,
        );
        resolved = resolve(&layout, &repository, &Reference::NoReference, 16);
        assert_eq!(MainLayout::scroll_offset(&resolved), 11);
        layout.page_canvas_toward(resolved.scrollbar_area.top(), &resolved);
        resolved = resolve(&layout, &repository, &Reference::NoReference, 16);
        assert_eq!(MainLayout::scroll_offset(&resolved), 0);
    }

    #[test]
    fn no_alignment_layout_uses_fill_to_anchor_reference_tracks() {
        let reference = Reference::BYOIndexedFasta("reference.fa".to_string());
        let repository = repository(vec![
            RepositoryFileIndex::Variant(0),
            RepositoryFileIndex::Bed(0),
        ]);
        let layout = MainLayout::new(0);
        let resolved = resolve(&layout, &repository, &reference, 24);

        assert_eq!(area_height(&resolved, AreaType::Fill), 15);
        let sequence = resolved
            .track_rects
            .iter()
            .find(|(area_type, _, _, _)| *area_type == AreaType::Sequence)
            .expect("the sequence area exists");
        assert_eq!(sequence.3.bottom(), 20);
        assert_eq!(MainLayout::scroll_limit(&resolved), 0);
    }

    #[test]
    fn sidebar_toggles_and_restores_its_requested_width_after_a_resize() {
        let (mut layout, repository, mut resolved) = alignment_layout(1, 24);
        assert_eq!(
            resolved.sidebar_area.width,
            MainLayout::SIDEBAR_DEFAULT_WIDTH
        );

        layout.resize_sidebar_to(30, resolved.terminal_area);
        resolved = resolve(&layout, &repository, &Reference::NoReference, 24);
        assert_eq!(resolved.sidebar_area.width, 30);

        let narrow = layout.resolve(
            Rect::new(0, 0, 10, 24),
            &Reference::NoReference,
            &repository,
        );
        assert_eq!(narrow.sidebar_area.width, 7);
        assert_eq!(narrow.main_area.width, 1);

        resolved = resolve(&layout, &repository, &Reference::NoReference, 24);
        assert_eq!(resolved.sidebar_area.width, 30);
        layout.toggle_sidebar();
        resolved = resolve(&layout, &repository, &Reference::NoReference, 24);
        assert_eq!(resolved.sidebar_area.width, 0);
        assert_eq!(resolved.main_area.width, 79);
    }

    #[test]
    fn scrollbar_drag_maps_the_thumb_to_the_canvas_ends() {
        let (mut layout, repository, mut resolved) = alignment_layout(3, 16);
        assert!(resolved.scrollbar_thumb_area.height < resolved.scrollbar_area.height);
        assert_eq!(
            resolved.scrollbar_thumb_area.top(),
            resolved.scrollbar_area.top()
        );
        let grab_offset = 0;
        layout.drag_scrollbar_thumb(
            resolved.scrollbar_area.bottom().saturating_sub(1),
            grab_offset,
            &resolved,
        );
        resolved = resolve(&layout, &repository, &Reference::NoReference, 16);
        assert_eq!(
            MainLayout::scroll_offset(&resolved),
            MainLayout::scroll_limit(&resolved)
        );

        layout.drag_scrollbar_thumb(resolved.scrollbar_area.y, grab_offset, &resolved);
        resolved = resolve(&layout, &repository, &Reference::NoReference, 16);
        assert_eq!(MainLayout::scroll_offset(&resolved), 0);
    }

    #[test]
    fn file_rects_preserve_repository_identity_and_span_alignment_rows() {
        let repository = repository(vec![
            RepositoryFileIndex::Alignment(0),
            RepositoryFileIndex::Variant(0),
        ]);
        let layout = MainLayout::new(1);
        let resolved = resolve(&layout, &repository, &Reference::NoReference, 24);

        assert_eq!(resolved.file_rects.len(), 2);
        assert_eq!(resolved.file_rects[0].0, RepositoryFileIndex::Alignment(0));
        assert_eq!(
            resolved.file_rects[0].1.height,
            area_height(&resolved, AreaType::Coverage(0))
                + area_height(&resolved, AreaType::Alignment(0))
        );
        assert_eq!(resolved.file_rects[1].0, RepositoryFileIndex::Variant(0));
    }
}

use crate::{
    settings::Settings,
    track_registry::{TrackId, TrackRegistry},
};
use gv_core::{
    message::{Scroll, Zoom},
    prelude::*,
};
use ratatui::layout::Rect;
use std::sync::Arc;
use unicode_width::UnicodeWidthChar;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AreaType {
    Cytoband,
    Coordinate,
    Coverage(TrackId),
    Alignment(TrackId),
    AlignmentDivider { upper: TrackId, lower: TrackId },
    Sequence,
    GeneTrack,
    Console,
    Error,
    Variant(TrackId),
    Bed(TrackId),
    Fill,
}

impl AreaType {
    fn desired_height(&self) -> Option<u16> {
        match self {
            AreaType::Cytoband => Some(2),
            AreaType::Coordinate => Some(2),
            AreaType::Coverage(_) => Some(6),
            AreaType::Alignment(_) => None,
            AreaType::AlignmentDivider { .. } => Some(1),
            AreaType::Sequence => Some(1),
            AreaType::GeneTrack => Some(2),
            AreaType::Console => Some(2),
            AreaType::Error => Some(2),
            AreaType::Variant(_) => Some(1),
            AreaType::Bed(_) => Some(1),
            AreaType::Fill => None,
        }
    }
}

/// Wrap a filename at terminal cell boundaries, including wide characters.
pub(crate) fn wrap_sidebar_label(label: &str, width: u16) -> Vec<String> {
    if width == 0 {
        return Vec::new();
    }

    let mut lines = Vec::new();
    let mut line = String::new();
    let mut line_width = 0usize;
    let width = width as usize;
    for character in label.chars() {
        let character = if character.is_control() {
            '�'
        } else {
            character
        };
        let character_width = UnicodeWidthChar::width(character).unwrap_or(1);
        if line_width + character_width > width && !line.is_empty() {
            lines.push(std::mem::take(&mut line));
            line_width = 0;
        }
        if character_width > width {
            line.push('…');
            lines.push(std::mem::take(&mut line));
            line_width = 0;
        } else {
            line.push(character);
            line_width += character_width;
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
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
        Self::new_with_zoom(focus, 1, alignment_count)
    }

    /// Creates an alignment view with an explicit initial zoom.
    pub fn new_with_zoom(focus: Focus, zoom: u64, alignment_count: usize) -> Self {
        Self {
            focus,
            zoom,
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

    pub fn scroll(&mut self, scroll: Scroll, depth: usize) {
        match scroll {
            Scroll::Up { index, n } => self.y[index] = self.y[index].saturating_sub(n),
            Scroll::Down { index, n } => self.y[index] = self.y[index].saturating_add(n).min(depth),
            Scroll::Position { index, position } => self.y[index] = position,
            Scroll::Bottom { index } => self.y[index] = depth.saturating_sub(1),
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

                self.zoom.saturating_mul(r) // Will be bounded and self-corrected later.
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
            self.zoom = u64::min(self.zoom, contig_length / area.width as u64).max(1);

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

/// Persistent state for the main page layout.
pub struct MainLayout {
    /// Track area types. Length matches number of tracks to display.
    pub tracks: Vec<AreaType>,
    /// Requested heights, indexed in parallel with `tracks`.
    pub track_heights: Vec<Option<u16>>,
    /// Requested sidebar width, retained while the sidebar is hidden.
    pub sidebar_width: u16,
    pub sidebar_visible: bool,
    pub track_registry: Arc<TrackRegistry>,
}

impl MainLayout {
    const ALIGNMENT_MIN_HEIGHT: u16 = 1;
    const SIDEBAR_DEFAULT_WIDTH: u16 = 18;
    const SIDEBAR_MIN_WIDTH: u16 = 6;

    pub fn new(
        settings: &Settings,
        track_registry: Arc<TrackRegistry>,
        visible: &[TrackId],
    ) -> Self {
        let mut tracks = vec![];
        if settings.core.reference.needs_track() {
            tracks.push(AreaType::Cytoband);
        }

        tracks.push(AreaType::Coordinate);

        let mut last_alignment_id = None;
        for &id in visible {
            match track_registry.get(id).repository_index {
                RepositoryFileIndex::Alignment(_) => {
                    if let Some(upper) = last_alignment_id {
                        tracks.push(AreaType::AlignmentDivider { upper, lower: id });
                    }
                    tracks.push(AreaType::Coverage(id));
                    tracks.push(AreaType::Alignment(id));
                    last_alignment_id = Some(id);
                }
                RepositoryFileIndex::Variant(_) => tracks.push(AreaType::Variant(id)),
                RepositoryFileIndex::Bed(_) => tracks.push(AreaType::Bed(id)),
            }
        }

        if last_alignment_id.is_none() {
            tracks.push(AreaType::Fill);
        }
        if settings.core.reference.needs_sequence() {
            tracks.push(AreaType::Sequence);
        }
        if settings.core.reference.needs_track() {
            tracks.push(AreaType::GeneTrack);
        }

        tracks.push(AreaType::Console);
        tracks.push(AreaType::Error);
        let track_heights = tracks.iter().map(AreaType::desired_height).collect();
        MainLayout {
            tracks,
            track_heights,
            sidebar_width: Self::SIDEBAR_DEFAULT_WIDTH,
            sidebar_visible: true,
            track_registry,
        }
    }

    pub fn toggle_sidebar(&mut self) {
        self.sidebar_visible = !self.sidebar_visible;
    }

    pub fn resize_sidebar_to(&mut self, column: u16, terminal_area: Rect) {
        if self.sidebar_visible && terminal_area.width >= Self::SIDEBAR_MIN_WIDTH + 2 {
            self.sidebar_width = column
                .saturating_sub(terminal_area.x)
                .clamp(Self::SIDEBAR_MIN_WIDTH, terminal_area.width - 2);
        }
    }

    pub fn resize_alignment_pair(
        &mut self,
        upper: TrackId,
        lower: TrackId,
        delta_rows: i32,
        resolved: &ResolvedMainLayout,
    ) {
        if delta_rows == 0 {
            return;
        }
        let Some(upper_track) = self
            .tracks
            .iter()
            .position(|area| *area == AreaType::Alignment(upper))
        else {
            return;
        };
        let Some(lower_track) = self
            .tracks
            .iter()
            .position(|area| *area == AreaType::Alignment(lower))
        else {
            return;
        };
        let upper_height = resolved.areas[upper_track].1.height;
        let lower_height = resolved.areas[lower_track].1.height;
        let minimum = if upper_height > 0 && lower_height > 0 {
            Self::ALIGNMENT_MIN_HEIGHT
        } else {
            0
        };
        let actual_delta = delta_rows.clamp(
            -(upper_height.saturating_sub(minimum) as i32),
            lower_height.saturating_sub(minimum) as i32,
        );
        if actual_delta != 0 {
            self.track_heights[upper_track] = Some((upper_height as i32 + actual_delta) as u16);
            self.track_heights[lower_track] = Some((lower_height as i32 - actual_delta) as u16);
        }
    }

    /// Resolve the requested layout within the terminal area.
    pub fn resolve(&self, terminal_area: Rect, repository: &Repository) -> ResolvedMainLayout {
        self.resolve_with_file_path(terminal_area, |index| repository.file_path(index))
    }

    fn resolve_with_file_path<'a>(
        &self,
        terminal_area: Rect,
        file_path: impl Fn(RepositoryFileIndex) -> &'a str,
    ) -> ResolvedMainLayout {
        let file_names: Vec<&str> = self
            .track_registry
            .entries
            .iter()
            .map(|entry| {
                let path = file_path(entry.repository_index);
                path.trim_end_matches(['/', '\\'])
                    .rsplit(['/', '\\'])
                    .next()
                    .filter(|name| !name.is_empty())
                    .unwrap_or(path)
            })
            .collect();
        let sidebar_width =
            if self.sidebar_visible && terminal_area.width >= Self::SIDEBAR_MIN_WIDTH + 2 {
                self.sidebar_width
                    .clamp(Self::SIDEBAR_MIN_WIDTH, terminal_area.width - 2)
            } else {
                0
            };
        let divider_width = u16::from(sidebar_width > 0);
        let main_area = Rect::new(
            terminal_area
                .x
                .saturating_add(sidebar_width + divider_width),
            terminal_area.y,
            terminal_area
                .width
                .saturating_sub(sidebar_width + divider_width),
            terminal_area.height,
        );
        let mut effective_track_heights = self.track_heights.clone();
        if sidebar_width > 0 {
            for (area, height) in self.tracks.iter().zip(&mut effective_track_heights) {
                let track_id = match area {
                    AreaType::Variant(id) | AreaType::Bed(id) => Some(*id),
                    _ => None,
                };
                if let Some(track_id) = track_id {
                    let lines = wrap_sidebar_label(file_names[track_id], sidebar_width).len();
                    *height = Some(lines.saturating_add(1).min(u16::MAX as usize) as u16);
                }
            }
        }

        let fixed_height = self
            .tracks
            .iter()
            .zip(&effective_track_heights)
            .filter(|(area, _)| !matches!(area, AreaType::Alignment(_)))
            .filter_map(|(_, height)| *height)
            .map(usize::from)
            .sum::<usize>();
        let available = (main_area.height as usize).saturating_sub(fixed_height);
        let alignment_tracks: Vec<usize> = self
            .tracks
            .iter()
            .enumerate()
            .filter_map(|(index, area)| matches!(area, AreaType::Alignment(_)).then_some(index))
            .collect();
        let mut alignment_heights = vec![0; alignment_tracks.len()];
        if available < alignment_tracks.len() {
            alignment_heights
                .iter_mut()
                .take(available)
                .for_each(|height| {
                    *height = Self::ALIGNMENT_MIN_HEIGHT;
                });
        } else if !alignment_tracks.is_empty() {
            let mut remaining = available;
            for (position, track_index) in alignment_tracks.iter().enumerate() {
                let reserved = alignment_tracks.len() - position - 1;
                let maximum = remaining.saturating_sub(reserved);
                let requested =
                    self.track_heights[*track_index].unwrap_or(Self::ALIGNMENT_MIN_HEIGHT) as usize;
                let height = requested
                    .max(Self::ALIGNMENT_MIN_HEIGHT as usize)
                    .min(maximum);
                alignment_heights[position] = height as u16;
                remaining -= height;
            }
            for (position, height) in alignment_heights.iter_mut().enumerate() {
                *height += (remaining / alignment_tracks.len()) as u16;
                if position < remaining % alignment_tracks.len() {
                    *height += 1;
                }
            }
        }

        let mut y = main_area.y;
        let mut remaining = main_area.height;
        let mut alignment_position = 0;
        let areas = self
            .tracks
            .iter()
            .zip(&effective_track_heights)
            .map(|(area, requested)| {
                let desired = match area {
                    AreaType::Alignment(_) => {
                        let height = alignment_heights[alignment_position];
                        alignment_position += 1;
                        height
                    }
                    AreaType::Fill => available.min(u16::MAX as usize) as u16,
                    _ => requested.expect("fixed track has a requested height"),
                };
                let height = desired.min(remaining);
                let rect = Rect::new(main_area.x, y, main_area.width, height);
                y = y.saturating_add(height);
                remaining -= height;
                (*area, rect)
            })
            .collect::<Vec<_>>();
        let sidebar_divider_area = Rect::new(
            terminal_area.x.saturating_add(sidebar_width),
            terminal_area.y,
            divider_width,
            terminal_area.height,
        );
        let sidebar_areas = areas
            .iter()
            .map(|(_, area)| Rect::new(terminal_area.x, area.y, sidebar_width, area.height))
            .collect::<Vec<_>>();

        let mut sections = Vec::new();
        let mut track_index = 0;
        while track_index < areas.len() {
            let (area_type, rect) = areas[track_index];
            let section = match area_type {
                AreaType::Cytoband
                    if matches!(areas.get(track_index + 1), Some((AreaType::Coordinate, _))) =>
                {
                    let coordinate = areas[track_index + 1].1;
                    track_index += 2;
                    Rect::new(
                        terminal_area.x,
                        rect.y,
                        sidebar_width,
                        coordinate.bottom().saturating_sub(rect.y),
                    )
                }
                AreaType::Coverage(id) if matches!(areas.get(track_index + 1), Some((AreaType::Alignment(next), _)) if *next == id) =>
                {
                    let alignment = areas[track_index + 1].1;
                    track_index += 2;
                    let bottom = if matches!(
                        areas.get(track_index),
                        Some((AreaType::AlignmentDivider { .. }, _))
                    ) {
                        let divider = areas[track_index].1;
                        track_index += 1;
                        divider.bottom()
                    } else {
                        alignment.bottom()
                    };
                    Rect::new(
                        terminal_area.x,
                        rect.y,
                        sidebar_width,
                        bottom.saturating_sub(rect.y),
                    )
                }
                AreaType::Console
                    if matches!(areas.get(track_index + 1), Some((AreaType::Error, _))) =>
                {
                    let error = areas[track_index + 1].1;
                    track_index += 2;
                    Rect::new(
                        terminal_area.x,
                        rect.y,
                        sidebar_width,
                        error.bottom().saturating_sub(rect.y),
                    )
                }
                _ => {
                    track_index += 1;
                    Rect::new(terminal_area.x, rect.y, sidebar_width, rect.height)
                }
            };
            sections.push((area_type, section));
        }

        let separator_after = |index: usize| {
            let Some((next, _)) = sections.get(index + 1) else {
                return false;
            };
            let current = sections[index].0;
            let is_file = |area| {
                matches!(
                    area,
                    AreaType::Coverage(_) | AreaType::Variant(_) | AreaType::Bed(_)
                )
            };
            (is_file(current) || is_file(*next))
                && !matches!(next, AreaType::Console | AreaType::Error)
        };
        let sidebar_section_dividers = if sidebar_width == 0 {
            Vec::new()
        } else {
            sections
                .iter()
                .enumerate()
                .filter_map(|(index, (_, section))| {
                    (separator_after(index) && section.height > 0).then_some(Rect::new(
                        terminal_area.x,
                        section.bottom() - 1,
                        sidebar_width,
                        1,
                    ))
                })
                .collect()
        };
        let sidebar_alignment_depths = sections
            .iter()
            .enumerate()
            .filter_map(|(index, (area_type, section))| {
                let AreaType::Coverage(id) = area_type else {
                    return None;
                };
                let reserved = u16::from(separator_after(index));
                (sidebar_width > 0 && section.height > reserved).then_some((
                    Rect::new(section.x, section.bottom() - reserved - 1, section.width, 1),
                    *id,
                ))
            })
            .collect();
        let sidebar_labels = sections
            .iter()
            .enumerate()
            .filter_map(|(index, (area_type, section))| {
                let track_id = match area_type {
                    AreaType::Coverage(id) | AreaType::Variant(id) | AreaType::Bed(id) => *id,
                    _ => return None,
                };
                let name = file_names[track_id];
                let reserved = u16::from(separator_after(index))
                    + u16::from(matches!(area_type, AreaType::Coverage(_)));
                let label_area = Rect::new(
                    section.x,
                    section.y,
                    section.width,
                    section.height.saturating_sub(reserved),
                );
                (label_area.height > 0 && label_area.width > 0)
                    .then_some((label_area, name.to_owned()))
            })
            .collect();

        ResolvedMainLayout {
            terminal_area,
            main_area,
            areas,
            sidebar_width,
            sidebar_areas,
            sidebar_divider_area,
            sidebar_section_dividers,
            sidebar_labels,
            sidebar_alignment_depths,
            track_registry: Arc::clone(&self.track_registry),
        }
    }
}

/// Rectangles computed for one terminal size.
#[derive(Default, Clone, Debug)]
pub struct ResolvedMainLayout {
    pub terminal_area: Rect,
    pub main_area: Rect,
    pub areas: Vec<(AreaType, Rect)>,
    pub sidebar_width: u16,
    pub sidebar_areas: Vec<Rect>,
    pub sidebar_divider_area: Rect,
    pub sidebar_section_dividers: Vec<Rect>,
    pub sidebar_labels: Vec<(Rect, String)>,
    pub sidebar_alignment_depths: Vec<(Rect, TrackId)>,
    pub track_registry: Arc<TrackRegistry>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoveringAreaType {
    Sidebar(usize),
    SidebarDivider,
    Track(usize),
    None,
}

impl ResolvedMainLayout {
    pub fn get_area_type_at_position(&self, x: u16, y: u16) -> HoveringAreaType {
        if y < self.terminal_area.y
            || y >= self.terminal_area.bottom()
            || x < self.terminal_area.x
            || x >= self.terminal_area.right()
        {
            HoveringAreaType::None
        } else if self.sidebar_divider_area.width > 0 && x == self.sidebar_divider_area.x {
            HoveringAreaType::SidebarDivider
        } else if x < self.sidebar_divider_area.x {
            self.sidebar_areas
                .iter()
                .enumerate()
                .find_map(|(index, area)| {
                    (y >= area.y && y < area.bottom()).then_some(HoveringAreaType::Sidebar(index))
                })
                .unwrap_or(HoveringAreaType::None)
        } else {
            self.areas
                .iter()
                .enumerate()
                .find_map(|(index, (_, area))| {
                    (x >= area.x && x < area.right() && y >= area.y && y < area.bottom())
                        .then_some(HoveringAreaType::Track(index))
                })
                .unwrap_or(HoveringAreaType::None)
        }
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
    use gv_core::alignment::Alignment;
    use gv_core::{
        reference::Reference,
        settings::{AlignmentPath, FilePath},
    };
    use rstest::rstest;

    fn settings_without_reference(indexes: &[RepositoryFileIndex]) -> Settings {
        let mut settings = Settings::default();
        settings.core.reference = Reference::NoReference;
        settings.core.file_paths = indexes
            .iter()
            .map(|index| match index {
                RepositoryFileIndex::Alignment(index) => {
                    FilePath::AlignmentPath(AlignmentPath::Bam {
                        path: format!("sample-{index}.bam"),
                        index: format!("sample-{index}.bam.bai"),
                        source: gv_core::settings::BamSource::Local,
                    })
                }
                RepositoryFileIndex::Variant(index) => {
                    FilePath::VariantPath(format!("sample-{index}.vcf"))
                }
                RepositoryFileIndex::Bed(index) => FilePath::BedPath(format!("sample-{index}.bed")),
            })
            .collect();
        settings
    }

    fn resolve(layout: &MainLayout, area: Rect) -> ResolvedMainLayout {
        layout.resolve_with_file_path(area, |_| "sample.bam")
    }

    fn layout_for_indexes(indexes: &[RepositoryFileIndex]) -> MainLayout {
        let settings = settings_without_reference(indexes);
        let registry = Arc::new(TrackRegistry::new(indexes));
        let ids = registry
            .entries
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>();
        MainLayout::new(&settings, registry, &ids)
    }

    fn alignment_layout(alignment_count: usize, height: u16) -> (MainLayout, ResolvedMainLayout) {
        let repository_file_indexes = (0..alignment_count)
            .map(RepositoryFileIndex::Alignment)
            .collect::<Vec<_>>();
        let layout = layout_for_indexes(&repository_file_indexes);
        let resolved = resolve(&layout, Rect::new(0, 0, 80, height + 2));
        (layout, resolved)
    }

    fn area_height(layout: &ResolvedMainLayout, expected_area_type: AreaType) -> u16 {
        layout
            .areas
            .iter()
            .find_map(|(area_type, area)| (*area_type == expected_area_type).then_some(area.height))
            .expect("area exists")
    }

    fn alignment_with_depth(depth: usize) -> Alignment {
        let mut alignment = Alignment::default();
        use polars::prelude::*;
        alignment.tables.reads =
            DataFrame::full_null(&gv_core::alignment::tables::reads_schema(), depth)
                .lazy()
                .with_columns([
                    lit(Series::new(
                        "y".into(),
                        (0..depth as u64).collect::<Vec<_>>(),
                    ))
                    .alias("y"),
                    lit(true).alias("show"),
                ])
                .collect()
                .unwrap();
        alignment
    }

    #[rstest]
    #[case(vec![], vec![AreaType::Coordinate, AreaType::Fill, AreaType::Console, AreaType::Error])]
    #[case(
        vec![RepositoryFileIndex::Alignment(0)],
        vec![
            AreaType::Coordinate,
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
            AreaType::Coordinate,
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
            AreaType::Coordinate,
            AreaType::Variant(0),
            AreaType::Coverage(1),
            AreaType::Alignment(1),
            AreaType::Bed(2),
            AreaType::AlignmentDivider { upper: 1, lower: 3 },
            AreaType::Coverage(3),
            AreaType::Alignment(3),
            AreaType::Console,
            AreaType::Error,
        ]
    )]
    fn layout_adds_alignment_dividers_between_alignment_groups(
        #[case] repository_file_indexes: Vec<RepositoryFileIndex>,
        #[case] expected_tracks: Vec<AreaType>,
    ) {
        let layout = layout_for_indexes(&repository_file_indexes);
        assert_eq!(layout.tracks, expected_tracks);
        let resolved = resolve(&layout, Rect::new(0, 0, 80, 24));
        assert_eq!(resolved.sidebar_width, 18);
        for (area_type, area) in &resolved.areas {
            if matches!(area_type, AreaType::Console | AreaType::Error) {
                assert_eq!(area.x, 19);
                assert_eq!(area.width, 61);
            }
        }
    }

    #[test]
    fn alignment_view_scrolls_only_the_requested_alignment() {
        let alignments = vec![alignment_with_depth(10), alignment_with_depth(10)];
        let mut alignment_view = AlignmentView::new(Focus::default(), alignments.len());

        alignment_view.scroll(
            Scroll::Down { index: 1, n: 3 },
            alignments[1].depth().unwrap(),
        );
        assert_eq!(alignment_view.top(0), 0);
        assert_eq!(alignment_view.top(1), 3);

        alignment_view.scroll(
            Scroll::Up { index: 1, n: 1 },
            alignments[1].depth().unwrap(),
        );
        assert_eq!(alignment_view.top(0), 0);
        assert_eq!(alignment_view.top(1), 2);

        alignment_view.scroll(
            Scroll::Down { index: 0, n: 4 },
            alignments[0].depth().unwrap(),
        );
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
        let (mut layout, mut resolved) = alignment_layout(2, 24);
        let initial_upper_height = area_height(&resolved, AreaType::Alignment(0));
        let initial_lower_height = area_height(&resolved, AreaType::Alignment(1));
        let initial_first_coverage_height = area_height(&resolved, AreaType::Coverage(0));
        let initial_second_coverage_height = area_height(&resolved, AreaType::Coverage(1));

        layout.resize_alignment_pair(0, 1, initial_delta as i32, &resolved);
        resolved = resolve(&layout, Rect::new(0, 0, 80, 26));
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
        resolved = resolve(&layout, Rect::new(0, 0, 80, 26));
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
        let (mut layout, mut resolved) = alignment_layout(2, 24);

        layout.resize_alignment_pair(0, 1, delta as i32, &resolved);
        resolved = resolve(&layout, Rect::new(0, 0, 80, 26));

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
    fn small_windows_allocate_layout_top_first() {
        let (layout_state, layout) = alignment_layout(3, 16);

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

        assert_eq!(
            layout.get_area_type_at_position(0, 0),
            HoveringAreaType::Sidebar(0)
        );
        assert_eq!(
            layout.get_area_type_at_position(18, 0),
            HoveringAreaType::SidebarDivider
        );
        assert_eq!(
            layout.get_area_type_at_position(19, 0),
            HoveringAreaType::Track(0)
        );
        assert_eq!(
            layout.get_area_type_at_position(80, 0),
            HoveringAreaType::None
        );
        let narrow = resolve(&layout_state, Rect::new(0, 0, 7, 16));
        assert_eq!(narrow.sidebar_width, 0);
        assert_eq!(
            resolve(&layout_state, Rect::new(0, 0, 80, 16)).sidebar_width,
            18
        );
    }
}

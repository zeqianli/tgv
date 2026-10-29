//! The terminal layout: which rows each area takes, and the viewport in terminal cells.

use gv_core::prelude::*;
pub use gv_viewer::layout::AreaType;
use gv_viewer::{prelude::*, settings::Settings};
use ratatui::layout::Rect;
use std::{collections::HashMap, sync::Arc};
use unicode_width::UnicodeWidthChar;

/// The height an area asks for, in rows. Alignments and the fill take what is left.
fn desired_height(area: &AreaType) -> Option<u16> {
    match area {
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

/// The viewport in terminal cells: one column is a whole number of bases.
pub type AlignmentView = Viewport<ColumnScale>;

/// The viewport's positions in a terminal area. The area's width is the track area's width.
pub trait CellView {
    fn left(&self, area: &Rect) -> u64;
    fn right(&self, area: &Rect) -> u64;

    /// Check if the viewing window overlaps with [left, right].
    /// 1-based, inclusive.
    fn overlaps_x_interval(&self, left: u64, right: u64, area: &Rect) -> bool;

    /// Bottom track # of the viewing window.
    /// 0-based, exclusive.
    fn bottom(&self, index: usize, area: &Rect) -> usize;

    /// Check if the viewing window overlaps with [top, bottom).
    /// y: 0-based.
    fn overlaps_y(&self, index: usize, y: usize, area: &Rect) -> bool;

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
    fn onscreen_x_coordinate(&self, x: u64, area: &Rect) -> OnScreenCoordinate;

    /// Given an onscreen x position, return the genome coordinate range (1-based, inclusive) at that x location.
    fn coordinates_of_onscreen_x(&self, x: u16, area: &Rect) -> Option<(u64, u64)>;

    /// Given an onscreen y position, return the row (0-based) at that y location.
    fn coordinate_of_onscreen_y(&self, index: usize, y: u16, area: &Rect) -> Option<usize>;

    /// Returns the onscreen y coordinate in the area. Example
    /// y: 0-based.
    fn onscreen_y_coordinate(&self, index: usize, y: usize, area: &Rect) -> OnScreenCoordinate;
}

impl CellView for AlignmentView {
    /// FIXME: cost of this is pretty high. Lots of useless calculation here.
    fn left(&self, area: &Rect) -> u64 {
        self.region(area.width as f64).start()
    }

    /// FIXME: cost of this is pretty high. Lots of useless calculation here.
    fn right(&self, area: &Rect) -> u64 {
        self.region(area.width as f64).end()
    }

    fn overlaps_x_interval(&self, left: u64, right: u64, area: &Rect) -> bool {
        // FIXME: can reduce some useless calculation here.
        left <= self.right(area) && right >= self.left(area)
    }

    fn bottom(&self, index: usize, area: &Rect) -> usize {
        self.top(index) + area.height as usize
    }

    fn overlaps_y(&self, index: usize, y: usize, area: &Rect) -> bool {
        (self.top(index)..self.bottom(index, area)).contains(&y)
    }

    fn onscreen_x_coordinate(&self, x: u64, area: &Rect) -> OnScreenCoordinate {
        // TODO: for now, we assume that left and right area equals to the alignment area. Fix this in the future if we need x axis layouts.
        let self_left = self.left(area);
        let self_right = self.right(area);
        let zoom = self.zoom.column_bases();

        if x < self_left {
            OnScreenCoordinate::Left(usize::max(((self_left - x) / zoom) as usize, 1))
        } else if x > self_right {
            OnScreenCoordinate::Right(usize::max(((x - self_right) / zoom) as usize, 1))
        } else {
            OnScreenCoordinate::OnScreen(((x - self_left) / zoom) as usize)
        }
    }

    fn coordinates_of_onscreen_x(&self, x: u16, area: &Rect) -> Option<(u64, u64)> {
        if x < area.left() || x >= area.right() {
            return None;
        }
        let zoom = self.zoom.column_bases();
        let left = self.left(area) + (x - area.left()) as u64 * zoom;

        Some((left, left + zoom - 1))
    }

    fn coordinate_of_onscreen_y(&self, index: usize, y: u16, area: &Rect) -> Option<usize> {
        if y < area.top() || y >= area.bottom() {
            return None;
        }

        Some(self.top(index) + (y - area.top()) as usize)
    }

    fn onscreen_y_coordinate(&self, index: usize, y: usize, area: &Rect) -> OnScreenCoordinate {
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

pub use gv_core::settings::file_name;

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

    /// Lays out the areas for the file tracks in `track_order`, the viewer's display order.
    pub fn new(
        settings: &Settings,
        track_registry: Arc<TrackRegistry>,
        track_order: &[TrackId],
    ) -> Self {
        let tracks = AreaType::for_tracks(&settings.core.reference, &track_registry, track_order);
        let track_heights = tracks.iter().map(desired_height).collect();
        MainLayout {
            tracks,
            track_heights,
            sidebar_width: Self::SIDEBAR_DEFAULT_WIDTH,
            sidebar_visible: true,
            track_registry,
        }
    }

    /// Rebuilds the track rows after tracks were added, removed, or reordered. Rows that remain
    /// keep their requested heights.
    pub fn set_tracks(
        &mut self,
        settings: &Settings,
        track_registry: Arc<TrackRegistry>,
        track_order: &[TrackId],
    ) {
        let rebuilt = Self::new(settings, track_registry, track_order);
        self.track_heights = rebuilt
            .tracks
            .iter()
            .zip(rebuilt.track_heights)
            .map(|(area, height)| {
                self.tracks
                    .iter()
                    .position(|old| old == area)
                    .map_or(height, |index| self.track_heights[index])
            })
            .collect();
        self.tracks = rebuilt.tracks;
        self.track_registry = rebuilt.track_registry;
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
    ///
    /// Labels are file names. Tracks with unsaved changes get a `● ` before theirs.
    pub fn resolve(&self, terminal_area: Rect, repository: &Repository) -> ResolvedMainLayout {
        self.resolve_with_labels(terminal_area, |index| {
            let name = file_name(repository.file_path(index));
            match index {
                RepositoryFileIndex::Bed(bed) if repository.bed_repositories[bed].is_modified() => {
                    format!("● {name}")
                }
                _ => name.to_owned(),
            }
        })
    }

    fn resolve_with_labels(
        &self,
        terminal_area: Rect,
        label: impl Fn(RepositoryFileIndex) -> String,
    ) -> ResolvedMainLayout {
        let file_names: HashMap<TrackId, String> = self
            .track_registry
            .entries
            .iter()
            .map(|entry| (entry.id, label(entry.repository_index)))
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
                    let lines = wrap_sidebar_label(&file_names[&track_id], sidebar_width).len();
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
                // The sequence and the gene track are one focus target, the reference.
                AreaType::Sequence
                    if matches!(areas.get(track_index + 1), Some((AreaType::GeneTrack, _))) =>
                {
                    let genes = areas[track_index + 1].1;
                    track_index += 2;
                    Rect::new(
                        terminal_area.x,
                        rect.y,
                        sidebar_width,
                        genes.bottom().saturating_sub(rect.y),
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
                let name = &file_names[&track_id];
                let reserved = u16::from(separator_after(index))
                    + u16::from(matches!(area_type, AreaType::Coverage(_)));
                let label_area = Rect::new(
                    section.x,
                    section.y,
                    section.width,
                    section.height.saturating_sub(reserved),
                );
                (label_area.height > 0 && label_area.width > 0).then(|| SidebarLabel {
                    area: label_area,
                    area_type: *area_type,
                    name: name.clone(),
                })
            })
            .collect();

        let sidebar_sections = if sidebar_width == 0 {
            Vec::new()
        } else {
            sections
        };
        ResolvedMainLayout {
            terminal_area,
            main_area,
            areas,
            sidebar_width,
            sidebar_sections,
            sidebar_areas,
            sidebar_divider_area,
            sidebar_section_dividers,
            sidebar_labels,
            sidebar_alignment_depths,
            track_registry: Arc::clone(&self.track_registry),
        }
    }
}

/// A file name in the sidebar, beside the section of the track it names.
#[derive(Clone, Debug)]
pub struct SidebarLabel {
    pub area: Rect,
    /// The first area of the track's section, such as `Coverage` for an alignment track.
    pub area_type: AreaType,
    pub name: String,
}

/// Rectangles computed for one terminal size.
#[derive(Default, Clone, Debug)]
pub struct ResolvedMainLayout {
    pub terminal_area: Rect,
    pub main_area: Rect,
    pub areas: Vec<(AreaType, Rect)>,
    pub sidebar_width: u16,
    /// The sidebar beside each track, with the track's first area, such as `Coverage` for an
    /// alignment track or `Sequence` for the reference. Empty while the sidebar is hidden.
    pub sidebar_sections: Vec<(AreaType, Rect)>,
    pub sidebar_areas: Vec<Rect>,
    pub sidebar_divider_area: Rect,
    pub sidebar_section_dividers: Vec<Rect>,
    pub sidebar_labels: Vec<SidebarLabel>,
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
    use gv_core::message::Scroll;
    use gv_core::{
        reference::Reference,
        settings::{AlignmentPath, FilePath},
    };
    use rstest::rstest;

    fn settings_without_reference() -> Settings {
        let mut settings = Settings::default();
        settings.core.reference = Reference::NoReference;
        settings
    }

    fn registry(indexes: &[RepositoryFileIndex]) -> TrackRegistry {
        let mut registry = TrackRegistry::default();
        for &index in indexes {
            let file_path = match index {
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
            };
            registry.push(index, file_path);
        }
        registry
    }

    fn resolve(layout: &MainLayout, area: Rect) -> ResolvedMainLayout {
        layout.resolve_with_labels(area, |_| "sample.bam".to_owned())
    }

    fn layout_for_indexes(indexes: &[RepositoryFileIndex]) -> MainLayout {
        let settings = settings_without_reference();
        let registry = Arc::new(registry(indexes));
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
        use gv_core::alignment::tables::ReadSchema;
        use polars::prelude::*;
        alignment.tables.reads =
            DataFrame::full_null(&gv_core::alignment::tables::ReadSchema::schema(), depth)
                .lazy()
                .with_columns([
                    lit(Series::new(
                        ReadSchema::Y.into(),
                        (0..depth as u64).collect::<Vec<_>>(),
                    ))
                    .alias(ReadSchema::Y),
                    lit(true).alias(ReadSchema::SHOW),
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
        let mut alignment_view =
            AlignmentView::new(Focus::default(), ColumnScale::default(), alignments.len());

        alignment_view.scroll(1, Scroll::Down(3), alignments[1].depth().unwrap());
        assert_eq!(alignment_view.top(0), 0);
        assert_eq!(alignment_view.top(1), 3);

        alignment_view.scroll(1, Scroll::Up(1), alignments[1].depth().unwrap());
        assert_eq!(alignment_view.top(0), 0);
        assert_eq!(alignment_view.top(1), 2);

        alignment_view.scroll(0, Scroll::Down(4), alignments[0].depth().unwrap());
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

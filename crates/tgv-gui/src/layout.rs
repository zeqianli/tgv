//! The window layout: where each area goes, in pixels, and the viewport as drawn.
//!
//! The areas and their order come from `gv_viewer::layout::AreaType::for_tracks`, as in the
//! terminal UI. This module only decides their heights and positions.

use gv_core::prelude::*;
pub use gv_viewer::layout::AreaType;
use gv_viewer::{prelude::*, settings::Settings};
use std::{collections::HashMap, sync::Arc};

/// A rectangle in window pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    pub fn is_empty(&self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }
}

/// The height of one stacking row of an alignment track.
pub const ALIGNMENT_ROW_HEIGHT: f32 = 12.0;

/// The height an area asks for, in pixels. Alignments and the fill take what is left.
fn desired_height(area: &AreaType) -> Option<f32> {
    match area {
        AreaType::Cytoband => Some(26.0),
        AreaType::Coordinate => Some(28.0),
        AreaType::Coverage(_) => Some(56.0),
        AreaType::Alignment(_) => None,
        AreaType::AlignmentDivider { .. } => Some(6.0),
        AreaType::Sequence => Some(16.0),
        AreaType::GeneTrack => Some(40.0),
        AreaType::Console => Some(22.0),
        AreaType::Error => Some(22.0),
        AreaType::Variant(_) => Some(18.0),
        AreaType::Bed(_) => Some(18.0),
        AreaType::Fill => None,
    }
}

/// The viewport as drawn: the shared viewport, plus a pixel offset for a pan that hasn't added
/// up to a whole base yet.
#[derive(Clone, Debug, PartialEq)]
pub struct PixelView {
    pub viewport: Viewport<PixelScale>,
    /// Pixels the content is shifted right.
    pub offset: f32,
}

impl PixelView {
    pub fn pixels_per_base(&self) -> f32 {
        self.viewport.zoom.pixels_per_base() as f32
    }

    /// The x position of the left edge of a base. The focused base is centered in the area.
    pub fn x(&self, position: u64, area: &Rect) -> f32 {
        let center = area.x + area.width / 2.0 + self.offset;
        center
            + ((position as f64 - self.viewport.focus.position as f64 - 0.5)
                * self.viewport.zoom.pixels_per_base()) as f32
    }

    /// The pixels from the left edge of `start` to the right edge of `end`, inclusive, clipped
    /// to the area and at least one pixel wide. `None` when the span is off screen.
    pub fn span(&self, start: u64, end: u64, area: &Rect) -> Option<(f32, f32)> {
        let left = self.x(start, area).max(area.x);
        let right = self.x(end + 1, area).min(area.right());
        if right < area.x || left > area.right() || end < start {
            return None;
        }
        let right = right.max(left + 1.0).min(area.right());
        (right > left).then_some((left, right - left))
    }

    /// The base under an x position.
    pub fn position_at(&self, x: f32, area: &Rect) -> u64 {
        let center = area.x + area.width / 2.0 + self.offset;
        let position = (x - center) as f64 * self.viewport.zoom.bases_per_pixel
            + self.viewport.focus.position as f64
            + 0.5;
        position.floor().max(1.0) as u64
    }

    /// The bases under one pixel column: one base when zoomed in, or several when zoomed out.
    pub fn bases_at(&self, x: f32, area: &Rect) -> (u64, u64) {
        let left = self.position_at(x, area);
        let right = self.position_at(x + 1.0, area).saturating_sub(1).max(left);
        (left, right)
    }

    /// The first and last base visible in the area.
    pub fn visible(&self, area: &Rect) -> (u64, u64) {
        (
            self.position_at(area.x, area),
            self.position_at(area.right() - 1.0, area),
        )
    }

    /// The region loaded for the area, as the viewer computes it.
    pub fn region(&self, area: &Rect) -> Region {
        self.viewport.region(f64::from(area.width))
    }
}

/// Persistent state for the main page layout.
#[derive(Clone)]
pub struct MainLayout {
    /// Track area types. Length matches number of tracks to display.
    pub tracks: Vec<AreaType>,
    /// Requested heights, indexed in parallel with `tracks`.
    pub track_heights: Vec<Option<f32>>,
    /// Requested sidebar width, retained while the sidebar is hidden.
    pub sidebar_width: f32,
    pub sidebar_visible: bool,
    pub track_registry: Arc<TrackRegistry>,
}

impl MainLayout {
    const ALIGNMENT_MIN_HEIGHT: f32 = ALIGNMENT_ROW_HEIGHT;
    const SIDEBAR_DEFAULT_WIDTH: f32 = 170.0;
    const SIDEBAR_MIN_WIDTH: f32 = 60.0;
    pub const SIDEBAR_DIVIDER_WIDTH: f32 = 4.0;

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

    pub fn resize_sidebar_to(&mut self, x: f32, window: Rect) {
        if self.sidebar_visible {
            self.sidebar_width = (x - window.x).clamp(Self::SIDEBAR_MIN_WIDTH, window.width / 2.0);
        }
    }

    /// Moves `delta` pixels of height from the lower alignment to the upper one.
    pub fn resize_alignment_pair(
        &mut self,
        upper: TrackId,
        lower: TrackId,
        delta: f32,
        resolved: &ResolvedMainLayout,
    ) {
        let position = |id| {
            self.tracks
                .iter()
                .position(|area| *area == AreaType::Alignment(id))
        };
        let (Some(upper_track), Some(lower_track)) = (position(upper), position(lower)) else {
            return;
        };
        let upper_height = resolved.areas[upper_track].1.height;
        let lower_height = resolved.areas[lower_track].1.height;
        let delta = delta.clamp(
            -(upper_height - Self::ALIGNMENT_MIN_HEIGHT).max(0.0),
            (lower_height - Self::ALIGNMENT_MIN_HEIGHT).max(0.0),
        );
        self.track_heights[upper_track] = Some(upper_height + delta);
        self.track_heights[lower_track] = Some(lower_height - delta);
    }

    /// Resolve the requested layout within the window. `labels` are the sidebar labels of the
    /// file tracks, by track ID.
    pub fn resolve(&self, window: Rect, labels: &HashMap<TrackId, String>) -> ResolvedMainLayout {
        let sidebar_width = if self.sidebar_visible {
            self.sidebar_width.min(window.width / 2.0)
        } else {
            0.0
        };
        let divider_width = if sidebar_width > 0.0 {
            Self::SIDEBAR_DIVIDER_WIDTH
        } else {
            0.0
        };
        let main_area = Rect::new(
            window.x + sidebar_width + divider_width,
            window.y,
            (window.width - sidebar_width - divider_width).max(0.0),
            window.height,
        );

        let fixed_height = self
            .tracks
            .iter()
            .zip(&self.track_heights)
            .filter(|(area, _)| !matches!(area, AreaType::Alignment(_)))
            .filter_map(|(_, height)| *height)
            .sum::<f32>();
        let available = (main_area.height - fixed_height).max(0.0);
        let alignment_tracks: Vec<usize> = self
            .tracks
            .iter()
            .enumerate()
            .filter_map(|(index, area)| matches!(area, AreaType::Alignment(_)).then_some(index))
            .collect();
        // Alignments split the space left, in proportion to the heights dividers asked for.
        let requested = alignment_tracks
            .iter()
            .map(|&index| self.track_heights[index].unwrap_or(1.0).max(1.0))
            .collect::<Vec<_>>();
        let requested_total = requested.iter().sum::<f32>().max(1.0);
        let mut alignment_heights = requested
            .iter()
            .map(|height| (available * height / requested_total).floor())
            .collect::<Vec<_>>();
        // Rounding down leaves a few pixels, which go to the last alignment.
        let rounded = alignment_heights.iter().sum::<f32>();
        if let Some(last) = alignment_heights.last_mut() {
            *last += available - rounded;
        }

        let mut y = main_area.y;
        let mut alignment_position = 0;
        let areas = self
            .tracks
            .iter()
            .zip(&self.track_heights)
            .map(|(area, requested)| {
                let height = match area {
                    AreaType::Alignment(_) => {
                        let height = alignment_heights[alignment_position];
                        alignment_position += 1;
                        height
                    }
                    AreaType::Fill => available,
                    _ => requested.expect("fixed track has a requested height"),
                };
                let height = height.min((main_area.bottom() - y).max(0.0));
                let rect = Rect::new(main_area.x, y, main_area.width, height);
                y += height;
                (*area, rect)
            })
            .collect::<Vec<_>>();

        // The sidebar section beside each track: an alignment's coverage and reads share one,
        // and so do the ruler and the cytoband, the reference rows, and the console rows.
        let mut sidebar_sections = Vec::new();
        let mut index = 0;
        while index < areas.len() {
            let (area_type, rect) = areas[index];
            let mut bottom = rect.bottom();
            index += 1;
            let joins = |next: &AreaType| match (area_type, next) {
                (AreaType::Cytoband, AreaType::Coordinate) => true,
                (AreaType::Coverage(id), AreaType::Alignment(next)) => id == *next,
                (AreaType::Sequence, AreaType::GeneTrack) => true,
                (AreaType::Console, AreaType::Error) => true,
                _ => false,
            };
            if let Some((next, next_rect)) = areas.get(index)
                && joins(next)
            {
                bottom = next_rect.bottom();
                index += 1;
            }
            sidebar_sections.push((
                area_type,
                Rect::new(window.x, rect.y, sidebar_width, bottom - rect.y),
            ));
        }

        let sidebar_labels = sidebar_sections
            .iter()
            .filter_map(|(area_type, section)| {
                let id = match area_type {
                    AreaType::Coverage(id) | AreaType::Variant(id) | AreaType::Bed(id) => *id,
                    _ => return None,
                };
                (sidebar_width > 0.0).then(|| SidebarLabel {
                    area: *section,
                    area_type: *area_type,
                    name: labels[&id].clone(),
                })
            })
            .collect();

        ResolvedMainLayout {
            window,
            main_area,
            areas,
            sidebar_width,
            sidebar_sections: if sidebar_width > 0.0 {
                sidebar_sections
            } else {
                Vec::new()
            },
            sidebar_divider_area: Rect::new(
                window.x + sidebar_width,
                window.y,
                divider_width,
                window.height,
            ),
            sidebar_labels,
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

/// Rectangles computed for one window size.
#[derive(Clone, Debug)]
pub struct ResolvedMainLayout {
    pub window: Rect,
    pub main_area: Rect,
    pub areas: Vec<(AreaType, Rect)>,
    pub sidebar_width: f32,
    /// The sidebar beside each track, with the track's first area. Empty while the sidebar is
    /// hidden.
    pub sidebar_sections: Vec<(AreaType, Rect)>,
    pub sidebar_divider_area: Rect,
    pub sidebar_labels: Vec<SidebarLabel>,
    pub track_registry: Arc<TrackRegistry>,
}

/// Where dropped files go: a position in the display order, and the gap that shows it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DropTarget {
    /// The position in the display order. 0 is above the first file track.
    pub position: usize,
    /// The y position of the gap between the tracks around the position.
    pub y: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoveringAreaType {
    /// A sidebar section, by index into `sidebar_sections`.
    Sidebar(usize),
    SidebarDivider,
    /// A track area, by index into `areas`.
    Track(usize),
    None,
}

impl ResolvedMainLayout {
    pub fn get_area_type_at_position(&self, x: f32, y: f32) -> HoveringAreaType {
        if !self.window.contains(x, y) {
            HoveringAreaType::None
        } else if self.sidebar_divider_area.width > 0.0 && self.sidebar_divider_area.contains(x, y)
        {
            HoveringAreaType::SidebarDivider
        } else if x < self.sidebar_divider_area.x {
            self.sidebar_sections
                .iter()
                .position(|(_, area)| y >= area.y && y < area.bottom())
                .map_or(HoveringAreaType::None, HoveringAreaType::Sidebar)
        } else {
            self.areas
                .iter()
                .position(|(_, area)| area.contains(x, y))
                .map_or(HoveringAreaType::None, HoveringAreaType::Track)
        }
    }

    /// The track area of a type.
    pub fn area(&self, area_type: AreaType) -> Option<Rect> {
        self.areas
            .iter()
            .find_map(|(area, rect)| (*area == area_type).then_some(*rect))
    }

    /// The gap nearest to `y` between the file tracks in `track_order`. Above a track's middle,
    /// files go above it; below, they go below it. Without file tracks, they go below the ruler.
    pub fn drop_target(&self, y: f32, track_order: &[TrackId]) -> DropTarget {
        // Each track's top and bottom, over all of its areas, such as an alignment's coverage
        // and reads.
        let spans = track_order
            .iter()
            .enumerate()
            .filter_map(|(index, &id)| {
                self.areas
                    .iter()
                    .filter(|(area, _)| {
                        matches!(
                            area,
                            AreaType::Coverage(track)
                                | AreaType::Alignment(track)
                                | AreaType::Variant(track)
                                | AreaType::Bed(track)
                                if *track == id
                        )
                    })
                    .map(|(_, rect)| (rect.y, rect.bottom()))
                    .reduce(|(top, bottom), (y, end)| (top.min(y), bottom.max(end)))
                    .map(|(top, bottom)| (index, top, bottom))
            })
            .collect::<Vec<_>>();
        let Some(&(first_index, first_top, _)) = spans.first() else {
            let y = self
                .area(AreaType::Coordinate)
                .map_or(self.main_area.y, |ruler| ruler.bottom());
            return DropTarget { position: 0, y };
        };
        let gap = spans
            .iter()
            .position(|(_, top, bottom)| y < (top + bottom) / 2.0)
            .unwrap_or(spans.len());
        let (position, y) = match gap {
            0 => (first_index, first_top),
            gap if gap == spans.len() => (spans[gap - 1].0 + 1, spans[gap - 1].2),
            // Between two tracks, the gap is the middle of any divider between them.
            gap => (spans[gap].0, (spans[gap - 1].2 + spans[gap].1) / 2.0),
        };
        DropTarget { position, y }
    }

    /// The width of the track area, in pixels, as the viewer measures it.
    pub fn track_width(&self) -> f64 {
        f64::from(self.main_area.width)
    }
}

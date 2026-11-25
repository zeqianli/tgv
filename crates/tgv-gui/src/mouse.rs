use crate::{
    layout::{
        ALIGNMENT_ROW_HEIGHT, AreaType, HoveringAreaType, PixelView, Rect, ResolvedMainLayout,
        row_height,
    },
    message::GuiAction,
    rendering::cytoband::CYTOBAND_TEXT_RIGHT_SPACING,
    worker::Frame,
};
use gv_core::message::{Scroll, Zoom};
use gv_core::{message::Movement, prelude::*};
use gv_viewer::{
    message::{
        Action, Anchor, ContextMenuTarget, FocusTarget, InputAction, InteractionAction,
        NavigationAction, UpdateLayoutAction,
    },
    prepare::hover,
    viewport::RowLayout,
};

const ZOOM_PER_PIXEL: f64 = 0.01;

#[derive(Clone, Copy, PartialEq, Eq)]
enum DragTarget {
    Area(AreaType),
    Divider(FocusTarget),
    Sidebar,
}

/// Mouse interaction state for the currently displayed layout.
#[derive(Clone, Default)]
pub struct MouseState {
    drag_target: Option<DragTarget>,
    last_x: f32,
    last_y: f32,
    dragged: bool,
    pub hovered_divider: Option<FocusTarget>,
    /// Vertical scrolling that hasn't added up to a whole row yet.
    scroll_remainder: f32,
}

impl MouseState {
    pub fn is_divider_highlighted(&self, area_type: &AreaType) -> bool {
        match area_type {
            AreaType::Divider(target) => {
                self.hovered_divider == Some(*target)
                    || self.drag_target == Some(DragTarget::Divider(*target))
            }
            _ => false,
        }
    }

    pub fn is_sidebar_divider_highlighted(&self) -> bool {
        self.drag_target == Some(DragTarget::Sidebar)
    }

    /// The area type a hovered position stands for: a track area, or the first area of a
    /// sidebar section.
    fn hovered_area(layout: &ResolvedMainLayout, hovered: HoveringAreaType) -> Option<AreaType> {
        match hovered {
            HoveringAreaType::Track(index) => Some(layout.areas[index].0),
            HoveringAreaType::Sidebar(index) => Some(layout.sidebar_sections[index].0),
            HoveringAreaType::SidebarDivider | HoveringAreaType::None => None,
        }
    }

    pub fn handle_left_mouse_down(
        &mut self,
        x: f32,
        y: f32,
        click_count: usize,
        frame: &Frame,
        layout: &ResolvedMainLayout,
        view: &PixelView,
    ) -> Result<Vec<GuiAction>, crate::Error> {
        let mut actions = Vec::new();
        let hovered = layout.get_area_type_at_position(x, y);
        self.last_x = x;
        self.last_y = y;
        self.drag_target =
            matches!(hovered, HoveringAreaType::SidebarDivider).then_some(DragTarget::Sidebar);
        self.dragged = false;
        // Clicking a track or its sidebar section focuses it. Hovering doesn't, so the
        // pointer can't undo a focus chosen with the keyboard.
        if let Some(target) =
            Self::hovered_area(layout, hovered).and_then(|area| area.focus_target())
        {
            actions.push(Action::Input(InputAction::Focus(target)).into());
        }
        if let HoveringAreaType::Track(track_index) = hovered {
            let (area, rect) = layout.areas[track_index];
            self.drag_target = Some(DragTarget::Area(area));
            // The cytoband spans the whole contig, so a click there goes to that part of
            // the contig at the same zoom.
            let bar_width = rect.width - CYTOBAND_TEXT_RIGHT_SPACING;
            if area == AreaType::Cytoband
                && x - rect.x < bar_width
                && let Some(length) = frame.state.contig_length(&view.viewport.focus)?
            {
                let fraction = f64::from(x - rect.x) / f64::from(bar_width);
                let position = ((fraction * length as f64) as u64).clamp(1, length);
                actions.push(Action::from(Movement::Position(position)).into());
            }
            // A double click zooms in around the point, as in IGV.
            if click_count == 2
                && matches!(
                    area,
                    AreaType::Alignment(_)
                        | AreaType::Coverage(_)
                        | AreaType::Sequence
                        | AreaType::GeneTrack
                        | AreaType::Variant(_)
                        | AreaType::Bed(_)
                        | AreaType::Coordinate
                )
            {
                actions.push(GuiAction::ZoomAround { factor: 2.0, x });
            }
        }
        if let Some(target) = divider_at(layout, hovered, y) {
            self.drag_target = Some(DragTarget::Divider(target));
        }
        Ok(actions)
    }

    pub fn handle_right_mouse_down(
        &self,
        x: f32,
        y: f32,
        layout: &ResolvedMainLayout,
        view: &PixelView,
    ) -> Vec<GuiAction> {
        let mut actions = Vec::new();
        let hovered = layout.get_area_type_at_position(x, y);
        let target = match hovered {
            HoveringAreaType::Sidebar(index) => Some(ContextMenuTarget::Sidebar {
                target: layout.sidebar_sections[index].0.focus_target(),
            }),
            HoveringAreaType::Track(track_index) => match &layout.areas[track_index] {
                (AreaType::Alignment(id) | AreaType::Coverage(id), area) => {
                    let (left, right) = view.bases_at(x, area);
                    Some(ContextMenuTarget::Alignment {
                        track: *id,
                        // Base actions need a single base under the pointer.
                        position: (left == right).then_some(left),
                    })
                }
                (AreaType::Bed(id) | AreaType::Variant(id), _) => {
                    Some(ContextMenuTarget::Track { track: *id })
                }
                (AreaType::Sequence | AreaType::GeneTrack, _) => Some(ContextMenuTarget::Reference),
                _ => None,
            },
            _ => None,
        };
        if let Some(target) = target {
            actions.push(
                Action::Interaction(InteractionAction::OpenContextMenu {
                    target,
                    anchor: Anchor { x, y },
                })
                .into(),
            );
        }
        actions
    }

    /// Translates pointer motion: a drag while the left button is down, or a hover.
    pub fn handle_mouse_move(
        &mut self,
        x: f32,
        y: f32,
        left_pressed: bool,
        frame: &Frame,
        layout: &ResolvedMainLayout,
        view: &PixelView,
    ) -> Result<Vec<GuiAction>, crate::Error> {
        let mut actions = Vec::new();
        let hovered = layout.get_area_type_at_position(x, y);
        self.hovered_divider = divider_at(layout, hovered, y);
        if !left_pressed {
            actions.push(GuiAction::Hover(Self::describe(x, y, frame, layout, view)?));
            return Ok(actions);
        }
        if (x, y) != (self.last_x, self.last_y) {
            self.dragged = true;
        }
        if self.drag_target == Some(DragTarget::Sidebar) {
            actions
                .push(Action::from(UpdateLayoutAction::SetSidebarWidth(x.max(0.0) as u16)).into());
        } else if let Some(DragTarget::Divider(target)) = self.drag_target {
            let delta = (y - self.last_y).round() as i32;
            if delta != 0 {
                actions
                    .push(Action::from(UpdateLayoutAction::ResizeTrack { target, delta }).into());
            } else {
                // Keep the fraction of a pixel for the next move.
                return Ok(actions);
            }
        } else if let Some(DragTarget::Area(area)) = self.drag_target {
            match area {
                AreaType::Alignment(_)
                | AreaType::Coverage(_)
                | AreaType::Bed(_)
                | AreaType::Variant(_)
                | AreaType::GeneTrack => {
                    actions.push(GuiAction::Pan {
                        pixels: x - self.last_x,
                    });
                    // A drag keeps scrolling the track where it started, even if it crosses
                    // into another.
                    if let Some(target) = scroll_target(&area)
                        && frame.viewport.tracks[&target].rows == RowLayout::Expanded
                    {
                        actions.extend(self.scroll_rows(
                            target,
                            row_height(&area),
                            self.last_y - y,
                        ));
                    }
                }
                AreaType::Sequence | AreaType::Coordinate => actions.push(GuiAction::Pan {
                    pixels: x - self.last_x,
                }),
                _ => {}
            }
        }
        self.last_x = x;
        self.last_y = y;
        Ok(actions)
    }

    pub fn handle_mouse_up(
        &mut self,
        x: f32,
        y: f32,
        frame: &Frame,
        layout: &ResolvedMainLayout,
        view: &PixelView,
    ) -> Result<Vec<GuiAction>, crate::Error> {
        let mut actions = Vec::new();
        // A click on a read, without dragging, shows its details.
        if !self.dragged
            && let Some(DragTarget::Area(AreaType::Alignment(track))) = self.drag_target
            && let HoveringAreaType::Track(track_index) = layout.get_area_type_at_position(x, y)
            && let (AreaType::Alignment(id), area) = &layout.areas[track_index]
            && *id == track
            && let Some((_, read_id)) = read_at(frame, layout, view, track, area, x, y)?
        {
            actions.push(
                Action::Interaction(InteractionAction::OpenReadDetails { track, read_id }).into(),
            );
        }
        self.drag_target = None;
        Ok(actions)
    }

    /// Translates a scroll: Control or Command zooms, a horizontal scroll pans, and a vertical
    /// scroll over an alignment scrolls its rows.
    pub fn handle_scroll(
        &mut self,
        dx: f32,
        dy: f32,
        x: f32,
        y: f32,
        zoom: bool,
        layout: &ResolvedMainLayout,
    ) -> Vec<GuiAction> {
        let HoveringAreaType::Track(track_index) = layout.get_area_type_at_position(x, y) else {
            return Vec::new();
        };
        let area = layout.areas[track_index].0;
        if matches!(
            area,
            AreaType::Console | AreaType::Error | AreaType::Divider(_)
        ) {
            return Vec::new();
        }
        if zoom {
            return vec![GuiAction::ZoomAround {
                factor: (f64::from(dy) * ZOOM_PER_PIXEL).exp(),
                x,
            }];
        }
        let mut actions = Vec::new();
        if dx != 0.0 {
            actions.push(GuiAction::Pan { pixels: dx });
        }
        if let Some(target) = scroll_target(&area) {
            actions.extend(self.scroll_rows(target, row_height(&area), -dy));
        }
        actions
    }

    /// Scrolls a track by whole rows, keeping the fraction for later. Positive pixels scroll
    /// down.
    fn scroll_rows(
        &mut self,
        target: FocusTarget,
        row_height: f32,
        pixels: f32,
    ) -> Option<GuiAction> {
        self.scroll_remainder += pixels;
        let rows = (self.scroll_remainder / row_height).trunc();
        if rows == 0.0 {
            return None;
        }
        self.scroll_remainder -= rows * row_height;
        let scroll = if rows > 0.0 {
            Scroll::Down(rows as usize)
        } else {
            Scroll::Up(-rows as usize)
        };
        Some(Action::Navigate(NavigationAction::ScrollTrack { target, scroll }).into())
    }

    /// What the message line says about the data under the pointer.
    fn describe(
        x: f32,
        y: f32,
        frame: &Frame,
        layout: &ResolvedMainLayout,
        view: &PixelView,
    ) -> Result<Option<String>, crate::Error> {
        let HoveringAreaType::Track(track_index) = layout.get_area_type_at_position(x, y) else {
            return Ok(None);
        };
        let (area_type, area) = &layout.areas[track_index];
        let state = &frame.state;
        let contig_index = view.viewport.focus.contig_index;
        let (left, right) = view.bases_at(x, area);
        let descriptions = match area_type {
            AreaType::Alignment(id) => match read_at(frame, layout, view, *id, area, x, y)? {
                Some((index, read_id)) => vec![hover::describe_read(
                    state,
                    index,
                    read_id,
                    contig_index,
                    left,
                    right,
                )?],
                None => Vec::new(),
            },
            AreaType::GeneTrack => {
                match feature_row_at(view, area_type, FocusTarget::Reference, area, y) {
                    Some(row) => hover::describe_genes(state, contig_index, left, right, row)?,
                    None => Vec::new(),
                }
            }
            AreaType::Sequence => vec![hover::describe_sequence(state, left, right)],
            AreaType::Coverage(id) => {
                let index = layout.track_registry.alignment_index(*id)?;
                vec![hover::describe_coverage(state, index, left, right)?]
            }
            AreaType::Variant(id) => {
                let index = layout.track_registry.variant_index(*id)?;
                match feature_row_at(view, area_type, FocusTarget::Track(*id), area, y) {
                    Some(row) => {
                        hover::describe_variants(state, index, contig_index, left, right, row)?
                    }
                    None => Vec::new(),
                }
            }
            AreaType::Bed(id) => {
                let index = layout.track_registry.bed_index(*id)?;
                match feature_row_at(view, area_type, FocusTarget::Track(*id), area, y) {
                    Some(row) => hover::describe_bed(state, index, contig_index, left, right, row)?,
                    None => Vec::new(),
                }
            }
            _ => Vec::new(),
        };
        Ok((!descriptions.is_empty()).then(|| descriptions.join("   ")))
    }
}

/// Find the read drawn at a point of an alignment track area, as `(alignment index, read ID)`.
fn read_at(
    frame: &Frame,
    layout: &ResolvedMainLayout,
    view: &PixelView,
    track: TrackId,
    area: &Rect,
    x: f32,
    y: f32,
) -> Result<Option<(usize, usize)>, crate::Error> {
    let index = layout.track_registry.alignment_index(track)?;
    if !area.contains(x, y) {
        return Ok(None);
    }
    let (left, right) = view.bases_at(x, area);
    let row = view.viewport.top(FocusTarget::Track(track))
        + ((y - area.y) / ALIGNMENT_ROW_HEIGHT) as usize;
    Ok(hover::read_at(&frame.state, index, left, right, row)?.map(|read_id| (index, read_id)))
}

fn divider_at(
    layout: &ResolvedMainLayout,
    hovered: HoveringAreaType,
    y: f32,
) -> Option<FocusTarget> {
    match hovered {
        HoveringAreaType::Track(_) | HoveringAreaType::Sidebar(_) => {
            layout.areas.iter().find_map(|(area, rect)| match area {
                AreaType::Divider(target) if y >= rect.y && y < rect.bottom() => Some(*target),
                _ => None,
            })
        }
        HoveringAreaType::SidebarDivider | HoveringAreaType::None => None,
    }
}

fn scroll_target(area: &AreaType) -> Option<FocusTarget> {
    match area {
        AreaType::Coverage(id) => Some(FocusTarget::Track(*id)),
        area => area.rows_target(),
    }
}

fn feature_row_at(
    view: &PixelView,
    area_type: &AreaType,
    target: FocusTarget,
    area: &Rect,
    y: f32,
) -> Option<u64> {
    if y < area.y {
        return None;
    }
    let offset = ((y - area.y) / row_height(area_type)) as u64;
    let track = view.viewport.tracks[&target];
    match track.rows {
        RowLayout::Collapsed => (offset == 0).then_some(0),
        RowLayout::Expanded => Some(track.top as u64 + offset),
    }
}

/// The zoom for a scale step, as the `z` and `o` keys send it.
pub fn zoom_step(zoom_in: bool) -> Action {
    Action::from(if zoom_in { Zoom::In(2) } else { Zoom::Out(2) })
}

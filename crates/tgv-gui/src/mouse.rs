//! Mouse and trackpad input in the window.
//!
//! As in the terminal UI, clicks focus tracks, right clicks open context menus, a click on a
//! read shows its details, and dividers resize. The window adds continuous gestures: a drag or
//! a horizontal scroll pans by pixels, a vertical scroll over an alignment scrolls its rows,
//! and Control or Command with a vertical scroll zooms around the pointer.

use crate::{
    layout::{
        ALIGNMENT_ROW_HEIGHT, AreaType, HoveringAreaType, PixelView, Rect, ResolvedMainLayout,
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
};

/// How much one pixel of vertical scrolling zooms, as a power: 100 pixels zoom by e.
const ZOOM_PER_PIXEL: f64 = 0.01;

/// A mouse button the window reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DragTarget {
    Area(AreaType),
    Divider(TrackId, TrackId),
    Sidebar,
}

/// Mouse interaction state for the currently displayed layout.
#[derive(Clone, Default)]
pub struct MouseRegister {
    drag_target: Option<DragTarget>,
    last_x: f32,
    last_y: f32,
    /// Whether the pointer moved since the left button went down, which makes it a drag
    /// rather than a click.
    dragged: bool,
    pub hovered_divider: Option<(TrackId, TrackId)>,
    /// Vertical scrolling that hasn't added up to a whole alignment row yet.
    scroll_remainder: f32,
}

impl MouseRegister {
    pub fn is_divider_highlighted(&self, area_type: &AreaType) -> bool {
        match area_type {
            AreaType::AlignmentDivider { upper, lower } => {
                let pair = Some((*upper, *lower));
                self.hovered_divider == pair
                    || self.drag_target == Some(DragTarget::Divider(*upper, *lower))
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

    #[allow(clippy::too_many_arguments)]
    pub fn handle_mouse_down(
        &mut self,
        button: MouseButton,
        x: f32,
        y: f32,
        click_count: usize,
        frame: &Frame,
        layout: &ResolvedMainLayout,
        view: &PixelView,
    ) -> Result<Vec<GuiAction>, TGVError> {
        let mut actions = Vec::new();
        let hovered = layout.get_area_type_at_position(x, y);
        match button {
            MouseButton::Left => {
                self.last_x = x;
                self.last_y = y;
                self.drag_target = matches!(hovered, HoveringAreaType::SidebarDivider)
                    .then_some(DragTarget::Sidebar);
                self.dragged = false;
                // Clicking a track or its sidebar section focuses it. Hovering doesn't, so the
                // pointer can't undo a focus chosen with the keyboard.
                match Self::hovered_area(layout, hovered) {
                    Some(
                        AreaType::Coverage(id)
                        | AreaType::Alignment(id)
                        | AreaType::Variant(id)
                        | AreaType::Bed(id),
                    ) => actions
                        .push(Action::Input(InputAction::Focus(FocusTarget::Track(id))).into()),
                    Some(AreaType::Sequence | AreaType::GeneTrack) => actions
                        .push(Action::Input(InputAction::Focus(FocusTarget::Reference)).into()),
                    _ => {}
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
                    if let AreaType::AlignmentDivider { upper, lower } = area {
                        self.drag_target = Some(DragTarget::Divider(upper, lower));
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
            }
            MouseButton::Right => {
                let target = match hovered {
                    HoveringAreaType::Sidebar(index) => Some(ContextMenuTarget::Sidebar {
                        track: match layout.sidebar_sections[index].0 {
                            AreaType::Coverage(id)
                            | AreaType::Alignment(id)
                            | AreaType::Variant(id)
                            | AreaType::Bed(id) => Some(id),
                            _ => None,
                        },
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
            }
        }
        Ok(actions)
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
    ) -> Result<Vec<GuiAction>, TGVError> {
        let mut actions = Vec::new();
        let hovered = layout.get_area_type_at_position(x, y);
        self.hovered_divider = match hovered {
            HoveringAreaType::Track(track_index) => match layout.areas[track_index].0 {
                AreaType::AlignmentDivider { upper, lower } => Some((upper, lower)),
                _ => None,
            },
            _ => None,
        };
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
        } else if let Some(DragTarget::Divider(upper, lower)) = self.drag_target {
            let delta = (y - self.last_y).round() as i32;
            if delta != 0 {
                actions.push(
                    Action::from(UpdateLayoutAction::ResizeAlignmentPair {
                        upper,
                        lower,
                        delta_rows: delta,
                    })
                    .into(),
                );
            } else {
                // Keep the fraction of a pixel for the next move.
                return Ok(actions);
            }
        } else if let Some(DragTarget::Area(area)) = self.drag_target {
            match area {
                AreaType::Alignment(id) | AreaType::Coverage(id) => {
                    actions.push(GuiAction::Pan {
                        pixels: x - self.last_x,
                    });
                    // A drag keeps scrolling the track where it started, even if it crosses
                    // into another.
                    actions.extend(self.scroll_rows(id, self.last_y - y));
                }
                AreaType::Bed(_)
                | AreaType::Variant(_)
                | AreaType::Sequence
                | AreaType::GeneTrack
                | AreaType::Coordinate => actions.push(GuiAction::Pan {
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
    ) -> Result<Vec<GuiAction>, TGVError> {
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
            AreaType::Console | AreaType::Error | AreaType::AlignmentDivider { .. }
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
        if let AreaType::Alignment(id) | AreaType::Coverage(id) = area {
            actions.extend(self.scroll_rows(id, -dy));
        }
        actions
    }

    /// Scrolls an alignment by whole rows, keeping the fraction for later. Positive pixels
    /// scroll down.
    fn scroll_rows(&mut self, track: TrackId, pixels: f32) -> Option<GuiAction> {
        self.scroll_remainder += pixels;
        let rows = (self.scroll_remainder / ALIGNMENT_ROW_HEIGHT).trunc();
        if rows == 0.0 {
            return None;
        }
        self.scroll_remainder -= rows * ALIGNMENT_ROW_HEIGHT;
        let scroll = if rows > 0.0 {
            Scroll::Down(rows as usize)
        } else {
            Scroll::Up(-rows as usize)
        };
        Some(Action::Navigate(NavigationAction::ScrollAlignment { track, scroll }).into())
    }

    /// What the message line says about the data under the pointer.
    fn describe(
        x: f32,
        y: f32,
        frame: &Frame,
        layout: &ResolvedMainLayout,
        view: &PixelView,
    ) -> Result<Option<String>, TGVError> {
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
            AreaType::GeneTrack => hover::describe_genes(state, contig_index, left, right)?,
            AreaType::Sequence => vec![hover::describe_sequence(state, left, right)],
            AreaType::Coverage(id) => {
                let index = layout.track_registry.alignment_index(*id)?;
                vec![hover::describe_coverage(state, index, left, right)?]
            }
            AreaType::Variant(id) => {
                let index = layout.track_registry.variant_index(*id)?;
                hover::describe_variants(state, index, contig_index, left, right)?
            }
            AreaType::Bed(id) => {
                let index = layout.track_registry.bed_index(*id)?;
                hover::describe_bed(state, index, contig_index, left, right)?
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
) -> Result<Option<(usize, usize)>, TGVError> {
    let index = layout.track_registry.alignment_index(track)?;
    if !area.contains(x, y) {
        return Ok(None);
    }
    let (left, right) = view.bases_at(x, area);
    let row = view.viewport.top(index) + ((y - area.y) / ALIGNMENT_ROW_HEIGHT) as usize;
    Ok(hover::read_at(&frame.state, index, left, right, row)?.map(|read_id| (index, read_id)))
}

/// The zoom for a scale step, as the `z` and `o` keys send it.
pub fn zoom_step(zoom_in: bool) -> Action {
    Action::from(if zoom_in { Zoom::In(2) } else { Zoom::Out(2) })
}

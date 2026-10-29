use crate::layout::{AlignmentView, AreaType, CellView, HoveringAreaType, ResolvedMainLayout};
use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use gv_core::message::{Movement, Scroll};
use gv_core::prelude::*;
use gv_viewer::message::{
    Action, Anchor, ContextMenuTarget, FocusTarget, InputAction, InteractionAction,
    NavigationAction, UpdateLayoutAction,
};
use gv_viewer::prepare::hover;
use ratatui::layout::Rect;

/// Columns panned per Shift+wheel notch. A wheel notch is coarser than a trackpad's
/// horizontal scroll events, which pan one column each.
const SHIFT_SCROLL_COLUMNS: u64 = 3;

/// Find the read drawn at a cell of an alignment track area, as `(alignment index, read ID)`.
fn read_at(
    state: &State,
    layout: &ResolvedMainLayout,
    alignment_view: &AlignmentView,
    track: TrackId,
    area: &Rect,
    column: u16,
    row: u16,
) -> Result<Option<(usize, usize)>, TGVError> {
    let index = layout.track_registry.alignment_index(track)?;
    let (Some((left, right)), Some(y)) = (
        alignment_view.coordinates_of_onscreen_x(column, area),
        alignment_view.coordinate_of_onscreen_y(index, row, area),
    ) else {
        return Ok(None);
    };
    Ok(hover::read_at(state, index, left, right, y)?.map(|read_id| (index, read_id)))
}

/// Mouse interaction state for the currently displayed layout.
#[derive(Default)]
pub struct MouseRegister {
    mouse_down_area: Option<AreaType>,
    last_x: u16,
    last_y: u16,
    active_divider: Option<(TrackId, TrackId)>,
    sidebar_resizing: bool,
    /// Whether the pointer moved since the left button went down, which makes it a drag
    /// rather than a click.
    dragged: bool,
    pub hovered_divider: Option<(TrackId, TrackId)>,

    /// The cell of the last handled motion event. Terminals can report several motion events
    /// within one cell, which would otherwise repeat the hover lookup and redraw. Reset this
    /// whenever the state under the cursor may have changed.
    pub last_hover: Option<(u16, u16)>,
}

impl MouseRegister {
    pub fn is_divider_highlighted(&self, area_type: &AreaType) -> bool {
        match area_type {
            AreaType::AlignmentDivider { upper, lower } => {
                let pair = Some((*upper, *lower));
                self.hovered_divider == pair || self.active_divider == pair
            }
            _ => false,
        }
    }

    pub fn is_sidebar_divider_highlighted(&self) -> bool {
        self.sidebar_resizing
    }

    /// Translate a mouse event into app messages using the last rendered layout.
    pub fn handle_mouse_event(
        &mut self,
        state: &State,
        layout: &ResolvedMainLayout,
        alignment_view: &AlignmentView,
        event: MouseEvent,
    ) -> Result<Vec<Action>, TGVError> {
        let cell = (event.column, event.row);
        if event.kind == MouseEventKind::Moved {
            if self.last_hover == Some(cell) {
                return Ok(Vec::new());
            }
            self.last_hover = Some(cell);
        } else {
            self.last_hover = None;
        }

        let mut messages = Vec::new();
        // Pans move by whole columns.
        let column = alignment_view.zoom.0;
        let hovered = layout.get_area_type_at_position(event.column, event.row);
        self.hovered_divider = match hovered {
            HoveringAreaType::Track(track_index) => match layout.areas[track_index].0 {
                AreaType::AlignmentDivider { upper, lower } => Some((upper, lower)),
                _ => None,
            },
            _ => None,
        };

        match event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.last_x = event.column;
                self.last_y = event.row;
                self.mouse_down_area = None;
                self.active_divider = None;
                self.dragged = false;
                self.sidebar_resizing = matches!(hovered, HoveringAreaType::SidebarDivider);
                // Clicking a track or its sidebar section focuses it. Hovering doesn't, so the
                // pointer can't undo a focus chosen with the keyboard.
                if let HoveringAreaType::Track(index) | HoveringAreaType::Sidebar(index) = hovered {
                    match layout.areas[index].0 {
                        AreaType::Coverage(id)
                        | AreaType::Alignment(id)
                        | AreaType::Variant(id)
                        | AreaType::Bed(id) => {
                            messages
                                .push(Action::Input(InputAction::Focus(FocusTarget::Track(id))));
                        }
                        AreaType::Sequence | AreaType::GeneTrack => {
                            messages
                                .push(Action::Input(InputAction::Focus(FocusTarget::Reference)));
                        }
                        _ => {}
                    }
                }
                if let HoveringAreaType::Track(track_index) = hovered {
                    let area = layout.areas[track_index].0;
                    self.mouse_down_area = Some(area);
                    // The cytoband spans the whole contig, so a click there goes to that part of
                    // the contig at the same zoom.
                    let rect = layout.areas[track_index].1;
                    let bar_width = rect
                        .width
                        .saturating_sub(crate::rendering::cytoband::CYTOBAND_TEXT_RIGHT_SPACING);
                    if area == AreaType::Cytoband
                        && event.column - rect.x < bar_width
                        && let Some(length) = state.contig_length(&alignment_view.focus)?
                    {
                        // Aim at the middle of the clicked column.
                        let fraction =
                            (f64::from(event.column - rect.x) + 0.5) / f64::from(bar_width);
                        let position = ((fraction * length as f64) as u64).clamp(1, length);
                        messages.push(Movement::Position(position).into());
                    }
                    if let AreaType::AlignmentDivider { upper, lower } = area {
                        self.active_divider = Some((upper, lower));
                    }
                }
            }
            MouseEventKind::Down(MouseButton::Right) => {
                let target = match hovered {
                    HoveringAreaType::Sidebar(index) => Some(ContextMenuTarget::Sidebar {
                        track: match layout.areas[index].0 {
                            AreaType::Coverage(id)
                            | AreaType::Alignment(id)
                            | AreaType::Variant(id)
                            | AreaType::Bed(id) => Some(id),
                            _ => None,
                        },
                    }),
                    HoveringAreaType::Track(track_index) => match &layout.areas[track_index] {
                        (AreaType::Alignment(id) | AreaType::Coverage(id), area) => {
                            Some(ContextMenuTarget::Alignment {
                                track: *id,
                                // Base actions need a single base under the cursor.
                                position: alignment_view
                                    .coordinates_of_onscreen_x(event.column, area)
                                    .and_then(|(left, right)| (left == right).then_some(left)),
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
                    messages.push(Action::Interaction(InteractionAction::OpenContextMenu {
                        target,
                        anchor: Anchor {
                            x: f32::from(event.column),
                            y: f32::from(event.row),
                        },
                    }));
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if (event.column, event.row) != (self.last_x, self.last_y) {
                    self.dragged = true;
                }
                if self.sidebar_resizing {
                    messages.push(Action::from(UpdateLayoutAction::SetSidebarWidth(
                        event.column,
                    )));
                } else if let Some((upper, lower)) = self.active_divider {
                    let delta_rows = event.row as i32 - self.last_y as i32;
                    if delta_rows != 0 {
                        messages.push(Action::from(UpdateLayoutAction::ResizeAlignmentPair {
                            upper,
                            lower,
                            delta_rows,
                        }));
                    }
                } else if let Some(area) = self.mouse_down_area {
                    match area {
                        AreaType::Alignment(id) | AreaType::Coverage(id) => {
                            if event.column < self.last_x {
                                messages.push(Movement::Right(column).into());
                            } else if event.column > self.last_x {
                                messages.push(Movement::Left(column).into());
                            }
                            // A drag keeps scrolling the track where it started, even if it
                            // crosses into another.
                            if event.row != self.last_y {
                                messages.push(Action::Navigate(
                                    NavigationAction::ScrollAlignment {
                                        track: id,
                                        scroll: if event.row > self.last_y {
                                            Scroll::Up(1)
                                        } else {
                                            Scroll::Down(1)
                                        },
                                    },
                                ));
                            }
                        }
                        AreaType::Bed(_) | AreaType::Variant(_) => {
                            if event.column < self.last_x {
                                messages.push(Movement::Right(column).into());
                            } else if event.column > self.last_x {
                                messages.push(Movement::Left(column).into());
                            }
                        }
                        _ => {}
                    }
                }
                self.last_x = event.column;
                self.last_y = event.row;
            }
            MouseEventKind::Up(MouseButton::Left) => {
                // A click on a read, without dragging, shows its details.
                if !self.dragged
                    && let Some(AreaType::Alignment(track)) = self.mouse_down_area
                    && let HoveringAreaType::Track(track_index) = hovered
                    && let (AreaType::Alignment(id), area) = &layout.areas[track_index]
                    && *id == track
                    && let Some((_, read_id)) = read_at(
                        state,
                        layout,
                        alignment_view,
                        track,
                        area,
                        event.column,
                        event.row,
                    )?
                {
                    messages.push(Action::Interaction(InteractionAction::OpenReadDetails {
                        track,
                        read_id,
                    }));
                }
                self.mouse_down_area = None;
                self.active_divider = None;
                self.sidebar_resizing = false;
            }
            MouseEventKind::Moved => {
                if let HoveringAreaType::Track(track_index) = hovered {
                    let (area_type, area) = &layout.areas[track_index];
                    let contig_index = alignment_view.focus.contig_index;
                    let Some((left, right)) =
                        alignment_view.coordinates_of_onscreen_x(event.column, area)
                    else {
                        return Ok(messages);
                    };
                    let descriptions = match area_type {
                        AreaType::Alignment(id) => {
                            match read_at(
                                state,
                                layout,
                                alignment_view,
                                *id,
                                area,
                                event.column,
                                event.row,
                            )? {
                                Some((index, read_id)) => vec![hover::describe_read(
                                    state,
                                    index,
                                    read_id,
                                    contig_index,
                                    left,
                                    right,
                                )?],
                                None => Vec::new(),
                            }
                        }
                        AreaType::GeneTrack => {
                            hover::describe_genes(state, contig_index, left, right)?
                        }
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
                    messages.extend(descriptions.into_iter().map(Action::message));
                }
            }
            // Shift turns the vertical wheel into horizontal panning, as in browsers.
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp
                if event.modifiers.contains(KeyModifiers::SHIFT) =>
            {
                if matches!(hovered, HoveringAreaType::Track(_)) {
                    let movement = if event.kind == MouseEventKind::ScrollDown {
                        Movement::Right(SHIFT_SCROLL_COLUMNS * column)
                    } else {
                        Movement::Left(SHIFT_SCROLL_COLUMNS * column)
                    };
                    messages.push(movement.into());
                }
            }
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                if let HoveringAreaType::Track(track_index) = hovered
                    && let AreaType::Alignment(id) | AreaType::Coverage(id) =
                        layout.areas[track_index].0
                {
                    messages.push(Action::Navigate(NavigationAction::ScrollAlignment {
                        track: id,
                        scroll: if matches!(event.kind, MouseEventKind::ScrollDown) {
                            Scroll::Down(1)
                        } else {
                            Scroll::Up(1)
                        },
                    }));
                }
            }
            MouseEventKind::ScrollLeft | MouseEventKind::ScrollRight => {
                if matches!(hovered, HoveringAreaType::Track(_)) {
                    let movement = if matches!(event.kind, MouseEventKind::ScrollLeft) {
                        Movement::Left(column)
                    } else {
                        Movement::Right(column)
                    };
                    messages.push(movement.into());
                }
            }
            _ => {}
        }
        Ok(messages)
    }
}

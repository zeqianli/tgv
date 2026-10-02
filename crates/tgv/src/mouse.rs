use crate::{
    layout::{AlignmentView, AreaType, HoveringAreaType, ResolvedMainLayout},
    message::{Message, Movement, Scroll, UpdateLayoutMessage},
    track_registry::TrackId,
};
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use gv_core::{alignment::BaseCoverage, prelude::*};
use itertools::Itertools;

/// Mouse interaction state for the currently displayed layout.
#[derive(Default)]
pub struct MouseRegister {
    mouse_down_area: Option<AreaType>,
    last_x: u16,
    last_y: u16,
    active_divider: Option<(TrackId, TrackId)>,
    sidebar_resizing: bool,
    pub hovered_alignment: Option<TrackId>,
    pub hovered_divider: Option<(TrackId, TrackId)>,
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
    ) -> Result<Vec<Message>, TGVError> {
        let mut messages = Vec::new();
        let hovered = layout.get_area_type_at_position(event.column, event.row);
        self.hovered_alignment = match hovered {
            HoveringAreaType::Track(track_index) => match layout.areas[track_index].0 {
                AreaType::Alignment(id) | AreaType::Coverage(id) => Some(id),
                _ => None,
            },
            _ => None,
        };
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
                self.sidebar_resizing = matches!(hovered, HoveringAreaType::SidebarDivider);
                if let HoveringAreaType::Track(track_index) = hovered {
                    let area = layout.areas[track_index].0;
                    self.mouse_down_area = Some(area);
                    if let AreaType::AlignmentDivider { upper, lower } = area {
                        self.active_divider = Some((upper, lower));
                    }
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if self.sidebar_resizing {
                    messages.push(Message::UpdateLayout(UpdateLayoutMessage::SetSidebarWidth(
                        event.column,
                    )));
                } else if let Some((upper, lower)) = self.active_divider {
                    let delta_rows = event.row as i32 - self.last_y as i32;
                    if delta_rows != 0 {
                        messages.push(Message::UpdateLayout(
                            UpdateLayoutMessage::ResizeAlignmentPair {
                                upper,
                                lower,
                                delta_rows,
                            },
                        ));
                    }
                } else if let Some(area) = self.mouse_down_area {
                    match area {
                        AreaType::Alignment(id) | AreaType::Coverage(id) => {
                            if event.column < self.last_x {
                                messages.push(Movement::Right(1).into());
                            } else if event.column > self.last_x {
                                messages.push(Movement::Left(1).into());
                            }
                            if event.row > self.last_y {
                                let index = layout.track_registry.alignment_index(id)?;
                                messages.push(Scroll::Up { index, n: 1 }.into());
                            } else if event.row < self.last_y {
                                let index = layout.track_registry.alignment_index(id)?;
                                messages.push(Scroll::Down { index, n: 1 }.into());
                            }
                        }
                        AreaType::Bed(_) | AreaType::Variant(_) => {
                            if event.column < self.last_x {
                                messages.push(Movement::Right(1).into());
                            } else if event.column > self.last_x {
                                messages.push(Movement::Left(1).into());
                            }
                        }
                        _ => {}
                    }
                }
                self.last_x = event.column;
                self.last_y = event.row;
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.mouse_down_area = None;
                self.active_divider = None;
                self.sidebar_resizing = false;
            }
            MouseEventKind::Moved => {
                if let HoveringAreaType::Track(track_index) = hovered {
                    let (area_type, area) = &layout.areas[track_index];
                    match area_type {
                        AreaType::Alignment(id) => {
                            let index = layout.track_registry.alignment_index(*id)?;
                            if let (Some((left_coordinate, right_coordinate)), Some(y_coordinate)) = (
                                alignment_view.coordinates_of_onscreen_x(event.column, area),
                                alignment_view.coordinate_of_onscreen_y(index, event.row, area),
                            ) && let Some(alignment) = state.alignments.get(index)
                                && let Some(read_id) = alignment.read_overlapping(
                                    left_coordinate,
                                    right_coordinate,
                                    y_coordinate,
                                )?
                            {
                                messages.push(Message::message(
                                    gv_core::alignment::AlignedReadRef::borrowed(
                                        alignment.record(read_id),
                                    )?
                                    .describe()?,
                                ));
                            }
                        }
                        AreaType::Sequence => {
                            if let Some((left, right)) =
                                alignment_view.coordinates_of_onscreen_x(event.column, area)
                            {
                                let description = (left..=right)
                                    .filter_map(|coordinate| {
                                        state
                                            .sequence
                                            .base_at(coordinate)
                                            .map(|base| format!("{}: {}", coordinate, base as char))
                                    })
                                    .join(", ");
                                messages.push(Message::message(description));
                            }
                        }
                        AreaType::Coverage(id) => {
                            let index = layout.track_registry.alignment_index(*id)?;
                            if let Some((left, right)) =
                                alignment_view.coordinates_of_onscreen_x(event.column, area)
                                && let Some(alignment) = state.alignments.get(index)
                            {
                                let mut coverage = BaseCoverage::default();
                                for coordinate in left..=right {
                                    coverage.add(&alignment.coverage.at(coordinate)?);
                                }
                                let description = if left == right {
                                    format!("{}: {}", left, coverage.describe())
                                } else {
                                    format!("{} - {}: {}", left, right, coverage.describe())
                                };
                                messages.push(Message::message(description));
                            }
                        }
                        AreaType::Variant(id) => {
                            let index = layout.track_registry.variant_index(*id)?;
                            if let Some((left, right)) =
                                alignment_view.coordinates_of_onscreen_x(event.column, area)
                                && let Some(variants) = state.variants.get(index)
                            {
                                for variant in variants.overlapping(
                                    alignment_view.focus.contig_index,
                                    left,
                                    right,
                                )? {
                                    messages.push(Message::message(variant.describe()));
                                }
                            }
                        }
                        AreaType::Bed(id) => {
                            let index = layout.track_registry.bed_index(*id)?;
                            if let Some((left, right)) =
                                alignment_view.coordinates_of_onscreen_x(event.column, area)
                                && let Some(intervals) = state.bed_intervals.get(index)
                            {
                                for interval in intervals.overlapping(
                                    alignment_view.focus.contig_index,
                                    left,
                                    right,
                                )? {
                                    messages.push(Message::message(interval.describe()));
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                if let HoveringAreaType::Track(track_index) = hovered {
                    let area = layout.areas[track_index].0;
                    if let AreaType::Alignment(id) | AreaType::Coverage(id) = area {
                        let index = layout.track_registry.alignment_index(id)?;
                        let scroll = if matches!(event.kind, MouseEventKind::ScrollDown) {
                            Scroll::Down { index, n: 1 }
                        } else {
                            Scroll::Up { index, n: 1 }
                        };
                        messages.push(scroll.into());
                    }
                }
            }
            MouseEventKind::ScrollLeft | MouseEventKind::ScrollRight => {
                if matches!(hovered, HoveringAreaType::Track(_)) {
                    let movement = if matches!(event.kind, MouseEventKind::ScrollLeft) {
                        Movement::Left(1)
                    } else {
                        Movement::Right(1)
                    };
                    messages.push(movement.into());
                }
            }
            _ => {}
        }
        Ok(messages)
    }
}

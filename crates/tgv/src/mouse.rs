use crate::{
    layout::{AlignmentView, AreaType, MainLayout},
    message::{Message, Movement, Scroll},
};
use crossterm::event;
use gv_core::{alignment::BaseCoverage, error::TGVError, state::State};
use itertools::Itertools;

pub struct MouseRegister {
    /// Resize event handling
    pub mouse_down_x: u16,
    pub mouse_down_y: u16,
    pub mouse_down_area_type: AreaType,
    pub resizing: bool,
    pub hovered_alignment: Option<usize>,
    pub hovered_divider: Option<AreaType>,
    pub active_divider: Option<AreaType>,

    // Track mouse dragging
    pub mouse_drag_x: u16,
    pub mouse_drag_y: u16,
    // root layout at mousedown.
    //pub root: LayoutNode,
}

impl Default for MouseRegister {
    fn default() -> Self {
        Self {
            mouse_down_x: 0,
            mouse_down_y: 0,
            mouse_down_area_type: AreaType::Error,
            resizing: false,
            hovered_alignment: None,
            hovered_divider: None,
            active_divider: None,
            mouse_drag_x: 0,
            mouse_drag_y: 0,
            //root: root.clone(),
        }
    }
}

impl MouseRegister {
    pub fn handle_mouse_event(
        &mut self,
        state: &State,
        layout: &mut MainLayout,
        alignment_view: &AlignmentView,
        event: event::MouseEvent,
    ) -> Result<Vec<Message>, TGVError> {
        let mut messages = Vec::new();
        self.update_hovered_areas(layout, event.column, event.row);

        match event.kind {
            event::MouseEventKind::Down(_) => {
                self.mouse_down_x = event.column;
                self.mouse_down_y = event.row;
                self.mouse_drag_x = event.column;
                self.mouse_drag_y = event.row;
                self.resizing = false;
                self.active_divider = None;
                self.mouse_down_area_type = AreaType::Error;
                //self.root = state.layout.root.clone();

                if let Some((area_type, area)) =
                    layout.get_area_type_at_position(event.column, event.row)
                {
                    if event.column == area.left()
                        || event.column + 1 == area.right()
                        || event.row == area.top()
                        || event.row + 1 == area.bottom()
                    {
                        self.resizing = true;
                    }
                    self.mouse_down_area_type = *area_type;
                    if matches!(area_type, AreaType::AlignmentDivider { .. }) {
                        self.resizing = true;
                        self.active_divider = Some(*area_type);
                        log::debug!(
                            "Started alignment divider drag: divider={:?} column={} row={}",
                            area_type,
                            event.column,
                            event.row,
                        );
                    }
                }
            }

            event::MouseEventKind::Drag(_) => {
                if let Some(AreaType::AlignmentDivider { upper, lower }) = self.active_divider {
                    let delta_rows = event.row as i32 - self.mouse_drag_y as i32;
                    if delta_rows != 0 {
                        layout.resize_alignment_pair(upper, lower, delta_rows);
                    }
                    self.mouse_drag_x = event.column;
                    self.mouse_drag_y = event.row;
                } else if self.resizing {
                    if (event.row != self.mouse_down_y) || (event.column != self.mouse_down_x) {
                        // TODO: next release
                        // messages.push(StateMessage::ResizeTrack {
                        //     mouse_down_x: self.mouse_down_x,
                        //     mouse_down_y: self.mouse_down_y,
                        //     mouse_released_x: event.column,
                        //     mouse_released_y: event.row,
                        // });
                    }
                } else {
                    // move alignment
                    match Self::alignment_index_for_area_type(&self.mouse_down_area_type) {
                        Some(index) => {
                            if event.column < self.mouse_drag_x {
                                messages.push(Movement::Right(1).into())
                            } else if event.column > self.mouse_drag_x {
                                messages.push(Movement::Left(1).into())
                            }

                            if event.row > self.mouse_drag_y {
                                messages.push(Scroll::Up { index, n: 1 }.into())
                            } else if event.row < self.mouse_drag_y {
                                messages.push(Scroll::Down { index, n: 1 }.into())
                            }
                        }
                        _ => {}
                    }

                    self.mouse_drag_x = event.column;
                    self.mouse_drag_y = event.row;
                }
            }

            event::MouseEventKind::Up(_) => {
                if let Some(active_divider) = self.active_divider {
                    log::debug!(
                        "Finished alignment divider drag: divider={:?} column={} row={}",
                        active_divider,
                        event.column,
                        event.row,
                    );
                }
                self.resizing = false;
                self.active_divider = None;
            }

            event::MouseEventKind::Moved => {
                // Display read information
                if let Some((area_type, area)) =
                    layout.get_area_type_at_position(event.column, event.row)
                {
                    match area_type {
                        AreaType::Alignment(index) => {
                            if let (Some((left_coordinate, right_coordinate)), Some(y_coordinate)) = (
                                &alignment_view.coordinates_of_onscreen_x(event.column, area),
                                &alignment_view.coordinate_of_onscreen_y(*index, event.row, area),
                            ) && let Some(alignment) = state.alignments.get(*index)
                                && let Some(read) = alignment.read_overlapping(
                                    *left_coordinate,
                                    *right_coordinate,
                                    *y_coordinate,
                                )
                            {
                                messages.push(Message::Core(gv_core::message::Message::Message(
                                    read.describe()?,
                                )))
                            }
                        }

                        AreaType::Sequence => {
                            if let Some((left_coordinate, right_coordinate)) =
                                alignment_view.coordinates_of_onscreen_x(event.column, area)
                            {
                                let description: String = (left_coordinate..=right_coordinate)
                                    .filter_map(|coordinate| {
                                        state.sequence.base_at(coordinate).map(|base_u8| {
                                            format!("{}: {}", coordinate, base_u8 as char)
                                        })
                                    })
                                    .join(", ");

                                messages.push(Message::message(description));
                            }
                        }

                        AreaType::Coverage(index) => {
                            if let Some((left_coordinate, right_coordinate)) =
                                alignment_view.coordinates_of_onscreen_x(event.column, area)
                                && let Some(alignment) = state.alignments.get(*index)
                            {
                                let mut total_coverage: BaseCoverage = BaseCoverage::default();
                                (left_coordinate..=right_coordinate).for_each(|coordinate| {
                                    total_coverage.add(alignment.coverage_at(coordinate))
                                });

                                let message = if left_coordinate == right_coordinate {
                                    format!("{}: {}", left_coordinate, total_coverage.describe())
                                } else {
                                    format!(
                                        "{} - {}: {}",
                                        left_coordinate,
                                        right_coordinate,
                                        total_coverage.describe()
                                    )
                                };

                                messages.push(Message::message(message));
                            }
                        }
                        AreaType::Variant(index) => {
                            if let Some((left_coordinate, right_coordinate)) =
                                alignment_view.coordinates_of_onscreen_x(event.column, area)
                                && let Some(variants) = state.variants.get(*index)
                            {
                                variants
                                    .overlapping(
                                        alignment_view.focus.contig_index,
                                        left_coordinate,
                                        right_coordinate,
                                    )?
                                    .into_iter()
                                    .for_each(|variant| {
                                        messages.push(Message::message(variant.describe()));
                                    });
                            }
                        }

                        AreaType::Bed(index) => {
                            if let Some((left_coordinate, right_coordinate)) =
                                alignment_view.coordinates_of_onscreen_x(event.column, area)
                                && let Some(bed_intervals) = state.bed_intervals.get(*index)
                            {
                                bed_intervals
                                    .overlapping(
                                        alignment_view.focus.contig_index,
                                        left_coordinate,
                                        right_coordinate,
                                    )?
                                    .into_iter()
                                    .for_each(|bed_interval| {
                                        messages.push(Message::message(bed_interval.describe()));
                                    });
                            }
                        }
                        _ => {}
                    }
                }
            }

            event::MouseEventKind::ScrollDown => {
                if let Some(index) =
                    Self::alignment_index_at_position(layout, event.column, event.row)
                {
                    log::debug!(
                        "Mouse wheel generated vertical scroll: alignment_index={} direction=down column={} row={}",
                        index,
                        event.column,
                        event.row,
                    );
                    messages.push(Scroll::Down { index, n: 1 }.into());
                }
            }

            event::MouseEventKind::ScrollUp => {
                if let Some(index) =
                    Self::alignment_index_at_position(layout, event.column, event.row)
                {
                    log::debug!(
                        "Mouse wheel generated vertical scroll: alignment_index={} direction=up column={} row={}",
                        index,
                        event.column,
                        event.row,
                    );
                    messages.push(Scroll::Up { index, n: 1 }.into());
                }
            }

            event::MouseEventKind::ScrollLeft => {
                log::debug!(
                    "Mouse wheel generated horizontal movement: direction=left column={} row={}",
                    event.column,
                    event.row,
                );
                messages.push(Movement::Left(1).into());
            }

            event::MouseEventKind::ScrollRight => {
                log::debug!(
                    "Mouse wheel generated horizontal movement: direction=right column={} row={}",
                    event.column,
                    event.row,
                );
                messages.push(Movement::Right(1).into());
            }

            _ => {}
        }

        Ok(messages)
    }

    pub fn is_divider_highlighted(&self, area_type: &AreaType) -> bool {
        matches!(area_type, AreaType::AlignmentDivider { .. })
            && (self.hovered_divider == Some(*area_type) || self.active_divider == Some(*area_type))
    }

    fn update_hovered_areas(&mut self, layout: &MainLayout, x: u16, y: u16) {
        self.hovered_alignment = Self::alignment_index_at_position(layout, x, y);
        self.hovered_divider = match layout.get_area_type_at_position(x, y) {
            Some((area_type @ AreaType::AlignmentDivider { .. }, _area)) => Some(*area_type),
            _ => None,
        };
    }

    fn alignment_index_at_position(layout: &MainLayout, x: u16, y: u16) -> Option<usize> {
        layout
            .get_area_type_at_position(x, y)
            .and_then(|(area_type, _area)| Self::alignment_index_for_area_type(area_type))
    }

    fn alignment_index_for_area_type(area_type: &AreaType) -> Option<usize> {
        match area_type {
            AreaType::Alignment(index) | AreaType::Coverage(index) => Some(*index),
            _ => None,
        }
    }
}

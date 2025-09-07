use crate::alignment::BaseCoverage;
use crate::error::TGVError;
use crate::intervals::Region;
use crate::message::StateMessage;
use crate::rendering::layout::{AreaType, LayoutNode};
use crate::repository::Repository;
use crate::states::State;
use crossterm::event;
use itertools::Itertools;
pub struct MouseRegister {
    /// Resize event handling
    pub mouse_down_x: u16,
    pub mouse_down_y: u16,
    pub mouse_down_area_type: AreaType,
    pub resizing: bool,

    // Track mouse dragging
    pub mouse_drag_x: u16,
    pub mouse_drag_y: u16,

    /// root layout at mousedown.
    pub root: LayoutNode,
}

impl MouseRegister {
    pub fn new(root: &LayoutNode) -> Self {
        Self {
            mouse_down_x: 0,
            mouse_down_y: 0,
            mouse_down_area_type: AreaType::Error,
            resizing: false,
            mouse_drag_x: 0,
            mouse_drag_y: 0,
            root: root.clone(),
        }
    }

    pub fn handle_mouse_event(
        &mut self,
        state: &State,
        repository: &Repository,
        event: event::MouseEvent,
    ) -> Result<Vec<StateMessage>, TGVError> {
        let mut messages = Vec::new();
        match event.kind {
            event::MouseEventKind::Down(_) => {
                self.mouse_down_x = event.column;
                self.mouse_down_y = event.row;
                self.root = state.layout.root.clone();

                if let Some((area_type, area)) = state
                    .layout
                    .get_area_type_at_position(event.column, event.row)
                {
                    if event.column == area.left()
                        || event.column + 1 == area.right()
                        || event.row == area.top()
                        || event.row + 1 == area.bottom()
                    {
                        self.resizing = true;
                    }
                    self.mouse_down_area_type = *area_type
                }
            }

            event::MouseEventKind::Drag(_) => {
                if self.resizing {
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
                    match self.mouse_down_area_type {
                        AreaType::Alignment | AreaType::Coverage => {
                            if event.column < self.mouse_drag_x {
                                messages.push(StateMessage::MoveRight(1))
                            } else if event.column > self.mouse_drag_x {
                                messages.push(StateMessage::MoveLeft(1))
                            }

                            if event.row > self.mouse_down_y {
                                messages.push(StateMessage::MoveUp(1))
                            } else if event.row < self.mouse_down_y {
                                messages.push(StateMessage::MoveDown(1))
                            }
                        }
                        _ => {}
                    }

                    self.mouse_drag_x = event.column;
                    self.mouse_drag_y = event.row;
                }
            }

            event::MouseEventKind::Up(_) => {
                self.resizing = false;
            }

            event::MouseEventKind::Moved => {
                // Display read information
                if let Some((area_type, area)) = state
                    .layout
                    .get_area_type_at_position(event.column, event.row)
                {
                    match area_type {
                        AreaType::Alignment => {
                            if let (Some((left_coordinate, right_coordinate)), Some(y_coordinate)) = (
                                &state.window.coordinates_of_onscreen_x(event.column, area),
                                &state.window.coordinate_of_onscreen_y(event.row, area),
                            ) {
                                if let Some(alignment) = &state.alignment {
                                    if let Some(read) = alignment.read_overlapping(
                                        *left_coordinate,
                                        *right_coordinate,
                                        *y_coordinate,
                                    ) {
                                        messages.push(StateMessage::Message(read.describe()?))
                                    }
                                }
                            }
                        }

                        AreaType::Sequence => {
                            if let Some((left_coordinate, right_coordinate)) =
                                &state.window.coordinates_of_onscreen_x(event.column, area)
                            {
                                if let Some(sequence) = state.sequence.as_ref() {
                                    let description: String = (*left_coordinate
                                        ..=*right_coordinate)
                                        .filter_map(|coordinate| {
                                            sequence.base_at(coordinate).map(|base_u8| {
                                                format!("{}: {}", coordinate, base_u8 as char)
                                            })
                                        })
                                        .join(", ");

                                    messages.push(StateMessage::Message(description))
                                }
                            }
                        }

                        AreaType::Coverage => {
                            if let Some((left_coordinate, right_coordinate)) =
                                &state.window.coordinates_of_onscreen_x(event.column, area)
                            {
                                if let Some(alignment) = state.alignment.as_ref() {
                                    let mut total_coverage: BaseCoverage = BaseCoverage::default();
                                    (*left_coordinate..=*right_coordinate).for_each(|coordinate| {
                                        total_coverage.add(alignment.coverage_at(coordinate))
                                    });

                                    let message = if *left_coordinate == *right_coordinate {
                                        format!(
                                            "{}: {}",
                                            *left_coordinate,
                                            total_coverage.describe()
                                        )
                                    } else {
                                        format!(
                                            "{} - {}: {}",
                                            *left_coordinate,
                                            *right_coordinate,
                                            total_coverage.describe()
                                        )
                                    };

                                    messages.push(StateMessage::Message(message))
                                }
                            }
                        }
                        AreaType::Variant => {
                            if let Some((left_coordinate, right_coordinate)) =
                                &state.window.coordinates_of_onscreen_x(event.column, area)
                            {
                                if let Some(variants) = repository.variant_repository.as_ref() {
                                    let region = Region {
                                        contig_index: state.contig_index(),
                                        start: *left_coordinate,
                                        end: *right_coordinate,
                                    };
                                    variants.overlapping(&region)?.into_iter().for_each(
                                        |variant| {
                                            messages.push(StateMessage::Message(variant.describe()))
                                        },
                                    );
                                }
                            }
                        }

                        AreaType::Bed => {
                            if let Some((left_coordinate, right_coordinate)) =
                                &state.window.coordinates_of_onscreen_x(event.column, area)
                            {
                                if let Some(bed_intervals) = repository.bed_intervals.as_ref() {
                                    let region = Region {
                                        contig_index: state.contig_index(),
                                        start: *left_coordinate,
                                        end: *right_coordinate,
                                    };
                                    bed_intervals.overlapping(&region)?.into_iter().for_each(
                                        |bed_interval| {
                                            messages.push(StateMessage::Message(
                                                bed_interval.describe(),
                                            ))
                                        },
                                    );
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }

            event::MouseEventKind::ScrollDown => messages.push(StateMessage::MoveDown(1)),

            event::MouseEventKind::ScrollUp => messages.push(StateMessage::MoveUp(1)),

            event::MouseEventKind::ScrollLeft => messages.push(StateMessage::MoveLeft(1)),

            event::MouseEventKind::ScrollRight => messages.push(StateMessage::MoveRight(1)),

            _ => {}
        }

        Ok(messages)
    }
}

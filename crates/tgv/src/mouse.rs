use crate::{
    layout::{AlignmentView, AreaType, LayoutNode, MainLayout},
    message::Message,
};
use crossterm::event;
use gv_core::{alignment::BaseCoverage, error::TGVError, intervals::Region, state::State};
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
        layout: &MainLayout,
        alignment_view: &AlignmentView,
        event: event::MouseEvent,
    ) -> Result<Vec<Message>, TGVError> {
        let mut messages = Vec::new();
        match event.kind {
            event::MouseEventKind::Down(_) => {
                self.mouse_down_x = event.column;
                self.mouse_down_y = event.row;
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
                                messages.push(Message::Core(gv_core::message::Message::Move(
                                    gv_core::message::Movement::Right(1),
                                )))
                            } else if event.column > self.mouse_drag_x {
                                messages.push(Message::Core(gv_core::message::Message::Move(
                                    gv_core::message::Movement::Left(1),
                                )))
                            }

                            if event.row > self.mouse_down_y {
                                messages.push(Message::Core(gv_core::message::Message::Scroll(
                                    gv_core::message::Scroll::Up(1),
                                )))
                            } else if event.row < self.mouse_down_y {
                                messages.push(Message::Core(gv_core::message::Message::Scroll(
                                    gv_core::message::Scroll::Down(1),
                                )))
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
                if let Some((area_type, area)) =
                    layout.get_area_type_at_position(event.column, event.row)
                {
                    match area_type {
                        AreaType::Alignment => {
                            if let (Some((left_coordinate, right_coordinate)), Some(y_coordinate)) = (
                                &alignment_view.coordinates_of_onscreen_x(event.column, area),
                                &alignment_view.coordinate_of_onscreen_y(event.row, area),
                            ) {
                                if let Some(read) = state.alignment.read_overlapping(
                                    *left_coordinate,
                                    *right_coordinate,
                                    *y_coordinate,
                                ) {
                                    messages.push(Message::Core(
                                        gv_core::message::Message::Message(read.describe()?),
                                    ))
                                }
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

                                messages.push(Message::Core(gv_core::message::Message::Message(
                                    (description),
                                )));
                            }
                        }

                        AreaType::Coverage => {
                            if let Some((left_coordinate, right_coordinate)) =
                                alignment_view.coordinates_of_onscreen_x(event.column, area)
                            {
                                let mut total_coverage: BaseCoverage = BaseCoverage::default();
                                (left_coordinate..=right_coordinate).for_each(|coordinate| {
                                    total_coverage.add(state.alignment.coverage_at(coordinate))
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

                                messages.push(Message::Core(gv_core::message::Message::Message(
                                    (message),
                                )));
                            }
                        }
                        AreaType::Variant => {
                            if let Some((left_coordinate, right_coordinate)) =
                                alignment_view.coordinates_of_onscreen_x(event.column, area)
                            {
                                state
                                    .variants
                                    .overlapping(
                                        alignment_view.focus.contig_index,
                                        left_coordinate,
                                        right_coordinate,
                                    )?
                                    .into_iter()
                                    .for_each(|variant| {
                                        messages.push(Message::Core(
                                            gv_core::message::Message::Message(
                                                (variant.describe()),
                                            ),
                                        ));
                                    });
                            }
                        }

                        AreaType::Bed => {
                            if let Some((left_coordinate, right_coordinate)) =
                                alignment_view.coordinates_of_onscreen_x(event.column, area)
                            {
                                state
                                    .bed_intervals
                                    .overlapping(
                                        alignment_view.focus.contig_index,
                                        left_coordinate,
                                        right_coordinate,
                                    )?
                                    .into_iter()
                                    .for_each(|bed_interval| {
                                        messages.push(Message::Core(
                                            gv_core::message::Message::Message(
                                                (bed_interval.describe()),
                                            ),
                                        ));
                                    });
                            }
                        }
                        _ => {}
                    }
                }
            }

            event::MouseEventKind::ScrollDown => messages.push(Message::Core(
                gv_core::message::Message::Scroll(gv_core::message::Scroll::Down(1)),
            )),

            event::MouseEventKind::ScrollUp => messages.push(Message::Core(
                gv_core::message::Message::Scroll(gv_core::message::Scroll::Up(1)),
            )),

            event::MouseEventKind::ScrollLeft => messages.push(Message::Core(
                gv_core::message::Message::Move(gv_core::message::Movement::Left(1)),
            )),

            event::MouseEventKind::ScrollRight => messages.push(Message::Core(
                gv_core::message::Message::Move(gv_core::message::Movement::Right(1)),
            )),

            _ => {}
        }

        Ok(messages)
    }
}

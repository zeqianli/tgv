use crate::{
    layout::{AlignmentView, AreaType, HoveringAreaType, MainLayout, ResolvedMainLayout},
    message::{Message, Movement, Scroll, UpdateLayoutMessage},
};
use crossterm::event;
use gv_core::{alignment::BaseCoverage, error::TGVError, state::State};
use itertools::Itertools;

pub struct MouseRegister {
    /// x at the mouse down event
    pub mouse_down_x: u16,
    /// y at the mouse down event
    pub mouse_down_y: u16,
    /// Track mouse dragging
    pub mouse_drag_x: u16,
    /// Track mouse dragging
    pub mouse_drag_y: u16,

    pub mouse_down_area_type: HoveringAreaType,

    /// Whether doing track resizing
    /// (track index 1, optional track index 2 (can happen whe resizing the last track))
    pub track_resizing: Option<(usize, Option<usize>)>,
    //pub hovered_alignment: Option<usize>,
    //pub hovered_divider: Option<AreaType>,
    //pub active_divider: Option<AreaType>,

    // root layout at mousedown.
    //pub root: LayoutNode,
}

impl Default for MouseRegister {
    fn default() -> Self {
        Self {
            mouse_down_x: 0,
            mouse_down_y: 0,
            mouse_down_area_type: HoveringAreaType::None,
            track_resizing: None,
            //hovered_alignment: None,
            //hovered_divider: None,
            // active_divider: None,
            mouse_drag_x: 0,
            mouse_drag_y: 0,
            //root: root.clone(),
        }
    }
}

impl MouseRegister {
    /// Translate a mouse event into a message.
    pub fn handle_mouse_event(
        &mut self,
        state: &State,
        layout_state: &mut MainLayout,
        layout: &ResolvedMainLayout,
        alignment_view: &AlignmentView,
        event: event::MouseEvent,
    ) -> Result<(Vec<Message>, HoveringAreaType), TGVError> {
        let mut messages = Vec::new();
        let hovering_area_type = layout.get_area_type_at_position(event.column, event.row);
        //self.update_hovered_areas(layout, event.column, event.row);

        match event.kind {
            event::MouseEventKind::Down(_) => {
                self.mouse_down_x = event.column;
                self.mouse_down_y = event.row;
                self.mouse_drag_x = event.column;
                self.mouse_drag_y = event.row;
                self.mouse_down_area_type = hovering_area_type.clone();

                match hovering_area_type {
                    HoveringAreaType::Track(track_index) => {
                        let (area_type, rect) = layout.areas[track_index];
                        //self.resizing = true;
                        //self.active_divider = Some(*area_type);
                        match area_type {
                            AreaType::AlignmentDivider => {
                                // Resize alignments: find the alignment

                                let i1 = layout.areas.iter().enumerate().find_map(
                                    |(i, (area, rect))| {
                                        if i < track_index && matches!(area, AreaType::Alignment(_))
                                        {
                                            Some(i)
                                        } else {
                                            None
                                        }
                                    },
                                );

                                let i2 =
                                    layout.areas.iter().enumerate().skip(track_index).find_map(
                                        |(i, (area, rect))| {
                                            if i > track_index
                                                && matches!(area, AreaType::Alignment(_))
                                            {
                                                Some(i)
                                            } else {
                                                None
                                            }
                                        },
                                    );
                                if let Some(i1) = i1 {
                                    self.track_resizing = Some((i1, i2.clone()));
                                    log::debug!(
                                        "Started alignment divider drag: divider={:?} column={} row={}",
                                        (i1, i2),
                                        event.column,
                                        event.row,
                                    );
                                }
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }

            event::MouseEventKind::Drag(_) => {
                if let Some((i1, i2)) = self.track_resizing
                    && event.row != self.mouse_drag_y
                {
                    // Resizing alignment tracks
                    messages.push(Message::UpdateLayout(UpdateLayoutMessage::ResizeTracks(
                        i1,
                        Some(layout.areas[i1].1.height + event.row - self.mouse_down_y),
                    )));
                    if let Some(i2) = i2 {
                        messages.push(Message::UpdateLayout(UpdateLayoutMessage::ResizeTracks(
                            i2,
                            Some(layout.areas[i2].1.height + event.row - self.mouse_down_y),
                        )));
                    }

                    self.mouse_drag_x = event.column;
                    self.mouse_drag_y = event.row;
                } else {
                    match self.mouse_down_area_type {
                        HoveringAreaType::Track(track_index) => {
                            // move alignment
                            match layout.areas[track_index].0 {
                                AreaType::Alignment(index) => {
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
                                AreaType::Bed(_) | AreaType::Coverage(_) | AreaType::Bed(_) => {
                                    if event.column < self.mouse_drag_x {
                                        messages.push(Movement::Right(1).into())
                                    } else if event.column > self.mouse_drag_x {
                                        messages.push(Movement::Left(1).into())
                                    }
                                }
                                _ => {}
                            };

                            self.mouse_drag_x = event.column;
                            self.mouse_drag_y = event.row;
                        }
                        _ => {}
                    }
                }
            }

            event::MouseEventKind::Up(_) => {
                if let Some((i1, i2)) = self.track_resizing {
                    log::debug!(
                        "Finished alignment divider drag: divider={:?} column={} row={}",
                        (i1, i2),
                        event.column,
                        event.row,
                    );
                }
                self.track_resizing = None;
            }

            event::MouseEventKind::Moved => {
                // Display read information
                match hovering_area_type {
                    HoveringAreaType::Track(track_index) => {
                        let area = &layout.areas[track_index].1;
                        let area_type = &layout.areas[track_index].0;

                        match area_type {
                            AreaType::Alignment(index) => {
                                if let (
                                    Some((left_coordinate, right_coordinate)),
                                    Some(y_coordinate),
                                ) = (
                                    &alignment_view.coordinates_of_onscreen_x(event.column, area),
                                    &alignment_view
                                        .coordinate_of_onscreen_y(*index, event.row, area),
                                ) && let Some(aAlignmentlignment) = state.alignments.get(*index)
                                    && let Some(read) = alignment.read_overlapping(
                                        *left_coordinate,
                                        *right_coordinate,
                                        *y_coordinate,
                                    )
                                {
                                    messages.push(Message::Core(
                                        gv_core::message::Message::Message(read.describe()?),
                                    ));
                                }
                            }

                            AreaType::Sequence => {
                                if let Some((left_coordinate, right_coordinate)) =
                                    alignment_view.coordinates_of_onscreen_x(event.column, &area)
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
                                    alignment_view.coordinates_of_onscreen_x(event.column, &area)
                                    && let Some(alignment) = state.alignments.get(*index)
                                {
                                    let total_coverage = (left_coordinate..=right_coordinate).fold(
                                        BaseCoverage::default(),
                                        |accu, coordinate| {
                                            accu.add(alignment.coverage_at(coordinate))
                                        },
                                    );

                                    let message: String = if left_coordinate == right_coordinate {
                                        format!(
                                            "{}: {}",
                                            left_coordinate,
                                            total_coverage.describe()
                                        )
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
                                            messages
                                                .push(Message::message(bed_interval.describe()));
                                        });
                                }
                            }
                            _ => {}
                        }
                    }
                    HoveringAreaType::Sidebar(track_index) => {}
                    _ => {}
                }
            }

            event::MouseEventKind::ScrollDown => match hovering_area_type {
                HoveringAreaType::Track(track_index) => {
                    let area = &layout.areas[track_index].1;
                    let area_type = &layout.areas[track_index].0;

                    match *area_type {
                        AreaType::Alignment(index) => {
                            log::debug!(
                                "Mouse wheel generated vertical scroll: alignment_index={} direction=down column={} row={}",
                                index,
                                event.column,
                                event.row,
                            );
                            messages.push(Scroll::Down { index, n: 1 }.into());
                        }

                        _ => {}
                    }
                }
                _ => {}
            },

            event::MouseEventKind::ScrollUp => match hovering_area_type {
                HoveringAreaType::Track(track_index) => {
                    let area = &layout.areas[track_index].1;
                    let area_type = &layout.areas[track_index].0;

                    match *area_type {
                        AreaType::Alignment(index) => {
                            log::debug!(
                                "Mouse wheel generated vertical scroll: alignment_index={} direction=up column={} row={}",
                                index,
                                event.column,
                                event.row,
                            );
                            messages.push(Scroll::Up { index, n: 1 }.into());
                        }
                        _ => {}
                    }
                }
                _ => {}
            },

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

        Ok((messages, hovering_area_type))
    }

  
}

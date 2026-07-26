use crate::{
    layout::{AlignmentView, MainLayout, MainLayoutArea, ResolvedMainLayout},
    message::{Message, Movement, Scroll},
};
use crossterm::event;
use gv_core::{alignment::BaseCoverage, error::TGVError, state::State};
use itertools::Itertools;
use ratatui::layout::Rect;

pub struct MouseRegister {
    /// Resize event handling
    pub mouse_down_x: u16,
    pub mouse_down_y: u16,
    pub mouse_down_area_type: MainLayoutArea,
    pub resizing: bool,
    pub hovered_alignment: Option<usize>,
    pub hovered_divider: Option<MainLayoutArea>,
    pub active_divider: Option<MainLayoutArea>,
    pub sidebar_resizing: bool,
    pub scrollbar_grab_offset: Option<u16>,

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
            mouse_down_area_type: MainLayoutArea::Error,
            resizing: false,
            hovered_alignment: None,
            hovered_divider: None,
            active_divider: None,
            sidebar_resizing: false,
            scrollbar_grab_offset: None,
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
        resolved_layout: &ResolvedMainLayout,
        alignment_view: &AlignmentView,
        event: event::MouseEvent,
    ) -> Result<Vec<Message>, TGVError> {
        let mut messages = Vec::new();
        self.update_hovered_areas(resolved_layout, event.column, event.row);

        match event.kind {
            event::MouseEventKind::Down(button) => {
                self.mouse_down_x = event.column;
                self.mouse_down_y = event.row;
                self.mouse_drag_x = event.column;
                self.mouse_drag_y = event.row;
                self.resizing = false;
                self.active_divider = None;
                self.sidebar_resizing = false;
                self.scrollbar_grab_offset = None;
                self.mouse_down_area_type = MainLayoutArea::Error;

                if button == event::MouseButton::Left
                    && resolved_layout
                        .sidebar_divider_area
                        .contains((event.column, event.row).into())
                {
                    self.resizing = true;
                    self.sidebar_resizing = true;
                    return Ok(messages);
                }

                if button == event::MouseButton::Left
                    && resolved_layout
                        .scrollbar_area
                        .contains((event.column, event.row).into())
                {
                    if resolved_layout
                        .scrollbar_thumb_area
                        .contains((event.column, event.row).into())
                    {
                        self.scrollbar_grab_offset = Some(
                            event
                                .row
                                .saturating_sub(resolved_layout.scrollbar_thumb_area.y),
                        );
                    } else {
                        layout.page_canvas_toward(event.row, resolved_layout);
                    }
                    return Ok(messages);
                }

                if let Some((area_type, _full_rect, _source_rect, destination_rect)) =
                    Self::track_at(resolved_layout, event.column, event.row)
                {
                    let area_type = *area_type;
                    let area = *destination_rect;
                    if event.column == area.left()
                        || event.column + 1 == area.right()
                        || event.row == area.top()
                        || event.row + 1 == area.bottom()
                    {
                        self.resizing = true;
                    }
                    self.mouse_down_area_type = area_type;
                    if matches!(area_type, MainLayoutArea::AlignmentDivider { .. }) {
                        self.resizing = true;
                        self.active_divider = Some(area_type);
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
                if self.sidebar_resizing {
                    layout.resize_sidebar_to(event.column, resolved_layout.terminal_area);
                    self.mouse_drag_x = event.column;
                    self.mouse_drag_y = event.row;
                } else if let Some(grab_offset) = self.scrollbar_grab_offset {
                    layout.drag_scrollbar_thumb(event.row, grab_offset, resolved_layout);
                    self.mouse_drag_x = event.column;
                    self.mouse_drag_y = event.row;
                } else if let Some(MainLayoutArea::AlignmentDivider { upper, lower }) =
                    self.active_divider
                {
                    let delta_rows = event.row as i32 - self.mouse_drag_y as i32;
                    if delta_rows != 0 {
                        layout.resize_alignment_pair(upper, lower, delta_rows, resolved_layout);
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
                    if let Some(index) =
                        Self::alignment_index_for_area_type(&self.mouse_down_area_type)
                    {
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
                self.sidebar_resizing = false;
                self.scrollbar_grab_offset = None;
            }

            event::MouseEventKind::Moved => {
                // Display read information
                if let Some((area_type, _full_rect, source_rect, destination_rect)) =
                    Self::track_at(resolved_layout, event.column, event.row)
                {
                    let area_type = *area_type;
                    let area = *destination_rect;
                    match area_type {
                        MainLayoutArea::Alignment(index) => {
                            let y_coordinate = alignment_view
                                .top(index)
                                .saturating_add(source_rect.y as usize)
                                .saturating_add(event.row.saturating_sub(area.top()) as usize);
                            if let Some((left_coordinate, right_coordinate)) =
                                &alignment_view.coordinates_of_onscreen_x(event.column, &area)
                                && let Some(alignment) = state.alignments.get(index)
                                && let Some(read) = alignment.read_overlapping(
                                    *left_coordinate,
                                    *right_coordinate,
                                    y_coordinate,
                                )
                            {
                                messages.push(Message::Core(gv_core::message::Message::Message(
                                    read.describe()?,
                                )))
                            }
                        }

                        MainLayoutArea::Sequence => {
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

                        MainLayoutArea::Coverage(index) => {
                            if let Some((left_coordinate, right_coordinate)) =
                                alignment_view.coordinates_of_onscreen_x(event.column, &area)
                                && let Some(alignment) = state.alignments.get(index)
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
                        MainLayoutArea::Variant(index) => {
                            if let Some((left_coordinate, right_coordinate)) =
                                alignment_view.coordinates_of_onscreen_x(event.column, &area)
                                && let Some(variants) = state.variants.get(index)
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

                        MainLayoutArea::Bed(index) => {
                            if let Some((left_coordinate, right_coordinate)) =
                                alignment_view.coordinates_of_onscreen_x(event.column, &area)
                                && let Some(bed_intervals) = state.bed_intervals.get(index)
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
                    Self::alignment_index_at_position(resolved_layout, event.column, event.row)
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
                    Self::alignment_index_at_position(resolved_layout, event.column, event.row)
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
        }

        Ok(messages)
    }

    pub fn is_divider_highlighted(&self, area_type: &MainLayoutArea) -> bool {
        matches!(area_type, MainLayoutArea::AlignmentDivider { .. })
            && (self.hovered_divider == Some(*area_type) || self.active_divider == Some(*area_type))
    }

    fn update_hovered_areas(&mut self, layout: &ResolvedMainLayout, x: u16, y: u16) {
        self.hovered_alignment = Self::alignment_index_at_position(layout, x, y);
        self.hovered_divider = match Self::track_at(layout, x, y) {
            Some((area_type, _, _, _))
                if matches!(area_type, MainLayoutArea::AlignmentDivider { .. }) =>
            {
                Some(*area_type)
            }
            _ => None,
        };
    }

    fn alignment_index_at_position(layout: &ResolvedMainLayout, x: u16, y: u16) -> Option<usize> {
        Self::track_at(layout, x, y)
            .and_then(|(area_type, _, _, _)| Self::alignment_index_for_area_type(area_type))
    }

    fn alignment_index_for_area_type(area_type: &MainLayoutArea) -> Option<usize> {
        match area_type {
            MainLayoutArea::Alignment(index) | MainLayoutArea::Coverage(index) => Some(*index),
            _ => None,
        }
    }

    fn track_at(
        layout: &ResolvedMainLayout,
        x: u16,
        y: u16,
    ) -> Option<&(MainLayoutArea, Rect, Rect, Rect)> {
        layout.track_rects.iter().find(|(_, _, _, destination)| {
            x >= destination.x
                && x < destination.right()
                && y >= destination.y
                && y < destination.bottom()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::MainLayoutArea as AreaType;
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    use gv_core::{
        contig_header::ContigHeader,
        intervals::Focus,
        reference::Reference,
        repository::{Repository, RepositoryFileIndex},
    };
    use ratatui::layout::Rect;

    fn mouse_event(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    fn state_with_alignments(alignment_count: usize) -> State {
        let reference = Reference::NoReference;
        let mut state = State::new(reference.clone(), ContigHeader::new(reference))
            .expect("the state is valid");
        for _ in 0..alignment_count {
            state.add_alignment_track();
        }
        state
    }

    fn repository_with_alignments(alignment_count: usize) -> Repository {
        let file_indexes = (0..alignment_count)
            .map(RepositoryFileIndex::Alignment)
            .collect::<Vec<_>>();
        Repository {
            file_indexes,
            alignment_repositories: Vec::new(),
            variant_repositories: Vec::new(),
            bed_repositories: Vec::new(),
            track_service: None,
            sequence_service: None,
        }
    }

    fn resolve(layout: &MainLayout, repository: &Repository, height: u16) -> ResolvedMainLayout {
        layout.resolve(
            Rect::new(0, 0, 80, height),
            &Reference::NoReference,
            repository,
        )
    }

    fn scroll_offset(layout: &ResolvedMainLayout) -> usize {
        let mut content_y = 0usize;
        for (_, full_rect, source_rect, destination_rect) in &layout.track_rects {
            if destination_rect.height > 0 {
                return content_y.saturating_add(source_rect.y as usize);
            }
            content_y = content_y.saturating_add(full_rect.height as usize);
        }
        0
    }

    #[test]
    fn dragging_the_sidebar_divider_resizes_the_sidebar() {
        let state = state_with_alignments(0);
        let repository = repository_with_alignments(0);
        let mut layout = MainLayout::new(0);
        let mut resolved = resolve(&layout, &repository, 24);
        let alignment_view = AlignmentView::new(Focus::default(), 0);
        let mut mouse = MouseRegister::default();
        let divider_x = resolved.sidebar_divider_area.x;

        mouse
            .handle_mouse_event(
                &state,
                &mut layout,
                &resolved,
                &alignment_view,
                mouse_event(MouseEventKind::Down(MouseButton::Left), divider_x, 3),
            )
            .expect("the mouse down is handled");
        assert!(mouse.sidebar_resizing);

        resolved = resolve(&layout, &repository, 24);
        mouse
            .handle_mouse_event(
                &state,
                &mut layout,
                &resolved,
                &alignment_view,
                mouse_event(MouseEventKind::Drag(MouseButton::Left), 30, 3),
            )
            .expect("the mouse drag is handled");
        resolved = resolve(&layout, &repository, 24);
        assert_eq!(resolved.sidebar_area.width, 30);

        mouse
            .handle_mouse_event(
                &state,
                &mut layout,
                &resolved,
                &alignment_view,
                mouse_event(MouseEventKind::Up(MouseButton::Left), 30, 3),
            )
            .expect("the mouse up is handled");
        assert!(!mouse.sidebar_resizing);
    }

    #[test]
    fn dragging_the_scrollbar_thumb_scrolls_the_canvas() {
        let state = state_with_alignments(3);
        let repository = repository_with_alignments(3);
        let mut layout = MainLayout::new(3);
        let mut resolved = resolve(&layout, &repository, 16);
        let alignment_view = AlignmentView::new(Focus::default(), 3);
        let mut mouse = MouseRegister::default();
        let scrollbar_x = resolved.scrollbar_area.x;

        mouse
            .handle_mouse_event(
                &state,
                &mut layout,
                &resolved,
                &alignment_view,
                mouse_event(MouseEventKind::Down(MouseButton::Left), scrollbar_x, 0),
            )
            .expect("the scrollbar mouse down is handled");
        assert_eq!(mouse.scrollbar_grab_offset, Some(0));

        resolved = resolve(&layout, &repository, 16);
        mouse
            .handle_mouse_event(
                &state,
                &mut layout,
                &resolved,
                &alignment_view,
                mouse_event(MouseEventKind::Drag(MouseButton::Left), scrollbar_x, 15),
            )
            .expect("the scrollbar drag is handled");
        resolved = resolve(&layout, &repository, 16);
        let scroll_limit = resolved
            .track_rects
            .iter()
            .map(|(_, full_rect, _, _)| full_rect.height as usize)
            .sum::<usize>()
            .saturating_sub(resolved.main_area.height as usize);
        assert_eq!(scroll_offset(&resolved), scroll_limit);
    }

    #[test]
    fn the_mouse_wheel_still_scrolls_reads_over_an_alignment() {
        let state = state_with_alignments(1);
        let repository = repository_with_alignments(1);
        let mut layout = MainLayout::new(1);
        let resolved = resolve(&layout, &repository, 24);
        let alignment_view = AlignmentView::new(Focus::default(), 1);
        let mut mouse = MouseRegister::default();
        let alignment_area = resolved
            .track_rects
            .iter()
            .find(|(area_type, _, _, _)| *area_type == AreaType::Alignment(0))
            .expect("the alignment area exists")
            .3;

        let messages = mouse
            .handle_mouse_event(
                &state,
                &mut layout,
                &resolved,
                &alignment_view,
                mouse_event(
                    MouseEventKind::ScrollDown,
                    alignment_area.x,
                    alignment_area.y,
                ),
            )
            .expect("the mouse wheel is handled");
        assert_eq!(messages, vec![Scroll::Down { index: 0, n: 1 }.into()]);
    }
}

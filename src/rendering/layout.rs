use std::path::Path;

use crate::alignment;
use crate::message::{self, StateMessage};
pub use crate::rendering::colors::Palette;
pub use crate::rendering::{
    render_alignment, render_bed, render_console, render_coordinates, render_coverage,
    render_cytobands, render_sequence, render_status_bar, render_track, render_variants,
};

use crate::error::TGVError;
use crate::register::{RegisterType, Registers};
use crate::repository::Repository;
use crate::settings::Settings;
use crate::states::State;
use crossterm::event;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
};
use reqwest::header::X_CONTENT_TYPE_OPTIONS;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AreaType {
    Cytoband,
    Coordinate,
    Coverage,
    Alignment,
    Sequence,
    GeneTrack,
    Console,
    Error,
    Variant,
    Bed,
}

impl AreaType {
    /// Whether the area should have an alternate background color. Useful to distinguish between tracks (alignment, vcf, bed, etc.)
    fn alternate_background(&self) -> bool {
        match self {
            AreaType::Variant | AreaType::Bed | AreaType::Alignment => true,
            _ => false,
        }
    }

    /// Whether the track can be resized.
    fn resizeable(&self) -> bool {
        // TODO: improve resizing code to allow more intuitive and flexible actions.
        match self {
            AreaType::Alignment | AreaType::Variant | AreaType::Bed | AreaType::Error => true,
            _ => false,
        }
    }
}

/// N-nary layout tree
#[derive(Debug, Clone)]
pub enum LayoutNode {
    Split {
        //id: usize,
        direction: Direction,
        constraint: Constraint,
        children: Vec<LayoutNode>,
    },
    Area {
        //id: usize,
        constraint: Constraint,
        area_type: AreaType,
    },
}

impl LayoutNode {
    pub fn constraint(&self) -> &Constraint {
        match self {
            LayoutNode::Split { constraint, .. } => constraint,
            LayoutNode::Area { constraint, .. } => constraint,
        }
    }

    fn set_constraint(&mut self, new_constraint: Constraint) {
        match self {
            LayoutNode::Split { constraint, .. } => *constraint = new_constraint,
            LayoutNode::Area { constraint, .. } => *constraint = new_constraint,
        }
    }

    pub fn reduce_constraint(&mut self, d: u16) {
        if !self.resizable() {
            return;
        }
        if let Constraint::Length(x) = self.constraint() {
            self.set_constraint(Constraint::Length(*x - u16::min(d, *x - 1)));
        }
    }

    pub fn increase_constraint(&mut self, d: u16) {
        if !self.resizable() {
            return;
        }
        if let Constraint::Length(x) = self.constraint() {
            self.set_constraint(Constraint::Length(*x + d));
        }
    }

    fn resizable(&self) -> bool {
        match self {
            LayoutNode::Area {
                constraint,
                area_type,
            } => area_type.resizeable(),
            LayoutNode::Split {
                direction,
                constraint,
                children,
            } => true,
        }
    }

    fn get_areas(&self, area: Rect, areas: &mut Vec<(AreaType, Rect)>) -> Result<(), TGVError> {
        match self {
            LayoutNode::Split {
                direction,
                constraint,
                children,
            } => {
                let child_areas = Layout::default()
                    .direction(*direction)
                    .constraints(children.iter().map(|child| child.constraint()))
                    .split(area);

                for (child, &child_area) in children.iter().zip(child_areas.iter()) {
                    child.get_areas(child_area, areas)?;
                }
            }
            LayoutNode::Area {
                constraint,
                area_type,
            } => {
                areas.push((*area_type, area));
            }
        }
        Ok(())
    }

    /// Return the area node at a screen position.
    /// Used to handle mouse event.
    pub fn get_area_type_at_position(
        &self,
        x: u16,
        y: u16,
        area: Rect,
    ) -> Option<(Rect, AreaType)> {
        match self {
            LayoutNode::Split {
                direction,
                constraint,
                children,
            } => {
                let child_areas = Layout::default()
                    .direction(*direction)
                    .constraints(children.iter().map(|child| child.constraint()))
                    .split(area);

                for (child, &child_area) in children.iter().zip(child_areas.iter()) {
                    if let Some((area, area_type)) =
                        child.get_area_type_at_position(x, y, child_area)
                    {
                        return Some((area, area_type));
                    }
                }
                None
            }

            LayoutNode::Area {
                constraint,
                area_type,
            } => {
                if x >= area.x && x < area.right() && y >= area.y && y < area.bottom() {
                    Some((area, area_type.clone()))
                } else {
                    None
                }
            }
        }
    }
}

/// Main page layout
pub struct MainLayout {
    pub root: LayoutNode,
}

impl MainLayout {
    pub fn new(root: LayoutNode) -> Result<Self, TGVError> {
        Ok(Self { root })
    }

    pub fn initialize(settings: &Settings) -> Result<Self, TGVError> {
        let mut children = Vec::new();

        if settings.needs_track() {
            children.extend(vec![
                LayoutNode::Area {
                    constraint: Constraint::Length(2),
                    area_type: AreaType::Cytoband,
                },
                LayoutNode::Area {
                    constraint: Constraint::Length(2),
                    area_type: AreaType::Coordinate,
                },
            ]);
        }

        if settings.needs_alignment() {
            children.push(LayoutNode::Area {
                constraint: Constraint::Length(6),
                area_type: AreaType::Coverage,
            })
        };
        if settings.needs_variants() {
            children.push(LayoutNode::Area {
                constraint: Constraint::Length(1),
                area_type: AreaType::Variant,
            });
        }

        if settings.needs_bed() {
            children.push(LayoutNode::Area {
                constraint: Constraint::Length(1),
                area_type: AreaType::Bed,
            });
        }

        children.extend(vec![LayoutNode::Area {
            constraint: Constraint::Fill(1),
            area_type: AreaType::Alignment,
        }]);

        if settings.needs_track() {
            children.extend(vec![
                LayoutNode::Area {
                    constraint: Constraint::Length(1),
                    area_type: AreaType::Sequence,
                },
                LayoutNode::Area {
                    constraint: Constraint::Length(2),
                    area_type: AreaType::GeneTrack,
                },
            ]);
        }

        children.extend(vec![
            LayoutNode::Area {
                constraint: Constraint::Length(2),
                area_type: AreaType::Console,
            },
            LayoutNode::Area {
                constraint: Constraint::Length(2),
                area_type: AreaType::Error,
            },
        ]);

        let root = LayoutNode::Split {
            constraint: Constraint::Fill(1), // Doesn't matter
            direction: Direction::Vertical,
            children,
        };

        Self::new(root)
    }

    /// Render all areas in the layout
    pub fn render_all(
        &self,
        area: Rect,
        buf: &mut Buffer,
        state: &State,
        registers: &Registers,
        repository: &Repository,
        pallete: &Palette,
    ) -> Result<(), TGVError> {
        let mut areas: Vec<(AreaType, Rect)> = Vec::new();
        self.root.get_areas(area, &mut areas)?;

        // Render each area based on its type
        let mut alternate_background = 0;
        for (i, (area_type, rect)) in areas.iter().enumerate() {
            let background_color = if area_type.alternate_background() {
                alternate_background += 1;
                match alternate_background % 2 {
                    1 => Some(pallete.background_1),
                    _ => Some(pallete.background_2),
                }
            } else {
                alternate_background = 0;
                None
            };
            Self::render_by_area_type(
                *area_type,
                rect,
                buf,
                background_color,
                state,
                registers,
                repository,
                pallete,
            )?;
        }
        Ok(())
    }

    /// Render an area based on its type
    fn render_by_area_type(
        area_type: AreaType,
        rect: &Rect,
        buf: &mut Buffer,
        background_color: Option<Color>,
        state: &State,
        registers: &Registers,
        repository: &Repository,
        pallete: &Palette,
    ) -> Result<(), TGVError> {
        let background_color = background_color.unwrap_or(pallete.background_1);
        match area_type {
            AreaType::Cytoband => render_cytobands(rect, buf, state, pallete)?,
            AreaType::Coordinate => render_coordinates(rect, buf, state)?,
            AreaType::Coverage => {
                if state.alignment_renderable() {
                    render_coverage(rect, buf, state, pallete)?;
                }
            }
            AreaType::Alignment => {
                if state.alignment_renderable() {
                    render_alignment(rect, buf, state, &background_color, pallete)?;
                }
            }
            AreaType::Sequence => {
                if state.sequence_renderable() {
                    render_sequence(rect, buf, state, pallete)?;
                }
            }
            AreaType::GeneTrack => {
                if state.track_renderable() {
                    render_track(rect, buf, state, pallete)?;
                }
            }
            AreaType::Console => {
                if registers.current == RegisterType::Command {
                    render_console(rect, buf, &registers.command)?;
                }
            }
            AreaType::Error => {
                render_status_bar(rect, buf, state)?;
            }
            AreaType::Variant => {
                if let Some(variants) = repository.variant_repository.as_ref() {
                    render_variants(rect, buf, variants, state, pallete)?
                }
            }
            AreaType::Bed => {
                if let Some(bed) = repository.bed_intervals.as_ref() {
                    render_bed(rect, buf, bed, state, pallete)?
                }
            }
        };
        Ok(())
    }
}

pub fn resize_node(
    node: &mut LayoutNode,
    area: Rect,
    mouse_down_x: u16,
    mouse_down_y: u16,
    mouse_released_x: u16,
    mouse_released_y: u16,
) -> Result<(), TGVError> {
    match node {
        LayoutNode::Split {
            direction,
            constraint,
            children,
        } => {
            if children.len() <= 1 {
                return Ok(());
            }

            // Mouse down is inside the area

            if direction == &Direction::Horizontal
                && (mouse_down_y < area.y || mouse_down_y > area.y + area.height)
            {
                return Ok(());
            }

            if direction == &Direction::Vertical
                && (mouse_down_y < area.y || mouse_down_y > area.y + area.height)
            {
                return Ok(());
            }

            let children_areas = Layout::default()
                .direction(*direction)
                .constraints(children.iter().map(|child| child.constraint()))
                .split(area);

            for i_child in 0..children.len() - 1 {
                // let mut first_child = children.get_mut(i_child).unwrap();
                // let mut second_child = children.get_mut(i_child + 1).unwrap();

                let first_child_area = children_areas[i_child];
                let second_child_area = children_areas[i_child + 1];

                match direction {
                    Direction::Horizontal => {
                        // if mouse_down_x < first_child_area.x {
                        //     break;
                        // }

                        let mouse_down_in_first_area = mouse_down_x >= first_child_area.x
                            && mouse_down_x < first_child_area.x + first_child_area.width;
                        let mouse_down_in_second_area = mouse_down_x >= second_child_area.x
                            && mouse_down_x < second_child_area.x + second_child_area.width - 1;
                        // Explaination for the -1:
                        // The last pixel needs to be handled by the next loop, or skipped because it's on the edge of the screen.
                        // Example:
                        // **|***|****
                        //  ^
                        //  i
                        //      ^ If clicked here, the ith loop shouldn't handle it. It should be handled by (i+1)th loop.

                        if !mouse_down_in_first_area && !mouse_down_in_second_area {
                            continue;
                        }

                        let mouse_on_boarder = mouse_down_x
                            == first_child_area.x + first_child_area.width - 1
                            || mouse_down_x == second_child_area.x;

                        if mouse_on_boarder {
                            if !children[i_child].resizable() && !children[i_child + 1].resizable()
                            {
                                continue;
                            }
                            if mouse_released_x > mouse_down_x {
                                let dx = u16::min(
                                    mouse_released_x - mouse_down_x,
                                    second_child_area.width - 1,
                                );

                                children[i_child].increase_constraint(dx);
                                children[i_child + 1].reduce_constraint(dx);

                                return Ok(());
                            } else if mouse_released_x < mouse_down_x {
                                let dx = u16::min(
                                    mouse_down_x - mouse_released_x,
                                    first_child_area.width - 1,
                                );

                                children[i_child].reduce_constraint(dx);
                                children[i_child + 1].increase_constraint(dx);

                                return Ok(());
                            }
                        }

                        // Go into children nodes.
                        if mouse_down_in_first_area {
                            return resize_node(
                                &mut children[i_child],
                                first_child_area,
                                mouse_down_x,
                                mouse_down_y,
                                mouse_released_x,
                                mouse_released_y,
                            );
                        } else {
                            // mouse_down_in
                            return resize_node(
                                &mut children[i_child + 1],
                                second_child_area,
                                mouse_down_x,
                                mouse_down_y,
                                mouse_released_x,
                                mouse_released_y,
                            );
                        }
                    }
                    Direction::Vertical => {
                        if mouse_down_y < first_child_area.y {
                            break;
                        }
                        let mouse_down_in_first_area = mouse_down_y >= first_child_area.y
                            && mouse_down_y < first_child_area.y + first_child_area.height;
                        let mouse_down_in_second_area = mouse_down_y >= second_child_area.y
                            && mouse_down_y < second_child_area.y + second_child_area.height - 1;
                        // Explaination for the -1:
                        // The last pixel needs to be handled by the next loop, or skipped because it's on the edge of the screen.
                        // Example:
                        // **|***|****
                        //  ^
                        //  i
                        //      ^ If clicked here, the ith loop shouldn't handle it. It should be handled by (i+1)th loop.

                        if !mouse_down_in_first_area && !mouse_down_in_second_area {
                            continue;
                        }

                        let mouse_on_boarder = mouse_down_y == second_child_area.y
                            || mouse_down_y == first_child_area.y + first_child_area.height - 1;

                        if mouse_on_boarder {
                            // if !children[i_child].resizable() && !children[i_child + 1].resizable()
                            // {
                            //     continue;
                            // }
                            if mouse_released_y > mouse_down_y {
                                let dy = u16::min(
                                    mouse_released_y - mouse_down_y,
                                    second_child_area.height - 1,
                                );

                                children[i_child].increase_constraint(dy);
                                children[i_child + 1].reduce_constraint(dy);
                                return Ok(());
                            } else if mouse_released_y < mouse_down_y {
                                let dy = u16::min(
                                    mouse_down_y - mouse_released_y,
                                    first_child_area.height - 1,
                                );
                                children[i_child].reduce_constraint(dy);
                                children[i_child + 1].increase_constraint(dy);
                                return Ok(());
                            }
                        }

                        // Go into children nodes.
                        if mouse_down_in_first_area {
                            return resize_node(
                                &mut children[i_child],
                                first_child_area,
                                mouse_down_x,
                                mouse_down_y,
                                mouse_released_x,
                                mouse_released_y,
                            );
                        } else {
                            return resize_node(
                                &mut children[i_child + 1],
                                second_child_area,
                                mouse_down_x,
                                mouse_down_y,
                                mouse_released_x,
                                mouse_released_y,
                            );
                        }
                    }
                }
            }
        }
        LayoutNode::Area {
            constraint,
            area_type,
        } => {}
    }

    Ok(())
}

pub struct MouseRegister {
    /// Resize event handling
    pub mouse_down_x: u16,
    pub mouse_down_y: u16,

    /// root layout at mousedown.
    pub root: LayoutNode,
}

impl MouseRegister {
    pub fn new(root: &LayoutNode) -> Self {
        Self {
            mouse_down_x: 0,
            mouse_down_y: 0,
            root: root.clone(),
        }
    }

    pub fn handle_mouse_event(
        &mut self,
        state: &State,
        event: event::MouseEvent,
    ) -> Result<Vec<StateMessage>, TGVError> {
        let mut messages = Vec::new();
        match event.kind {
            event::MouseEventKind::Down(_) => {
                self.mouse_down_x = event.column;
                self.mouse_down_y = event.row;
                self.root = state.layout.root.clone();
            }

            event::MouseEventKind::Drag(_) => {
                if (event.row != self.mouse_down_y) || (event.column != self.mouse_down_x) {
                    messages.push(StateMessage::ResizeTrack {
                        mouse_down_x: self.mouse_down_x,
                        mouse_down_y: self.mouse_down_y,
                        mouse_released_x: event.column,
                        mouse_released_y: event.row,
                    });
                }
            }

            event::MouseEventKind::Moved => {
                // Display read information
                if let Some((area, area_type)) = state.layout.root.get_area_type_at_position(
                    event.column,
                    event.row,
                    state.area.clone(),
                ) {
                    match area_type {
                        AreaType::Alignment => {
                            if let (Some((left_coordinate, right_coordinate)), Some(y_coordinate)) = (
                                &state.window.coordinates_of_onscreen_x(event.column, &area),
                                &state.window.coordinate_of_onscreen_y(event.row, &area),
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

                        _ => {}
                    }
                }
            }

            event::MouseEventKind::ScrollDown => messages.push(StateMessage::MoveDown(1)),

            event::MouseEventKind::ScrollUp => messages.push(StateMessage::MoveUp(1)),

            _ => {}
        }

        Ok(messages)
    }
}

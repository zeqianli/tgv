use crate::message::UIMessage;
pub use crate::rendering::colors::DARK_THEME;
pub use crate::rendering::{
    render_alignment, render_bed, render_console, render_coordinates, render_coverage,
    render_cytobands, render_error, render_sequence, render_track, render_variants,
};

use crate::error::TGVError;
use crate::register::{RegisterType, Registers};
use crate::repository::{self, Repository};
use crate::settings::Settings;
use crate::states::State;
use crossterm::event;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AreaType {
    Cytoband,
    Coordinate,
    Coverage,
    Alignment,
    Sequence,
    Track,
    Console,
    Error,
    Variant,
    Bed,
}

impl AreaType {
    fn alternate_background(&self) -> bool {
        match self {
            AreaType::Variant | AreaType::Bed => true,
            _ => false,
        }
    }

    fn resizeable(&self) -> bool {
        match self {
            AreaType::Alignment | AreaType::Variant | AreaType::Bed => true,
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
        match self.constraint() {
            Constraint::Length(x) => {
                self.set_constraint(Constraint::Length(*x - u16::min(d, *x - 1)));
            }
            _ => {}
        }
    }

    pub fn increase_constraint(&mut self, d: u16) {
        match self.constraint() {
            Constraint::Length(x) => {
                self.set_constraint(Constraint::Length(*x + d));
            }
            _ => {}
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
        let mut children = vec![
            LayoutNode::Area {
                constraint: Constraint::Length(2),
                area_type: AreaType::Cytoband,
            },
            LayoutNode::Area {
                constraint: Constraint::Length(6),
                area_type: AreaType::Coordinate,
            },
            LayoutNode::Area {
                constraint: Constraint::Length(1),
                area_type: AreaType::Coverage,
            },
        ];
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

        children.extend(vec![
            LayoutNode::Area {
                constraint: Constraint::Fill(1),
                area_type: AreaType::Alignment,
            },
            LayoutNode::Area {
                constraint: Constraint::Length(1),
                area_type: AreaType::Sequence,
            },
            LayoutNode::Area {
                constraint: Constraint::Length(2),
                area_type: AreaType::Track,
            },
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
            children: children,
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
    ) -> Result<(), TGVError> {
        let mut areas: Vec<(AreaType, Rect)> = Vec::new();
        self.root.get_areas(area, &mut areas)?;

        // Render each area based on its type
        for (i, (area_type, rect)) in areas.iter().enumerate() {
            let background_color = match i % 2 {
                0 => Some(DARK_THEME.background_1),
                _ => Some(DARK_THEME.background_2),
            };
            Self::render_by_area_type(
                *area_type,
                rect,
                buf,
                background_color,
                state,
                registers,
                repository,
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
    ) -> Result<(), TGVError> {
        if let Some(background_color) = background_color {
            buf.set_style(rect.clone(), Style::default().bg(background_color));
        }
        match area_type {
            AreaType::Cytoband => render_cytobands(&rect, buf, state)?,
            AreaType::Coordinate => render_coordinates(&rect, buf, state)?,
            AreaType::Coverage => {
                if state.alignment_renderable()? {
                    if let Some(alignment) = &state.alignment {
                        render_coverage(&rect, buf, state.viewing_window()?, alignment)?;
                    }
                }
            }
            AreaType::Alignment => {
                if state.alignment_renderable()? {
                    if let Some(alignment) = &state.alignment {
                        render_alignment(&rect, buf, state.viewing_window()?, alignment)?;
                    }
                }
            }
            AreaType::Sequence => {
                if state.sequence_renderable()? {
                    render_sequence(&rect, buf, state)?;
                }
            }
            AreaType::Track => {
                if state.track_renderable()? {
                    render_track(&rect, buf, state)?;
                }
            }
            AreaType::Console => {
                if registers.current == RegisterType::Command {
                    render_console(&rect, buf, &registers.command)?;
                }
            }
            AreaType::Error => {
                render_error(&rect, buf, &state.errors)?;
            }
            AreaType::Variant => {
                if let Some(variants) = repository.variant_repository.as_ref() {
                    render_variants(&rect, buf, variants, state)?
                }
            }
            AreaType::Bed => {
                if let Some(bed) = repository.bed_intervals.as_ref() {
                    render_bed(&rect, buf, bed, state)?
                }
            }
        };
        Ok(())
    }
}

fn resize_node(
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

            if direction == &Direction::Horizontal {
                if mouse_down_y < area.y || mouse_down_y > area.y + area.height {
                    return Ok(());
                }
            }

            if direction == &Direction::Vertical {
                if mouse_down_x < area.x || mouse_down_x > area.x + area.width {
                    return Ok(());
                }
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
                        let mouse_down_in_first_area = mouse_down_x >= first_child_area.x
                            && mouse_down_x < first_child_area.x + second_child_area.width;
                        let mouse_down_in_second_area = mouse_down_x >= second_child_area.x
                            && mouse_down_x < second_child_area.x + second_child_area.width;

                        if !mouse_down_in_first_area && !mouse_down_in_second_area {
                            continue;
                        }

                        let mouse_on_boarder = mouse_down_x
                            == first_child_area.x + first_child_area.width - 1
                            || mouse_down_x == second_child_area.x;

                        if mouse_on_boarder {
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
                        let mouse_down_in_first_area = mouse_down_y >= first_child_area.y
                            && mouse_down_y < first_child_area.y + second_child_area.height;
                        let mouse_down_in_second_area = mouse_down_y >= second_child_area.y
                            && mouse_down_y < second_child_area.y + second_child_area.height;

                        if !mouse_down_in_first_area && !mouse_down_in_second_area {
                            continue;
                        }

                        let mouse_on_boarder = mouse_down_y == second_child_area.y;
                        // || mouse_down_y == first_child_area.y + first_child_area.height - 1
                        // This doesn't work for some reason.

                        if mouse_on_boarder {
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

    return Ok(());
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
        root: &LayoutNode,
        event: event::MouseEvent,
    ) -> Result<Option<UIMessage>, TGVError> {
        match event.kind {
            event::MouseEventKind::Down(_) => {
                self.mouse_down_x = event.column;
                self.mouse_down_y = event.row;
                self.root = root.clone();
                return Ok(None);
            }

            event::MouseEventKind::Drag(_) => {
                if (event.row == self.mouse_down_y) && (event.column == self.mouse_down_x) {
                    return Ok(None);
                }
                return Ok(Some(UIMessage::ResizeTrack {
                    mouse_down_x: self.mouse_down_x,
                    mouse_down_y: self.mouse_down_y,
                    mouse_released_x: event.column,
                    mouse_released_y: event.row,
                }));
            }

            _ => {
                return Ok(None);
            }
        }
    }

    pub fn handle_ui_message(
        &self,
        main_layout: &mut MainLayout,
        area: Rect,
        message: UIMessage,
    ) -> Result<(), TGVError> {
        match message {
            UIMessage::ResizeTrack {
                mouse_down_x,
                mouse_down_y,
                mouse_released_x,
                mouse_released_y,
            } => {
                let mut new_node = self.root.clone();

                resize_node(
                    &mut new_node,
                    area,
                    mouse_down_x,
                    mouse_down_y,
                    mouse_released_x,
                    mouse_released_y,
                )?;

                main_layout.root = new_node;
            }
        }
        Ok(())
    }
}

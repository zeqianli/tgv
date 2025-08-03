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
}

/// N-nary layout tree
#[derive(Debug, Clone)]
pub enum LayoutNode {
    Split {
        //id: usize,
        direction: Direction,
        children: Vec<(Constraint, LayoutNode)>,
    },
    Area {
        //id: usize,
        area_type: AreaType,
    },
}

impl LayoutNode {
    fn get_areas(&self, area: Rect, areas: &mut Vec<(AreaType, Rect)>) -> Result<(), TGVError> {
        match self {
            LayoutNode::Split {
                direction,
                children,
            } => {
                let child_areas = Layout::default()
                    .direction(*direction)
                    .constraints(children.iter().map(|(constraint, _)| constraint))
                    .split(area);

                for ((_, child), &child_area) in children.iter().zip(child_areas.iter()) {
                    child.get_areas(child_area, areas)?;
                }
            }
            LayoutNode::Area { area_type } => {
                areas.push((*area_type, area));
            }
        }
        Ok(())
    }
}

/// Main page layout
pub struct MainLayout {
    root: LayoutNode,
}

impl MainLayout {
    pub fn new(root: LayoutNode) -> Result<Self, TGVError> {
        Ok(Self { root })
    }

    pub fn initialize(settings: &Settings) -> Result<Self, TGVError> {
        let mut children = vec![
            (
                Constraint::Length(2),
                LayoutNode::Area {
                    area_type: AreaType::Cytoband,
                },
            ),
            (
                Constraint::Length(6),
                LayoutNode::Area {
                    area_type: AreaType::Coordinate,
                },
            ),
            (
                Constraint::Length(1),
                LayoutNode::Area {
                    area_type: AreaType::Coverage,
                },
            ),
        ];
        if settings.needs_variants() {
            children.push((
                Constraint::Length(1),
                LayoutNode::Area {
                    area_type: AreaType::Variant,
                },
            ))
        }

        if settings.needs_bed() {
            children.push((
                Constraint::Length(1),
                LayoutNode::Area {
                    area_type: AreaType::Bed,
                },
            ));
        }

        children.extend(vec![
            (
                Constraint::Fill(1),
                LayoutNode::Area {
                    area_type: AreaType::Alignment,
                },
            ),
            (
                Constraint::Length(1),
                LayoutNode::Area {
                    area_type: AreaType::Sequence,
                },
            ),
            (
                Constraint::Length(2),
                LayoutNode::Area {
                    area_type: AreaType::Track,
                },
            ),
            (
                Constraint::Length(2),
                LayoutNode::Area {
                    area_type: AreaType::Console,
                },
            ),
            (
                Constraint::Length(2),
                LayoutNode::Area {
                    area_type: AreaType::Error,
                },
            ),
        ]);

        let root = LayoutNode::Split {
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
            let background_color = None; // TODO: alternate color
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

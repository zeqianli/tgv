pub use crate::rendering::{
    render_alignment, render_console, render_coordinates, render_coverage, render_cytobands,
    render_error, render_sequence, render_track,
};

use crate::error::TGVError;
use crate::register::{RegisterType, Registers};
use crate::settings::Settings;
use crate::states::State;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
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
}

/// N-nary layout tree
#[derive(Debug, Clone)]
pub enum LayoutNode {
    Split {
        id: usize,
        direction: Direction,
        constraints: Vec<Constraint>,
        children: Vec<LayoutNode>,
    },
    Area {
        id: usize,
        area_type: AreaType,
    },
}

impl LayoutNode {
    pub fn id(&self) -> usize {
        match self {
            LayoutNode::Split { id, .. } => *id,
            LayoutNode::Area { id, .. } => *id,
        }
    }

    pub fn populate_retrival_paths(
        &self,
        retrival_paths: &mut HashMap<usize, Vec<usize>>,
    ) -> Result<(), TGVError> {
        match self {
            LayoutNode::Split {
                id,
                direction: _,
                constraints: _,
                children,
            } => {
                match retrival_paths.get_mut(id) {
                    Some(path) => {
                        return Err(TGVError::StateError(format!("Duplicate area id: {}", id)));
                    }
                    None => {
                        retrival_paths.insert(*id, vec![]);
                    }
                }
                for (i, child) in children.iter().enumerate() {
                    // add the path to the parent node

                    child.populate_retrival_paths(retrival_paths)?;

                    for (j, path) in retrival_paths.iter_mut() {
                        if *j == child.id() {
                            // insert to the front of the path
                            path.insert(0, i);
                        }
                    }
                }
            }
            LayoutNode::Area { id, .. } => match retrival_paths.get_mut(id) {
                Some(path) => {
                    return Err(TGVError::StateError(format!("Duplicate area id: {}", id)));
                }
                None => {
                    retrival_paths.insert(*id, vec![]);
                }
            },
        }
        Ok(())
    }

    /// Calculate area for all leaf nodes
    fn calculate_rects_recursive(
        &self,
        area: Rect,
        rects: &mut HashMap<usize, Rect>,
    ) -> Result<(), TGVError> {
        match self {
            LayoutNode::Split {
                id,
                direction,
                constraints,
                children,
            } => {
                let areas = Layout::default()
                    .direction(*direction)
                    .constraints(constraints)
                    .split(area);

                if areas.len() != children.len() {
                    return Err(TGVError::StateError(format!(
                        "Invalid number of children: {}",
                        children.len()
                    )));
                }

                for (child, &child_area) in children.iter().zip(areas.iter()) {
                    child.calculate_rects_recursive(child_area, rects)?;
                }
            }
            LayoutNode::Area { id, .. } => {
                rects.insert(*id, area);
            }
        }
        Ok(())
    }
}

/// Main page layout
pub struct MainLayout {
    /// Root layout node
    root: LayoutNode,

    /// root_id -> retrival_path. Used for fast lookup by node id.
    retrival_paths: HashMap<usize, Vec<usize>>,
}

impl MainLayout {
    pub fn new(root: LayoutNode) -> Result<Self, TGVError> {
        let mut retrival_paths = HashMap::new();

        root.populate_retrival_paths(&mut retrival_paths)?;

        Ok(Self {
            root,
            retrival_paths,
        })
    }

    pub fn initialize(settings: &Settings) -> Result<Self, TGVError> {
        let root = LayoutNode::Split {
            id: 0,
            direction: Direction::Vertical,
            constraints: vec![
                Constraint::Length(2),
                Constraint::Length(2),
                Constraint::Length(6),
                Constraint::Fill(1),
                Constraint::Length(1),
                Constraint::Length(2),
                Constraint::Length(2),
                Constraint::Length(2),
            ],
            children: vec![
                LayoutNode::Area {
                    id: 1,
                    area_type: AreaType::Cytoband,
                },
                LayoutNode::Area {
                    id: 2,
                    area_type: AreaType::Coordinate,
                },
                LayoutNode::Area {
                    id: 3,
                    area_type: AreaType::Coverage,
                },
                LayoutNode::Area {
                    id: 4,
                    area_type: AreaType::Alignment,
                },
                LayoutNode::Area {
                    id: 5,
                    area_type: AreaType::Sequence,
                },
                LayoutNode::Area {
                    id: 6,
                    area_type: AreaType::Track,
                },
                LayoutNode::Area {
                    id: 7,
                    area_type: AreaType::Console,
                },
                LayoutNode::Area {
                    id: 8,
                    area_type: AreaType::Error,
                },
            ],
        };

        Self::new(root)
    }
    /// Lookup a node pointer by node id  
    pub fn get_node(&self, node_id: usize) -> Result<&LayoutNode, TGVError> {
        let path = self
            .retrival_paths
            .get(&node_id)
            .ok_or(TGVError::StateError(format!("Node not found: {}", node_id)))?;
        self.get_node_by_path(path)
    }

    /// Lookup a mutable node pointer by node id  
    pub fn get_node_mut(&mut self, node_id: usize) -> Result<&mut LayoutNode, TGVError> {
        let path = self
            .retrival_paths
            .get(&node_id)
            .ok_or(TGVError::StateError(format!("Node not found: {}", node_id)))?
            .clone();
        self.get_node_mut_by_path(&path) // TODO: performance
    }

    /// Lookup a node pointer by path
    fn get_node_by_path(&self, path: &Vec<usize>) -> Result<&LayoutNode, TGVError> {
        let mut node = &self.root;
        for id in path.iter() {
            match node {
                LayoutNode::Split {
                    id: _,
                    direction: _,
                    constraints: _,
                    children,
                } => {
                    if *id >= children.len() {
                        return Err(TGVError::StateError(format!(
                            "Invalid area path: {:?}",
                            path
                        )));
                    }
                    node = &children[*id];
                }
                LayoutNode::Area {
                    id: _,
                    area_type: _,
                } => {
                    return Err(TGVError::StateError(format!(
                        "Invalid area path: {:?}",
                        path
                    )));
                }
            }
        }
        Ok(node)
    }

    /// Lookup a mutable node pointer by path
    fn get_node_mut_by_path(&mut self, path: &Vec<usize>) -> Result<&mut LayoutNode, TGVError> {
        let mut node = &mut self.root;
        for id in path.iter() {
            match node {
                LayoutNode::Split {
                    id: _,
                    direction: _,
                    constraints: _,
                    children,
                } => {
                    if *id >= children.len() {
                        return Err(TGVError::StateError(format!(
                            "Invalid area path: {:?}",
                            path
                        )));
                    }
                    node = &mut children[*id];
                }
                LayoutNode::Area {
                    id: _,
                    area_type: _,
                } => {
                    return Err(TGVError::StateError(format!(
                        "Invalid area path: {:?}",
                        path
                    )));
                }
            }
        }
        Ok(node)
    }

    /// Render all areas in the layout
    pub fn render_all(
        &self,
        area: Rect,
        buf: &mut Buffer,
        state: &State,
        registers: &Registers,
    ) -> Result<(), TGVError> {
        let mut area_lookup = HashMap::new();
        self.root
            .calculate_rects_recursive(area, &mut area_lookup)?;

        // Render each area based on its type
        for (area_id, rect) in area_lookup.iter() {
            let node = self.get_node(*area_id)?;
            match node {
                LayoutNode::Area { id: _, area_type } => {
                    Self::render_by_area_type(*area_type, rect, buf, state, registers)?;
                }
                LayoutNode::Split {
                    id: _,
                    direction: _,
                    constraints: _,
                    children: _,
                } => {}
            }
        }
        Ok(())
    }

    /// Render an area based on its type
    fn render_by_area_type(
        area_type: AreaType,
        rect: &Rect,
        buf: &mut Buffer,
        state: &State,
        registers: &Registers,
    ) -> Result<(), TGVError> {
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
        };
        Ok(())
    }
}

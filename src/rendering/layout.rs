use crate::error::TGVError;
use crate::settings::Settings;
use ratatui::layout::{Constraint, Direction, Layout, Rect};

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
}

enum MousePosition {
    Top,
    Left,
    Bottom,
    Right,
    Center,
}

/// Main page layout
pub struct MainLayout {
    pub root: LayoutNode,

    pub main_area: Rect,

    pub areas: Vec<(AreaType, Rect)>,
}

impl MainLayout {
    pub fn new(root: LayoutNode) -> Result<Self, TGVError> {
        Ok(Self {
            root,
            main_area: Rect::default(),
            areas: Vec::new(),
        })
    }

    pub fn initialize(settings: &Settings, initial_area: Rect) -> Result<Self, TGVError> {
        let mut children = Vec::new();

        if settings.reference.needs_track() {
            children.extend(vec![LayoutNode::Area {
                constraint: Constraint::Length(2),
                area_type: AreaType::Cytoband,
            }]);
        }

        if settings.reference.needs_sequence() || settings.reference.needs_track() {
            children.extend(vec![LayoutNode::Area {
                constraint: Constraint::Length(2),
                area_type: AreaType::Coordinate,
            }]);
        }

        if settings.bam_path.is_some() {
            children.push(LayoutNode::Area {
                constraint: Constraint::Length(6),
                area_type: AreaType::Coverage,
            });
        }
        if settings.vcf_path.is_some() {
            children.push(LayoutNode::Area {
                constraint: Constraint::Length(1),
                area_type: AreaType::Variant,
            });
        }

        if settings.bed_path.is_some() {
            children.push(LayoutNode::Area {
                constraint: Constraint::Length(1),
                area_type: AreaType::Bed,
            });
        }

        children.extend(vec![LayoutNode::Area {
            constraint: Constraint::Fill(1),
            area_type: AreaType::Alignment,
        }]);

        if settings.reference.needs_sequence() {
            children.extend(vec![LayoutNode::Area {
                constraint: Constraint::Length(1),
                area_type: AreaType::Sequence,
            }]);
        }
        if settings.reference.needs_track() {
            children.extend(vec![LayoutNode::Area {
                constraint: Constraint::Length(2),
                area_type: AreaType::GeneTrack,
            }]);
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

        let mut layout = Self::new(root)?;
        layout.set_area(initial_area)?;
        Ok(layout)
    }

    pub fn set_area(&mut self, area: Rect) -> Result<&mut Self, TGVError> {
        self.main_area = area;
        let mut areas: Vec<(AreaType, Rect)> = Vec::new();
        self.root.get_areas(self.main_area, &mut areas)?;
        self.areas = areas;

        Ok(self)
    }

    pub fn get_area_type_at_position(&self, x: u16, y: u16) -> Option<&(AreaType, Rect)> {
        self.areas.iter().find(|(area_type, area)| {
            x >= area.x && x < area.right() && y >= area.y && y < area.bottom()
        })
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

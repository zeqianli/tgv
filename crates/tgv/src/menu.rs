//! Right-click context menus.
//!
//! A menu is built once when it opens, from the clicked target and the current state. While it
//! is open, it takes all mouse input: hovering highlights items and opens submenus, a click on an
//! item sends its action, and a click outside closes the menu.

use crate::message::{Action, AlignmentOptionUpdate, UpdateLayoutAction};
use crossterm::event::{MouseEvent, MouseEventKind};
use gv_core::{
    alignment::{Alignment, CoverageSchema},
    message::{AlignmentDisplayOption, AlignmentFilter, AlignmentSort},
    prelude::*,
};
use polars::prelude::ChunkAgg;
use ratatui::layout::{Position, Rect};

#[derive(Debug, Clone)]
pub enum MenuItem {
    /// An item that sends `action` when clicked. `None` shows the item as disabled.
    Entry {
        label: String,
        action: Option<Action>,
    },
    Submenu {
        label: String,
        items: Vec<MenuItem>,
    },
}

impl MenuItem {
    fn entry(label: impl Into<String>, action: Option<Action>) -> Self {
        Self::Entry {
            label: label.into(),
            action,
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Entry { label, .. } | Self::Submenu { label, .. } => label,
        }
    }

    pub fn enabled(&self) -> bool {
        match self {
            Self::Entry { action, .. } => action.is_some(),
            Self::Submenu { .. } => true,
        }
    }
}

/// One column of menu items.
#[derive(Debug, Clone)]
pub struct MenuPanel {
    pub area: Rect,
    pub items: Vec<MenuItem>,
    pub hovered: Option<usize>,
}

impl MenuPanel {
    /// Places the panel's top-left corner at `(x, y)`, shifted to fit within `bounds`.
    fn new(items: Vec<MenuItem>, x: u16, y: u16, bounds: Rect) -> Self {
        // One cell of padding on each side, plus room for the submenu marker.
        let width = items
            .iter()
            .map(|item| item.label().chars().count() + 4)
            .max()
            .unwrap_or(0)
            .min(bounds.width as usize) as u16;
        let height = (items.len() as u16).min(bounds.height);
        let x = x.min(bounds.right().saturating_sub(width)).max(bounds.x);
        let y = y.min(bounds.bottom().saturating_sub(height)).max(bounds.y);
        Self {
            area: Rect::new(x, y, width, height),
            items,
            hovered: None,
        }
    }

    fn item_at(&self, position: Position) -> Option<usize> {
        self.area
            .contains(position)
            .then(|| (position.y - self.area.y) as usize)
    }
}

#[derive(Debug, Clone)]
pub struct ContextMenu {
    pub main: MenuPanel,
    /// The open submenu and the index of its parent item in `main`.
    pub submenu: Option<(usize, MenuPanel)>,
    bounds: Rect,
}

impl ContextMenu {
    pub fn new(items: Vec<MenuItem>, column: u16, row: u16, bounds: Rect) -> Self {
        Self {
            main: MenuPanel::new(items, column, row, bounds),
            submenu: None,
            bounds,
        }
    }

    /// Items for an alignment track's reads or coverage, clicked at `position`.
    pub fn alignment_items(
        track: TrackId,
        position: Option<u64>,
        alignment: &Alignment,
        options: &[AlignmentDisplayOption],
    ) -> Result<Vec<MenuItem>, TGVError> {
        let update = |update| Action::UpdateAlignmentOptions { track, update };
        // Without a single clicked base, the base items have no position to name.
        let at = position.map_or_else(String::new, |position| format!(" at {position}"));
        let mut items = vec![MenuItem::entry(
            format!("Sort by base{at}"),
            position.map(|position| {
                update(AlignmentOptionUpdate::Sort(AlignmentSort::BaseAt(position)))
            }),
        )];

        let filter_label = format!("Filter by base{at}");
        items.push(match position {
            Some(position) => {
                let coverage = alignment.coverage.query(position, position)?;
                let mut counts = Vec::new();
                for (column, label, filter) in [
                    (CoverageSchema::A, "A", AlignmentFilter::Base(position, 'A')),
                    (CoverageSchema::C, "C", AlignmentFilter::Base(position, 'C')),
                    (CoverageSchema::G, "G", AlignmentFilter::Base(position, 'G')),
                    (CoverageSchema::T, "T", AlignmentFilter::Base(position, 'T')),
                    (CoverageSchema::N, "N", AlignmentFilter::Base(position, 'N')),
                    (
                        CoverageSchema::SOFTCLIP,
                        "Soft clip",
                        AlignmentFilter::BaseSoftclip(position),
                    ),
                ] {
                    let count = coverage.column(column)?.u64()?.sum().unwrap_or(0);
                    if count > 0 {
                        counts.push((count, label, filter));
                    }
                }
                // The most common choices come first.
                counts.sort_by_key(|(count, _, _)| std::cmp::Reverse(*count));
                if counts.is_empty() {
                    MenuItem::entry(filter_label, None)
                } else {
                    MenuItem::Submenu {
                        label: filter_label,
                        items: counts
                            .into_iter()
                            .map(|(count, label, filter)| {
                                MenuItem::entry(
                                    format!("{label} ({count})"),
                                    Some(update(AlignmentOptionUpdate::Filter(filter))),
                                )
                            })
                            .collect(),
                    }
                }
            }
            None => MenuItem::entry(filter_label, None),
        });

        let paired = options.contains(&AlignmentDisplayOption::ViewAsPairs);
        items.push(MenuItem::entry(
            if paired {
                "✓ View as pairs"
            } else {
                "View as pairs"
            },
            Some(update(AlignmentOptionUpdate::TogglePaired)),
        ));
        items.push(MenuItem::entry(
            "Reset display options",
            (!options.is_empty()).then(|| update(AlignmentOptionUpdate::Reset)),
        ));
        Ok(items)
    }

    pub fn sidebar_items() -> Vec<MenuItem> {
        vec![MenuItem::entry(
            "Hide sidebar (s)",
            Some(Action::UpdateLayout(UpdateLayoutAction::ToggleSidebar)),
        )]
    }

    /// Translates a mouse event while the menu is open.
    pub fn handle_mouse_event(&mut self, event: MouseEvent) -> Vec<Action> {
        let position = Position::new(event.column, event.row);
        let in_submenu = self
            .submenu
            .as_ref()
            .and_then(|(_, panel)| panel.item_at(position));
        let in_main = self.main.item_at(position);
        match event.kind {
            MouseEventKind::Moved | MouseEventKind::Drag(_) => {
                let before = self.hover_state();
                if let Some((_, panel)) = &mut self.submenu
                    && in_submenu.is_some()
                {
                    panel.hovered = in_submenu;
                } else if let Some(index) = in_main {
                    self.main.hovered = Some(index);
                    let opened = self.submenu.as_ref().map(|(parent, _)| *parent);
                    if opened != Some(index) {
                        self.submenu = match &self.main.items[index] {
                            MenuItem::Submenu { items, .. } => {
                                Some((index, self.submenu_panel(items.clone(), index)))
                            }
                            MenuItem::Entry { .. } => None,
                        };
                    }
                } else if let Some((_, panel)) = &mut self.submenu {
                    panel.hovered = None;
                }
                if self.hover_state() == before {
                    Vec::new()
                } else {
                    vec![Action::ContextMenuChanged]
                }
            }
            MouseEventKind::Down(_) => {
                let item = match (in_submenu, in_main) {
                    (Some(index), _) => {
                        &self.submenu.as_ref().expect("submenu is open").1.items[index]
                    }
                    (None, Some(index)) => &self.main.items[index],
                    (None, None) => return vec![Action::CloseContextMenu],
                };
                match item {
                    MenuItem::Entry {
                        action: Some(action),
                        ..
                    } => vec![action.clone(), Action::CloseContextMenu],
                    _ => Vec::new(),
                }
            }
            _ => Vec::new(),
        }
    }

    fn hover_state(&self) -> (Option<usize>, Option<(usize, Option<usize>)>) {
        (
            self.main.hovered,
            self.submenu
                .as_ref()
                .map(|(parent, panel)| (*parent, panel.hovered)),
        )
    }

    /// Places a submenu beside its parent item, on the left if the right has no room.
    fn submenu_panel(&self, items: Vec<MenuItem>, parent: usize) -> MenuPanel {
        let main = self.main.area;
        let mut panel = MenuPanel::new(items, main.right(), main.y + parent as u16, self.bounds);
        if panel.area.x < main.right() {
            panel.area.x = main.x.saturating_sub(panel.area.width).max(self.bounds.x);
        }
        panel
    }
}

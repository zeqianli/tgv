//! Right-click context menus in the terminal.
//!
//! The items come from `gv_viewer::menu`. While a menu is open, it takes all mouse input:
//! hovering highlights items and opens submenus, a click on an item sends its action, and a
//! click outside closes the menu.

use crossterm::event::{MouseEvent, MouseEventKind};
use gv_viewer::{
    menu::{Menu, MenuHit, MenuItem},
    message::{Action, UiAction},
};
use ratatui::layout::{Position, Rect};

#[derive(Debug, Clone)]
pub struct ContextMenu {
    pub menu: Menu,
    pub rects: Vec<Rect>,
    bounds: Rect,
}

impl ContextMenu {
    pub fn new(items: Vec<MenuItem>, column: u16, row: u16, bounds: Rect) -> Self {
        let rect = place(&items, column, row, bounds);
        Self {
            menu: Menu::new(items),
            rects: vec![rect],
            bounds,
        }
    }

    /// Translates a mouse event while the menu is open.
    pub fn handle_mouse_event(&mut self, event: MouseEvent) -> Vec<Action> {
        let hit = self.hit(Position::new(event.column, event.row));
        match event.kind {
            MouseEventKind::Moved | MouseEventKind::Drag(_) => {
                if !self.menu.hover(hit) {
                    return Vec::new();
                }
                self.rects.truncate(1);
                for level in 1..self.menu.levels.len() {
                    let parent = self.rects[level - 1];
                    let parent_index = self.menu.levels[level - 1]
                        .hovered
                        .expect("an open submenu's parent is hovered");
                    let mut rect = place(
                        &self.menu.levels[level].items,
                        parent.right(),
                        parent.y + parent_index as u16,
                        self.bounds,
                    );
                    if rect.x < parent.right() {
                        rect.x = parent.x.saturating_sub(rect.width).max(self.bounds.x);
                    }
                    self.rects.push(rect);
                }
                vec![UiAction::ContextMenuChanged.into()]
            }
            MouseEventKind::Down(_) => self.menu.click(hit),
            _ => Vec::new(),
        }
    }

    fn hit(&self, position: Position) -> MenuHit {
        self.rects
            .iter()
            .zip(&self.menu.levels)
            .enumerate()
            .rev()
            .find_map(|(level, (rect, menu_level))| {
                rect.contains(position)
                    .then(|| (position.y - rect.y) as usize)
                    .filter(|&index| index < menu_level.items.len())
                    .map(|index| MenuHit::Item { level, index })
            })
            .unwrap_or(MenuHit::Outside)
    }
}

/// Places the panel's top-left corner at `(x, y)`, shifted to fit within `bounds`.
fn place(items: &[MenuItem], x: u16, y: u16, bounds: Rect) -> Rect {
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
    Rect::new(x, y, width, height)
}

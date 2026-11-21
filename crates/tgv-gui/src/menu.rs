use crate::{layout::Rect, rendering::CHAR_WIDTH};
use gv_viewer::{
    menu::{Menu, MenuHit, MenuItem},
    message::{Action, UiAction},
};

/// The height of one menu item.
pub const ITEM_HEIGHT: f32 = 22.0;

#[derive(Debug, Clone)]
pub struct ContextMenu {
    pub menu: Menu,
    pub rects: Vec<Rect>,
    bounds: Rect,
}

impl ContextMenu {
    pub fn new(items: Vec<MenuItem>, x: f32, y: f32, bounds: Rect) -> Self {
        let rect = place(&items, x, y, bounds);
        Self {
            menu: Menu::new(items),
            rects: vec![rect],
            bounds,
        }
    }

    pub fn handle_mouse_move(&mut self, x: f32, y: f32) -> Vec<Action> {
        if !self.menu.hover(self.hit(x, y)) {
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
                parent.y + parent_index as f32 * ITEM_HEIGHT,
                self.bounds,
            );
            if rect.x < parent.right() {
                rect.x = (parent.x - rect.width).max(self.bounds.x);
            }
            self.rects.push(rect);
        }
        vec![UiAction::ContextMenuChanged.into()]
    }

    pub fn handle_mouse_down(&mut self, x: f32, y: f32) -> Vec<Action> {
        self.menu.click(self.hit(x, y))
    }

    fn hit(&self, x: f32, y: f32) -> MenuHit {
        self.rects
            .iter()
            .zip(&self.menu.levels)
            .enumerate()
            .rev()
            .find_map(|(level, (rect, menu_level))| {
                rect.contains(x, y)
                    .then(|| ((y - rect.y) / ITEM_HEIGHT) as usize)
                    .filter(|&index| index < menu_level.items.len())
                    .map(|index| MenuHit::Item { level, index })
            })
            .unwrap_or(MenuHit::Outside)
    }
}

fn place(items: &[MenuItem], x: f32, y: f32, bounds: Rect) -> Rect {
    let width = (items
        .iter()
        .map(|item| item.label().chars().count() + 4) // Padding on each side, plus room for the submenu marker.
        .max()
        .unwrap_or(0) as f32
        * CHAR_WIDTH)
        .min(bounds.width);
    let height = (items.len() as f32 * ITEM_HEIGHT).min(bounds.height);
    let x = x.min(bounds.right() - width).max(bounds.x);
    let y = y.min(bounds.bottom() - height).max(bounds.y);
    Rect::new(x, y, width, height)
}

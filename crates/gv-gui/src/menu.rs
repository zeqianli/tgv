//! Right-click context menus in the window.
//!
//! The items come from `gv_viewer::menu`. While a menu is open, it takes all mouse input:
//! hovering highlights items and opens submenus, a click on an item sends its action, and a
//! click outside closes the menu.

use crate::{layout::Rect, rendering::CHAR_WIDTH};
use gv_viewer::{
    menu::MenuItem,
    message::{Action, UiAction},
};

/// The height of one menu item.
pub const ITEM_HEIGHT: f32 = 22.0;

/// One column of menu items.
#[derive(Debug, Clone)]
pub struct MenuPanel {
    pub area: Rect,
    pub items: Vec<MenuItem>,
    pub hovered: Option<usize>,
}

impl MenuPanel {
    /// Places the panel's top-left corner at `(x, y)`, shifted to fit within `bounds`.
    fn new(items: Vec<MenuItem>, x: f32, y: f32, bounds: Rect) -> Self {
        // Padding on each side, plus room for the submenu marker.
        let width = (items
            .iter()
            .map(|item| item.label().chars().count() + 4)
            .max()
            .unwrap_or(0) as f32
            * CHAR_WIDTH)
            .min(bounds.width);
        let height = (items.len() as f32 * ITEM_HEIGHT).min(bounds.height);
        let x = x.min(bounds.right() - width).max(bounds.x);
        let y = y.min(bounds.bottom() - height).max(bounds.y);
        Self {
            area: Rect::new(x, y, width, height),
            items,
            hovered: None,
        }
    }

    fn item_at(&self, x: f32, y: f32) -> Option<usize> {
        self.area
            .contains(x, y)
            .then(|| ((y - self.area.y) / ITEM_HEIGHT) as usize)
            .filter(|&index| index < self.items.len())
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
    pub fn new(items: Vec<MenuItem>, x: f32, y: f32, bounds: Rect) -> Self {
        Self {
            main: MenuPanel::new(items, x, y, bounds),
            submenu: None,
            bounds,
        }
    }

    /// Translates pointer motion while the menu is open.
    pub fn handle_mouse_move(&mut self, x: f32, y: f32) -> Vec<Action> {
        let in_submenu = self
            .submenu
            .as_ref()
            .and_then(|(_, panel)| panel.item_at(x, y));
        let in_main = self.main.item_at(x, y);
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
            vec![UiAction::ContextMenuChanged.into()]
        }
    }

    /// Translates a click while the menu is open.
    pub fn handle_mouse_down(&mut self, x: f32, y: f32) -> Vec<Action> {
        let in_submenu = self
            .submenu
            .as_ref()
            .and_then(|(_, panel)| panel.item_at(x, y));
        let item = match (in_submenu, self.main.item_at(x, y)) {
            (Some(index), _) => &self.submenu.as_ref().expect("submenu is open").1.items[index],
            (None, Some(index)) => &self.main.items[index],
            (None, None) => return vec![UiAction::CloseContextMenu.into()],
        };
        match item {
            MenuItem::Entry {
                action: Some(action),
                ..
            } => vec![action.clone(), UiAction::CloseContextMenu.into()],
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
        let mut panel = MenuPanel::new(
            items,
            main.right(),
            main.y + parent as f32 * ITEM_HEIGHT,
            self.bounds,
        );
        if panel.area.x < main.right() {
            panel.area.x = (main.x - panel.area.width).max(self.bounds.x);
        }
        panel
    }
}

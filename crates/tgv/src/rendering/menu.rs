use crate::{
    menu::{ContextMenu, MenuItem, MenuPanel},
    rendering::colors::Palette,
};
use ratatui::{buffer::Buffer, style::Style};

/// Draw the context menu over everything else.
pub fn render_context_menu(buf: &mut Buffer, menu: &ContextMenu, palette: &Palette) {
    render_panel(buf, &menu.main, palette);
    if let Some((_, panel)) = &menu.submenu {
        render_panel(buf, panel, palette);
    }
}

fn render_panel(buf: &mut Buffer, panel: &MenuPanel, palette: &Palette) {
    let area = panel.area.intersection(buf.area);
    let width = area.width as usize;
    for (index, item) in panel.items.iter().enumerate().take(area.height as usize) {
        let background = if panel.hovered == Some(index) && item.enabled() {
            palette.MENU_HOVER_BACKGROUND
        } else {
            palette.MENU_BACKGROUND
        };
        let foreground = if item.enabled() {
            palette.MENU_FOREGROUND
        } else {
            palette.MENU_DISABLED_FOREGROUND
        };
        let marker = if matches!(item, MenuItem::Submenu { .. }) {
            "▸"
        } else {
            ""
        };
        let label = format!(" {}", item.label());
        let text = format!(
            "{label:<padding$}{marker} ",
            padding = width.saturating_sub(marker.chars().count() + 1)
        );
        buf.set_stringn(
            area.x,
            area.y + index as u16,
            text,
            width,
            Style::default().fg(foreground).bg(background),
        );
    }
}

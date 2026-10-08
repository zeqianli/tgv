use crate::{
    layout::wrap_sidebar_label,
    menu::{ContextMenu, MenuItem, MenuPanel},
    popup::TextPopup,
    rendering::colors::Palette,
};
use ratatui::{buffer::Buffer, layout::Rect, style::Style};

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

/// Draw a text popup centered in `area`. Long values wrap within the value column. Rows that
/// don't fit are cut, and the last shown line says so.
pub fn render_text_popup(buf: &mut Buffer, area: Rect, popup: &TextPopup, palette: &Palette) {
    let label_width = popup
        .rows
        .iter()
        .map(|(label, _)| label.chars().count())
        .max()
        .unwrap_or(0);
    let indent = if label_width == 0 { 0 } else { label_width + 2 };
    // One cell of padding on each side, within a two-cell margin around the screen edge.
    let content_width = popup
        .rows
        .iter()
        .map(|(_, value)| indent + value.chars().count())
        .chain([popup.title.chars().count()])
        .max()
        .unwrap_or(0)
        .min(area.width.saturating_sub(6) as usize);
    if content_width <= indent {
        return;
    }
    let mut lines: Vec<String> = vec![popup.title.clone(), String::new()];
    for (label, value) in &popup.rows {
        for (index, part) in wrap_sidebar_label(value, (content_width - indent) as u16)
            .into_iter()
            .enumerate()
        {
            let label = if index == 0 { label.as_str() } else { "" };
            lines.push(if indent == 0 {
                part
            } else {
                format!("{label:<label_width$}  {part}")
            });
        }
    }
    let max_height = area.height.saturating_sub(2) as usize;
    if lines.len() > max_height {
        lines.truncate(max_height.saturating_sub(1));
        lines.push("…".to_string());
    }
    let width = content_width as u16 + 2;
    let height = lines.len() as u16;
    let panel = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    )
    .intersection(buf.area);
    for (row, line) in lines.iter().enumerate().take(panel.height as usize) {
        let foreground = if row == 0 {
            palette.MENU_DISABLED_FOREGROUND
        } else {
            palette.MENU_FOREGROUND
        };
        buf.set_stringn(
            panel.x,
            panel.y + row as u16,
            format!(" {line:<content_width$} "),
            panel.width as usize,
            Style::default().fg(foreground).bg(palette.MENU_BACKGROUND),
        );
    }
}

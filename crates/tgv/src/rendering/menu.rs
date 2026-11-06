use crate::rendering::colors::ToColor;
use crate::{
    layout::wrap_sidebar_label,
    menu::{ContextMenu, MenuPanel},
    rendering::colors::Palette,
};
use gv_viewer::{menu::MenuItem, popup::TextPopup};
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
            palette.MENU_HOVER_BACKGROUND.color()
        } else {
            palette.MENU_BACKGROUND.color()
        };
        let foreground = if item.enabled() {
            palette.MENU_FOREGROUND.color()
        } else {
            palette.MENU_DISABLED_FOREGROUND.color()
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

/// A text popup's wrapped lines and how many of them fit in an area.
pub struct TextPopupLayout {
    /// The rows, wrapped to fit, below the title.
    pub lines: Vec<String>,
    /// The width of the text, without the padding.
    content_width: usize,
    /// How many lines show at a time.
    pub visible: usize,
}

impl TextPopupLayout {
    /// Wraps a popup's rows within the value column, for a popup centered in `area`.
    pub fn new(area: Rect, popup: &TextPopup) -> Self {
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
        let mut lines = Vec::new();
        if content_width > indent {
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
        }
        // The title and a blank line go above the rows. When the rows overflow, a line below
        // them says which ones show.
        let max_height = area.height.saturating_sub(2) as usize;
        let visible = if lines.len() + 2 > max_height {
            max_height.saturating_sub(3)
        } else {
            lines.len()
        };
        Self {
            lines,
            content_width,
            visible,
        }
    }
}

/// Draw a text popup centered in `area`. Long values wrap within the value column. Rows that
/// don't fit scroll, and the last line says which ones show.
pub fn render_text_popup(buf: &mut Buffer, area: Rect, popup: &TextPopup, palette: &Palette) {
    let TextPopupLayout {
        lines,
        content_width,
        visible,
    } = TextPopupLayout::new(area, popup);
    if content_width == 0 || lines.is_empty() {
        return;
    }
    let scroll = popup.scroll.min(lines.len() - visible);
    let mut shown: Vec<&str> = vec![popup.title.as_str(), ""];
    shown.extend(lines[scroll..scroll + visible].iter().map(String::as_str));
    let footer;
    if visible < lines.len() {
        footer = format!(
            "Lines {}–{} of {}",
            scroll + 1,
            scroll + visible,
            lines.len()
        );
        shown.push(&footer);
    }
    let width = content_width as u16 + 2;
    let height = shown.len() as u16;
    let panel = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    )
    .intersection(buf.area);
    let last = shown.len() - 1;
    for (row, line) in shown.iter().enumerate().take(panel.height as usize) {
        let foreground = if row == 0 || (row == last && visible < lines.len()) {
            palette.MENU_DISABLED_FOREGROUND.color()
        } else {
            palette.MENU_FOREGROUND.color()
        };
        buf.set_stringn(
            panel.x,
            panel.y + row as u16,
            format!(" {line:<content_width$} "),
            panel.width as usize,
            Style::default()
                .fg(foreground)
                .bg(palette.MENU_BACKGROUND.color()),
        );
    }
}

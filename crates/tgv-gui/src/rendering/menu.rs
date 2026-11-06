use crate::{
    layout::Rect,
    menu::{ContextMenu, ITEM_HEIGHT, MenuPanel},
    rendering::{
        CHAR_WIDTH, LINE_HEIGHT, Painter,
        colors::{MUTED_TEXT, Palette, TEXT, ToHsla},
    },
};
use gv_viewer::{menu::MenuItem, popup::TextPopup};

/// Draw the context menu over everything else.
pub fn render_context_menu(painter: &mut Painter, menu: &ContextMenu, palette: &Palette) {
    render_panel(painter, &menu.main, palette);
    if let Some((_, panel)) = &menu.submenu {
        render_panel(painter, panel, palette);
    }
}

fn render_panel(painter: &mut Painter, panel: &MenuPanel, palette: &Palette) {
    painter.fill(panel.area, palette.MENU_BACKGROUND.hsla());
    for (index, item) in panel.items.iter().enumerate() {
        let row = Rect::new(
            panel.area.x,
            panel.area.y + index as f32 * ITEM_HEIGHT,
            panel.area.width,
            ITEM_HEIGHT,
        );
        if row.y >= panel.area.bottom() {
            break;
        }
        if panel.hovered == Some(index) && item.enabled() {
            painter.fill(row, palette.MENU_HOVER_BACKGROUND.hsla());
        }
        let foreground = if item.enabled() {
            palette.MENU_FOREGROUND
        } else {
            palette.MENU_DISABLED_FOREGROUND
        };
        let y = row.y + (ITEM_HEIGHT - LINE_HEIGHT) / 2.0;
        painter.text(row.x + CHAR_WIDTH, y, item.label(), foreground);
        if matches!(item, MenuItem::Submenu { .. }) {
            painter.text(row.right() - 2.0 * CHAR_WIDTH, y, "▸", foreground);
        }
    }
}

/// A text popup's wrapped lines and how many of them fit in an area.
pub struct TextPopupLayout {
    /// The rows, wrapped to fit, as a label and a value.
    pub lines: Vec<(String, String)>,
    /// How many lines show at a time.
    pub visible: usize,
    width: f32,
    value_x: f32,
}

impl TextPopupLayout {
    /// Wraps a popup's rows within the value column, for a popup centered in `area`.
    pub fn new(area: Rect, popup: &TextPopup) -> Self {
        let label_width = popup
            .rows
            .iter()
            .map(|(label, _)| label.chars().count())
            .max()
            .unwrap_or(0) as f32
            * CHAR_WIDTH;
        let width = (area.width * 0.8).min(900.0);
        let value_x = label_width + 2.0 * CHAR_WIDTH;
        let value_columns = ((width - value_x - 2.0 * CHAR_WIDTH) / CHAR_WIDTH).max(8.0) as usize;
        let mut lines: Vec<(String, String)> = Vec::new();
        for (label, value) in &popup.rows {
            let chars = value.chars().collect::<Vec<_>>();
            for (index, chunk) in chars.chunks(value_columns).enumerate() {
                let label = if index == 0 {
                    label.clone()
                } else {
                    String::new()
                };
                lines.push((label, chunk.iter().collect()));
            }
            if chars.is_empty() {
                lines.push((label.clone(), String::new()));
            }
        }
        // The title goes above the rows. When the rows overflow, a line below them says which
        // ones show.
        let max_lines = (((area.height * 0.8) / LINE_HEIGHT) as usize).saturating_sub(2);
        let visible = if lines.len() > max_lines {
            max_lines.saturating_sub(1)
        } else {
            lines.len()
        };
        Self {
            lines,
            visible,
            width,
            value_x,
        }
    }
}

/// Draw a text popup centered in `area`. Long values wrap within the value column. Rows that
/// don't fit scroll, and the last line says which ones show.
pub fn render_text_popup(painter: &mut Painter, area: Rect, popup: &TextPopup, palette: &Palette) {
    let TextPopupLayout {
        lines,
        visible,
        width,
        value_x,
    } = TextPopupLayout::new(area, popup);
    let overflow = visible < lines.len();
    let scroll = popup.scroll.min(lines.len() - visible);
    let height = (visible as f32 + 2.0 + f32::from(u8::from(overflow))) * LINE_HEIGHT;
    let rect = Rect::new(
        area.x + (area.width - width) / 2.0,
        area.y + (area.height - height) / 2.0,
        width,
        height,
    );
    painter.fill(rect, palette.MENU_BACKGROUND.hsla());
    let x = rect.x + CHAR_WIDTH;
    painter.bold_text(x, rect.y + LINE_HEIGHT / 2.0, &popup.title, TEXT);
    for (index, (label, value)) in lines[scroll..scroll + visible].iter().enumerate() {
        let y = rect.y + (index as f32 + 1.5) * LINE_HEIGHT;
        painter.text(x, y, label, MUTED_TEXT);
        painter.text(x + value_x, y, value, palette.MENU_FOREGROUND);
    }
    if overflow {
        painter.text(
            x,
            rect.y + (visible as f32 + 1.5) * LINE_HEIGHT,
            &format!(
                "Lines {}–{} of {}",
                scroll + 1,
                scroll + visible,
                lines.len()
            ),
            MUTED_TEXT,
        );
    }
}

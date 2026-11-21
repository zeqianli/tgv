use crate::{
    layout::Rect,
    menu::{ContextMenu, ITEM_HEIGHT, MenuPanel},
    rendering::{
        CHAR_WIDTH, LINE_HEIGHT, TextContext,
        colors::{MUTED_TEXT, Palette, TEXT, ToHsla},
        fill_rect,
    },
};
use gpui::{App, Window};
use gv_viewer::{menu::MenuItem, popup::TextPopup};

/// Draw the context menu over everything else.
pub fn render_context_menu(
    menu: &ContextMenu,
    palette: &Palette,
    window: &mut Window,
    cx: &mut App,
) -> Result<(), gv_core::Error> {
    render_panel(&menu.main, palette, window, cx)?;
    if let Some((_, panel)) = &menu.submenu {
        render_panel(panel, palette, window, cx)?;
    }
    Ok(())
}

fn render_panel(
    panel: &MenuPanel,
    palette: &Palette,
    window: &mut Window,
    cx: &mut App,
) -> Result<(), gv_core::Error> {
    fill_rect(panel.area, palette.MENU_BACKGROUND.hsla(), window);
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
            fill_rect(row, palette.MENU_HOVER_BACKGROUND.hsla(), window);
        }
        let foreground = if item.enabled() {
            palette.MENU_FOREGROUND
        } else {
            palette.MENU_DISABLED_FOREGROUND
        };
        let y = row.y + (ITEM_HEIGHT - LINE_HEIGHT) / 2.0;
        TextContext::default()
            .text(item.label())
            .color(foreground)
            .paint(row.x + CHAR_WIDTH, y, LINE_HEIGHT, window, cx)?;
        if matches!(item, MenuItem::Submenu { .. }) {
            TextContext::default().text("▸").color(foreground).paint(
                row.right() - 2.0 * CHAR_WIDTH,
                y,
                LINE_HEIGHT,
                window,
                cx,
            )?;
        }
    }
    Ok(())
}

/// Draw a text popup centered in `area`. Long values wrap within the value column. Rows that
/// don't fit are cut, and the last shown line says so.
pub fn render_text_popup(
    area: Rect,
    popup: &TextPopup,
    palette: &Palette,
    window: &mut Window,
    cx: &mut App,
) -> Result<(), gv_core::Error> {
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
    let max_lines = (((area.height * 0.8) / LINE_HEIGHT) as usize).saturating_sub(2);
    let cut = lines.len() > max_lines;
    lines.truncate(max_lines);
    let height = (lines.len() as f32 + 2.0 + f32::from(u8::from(cut))) * LINE_HEIGHT;
    let rect = Rect::new(
        area.x + (area.width - width) / 2.0,
        area.y + (area.height - height) / 2.0,
        width,
        height,
    );
    fill_rect(rect, palette.MENU_BACKGROUND.hsla(), window);
    let x = rect.x + CHAR_WIDTH;
    TextContext::default()
        .text(&popup.title)
        .color(TEXT)
        .bold()
        .paint(x, rect.y + LINE_HEIGHT / 2.0, LINE_HEIGHT, window, cx)?;
    for (index, (label, value)) in lines.iter().enumerate() {
        let y = rect.y + (index as f32 + 1.5) * LINE_HEIGHT;
        TextContext::default().text(label).color(MUTED_TEXT).paint(
            x,
            y,
            LINE_HEIGHT,
            window,
            cx,
        )?;
        TextContext::default()
            .text(value)
            .color(palette.MENU_FOREGROUND)
            .paint(x + value_x, y, LINE_HEIGHT, window, cx)?;
    }
    if cut {
        TextContext::default()
            .text("… more rows don't fit")
            .color(MUTED_TEXT)
            .paint(
                x,
                rect.y + (lines.len() as f32 + 1.5) * LINE_HEIGHT,
                LINE_HEIGHT,
                window,
                cx,
            )?;
    }
    Ok(())
}

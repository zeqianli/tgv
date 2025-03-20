use ratatui::{buffer::Buffer, layout::Rect, style::Style};

pub fn render_debug(area: &Rect, buf: &mut Buffer, debug_message: &String) {
    buf.set_string(area.x, area.y, debug_message, Style::default());
}

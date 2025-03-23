use ratatui::{buffer::Buffer, layout::Rect, style::Style};

pub fn render_error(area: &Rect, buf: &mut Buffer, errors: &Vec<String>) {
    // render the last errors that fit in the area

    let index_start = errors.len().saturating_sub(area.height as usize);
    let index_end = errors.len();

    if index_start >= index_end {
        return;
    }

    for (i, error) in errors[index_start..index_end].iter().enumerate() {
        buf.set_string(area.x, area.y + i as u16, error.clone(), Style::default());
    }
}

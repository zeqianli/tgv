use ratatui::{buffer::Buffer, layout::Rect, style::Style};

const MIN_AREA_WIDTH: u16 = 2;
const MIN_AREA_HEIGHT: u16 = 1;

pub fn render_error(area: &Rect, buf: &mut Buffer, errors: &Vec<String>) {
    if area.width < MIN_AREA_WIDTH || area.height < MIN_AREA_HEIGHT {
        return;
    }

    // render the last errors that fit in the area

    let index_start = errors.len().saturating_sub(area.height as usize);
    let index_end = errors.len();

    if index_start >= index_end {
        return;
    }

    for (i, error) in errors[index_start..index_end].iter().enumerate() {
        if i >= area.height as usize {
            break;
        }
        buf.set_string(area.x, area.y + i as u16, error.clone(), Style::default());
    }
}

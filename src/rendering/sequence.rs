use crate::rendering::colors;
use crate::{error::TGVError, models::region::Region, models::sequence::Sequence};
use ratatui::{buffer::Buffer, layout::Rect, style::Style};
const MIN_AREA_WIDTH: u16 = 2;
const MIN_AREA_HEIGHT: u16 = 1;

pub fn render_sequence(
    area: &Rect,
    buf: &mut Buffer,
    region: &Region,
    sequence: &Sequence,
) -> Result<(), TGVError> {
    if area.width < MIN_AREA_WIDTH || area.height < MIN_AREA_HEIGHT {
        return Ok(());
    }

    let sequence_string = sequence.get_sequence(region).ok_or(())?;

    for i in 0..sequence_string.len() {
        let base = sequence_string.chars().nth(i).unwrap();
        let color = match base {
            'A' | 'a' => colors::BASE_A,
            'C' | 'c' => colors::BASE_C,
            'G' | 'g' => colors::BASE_G,
            'T' | 't' => colors::BASE_T,
            _ => colors::BASE_N,
        };

        buf.set_string(
            area.x + i as u16,
            area.y,
            base.to_string(),
            Style::default()
                .fg(colors::SEQUENCE_FOREGROUND_COLOR)
                .bg(color),
        );
    }

    Ok(())
}

pub fn render_sequence_at_2x(
    area: &Rect,
    buf: &mut Buffer,
    region: &Region,
    sequence: &Sequence,
) -> Result<(), ()> {
    let sequence_string = sequence.get_sequence(region).ok_or(())?;

    for i in 0..sequence_string.len() / 2 {
        let base_1 = sequence_string.chars().nth(i * 2).unwrap();
        let base_2 = sequence_string.chars().nth(i * 2 + 1).unwrap();

        let color_character = match base_1 {
            'A' | 'a' => colors::BASE_A,
            'C' | 'c' => colors::BASE_C,
            'G' | 'g' => colors::BASE_G,
            'T' | 't' => colors::BASE_T,
            _ => colors::BASE_N,
        };

        let color_background = match base_2 {
            'A' | 'a' => colors::BASE_A,
            'C' | 'c' => colors::BASE_C,
            'G' | 'g' => colors::BASE_G,
            'T' | 't' => colors::BASE_T,
            _ => colors::BASE_N,
        };

        buf.set_string(
            area.x + i as u16,
            area.y,
            "▌",
            Style::default().fg(color_character).bg(color_background),
        );
    }

    Ok(())
}

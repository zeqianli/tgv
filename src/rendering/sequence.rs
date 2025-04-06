use crate::models::region::Region;
use crate::models::sequence::Sequence;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{palette::tailwind, Color, Style},
};
pub fn render_sequence(
    area: &Rect,
    buf: &mut Buffer,
    region: &Region,
    sequence: &Sequence,
) -> Result<(), ()> {
    let sequence_string = sequence.get_sequence(region).ok_or(())?;

    for i in 0..sequence_string.len() {
        let base = sequence_string.chars().nth(i).unwrap();
        let color = match base {
            'A' | 'a' => BASE_A,
            'C' | 'c' => BASE_C,
            'G' | 'g' => BASE_G,
            'T' | 't' => BASE_T,
            _ => BASE_N,
        };

        buf.set_string(
            area.x + i as u16,
            area.y,
            base.to_string(),
            Style::default().bg(color),
        );
    }

    Ok(())
}

const BASE_A: Color = tailwind::RED.c300;
const BASE_C: Color = tailwind::GREEN.c300;
const BASE_G: Color = tailwind::BLUE.c300;
const BASE_T: Color = tailwind::YELLOW.c300;
const BASE_N: Color = tailwind::GRAY.c300;

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
            'A' | 'a' => BASE_A,
            'C' | 'c' => BASE_C,
            'G' | 'g' => BASE_G,
            'T' | 't' => BASE_T,
            _ => BASE_N,
        };

        let color_background = match base_2 {
            'A' | 'a' => BASE_A,
            'C' | 'c' => BASE_C,
            'G' | 'g' => BASE_G,
            'T' | 't' => BASE_T,
            _ => BASE_N,
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

use crate::rendering::colors::Palette;
use crate::states::State;
use crate::{error::TGVError, region::Region, sequence::Sequence};
use ratatui::{buffer::Buffer, layout::Rect, style::Style};

const MIN_AREA_WIDTH: u16 = 2;
const MIN_AREA_HEIGHT: u16 = 1;

pub fn render_sequence(
    area: &Rect,
    buf: &mut Buffer,
    state: &State,
    pallete: &Palette,
) -> Result<(), TGVError> {
    let region = &state.viewing_region()?;

    if let Some(sequence) = &state.sequence {
        match state.viewing_window()?.zoom() {
            1 => render_sequence_at_1x(area, buf, region, sequence, pallete),
            2 => render_sequence_at_2x(area, buf, region, sequence, pallete),
            _ => Ok(()),
        }
    } else {
        Ok(())
    }
}

fn render_sequence_at_1x(
    area: &Rect,
    buf: &mut Buffer,
    region: &Region,
    sequence: &Sequence,
    pallete: &Palette,
) -> Result<(), TGVError> {
    if area.width < MIN_AREA_WIDTH || area.height < MIN_AREA_HEIGHT {
        return Ok(());
    }

    let sequence_string = sequence
        .get_sequence(region)
        .ok_or(TGVError::StateError("Sequence not found".to_string()))?;

    for i in 0..sequence_string.len() {
        let base = sequence_string.chars().nth(i).unwrap();
        let color = match base {
            'A' | 'a' => pallete.BASE_A,
            'C' | 'c' => pallete.BASE_C,
            'G' | 'g' => pallete.BASE_G,
            'T' | 't' => pallete.BASE_T,
            _ => pallete.BASE_N,
        };

        buf.set_string(
            area.x + i as u16,
            area.y,
            base.to_string(),
            Style::default()
                .fg(pallete.SEQUENCE_FOREGROUND_COLOR)
                .bg(color),
        );
    }

    Ok(())
}

/// Rendering sequences at 2x zoom using a half-block trick:
/// for every 2 bases, render the left base using foreground color of the
/// half-block unicode character and the right base using background color.
/// See: https://ratatui.rs/examples/style/colors_rgb/#_top
fn render_sequence_at_2x(
    area: &Rect,
    buf: &mut Buffer,
    region: &Region,
    sequence: &Sequence,
    pallete: &Palette,
) -> Result<(), TGVError> {
    let sequence_string = sequence
        .get_sequence(region)
        .ok_or(TGVError::StateError("Sequence not found".to_string()))?;

    for i in 0..sequence_string.len() / 2 {
        let base_1 = sequence_string.chars().nth(i * 2).unwrap();
        let base_2 = sequence_string.chars().nth(i * 2 + 1).unwrap();

        let color_character = match base_1 {
            'A' | 'a' => pallete.BASE_A,
            'C' | 'c' => pallete.BASE_C,
            'G' | 'g' => pallete.BASE_G,
            'T' | 't' => pallete.BASE_T,
            _ => pallete.BASE_N,
        };

        let color_background = match base_2 {
            'A' | 'a' => pallete.BASE_A,
            'C' | 'c' => pallete.BASE_C,
            'G' | 'g' => pallete.BASE_G,
            'T' | 't' => pallete.BASE_T,
            _ => pallete.BASE_N,
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

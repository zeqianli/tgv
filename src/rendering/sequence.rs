use crate::rendering::colors::Palette;
use crate::states::State;
use crate::{error::TGVError, intervals::Region, sequence::Sequence};
use ratatui::{buffer::Buffer, layout::Rect, style::Style};

const MIN_AREA_WIDTH: u16 = 2;
const MIN_AREA_HEIGHT: u16 = 1;

pub fn render_sequence(
    area: &Rect,
    buf: &mut Buffer,
    state: &State,
    pallete: &Palette,
) -> Result<(), TGVError> {
    let region = &state.viewing_region();

    if let Some(sequence) = &state.sequence {
        match state.window.zoom {
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

    let sequence_string = String::from_utf8(
        sequence
            .get_sequence(region)
            .ok_or(TGVError::StateError("Sequence not found".to_string()))?,
    )?;

    for (i, base) in sequence_string.chars().enumerate() {
        buf.set_string(
            area.x + i as u16,
            area.y,
            base.to_string(),
            Style::default()
                .fg(pallete.SEQUENCE_FOREGROUND_COLOR)
                .bg(pallete.base_color(base as u8)),
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
    palette: &Palette,
) -> Result<(), TGVError> {
    if area.width < MIN_AREA_WIDTH || area.height < MIN_AREA_HEIGHT {
        return Ok(());
    }

    if let Some(sequence) = sequence.get_sequence(region) {
        for i in 0..sequence.len() / 2 {
            let base1 = sequence[i * 2];
            let base2 = sequence[i * 2 + 1];

            buf.set_string(
                area.x + i as u16,
                area.y,
                "▌",
                Style::default()
                    .fg(palette.base_color(base1))
                    .bg(palette.base_color(base2)),
            );
        }
    }

    Ok(())
}

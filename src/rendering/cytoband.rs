use crate::cytoband::{Cytoband, CytobandSegment, Stain};
use crate::error::TGVError;
use crate::rendering::colors;
use crate::states::State;

use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::Style,
};

const CYTOBAND_TEXT_MIN_LEFT_SPACING: u16 = 12;
const CYTOBAND_TEXT_RIGHT_SPACING: u16 = 7;
const MIN_AREA_WIDTH: u16 = CYTOBAND_TEXT_MIN_LEFT_SPACING + CYTOBAND_TEXT_RIGHT_SPACING + 1;
const MIN_AREA_HEIGHT: u16 = 2;
pub fn render_cytobands(area: &Rect, buf: &mut Buffer, state: &State) -> Result<(), TGVError> {
    if area.width <= MIN_AREA_WIDTH {
        return Ok(());
    }

    if area.height < MIN_AREA_HEIGHT {
        return Ok(());
    }

    // Left label: chromosome name
    let reference_description = match &state.reference {
        Some(reference) => reference.to_string(),
        None => "".to_string(),
    };

    let contig_description = match state.contig() {
        Ok(contig) => contig.full_name().to_string(),
        Err(_) => "".to_string(),
    };

    let cytoband_left_spacing = u16::max(
        CYTOBAND_TEXT_MIN_LEFT_SPACING,
        reference_description.len() as u16 + 1,
    );

    let viewing_window = state.viewing_window()?;
    let contig_length = state.contig_length()?;

    let cytoband = state
        .current_cytoband()?
        .ok_or(TGVError::StateError("No cytoband found".to_string()))?;

    for (x, string, style) in get_cytoband_xs_strings_and_styles(
        cytoband,
        cytoband_left_spacing,
        area.width - CYTOBAND_TEXT_RIGHT_SPACING,
    ) {
        buf.set_string(area.x + x, area.y, string, style);
    }

    buf.set_string(area.x, area.y, reference_description, Style::default());
    buf.set_string(area.x, area.y + 1, contig_description, Style::default());

    // Right label: total length

    buf.set_string(
        area.width - CYTOBAND_TEXT_RIGHT_SPACING + 1,
        area.y,
        get_cytoband_total_length_text(cytoband.length()),
        Style::default(),
    );

    // Highlight the current viewing window

    let viewing_window_start = linear_scale(
        viewing_window.left(),
        cytoband.length(),
        cytoband_left_spacing,
        area.width - CYTOBAND_TEXT_RIGHT_SPACING,
    );
    let viewing_window_end = linear_scale(
        match contig_length {
            Some(length) => usize::min(viewing_window.right(area), length),
            None => viewing_window.right(area),
        },
        cytoband.length(),
        cytoband_left_spacing,
        area.width - CYTOBAND_TEXT_RIGHT_SPACING,
    );

    for x in viewing_window_start..viewing_window_end + 1 {
        let cell = buf.cell_mut(Position::new(area.x + x, area.y));
        if let Some(cell) = cell {
            cell.set_char(' ');
            cell.set_bg(colors::HIGHLIGHT_COLOR);
        }
    }

    Ok(())
}

fn get_cytoband_total_length_text(length: usize) -> String {
    let mut length = length;
    let mut power = 0;

    while length >= 1000 {
        length /= 1000;
        power += 1;
    }

    format!(
        "{}{}",
        length,
        match power {
            0 => "bp",
            1 => "kb",
            2 => "Mb",
            3 => "Gb",
            4 => "Tb",
            _ => "",
        }
    )
}

fn get_cytoband_xs_strings_and_styles(
    cytoband: &Cytoband,
    area_start: u16,
    area_end: u16,
) -> Vec<(u16, String, Style)> {
    let mut second_centromere = false;
    let mut output = Vec::new();
    for segment in cytoband.segments.iter() {
        if let Some((x, string, style)) = get_cytoband_segment_x_string_and_style(
            segment,
            cytoband.length(),
            area_start,
            area_end,
            second_centromere,
        ) {
            output.push((x, string, style));
        }

        if segment.stain == Stain::Acen {
            second_centromere = true;
        }
    }
    output
}

fn get_cytoband_segment_x_string_and_style(
    segment: &CytobandSegment,
    total_length: usize,
    area_start: u16,
    area_end: u16,
    second_centromere: bool,
) -> Option<(u16, String, Style)> {
    let onscreen_x_start = linear_scale(segment.start - 1, total_length, area_start, area_end); // 0-based, inclusive
    let onscreen_x_end = linear_scale(segment.end, total_length, area_start, area_end); // 0-based, exclusive

    if onscreen_x_end <= onscreen_x_start {
        return None;
    }

    let style = get_cytoband_segment_style(segment);

    match segment.stain {
        Stain::Acen => {
            // Use unicode characters to draw the centromere
            let mut string = "-".repeat((onscreen_x_end - onscreen_x_start) as usize);
            if second_centromere {
                string.replace_range(0..1, "<");
            } else {
                string.replace_range(string.len() - 1..string.len(), ">");
            }
            Some((onscreen_x_start, string, style))
        }
        _ => {
            let string = "▅".repeat((onscreen_x_end - onscreen_x_start) as usize);
            Some((onscreen_x_start, string, style))
        }
    }
}

fn linear_scale(original_x: usize, original_length: usize, new_start: u16, new_end: u16) -> u16 {
    new_start + (original_x as f64 / (original_length) as f64 * (new_end - new_start) as f64) as u16
}

fn get_cytoband_segment_style(cytoband_segment: &CytobandSegment) -> Style {
    match &cytoband_segment.stain {
        Stain::Gneg => Style::default(),
        _ => Style::default().fg(cytoband_segment.stain.get_color()),
    }
}

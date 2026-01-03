use gv_core::{
    cytoband::{Cytoband, CytobandSegment, Stain},
    error::TGVError,
    state::State,
};

use crate::{
    layout::{AlignmentView, linear_scale},
    rendering::{colors::Palette, get_abbreviated_length_string},
};

use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::Style,
};

const CYTOBAND_TEXT_MIN_LEFT_SPACING: u16 = 12;
const CYTOBAND_TEXT_RIGHT_SPACING: u16 = 7;
const MIN_AREA_WIDTH: u16 = CYTOBAND_TEXT_MIN_LEFT_SPACING + CYTOBAND_TEXT_RIGHT_SPACING + 1;
const MIN_AREA_HEIGHT: u16 = 2;
pub fn render_cytobands(
    area: &Rect,
    buf: &mut Buffer,
    state: &State,
    alignment_view: &AlignmentView,
    pallete: &Palette,
) -> Result<(), TGVError> {
    if area.width <= MIN_AREA_WIDTH {
        return Ok(());
    }

    if area.height < MIN_AREA_HEIGHT {
        return Ok(());
    }

    // Left label: chromosome name
    let reference_description = state.reference.to_string();
    let contig_description = state.contig_name(&alignment_view.focus)?;

    let cytoband_left_spacing = u16::max(
        CYTOBAND_TEXT_MIN_LEFT_SPACING,
        reference_description.len() as u16 + 1,
    );

    // Left labels
    buf.set_string(area.x, area.y, reference_description, Style::default());
    buf.set_string(area.x, area.y + 1, contig_description, Style::default());

    // Right labels

    if let Some(contig_length) = state.contig_length(&alignment_view.focus)? {
        buf.set_string(
            area.width - CYTOBAND_TEXT_RIGHT_SPACING + 1,
            area.y,
            get_abbreviated_length_string(contig_length),
            Style::default(),
        );
    }

    // Cytoband
    if let Some(cytoband) = state.current_cytoband(&alignment_view.focus) {
        for (x, string, style) in get_cytoband_xs_strings_and_styles(
            cytoband,
            cytoband_left_spacing,
            area.width - CYTOBAND_TEXT_RIGHT_SPACING,
        )? {
            buf.set_string(area.x + x, area.y, string, style);
        }
    } else {
        buf.set_string(
            area.x + cytoband_left_spacing,
            area.y,
            "▅".repeat((area.width - cytoband_left_spacing - CYTOBAND_TEXT_RIGHT_SPACING) as usize),
            Style::default(),
        );
    }

    // Highlight the current viewing window
    if let Some(contig_length) = state.contig_length(&alignment_view.focus)? {
        let viewing_window_start = linear_scale(
            alignment_view.left(area),
            contig_length,
            cytoband_left_spacing,
            area.width - CYTOBAND_TEXT_RIGHT_SPACING,
        )?;
        let viewing_window_end = linear_scale(
            alignment_view.right(area),
            contig_length,
            cytoband_left_spacing,
            area.width - CYTOBAND_TEXT_RIGHT_SPACING,
        )?;

        for x in viewing_window_start..viewing_window_end + 1 {
            let cell = buf.cell_mut(Position::new(area.x + x, area.y));
            if let Some(cell) = cell {
                cell.set_char(' ');
                cell.set_bg(pallete.HIGHLIGHT_COLOR);
            }
        }
    }

    Ok(())
}

fn get_cytoband_xs_strings_and_styles(
    cytoband: &Cytoband,
    area_start: u16,
    area_end: u16,
) -> Result<Vec<(u16, String, Style)>, TGVError> {
    let mut second_centromere = false;
    let mut output = Vec::new();
    for segment in cytoband.segments.iter() {
        if let Some((x, string, style)) = get_cytoband_segment_x_string_and_style(
            segment,
            cytoband.length(),
            area_start,
            area_end,
            second_centromere,
        )? {
            output.push((x, string, style));
        }

        if segment.stain == Stain::Acen {
            second_centromere = true;
        }
    }
    Ok(output)
}

fn get_cytoband_segment_x_string_and_style(
    segment: &CytobandSegment,
    total_length: usize,
    area_start: u16,
    area_end: u16,
    second_centromere: bool,
) -> Result<Option<(u16, String, Style)>, TGVError> {
    let onscreen_x_start = linear_scale(segment.start - 1, total_length, area_start, area_end)?; // 0-based, inclusive
    let onscreen_x_end = linear_scale(segment.end, total_length, area_start, area_end)?; // 0-based, exclusive

    if onscreen_x_end <= onscreen_x_start {
        return Ok(None);
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
            Ok(Some((onscreen_x_start, string, style)))
        }
        _ => {
            let string = "▅".repeat((onscreen_x_end - onscreen_x_start) as usize);
            Ok(Some((onscreen_x_start, string, style)))
        }
    }
}

fn get_cytoband_segment_style(cytoband_segment: &CytobandSegment) -> Style {
    match &cytoband_segment.stain {
        Stain::Gneg => Style::default(),
        _ => Style::default().fg(cytoband_segment.stain.get_color()),
    }
}

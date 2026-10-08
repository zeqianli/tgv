use gv_core::{
    cytoband::{Cytoband, CytobandSegment, Stain},
    prelude::*,
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

/// Columns right of the cytoband for the contig length label.
pub(crate) const CYTOBAND_TEXT_RIGHT_SPACING: u16 = 7;
const MIN_AREA_HEIGHT: u16 = 2;
pub fn render_cytobands(
    area: &Rect,
    buf: &mut Buffer,
    state: &State,
    alignment_view: &AlignmentView,
    pallete: &Palette,
) -> Result<(), TGVError> {
    if area.width <= CYTOBAND_TEXT_RIGHT_SPACING + 1 {
        return Ok(());
    }

    if area.height < MIN_AREA_HEIGHT {
        return Ok(());
    }

    let cytoband_left_spacing = 0;

    if cytoband_left_spacing >= area.width.saturating_sub(CYTOBAND_TEXT_RIGHT_SPACING) {
        return Ok(());
    }

    // Right labels

    if let Some(contig_length) = state.contig_length(&alignment_view.focus)? {
        buf.set_string(
            area.x + area.width - CYTOBAND_TEXT_RIGHT_SPACING + 1,
            area.y,
            get_abbreviated_length_string(contig_length),
            Style::default(),
        );
    }

    // Cytoband
    if let Some(cytoband) = state.current_cytoband(&alignment_view.focus)? {
        for (x, string, style) in get_cytoband_xs_strings_and_styles(
            cytoband,
            cytoband_left_spacing,
            area.width - CYTOBAND_TEXT_RIGHT_SPACING,
            pallete,
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
    palette: &Palette,
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
            palette,
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
    total_length: u64,
    area_start: u16,
    area_end: u16,
    second_centromere: bool,
    palette: &Palette,
) -> Result<Option<(u16, String, Style)>, TGVError> {
    let onscreen_x_start = linear_scale(segment.start - 1, total_length, area_start, area_end)?; // 0-based, inclusive
    let onscreen_x_end = linear_scale(segment.end, total_length, area_start, area_end)?; // 0-based, exclusive

    if onscreen_x_end <= onscreen_x_start {
        return Ok(None);
    }

    let style = Style::default().fg(palette.cytoband_color(segment.stain.clone()));

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

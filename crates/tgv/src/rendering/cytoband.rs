use gv_core::{
    cytoband::{CytobandSchema, Stain},
    prelude::*,
};
use polars::prelude::{ChunkAgg, DataFrame};

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
    let bands = state
        .cytobands
        .query(alignment_view.focus.contig_index, 1, u64::MAX)?;
    if bands.height() > 0 {
        for (x, string, style) in get_cytoband_xs_strings_and_styles(
            &bands,
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

/// Lays out one contig's bands, scaled to the end of its last band.
fn get_cytoband_xs_strings_and_styles(
    bands: &DataFrame,
    area_start: u16,
    area_end: u16,
    palette: &Palette,
) -> Result<Vec<(u16, String, Style)>, TGVError> {
    let starts = bands.column(CytobandSchema::START)?.u64()?;
    let ends = bands.column(CytobandSchema::END)?.u64()?;
    let stains = bands.column(CytobandSchema::STAIN)?.str()?;
    let total_length = ends.max().unwrap_or(0);

    let mut second_centromere = false;
    let mut output = Vec::new();
    for ((start, end), stain) in starts
        .into_no_null_iter()
        .zip(ends.into_no_null_iter())
        .zip(stains.iter().map(Option::unwrap_or_default))
    {
        let stain = Stain::try_from(stain)?;
        if let Some((x, string, style)) = get_cytoband_segment_x_string_and_style(
            start,
            end,
            &stain,
            total_length,
            area_start,
            area_end,
            second_centromere,
            palette,
        )? {
            output.push((x, string, style));
        }

        if stain == Stain::Acen {
            second_centromere = true;
        }
    }
    Ok(output)
}

/// Lays out one band. `start` and `end` are 1-based and inclusive.
#[allow(clippy::too_many_arguments)]
fn get_cytoband_segment_x_string_and_style(
    start: u64,
    end: u64,
    stain: &Stain,
    total_length: u64,
    area_start: u16,
    area_end: u16,
    second_centromere: bool,
    palette: &Palette,
) -> Result<Option<(u16, String, Style)>, TGVError> {
    let onscreen_x_start = linear_scale(start - 1, total_length, area_start, area_end)?; // 0-based, inclusive
    let onscreen_x_end = linear_scale(end, total_length, area_start, area_end)?; // 0-based, exclusive

    if onscreen_x_end <= onscreen_x_start {
        return Ok(None);
    }

    let style = Style::default().fg(palette.cytoband_color(stain.clone()));

    match stain {
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

use crate::register::Registers;
use gv_core::{contig_header::Contig, error::TGVError, state::State};
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::Style,
};

use crate::rendering::{colors::Palette, get_abbreviated_length_string};
const MIN_CONTIG_NAME_SPACING: u16 = 10;
const MIN_CONTIG_LENGTH_SPACING: u16 = 10;

pub fn render_contig_list(
    area: &Rect,
    buf: &mut Buffer,
    state: &State,
    registers: &Registers,
    pallete: &Palette,
) -> Result<(), TGVError> {
    if area.height <= 1 {
        return Ok(());
    }
    if area.width <= MIN_CONTIG_NAME_SPACING + MIN_CONTIG_LENGTH_SPACING {
        return Ok(());
    }

    // First line: reference name
    buf.set_string(
        area.x,
        area.y,
        state.reference.to_string(),
        Style::default(),
    );

    // Highlight the selection row
    let selection_row = area.height / 2;
    for x in area.x..area.x + area.width {
        let cell = buf.cell_mut(Position::new(x, area.y + selection_row));
        if let Some(cell) = cell {
            cell.set_char(' ');
            cell.set_bg(pallete.HIGHLIGHT_COLOR);
        }
    }

    // Left label: contig name
    let max_contig_name_length = state
        .contig_header
        .contigs
        .iter()
        .map(|c| c.name.len())
        .max()
        .unwrap_or(0) as u16;

    let contig_name_spacing = u16::max(MIN_CONTIG_NAME_SPACING, max_contig_name_length);

    if area.width <= contig_name_spacing {
        return Ok(());
    }

    // Right label: contig length
    let mut max_contig_length: Option<u64> = None;
    for contig in state.contig_header.contigs.iter() {
        if let Some(length) = contig.length {
            if let Some(max_length) = max_contig_length {
                max_contig_length = Some(max_length.max(length));
            } else {
                max_contig_length = Some(length);
            }
        }
    }

    if area.width <= contig_name_spacing + MIN_CONTIG_LENGTH_SPACING + 1 {
        return Ok(());
    }

    // Middle: contig bars
    let selected_index = registers.contig_list_cursor;

    for (y, contig_index) in get_indexes(
        area.height,
        state.contig_header.contigs.len(),
        selected_index,
    ) {
        render_contig_at_y(
            area,
            buf,
            &state.contig_header.contigs[contig_index],
            contig_name_spacing,
            max_contig_length,
            y,
            pallete,
        )?;
    }

    Ok(())
}

fn get_indexes(height: u16, n_contigs: usize, selected_index: usize) -> Vec<(u16, usize)> {
    if n_contigs == 0 {
        return vec![];
    }
    if selected_index >= n_contigs {
        return vec![];
    }

    let mut output = Vec::new();

    let selection_x = (height / 2) as usize;

    for i in 1..height as usize {
        if selected_index + i >= selection_x && selected_index + i < n_contigs + selection_x {
            output.push((i as u16, selected_index + i - selection_x));
        }
    }

    output
}

fn render_contig_at_y(
    area: &Rect,
    buf: &mut Buffer,
    contig: &Contig,
    left_spacing: u16,
    max_contig_length: Option<u64>,
    y: u16,
    pallete: &Palette,
) -> Result<(), TGVError> {
    let contig_name = contig.name.clone();
    let contig_length = contig.length;

    buf.set_string(area.x, area.y + y, contig_name, Style::default());

    if let Some(contig_length) = contig_length {
        buf.set_string(
            area.x + area.width - MIN_CONTIG_LENGTH_SPACING,
            area.y + y,
            get_abbreviated_length_string(contig_length),
            Style::default(),
        );
    }

    if let (Some(max_contig_length), Some(contig_length)) = (max_contig_length, contig_length) {
        if area.width >= MIN_CONTIG_LENGTH_SPACING + left_spacing + 5 {
            let contig_length_x = usize::max(
                0,
                (contig_length as f64 / (max_contig_length) as f64
                    * (area.width - MIN_CONTIG_LENGTH_SPACING - left_spacing) as f64)
                    as usize,
            );

            buf.set_string(
                area.x + left_spacing,
                area.y + y,
                "▅".repeat(contig_length_x),
                Style::default(),
            );
        }
    }

    Ok(())
}

// FEAT: command mode in listing contig to search reference genome

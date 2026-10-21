use crate::{
    layout::Rect,
    rendering::{
        CHAR_WIDTH, LINE_HEIGHT, Painter,
        colors::{MUTED_TEXT, Palette, TEXT, ToHsla},
    },
};
use gv_core::prelude::*;
use gv_viewer::{prepare::coordinate::get_abbreviated_length_string, register::Registers};

const ROW_HEIGHT: f32 = 20.0;
const BAR_HEIGHT: f32 = 8.0;
const PADDING: f32 = 12.0;

/// Lists the contigs, with the selected one in the middle, as in the terminal UI.
pub fn render_contig_list(
    painter: &mut Painter,
    area: &Rect,
    state: &State,
    registers: &Registers,
    palette: &Palette,
) -> Result<(), TGVError> {
    painter.fill(*area, palette.background.hsla());
    painter.text(
        area.x + PADDING,
        area.y + 4.0,
        &state.reference.to_string(),
        TEXT,
    );
    let contigs = &state.contig_header.contigs;
    if contigs.is_empty() {
        return Ok(());
    }
    let name_width = contigs
        .iter()
        .map(|contig| contig.name.chars().count())
        .max()
        .unwrap_or(0) as f32
        * CHAR_WIDTH
        + PADDING;
    let max_length = contigs.iter().filter_map(|contig| contig.length).max();
    let bar_x = area.x + PADDING + name_width;
    let bar_width = (area.width - name_width - 3.0 * PADDING - 8.0 * CHAR_WIDTH).max(0.0);

    let selected = registers.contig_list_cursor;
    let middle = area.y + (area.height / 2.0 / ROW_HEIGHT).floor() * ROW_HEIGHT;
    painter.fill(
        Rect::new(area.x, middle, area.width, ROW_HEIGHT),
        palette.HIGHLIGHT_COLOR.alpha(0.6),
    );
    let first_row = area.y + ROW_HEIGHT;
    for (index, contig) in contigs.iter().enumerate() {
        let y = middle + (index as f32 - selected as f32) * ROW_HEIGHT;
        if y < first_row || y + ROW_HEIGHT > area.bottom() {
            continue;
        }
        let text_y = y + (ROW_HEIGHT - LINE_HEIGHT) / 2.0;
        painter.text(area.x + PADDING, text_y, &contig.name, TEXT);
        if let (Some(length), Some(max_length)) = (contig.length, max_length) {
            let width = (bar_width * length as f32 / max_length.max(1) as f32).max(1.0);
            painter.fill(
                Rect::new(
                    bar_x,
                    y + (ROW_HEIGHT - BAR_HEIGHT) / 2.0,
                    width,
                    BAR_HEIGHT,
                ),
                palette.GENE_BACKGROUND_COLOR.hsla(),
            );
            painter.text(
                bar_x + width + PADDING,
                text_y,
                &get_abbreviated_length_string(length),
                MUTED_TEXT,
            );
        }
    }
    Ok(())
}

use crate::{
    layout::{AlignmentView, OnScreenCoordinate},
    rendering::{colors::Palette, intervals::render_zoom_in_notice},
};
use gv_core::{
    bed::{BedSchema, BedTable},
    prelude::*,
};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
};

/// Draws each feature as a bar, filled with strand chevrons and labeled with its name when the
/// name fits.
pub fn render_bed(
    area: &Rect,
    buf: &mut Buffer,
    bed: &BedTable,
    alignment_view: &AlignmentView,
    palette: &Palette,
) -> Result<(), TGVError> {
    let region = alignment_view.region(area);
    if !bed.has_complete_data(&region) {
        render_zoom_in_notice(area, buf, "BED features");
        return Ok(());
    }
    let rows = bed.query(region.contig_index(), region.start(), region.end())?;
    let starts = rows.column(BedSchema::START)?.u64()?;
    let ends = rows.column(BedSchema::END)?.u64()?;
    let names = rows.column(BedSchema::NAME)?.str()?;
    let strands = rows.column(BedSchema::STRAND)?.str()?;
    let colors = [palette.BED1, palette.BED2];
    for row in 0..rows.height() {
        let start = alignment_view
            .onscreen_x_coordinate(starts.get(row).expect("BED starts are non-null"), area);
        let end = alignment_view
            .onscreen_x_coordinate(ends.get(row).expect("BED ends are non-null"), area);
        let Some((x, width)) = OnScreenCoordinate::onscreen_start_and_length(&start, &end, area)
        else {
            continue;
        };
        let width = usize::from(width);
        let fill = match strands.get(row) {
            Some("+") => '›',
            Some("-") => '‹',
            _ => ' ',
        };
        let mut text: Vec<char> = vec![fill; width];
        // Names are placed by character count, so only ASCII names are drawn.
        if let Some(name) = names.get(row)
            && name.is_ascii()
            && name.len() + 2 <= width
        {
            let offset = (width - name.len()) / 2;
            text.splice(offset..offset + name.len(), name.chars());
        }
        // Colors alternate by on-screen order, because row IDs of indexed files change between
        // loads.
        buf.set_string(
            area.x + x,
            area.y,
            text.into_iter().collect::<String>(),
            Style::default().bg(colors[row % 2]).fg(Color::White),
        );
    }
    Ok(())
}

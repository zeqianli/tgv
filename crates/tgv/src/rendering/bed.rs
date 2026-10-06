use crate::{
    layout::AlignmentView,
    rendering::{colors::Palette, intervals::render_simple_intervals},
};
use gv_core::{
    bed::{BedSchema, BedTable},
    prelude::*,
};
use ratatui::{buffer::Buffer, layout::Rect};

pub fn render_bed(
    area: &Rect,
    buf: &mut Buffer,
    bed: &BedTable,
    alignment_view: &AlignmentView,
    palette: &Palette,
) -> Result<(), TGVError> {
    let region = alignment_view.region(area);
    let rows = bed.query(region.contig_index(), region.start(), region.end())?;
    if rows.height() > 0 {
        let first_color = rows
            .column(BedSchema::ROW_ID)?
            .u64()?
            .get(0)
            .expect("row IDs are non-null") as usize
            % 2;
        render_simple_intervals(
            area,
            buf,
            &rows,
            alignment_view,
            &[palette.BED1, palette.BED2],
            first_color,
        )?;
    }
    Ok(())
}

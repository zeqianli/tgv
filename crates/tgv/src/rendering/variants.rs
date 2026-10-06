use crate::{
    layout::AlignmentView,
    rendering::{colors::Palette, intervals::render_simple_intervals},
};
use gv_core::{
    prelude::*,
    variant::{VariantSchema, VariantTable},
};
use ratatui::{buffer::Buffer, layout::Rect};

pub fn render_variants(
    area: &Rect,
    buf: &mut Buffer,
    variants: &VariantTable,
    alignment_view: &AlignmentView,
    palette: &Palette,
) -> Result<(), TGVError> {
    let region = alignment_view.region(area);
    let rows = variants.query(region.contig_index(), region.start(), region.end())?;
    if rows.height() > 0 {
        let first_color = rows
            .column(VariantSchema::ROW_ID)?
            .u64()?
            .get(0)
            .expect("row IDs are non-null") as usize
            % 2;
        render_simple_intervals(
            area,
            buf,
            &rows,
            alignment_view,
            &[palette.VCF1, palette.VCF2],
            first_color,
        )?;
    }
    Ok(())
}

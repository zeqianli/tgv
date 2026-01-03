use gv_core::{error::TGVError, state::State};

use crate::{
    layout::AlignmentView,
    rendering::{colors::Palette, intervals::render_simple_intervals},
};
use ratatui::{buffer::Buffer, layout::Rect};

pub fn render_variants(
    area: &Rect,
    buf: &mut Buffer,
    state: &State,
    alignment_view: &AlignmentView,
    pallete: &Palette,
) -> Result<(), TGVError> {
    let variants = state.variants.overlapping(&alignment_view.region(area))?;
    if !variants.is_empty() {
        let first_color_index = variants[0].index % 2;
        render_simple_intervals(
            area,
            buf,
            variants,
            alignment_view,
            vec![pallete.VCF1, pallete.VCF2],
            first_color_index,
        )?;
    }
    Ok(())
}

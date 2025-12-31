use gv_core::{error::TGVError, state::State, variant::VariantRepository};

use crate::rendering::{colors::Palette, intervals::render_simple_intervals};
use ratatui::{buffer::Buffer, layout::Rect};

pub fn render_variants(
    area: &Rect,
    buf: &mut Buffer,
    variants: &VariantRepository,
    state: &State,
    pallete: &Palette,
) -> Result<(), TGVError> {
    let variants = variants.variants.overlapping(&state.viewing_region())?;
    if !variants.is_empty() {
        let first_color_index = variants[0].index % 2;
        render_simple_intervals(
            area,
            buf,
            variants,
            state,
            vec![pallete.VCF1, pallete.VCF2],
            first_color_index,
        )?;
    }
    Ok(())
}

use crate::error::TGVError;

use crate::variant::VariantRepository;
use crate::{
    rendering::{self, intervals::render_simple_intervals},
    states::State,
};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
};

pub fn render_variants(
    area: &Rect,
    buf: &mut Buffer,
    variants: &VariantRepository,
    state: &State,
) -> Result<(), TGVError> {
    let variants = variants.variants.overlapping(&state.viewing_region()?)?;
    if !variants.is_empty() {
        let first_color_index = variants[0].index % 2;
        render_simple_intervals(
            area,
            buf,
            variants,
            state,
            vec![rendering::colors::VCF1, rendering::colors::VCF2],
            first_color_index,
        )?;
    }
    Ok(())
}

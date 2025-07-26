use crate::error::TGVError;

use crate::intervals::GenomeInterval;
use crate::variant::VariantRepository;
use crate::{
    rendering::{self, intervals::render_simple_intervals},
    states::State,
    window::{OnScreenCoordinate, ViewingWindow},
};
use crossterm::style;
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::Color,
    style::Style,
};

pub fn render_variants(
    area: &Rect,
    buf: &mut Buffer,
    variants: &VariantRepository,
    state: &State,
) -> Result<(), TGVError> {
    render_simple_intervals(
        area,
        buf,
        variants.variants.overlapping(&state.viewing_region()?)?,
        state,
        rendering::colors::VCF,
    )
}

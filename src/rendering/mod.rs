mod alignment;
mod bed;
mod colors;
mod console;
mod contig_list;
mod coordinate;
mod coverage;
mod cytoband;
mod help;
mod intervals;
pub mod layout;
mod sequence;
mod status_bar;
mod track;
mod variants;
pub use alignment::render_alignment;
pub use bed::render_bed;
pub use colors::{Palette, DARK_THEME};
pub use console::render_console;
pub use contig_list::render_contig_list;
pub use coordinate::render_coordinates;
pub use coverage::render_coverage;
pub use cytoband::render_cytobands;
pub use help::render_help;
pub use layout::MainLayout;
pub use sequence::render_sequence;
pub use status_bar::render_status_bar;
pub use track::render_track;
pub use variants::render_variants;

use crate::{
    error::TGVError,
    register::DisplayMode,
    register::{RegisterType, Registers},
    rendering::layout::AreaType,
    repository::Repository,
    states::State,
};
use ratatui::{buffer::Buffer, layout::Rect};

#[derive(Debug, Default)]
pub struct Renderer {
    pub last_frame_area: Rect,

    pub needs_refresh: bool,
}

impl Renderer {
    pub fn update(&mut self, state: &State) -> Result<&mut Self, TGVError> {
        if self.last_frame_area.width != state.area().width
            || self.last_frame_area.height != state.area().height
        {
            self.needs_refresh = true;
            self.last_frame_area = *state.area();
        } else {
            self.needs_refresh = false;
        }

        Ok(self)
    }

    pub fn render(
        &self,

        buf: &mut Buffer,
        state: &State,
        registers: &Registers,
        repository: &Repository,
        pallete: &Palette,
    ) -> Result<(), TGVError> {
        match &state.display_mode {
            DisplayMode::Main => {
                // TODO: Get layout tree from state
                // For now, fall back to existing layout
                Self::render_main(buf, state, registers, repository, pallete)?;
            }
            DisplayMode::Help => {
                render_help(state.area(), buf)?;
            }
            DisplayMode::ContigList => {
                render_contig_list(state.area(), buf, state, registers, pallete)?;
            }
        }
        Ok(())
    }

    /// Render all areas in the layout
    pub fn render_main(
        buf: &mut Buffer,
        state: &State,
        registers: &Registers,
        repository: &Repository,
        pallete: &Palette,
    ) -> Result<(), TGVError> {
        // Render each area based on its type
        for (i, (area_type, rect)) in state.layout.areas.iter().enumerate() {
            if rect.y >= buf.area.height || rect.x >= buf.area.width {
                // bound check
                continue;
            }

            match area_type {
                AreaType::Cytoband => render_cytobands(rect, buf, state, pallete)?,
                AreaType::Coordinate => render_coordinates(rect, buf, state)?,
                AreaType::Coverage => {
                    if repository.alignment_repository.is_some() {
                        render_coverage(rect, buf, state, pallete)?;
                    }
                }
                AreaType::Alignment => {
                    if repository.alignment_repository.is_some() {
                        render_alignment(rect, buf, state, pallete)?;
                    }
                }
                AreaType::Sequence => {
                    if repository.sequence_service.is_some() {
                        render_sequence(rect, buf, state, pallete)?;
                    }
                }
                AreaType::GeneTrack => {
                    if repository.track_service.is_some() {
                        render_track(rect, buf, state, pallete)?;
                    }
                }
                AreaType::Console => {
                    if registers.current == RegisterType::Command {
                        render_console(rect, buf, &registers.command)?;
                    }
                }
                AreaType::Error => {
                    render_status_bar(rect, buf, state)?;
                }
                AreaType::Variant => {
                    if let Some(variants) = repository.variant_repository.as_ref() {
                        render_variants(rect, buf, variants, state, pallete)?
                    }
                }
                AreaType::Bed => {
                    if let Some(bed) = repository.bed_intervals.as_ref() {
                        render_bed(rect, buf, bed, state, pallete)?
                    }
                }
            };
        }
        Ok(())
    }
}

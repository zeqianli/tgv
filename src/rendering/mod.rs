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

use crate::error::TGVError;
use crate::register::DisplayMode;
use crate::register::Registers;
use crate::repository::Repository;
use crate::states::State;
use ratatui::{buffer::Buffer, layout::Rect};

#[derive(Debug, Default)]
pub struct RenderingState {
    pub last_frame_area: Rect,

    pub needs_refresh: bool,
}

impl RenderingState {
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
                state
                    .layout
                    .render_all(buf, state, registers, repository, pallete)?;
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
}

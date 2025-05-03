mod alignment;
mod colors;
mod console;
mod coordinate;
mod coverage;
mod cytoband;
mod error;
mod help;
mod sequence;
mod track;
pub use alignment::render_alignment;
pub use console::render_console;
pub use coordinate::render_coordinates;
pub use coverage::render_coverage;
pub use cytoband::render_cytobands;
pub use error::render_error;
pub use help::render_help;
pub use sequence::render_sequence;
pub use track::render_track;

use crate::error::TGVError;
use crate::models::register::{RegisterType, Registers};
use crate::states::State;
use ratatui::{
    buffer::Buffer,
    layout::{
        Constraint::{Fill, Length},
        Layout, Rect,
    },
};

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum RenderingStateEnum {
    Normal, // Display alignments
    Help,   // Display help screen
    Skip,   // Skip rendering
}

pub struct RenderingState {
    state: RenderingStateEnum,

    last_frame_area: Rect,

    refresh: bool,
}

// if area.width < MIN_AREA_WIDTH || area.height < MIN_AREA_HEIGHT {
//     return; // TOO small. Skip rendering to prevent overflow.
// }

const MAX_ZOOM_TO_DISPLAY_ALIGNMENTS: u32 = 2;

impl RenderingState {
    pub fn new() -> Self {
        Self {
            state: RenderingStateEnum::Normal,
            refresh: false,
            last_frame_area: Rect::default(),
        }
    }

    pub fn needs_refresh(&self) -> bool {
        self.refresh
    }

    pub fn update(&mut self, state: &State) -> Result<&mut Self, TGVError> {
        // TODO: help screen
        // let new_state = match state.input_mode.clone() {
        //     InputMode::Command | InputMode::Normal => RenderingStateEnum::Normal,
        //     InputMode::Help => RenderingStateEnum::Help,
        // };

        // self.refresh = match (&self.state, &new_state) {
        //     (RenderingStateEnum::Normal, RenderingStateEnum::Help) => true,
        //     (RenderingStateEnum::Help, RenderingStateEnum::Normal) => true,
        //     _ => false,
        // };

        // check if the frame area has changed
        if self.last_frame_area.width != state.current_frame_area()?.width
            || self.last_frame_area.height != state.current_frame_area()?.height
        {
            self.refresh = true;
        }
        self.last_frame_area = state.current_frame_area()?.clone();

        //self.state = new_state;

        Ok(self)
    }

    pub fn render(
        &self,
        area: Rect,
        buf: &mut Buffer,
        state: &State,
        registers: &Registers,
    ) -> Result<(), TGVError> {
        match &self.state {
            RenderingStateEnum::Normal => {
                let [cytoband_area, coordinate_area, coverage_area, alignment_area, sequence_area, track_area, console_area, error_area] =
                    Layout::vertical([
                        Length(2), // cytobands
                        Length(2), // coordinate
                        Length(6), // coverage
                        Fill(1),   // alignment
                        Length(1), // sequence
                        Length(2), // track
                        Length(2), // console
                        Length(2), // error
                    ])
                    .areas(area);

                // Cytobands
                if state.cytoband_renderable()? {
                    render_cytobands(&cytoband_area, buf, state)?;
                }

                // Coordinates
                render_coordinates(&coordinate_area, buf, state)?;

                // Coverage, Alignments, and Tracks
                if state.alignment_renderable()? {
                    if let Some(alignment) = &state.alignment {
                        render_coverage(&coverage_area, buf, state.viewing_window()?, alignment)?;
                        render_alignment(&alignment_area, buf, state.viewing_window()?, alignment)?;
                    }
                }

                // Sequence
                if state.sequence_renderable()? {
                    render_sequence(&sequence_area, buf, state)?;
                }

                // Tracks
                if state.track_renderable()? {
                    render_track(&track_area, buf, state)?;
                }

                // Console

                if registers.current == RegisterType::Command {
                    render_console(&console_area, buf, &registers.command)?;
                }

                // Error
                render_error(&error_area, buf, &state.errors)?;
            }
            RenderingStateEnum::Help => {
                render_help(area, buf)?;
            }
            RenderingStateEnum::Skip => {
                // Do nothing
            }
        }
        Ok(())
    }
}

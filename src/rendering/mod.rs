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
pub use sequence::{render_sequence, render_sequence_at_2x};
pub use track::render_track;

use crate::models::register::RegisterEnum;
use crate::states::{State, StateHandler};
use crate::{error::TGVError, models::mode::InputMode};
use ratatui::{
    buffer::Buffer,
    layout::{
        Constraint::{Fill, Length},
        Layout, Rect,
    },
    prelude::Backend,
    widgets::Widget,
    Frame, Terminal,
};

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum RenderingStateEnum {
    Normal,
    Help,
    Skip,
}

pub struct RenderingState {
    state: RenderingStateEnum,

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
        }
    }

    pub fn update(&mut self, state: &State) -> Result<&mut Self, TGVError> {
        let new_state = match state.input_mode {
            InputMode::Command | InputMode::Normal => RenderingStateEnum::Normal,
            InputMode::Help => RenderingStateEnum::Help,
        };

        self.refresh = match (&self.state, &new_state) {
            (RenderingStateEnum::Normal, RenderingStateEnum::Help) => true,
            (RenderingStateEnum::Help, RenderingStateEnum::Normal) => true,
            _ => false,
        };

        self.state = new_state;
        Ok(self)
    }

    fn alignment_renderable(&self, state: &State) -> bool {
        state.settings.bam_path.is_some()
            && state.viewing_window()?.zoom() <= StateHandler::MAX_ZOOM_TO_DISPLAY_ALIGNMENTS
    }

    pub fn render(
        &self,
        area: Rect,
        buf: &mut Buffer,
        state: &State,
        register: &RegisterEnum,
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
                if let Ok(Some(cytoband)) = state.current_cytoband() {
                    render_cytobands(
                        &cytoband_area,
                        buf,
                        cytoband,
                        state.viewing_window()?,
                        state.contig_length()?,
                    );
                }

                // Coordinates
                render_coordinates(
                    &coordinate_area,
                    buf,
                    state.viewing_window()?,
                    state.contig_length()?,
                )
                .unwrap();

                // Coverage, Alignments, and Tracks
                if self.alignment_renderable(&state) {
                    if let Some(alignment) = &state.alignment {
                        render_coverage(&coverage_area, buf, state.viewing_window()?, alignment)?;
                        render_alignment(&alignment_area, buf, state.viewing_window()?, alignment)?;
                    }
                }

                if state.settings.reference.is_some() {
                    if state.viewing_window()?.is_basewise() {
                        match &state.sequence {
                            Some(sequence) => {
                                render_sequence(
                                    &sequence_area,
                                    buf,
                                    &state.viewing_region()?,
                                    sequence,
                                )
                                .unwrap();
                            }
                            None => {} // TODO: handle error
                        }
                    } else if state.viewing_window()?.zoom() == 2 {
                        match &state.sequence {
                            Some(sequence) => {
                                render_sequence_at_2x(
                                    &sequence_area,
                                    buf,
                                    &state.viewing_region()?,
                                    sequence,
                                )
                                .unwrap();
                            }
                            None => {} // TODO: handle error
                        }
                    }

                    match &state.track {
                        Some(track) => {
                            render_track(
                                &track_area,
                                buf,
                                state.viewing_window()?,
                                track,
                                state.settings.reference.as_ref(),
                            );
                        }
                        None => {} // TODO: handle error
                    }
                }

                if state.input_mode == InputMode::Command {
                    render_console(&console_area, buf, register)
                }

                render_error(&error_area, buf, &state.errors);
            }
            RenderingStateEnum::Help => {
                render_help(area, buf);
            }
            RenderingStateEnum::Skip => {
                // Do nothing
            }
        }
        Ok(())
    }
}

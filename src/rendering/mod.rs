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
use ratatui::widgets::RenderDirection;
pub use sequence::{render_sequence, render_sequence_at_2x};
pub use track::render_track;

use crate::states::State;
use crate::models::register::RegisterEnum;
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

pub enum RenderingStateEnum {
    Normal,
    Help,
    Skip
}

pub struct RenderingState{
    rendering_state: RenderingStateEnum,

    refresh: bool,

}

// if area.width < MIN_AREA_WIDTH || area.height < MIN_AREA_HEIGHT {
//     return; // TOO small. Skip rendering to prevent overflow.
// }

impl RenderingState{

    pub fn new() -> Self {
        Self {
            rendering_state: RenderingStateEnum::Normal,
            refresh: false
        }
    }

    pub fn update(state: &State) -> Self{
        match state.input_mode {
            // TODO: logic for state refershing, 
            InputMode::Command | InputMode::Normal => RenderingState::Normal,
            InputMode::Help => RenderingState::Help
        }

    }
    pub fn render(&self, area: Rect, buf: &mut Buffer, state: &State, register: &RegisterEnum)  -> Result<(), TGVError> {
        match self {
            RenderingState::Normal => {

                let contig_length = state.contig_length().unwrap();
        let viewing_window = state.viewing_window().unwrap();
        let viewing_region = state.viewing_region().unwrap();
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

        if let Ok(Some(cytoband)) = self.state.current_cytoband() {
            render_cytobands(&cytoband_area, buf, cytoband, viewing_window, contig_length);
        }

        render_coordinates(&coordinate_area, buf, viewing_window, contig_length).unwrap();

        if self.state.settings.bam_path.is_some()
            && viewing_window.zoom() <= State::MAX_ZOOM_TO_DISPLAY_ALIGNMENTS
        {
            match &self.state.data.alignment {
                Some(alignment) => {
                    render_coverage(&coverage_area, buf, viewing_window, alignment).unwrap();

                    render_alignment(&alignment_area, buf, viewing_window, alignment);
                }
                None => {} // TODO: handle error
            }
        }

        if self.state.settings.reference.is_some() {
            if viewing_window.is_basewise() {
                match &self.state.data.sequence {
                    Some(sequence) => {
                        render_sequence(&sequence_area, buf, &viewing_region, sequence).unwrap();
                    }
                    None => {} // TODO: handle error
                }
            } else if viewing_window.zoom() == 2 {
                match &self.state.data.sequence {
                    Some(sequence) => {
                        render_sequence_at_2x(&sequence_area, buf, &viewing_region, sequence)
                            .unwrap();
                    }
                    None => {} // TODO: handle error
                }
            }

            match &self.state.data.track {
                Some(track) => {
                    render_track(
                        &track_area,
                        buf,
                        viewing_window,
                        track,
                        self.state.settings.reference.as_ref(),
                    );
                }
                None => {} // TODO: handle error
            }
        }

        if self.state.input_mode == InputMode::Command {
            render_console(&console_area, buf, self.state.command_mode_register())
        }

        render_error(&error_area, buf, &self.state.errors);


            },
            RenderingState::Help => {
                render_help(area, buf);
            }
        }

    }
}

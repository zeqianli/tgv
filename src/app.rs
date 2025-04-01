/// The main app object
///
use crossterm::event::{self, Event, KeyEventKind};
use ratatui::{
    buffer::Buffer,
    layout::{
        Constraint::{Fill, Length},
        Layout, Rect,
    },
    widgets::Widget,
    DefaultTerminal, Frame,
};

use crate::error::TGVError;
use crate::models::{data::Data, message::StateMessage, mode::InputMode};
use crate::rendering::{
    render_alignment, render_console, render_coordinates, render_coverage, render_cytobands,
    render_error, render_help, render_sequence, render_sequence_at_2x, render_track,
};
use crate::settings::Settings;
use crate::states::State;
pub struct App {
    pub data: Data,
    pub state: State,
}

// initialization
impl App {
    pub async fn new(settings: Settings) -> Result<Self, TGVError> {
        let data = Data::new(&settings).await?;
        let state = State::new(settings).await?;

        Ok(Self { data, state })
    }
}

// event handling
impl App {
    /// Main loop
    pub async fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<(), TGVError> {
        let mut last_frame_mode = InputMode::Normal;

        while !self.state.exit {
            self.state.update_frame_area(terminal.get_frame().area());

            if !self.state.initialized() {
                /// Handle the initial messages
                self.update_state_and_data(self.state.settings.initial_state_messages.clone())
                    .await?;
            }

            terminal
                .draw(|frame| {
                    self.draw(frame);
                })
                .unwrap();
            self.handle_events().await?;

            // terminal.clear() is needed when the layout changes significantly, or the last frame is burned into the new frame.
            // Not sure why.
            let need_screen_refresh = (last_frame_mode == InputMode::Help)
                && (self.state.input_mode != InputMode::Help)
                || (last_frame_mode != InputMode::Help)
                    && (self.state.input_mode == InputMode::Help);

            if need_screen_refresh {
                terminal.clear();
            }

            last_frame_mode = self.state.input_mode.clone();
        }
        Ok(())
    }

    /// Draw the app
    pub fn draw(&self, frame: &mut Frame) {
        if !self.state.initialized() {
            panic!("The initial window is not initialized");
        }
        frame.render_widget(self, frame.area());
    }

    /// Main event handler
    pub async fn handle_events(&mut self) -> Result<(), TGVError> {
        match event::read() {
            Ok(Event::Key(key_event)) if key_event.kind == KeyEventKind::Press => {
                self.update_state_and_data(self.state.translate_key_event(key_event))
                    .await?;
            }
            _ => {}
        };
        Ok(())
    }

    async fn update_state_and_data(
        &mut self,
        state_messages: Vec<StateMessage>,
    ) -> Result<(), TGVError> {
        let data_messages = self.state.handle_messages(state_messages).await?;
        let loaded_data = self.data.handle_data_messages(data_messages).await?;
        if loaded_data {
            self.state.debug_message = "Data loaded".to_string();
        } else {
            self.state.debug_message = "Data not loaded".to_string();
        }
        Ok(())
    }

    /// close connections
    pub async fn close(&mut self) -> Result<(), TGVError> {
        self.data.close().await?;
        self.state.close().await?;
        Ok(())
    }
}

impl Widget for &App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if self.state.input_mode == InputMode::Help {
            render_help(area, buf);
            return;
        }

        let viewing_window = self.state.viewing_window().unwrap();
        let viewing_region = self.state.viewing_region().unwrap();

        let [cytoband_area, coordinate_area, coverage_area, alignment_area, sequence_area, track_area, console_area, error_area] =
            Layout::vertical([
                Length(1), // cytobands
                Length(2), // coordinate
                Length(6), // coverage
                Fill(1),   // alignment
                Length(1), // sequence
                Length(2), // track
                Length(2), // console
                Length(2), // error
            ])
            .areas(area);

        if let (Some(cytobands), Some(current_cytoband_index)) = (
            self.state.cytobands(),
            self.state.current_cytoband_index().unwrap(),
        ) {
            render_cytobands(
                &cytoband_area,
                buf,
                &cytobands[current_cytoband_index],
                &viewing_window,
            );
        }

        render_coordinates(&coordinate_area, buf, viewing_window).unwrap();

        if self.state.settings.bam_path.is_some() {
            match &self.data.alignment {
                Some(alignment) => {
                    render_coverage(&coverage_area, buf, viewing_window, alignment).unwrap();

                    render_alignment(&alignment_area, buf, viewing_window, alignment);
                }
                None => {} // TODO: handle error
            }
        }

        if self.state.settings.reference.is_some() {
            if viewing_window.is_basewise() {
                match &self.data.sequence {
                    Some(sequence) => {
                        render_sequence(&sequence_area, buf, &viewing_region, sequence).unwrap();
                    }
                    None => {} // TODO: handle error
                }
            } else if viewing_window.zoom() == 2 {
                match &self.data.sequence {
                    Some(sequence) => {
                        render_sequence_at_2x(&sequence_area, buf, &viewing_region, sequence)
                            .unwrap();
                    }
                    None => {} // TODO: handle error
                }
            }

            match &self.data.track {
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

        // TODO: a proper debug widget
    }
}

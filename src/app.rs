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

use crate::models::{data::Data, mode::InputMode};
use crate::rendering::{
    render_alignment, render_console, render_coverage, render_help, render_sequence, render_track,
};
use crate::settings::Settings;
use crate::states::State;
use std::io;

pub struct App {
    pub data: Data,
    pub state: State,
}

// initialization
impl App {
    pub async fn new(settings: Settings) -> Result<Self, String> {
        let data = Data::new(&settings).await;
        let state = State::new(settings).await.unwrap();

        Ok(Self { data, state })
    }
}

// event handling
impl App {
    pub async fn handle_events(&mut self) -> io::Result<()> {
        match event::read()? {
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                self.update_state_and_data(self.state.translate_key_event(key_event))
                    .await?;
            }
            _ => {}
        };
        Ok(())
    }

    fn key_pressed(&self) -> bool {
        match event::read()? {
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => true,
            _ => false,
        }
    }

    async fn update_state_and_data(&mut self, state_messages: Vec<StateMessage>) -> io::Result<()> {
        let data_messages = self.state.handle_messages(state_messages).await;
        let loaded_data = self.data.handle_data_messages(data_messages).await.unwrap();
        if loaded_data {
            self.state.debug_message = "Data loaded".to_string();
        } else {
            self.state.debug_message = "Data not loaded".to_string();
        }
    }

    /// Main loop
    pub async fn run(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        let mut last_frame_mode = InputMode::Normal;

        while !self.state.exit {
            self.state.update_frame_area(terminal.get_frame().area());

            if !self.state.initialized() {
                /// Handle the initial messages
                self.update_state_and_data(self.state.initial_state_messages)
                    .await?;
            }

            terminal.draw(|frame| {
                self.draw(frame);
            })?;
            self.handle_events().await?;

            // terminal.clear() is needed when the layout changes significantly, or the last frame is burned into the new frame.
            // Not sure why.
            let need_screen_refresh = (last_frame_mode == InputMode::Help)
                && (self.state.input_mode != InputMode::Help)
                || (last_frame_mode != InputMode::Help)
                    && (self.state.input_mode == InputMode::Help);

            if need_screen_refresh {
                terminal.clear()?;
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
}

impl Widget for &App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if self.state.input_mode == InputMode::Help {
            render_help(area, buf);
            return;
        }

        let [coverage_area, alignment_area, sequence_area, track_area, console_area] =
            Layout::vertical([
                Length(6), // coverage
                Fill(1),   // alignment
                Length(1), // sequence
                Length(2), // track
                Length(2), // console
                           //Length(1), // debug
            ])
            .areas(area);

        match &self.data.alignment {
            Some(alignment) => {
                render_coverage(
                    &coverage_area,
                    buf,
                    &self.state.viewing_window().unwrap(),
                    alignment,
                )
                .unwrap();

                render_alignment(
                    &alignment_area,
                    buf,
                    &self.state.viewing_window().unwrap(),
                    alignment,
                );
            }
            None => {} // TODO: handle error
        }

        if self.state.viewing_window().unwrap().is_basewise() {
            match &self.data.sequence {
                Some(sequence) => {
                    render_sequence(
                        &sequence_area,
                        buf,
                        &self.state.viewing_region().unwrap(),
                        sequence,
                    )
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
                    &self.state.viewing_window().unwrap(),
                    track,
                );
            }
            None => {} // TODO: handle error
        }

        if self.state.input_mode == InputMode::Command {
            render_console(&console_area, buf, &self.state.command_mode_register())
        }

        // TODO: a proper debug widget
    }
}

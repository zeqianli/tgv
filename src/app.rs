/// The main app object
///
use crossterm::event::{self, Event, KeyEventKind};
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

use crate::error::TGVError;
use crate::models::mode::InputMode;
use crate::models::register::RegisterEnum;
use crate::rendering::RenderingState;
use crate::settings::Settings;
use crate::states::{State, StateHandler};
pub struct App {
    pub state: State, // Holds all states and data

    pub state_handler: StateHandler, // Update states accourding from state messages

    pub register: RegisterEnum, // Controls key event translation to StateMessages. Uses the State pattern.

    pub rendering_state: RenderingState,
}

// initialization
impl App {
    pub async fn new(settings: Settings) -> Result<Self, TGVError> {
        Ok(Self {
            state: State::new(&settings)?,
            state_handler: StateHandler::new(&settings).await?,
            register: RegisterEnum::new(InputMode::Normal)?,
            rendering_state: RenderingState::new(),
        })
    }
}

// event handling
impl App {
    /// Main loop
    pub async fn run<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<(), TGVError> {
        let mut last_frame_mode = InputMode::Normal;

        while !self.state.exit {
            let frame_area = terminal.get_frame().area();
            self.state.update_frame_area(frame_area);

            if !self.state.initialized() {
                // Handle the initial messages

                self.state
                    .handle(self.state.settings.initial_state_messages.clone())
                    .await?;
            }

            terminal
                .draw(|frame| {
                    self.draw(frame);
                })
                .unwrap();

            // handle events
            if !self.state.settings.test_mode {
                match event::read() {
                    Ok(Event::Key(key_event)) if key_event.kind == KeyEventKind::Press => {
                        self.state.handle_key_event(key_event).await?;
                    }
                    Ok(Event::Resize(_width, _height)) => {
                        self.state.self_correct_viewing_window();
                    }

                    _ => {}
                };
            }

            // terminal.clear() is needed when the layout changes significantly, or the last frame is burned into the new frame.
            let need_screen_refresh = ((last_frame_mode == InputMode::Help)
                && (self.state.input_mode != InputMode::Help))
                || ((last_frame_mode != InputMode::Help)
                    && (self.state.input_mode == InputMode::Help))
                || frame_area.width != terminal.get_frame().area().width
                || frame_area.height != terminal.get_frame().area().height;

            if need_screen_refresh {
                let _ = terminal.clear();
            }

            last_frame_mode = self.state.input_mode.clone();

            if self.state.settings.test_mode {
                break;
            }
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

    /// close connections
    pub async fn close(&mut self) -> Result<(), TGVError> {
        self.state_handler.close().await?;
        Ok(())
    }
}
const MIN_AREA_WIDTH: u16 = 10;
const MIN_AREA_HEIGHT: u16 = 6;
impl Widget for &App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.rendering_state
            .update(&self.state.input_mode)
            .render(area, buf, &self.state, &self.register)
            .unwrap()
    }
}

/// The main app object
///
use crossterm::event::{self, Event, KeyEventKind};
use ratatui::{buffer::Buffer, layout::Rect, prelude::Backend, widgets::Widget, Frame, Terminal};

use crate::error::TGVError;
use crate::register::{Register, Registers};
use crate::rendering::RenderingState;
use crate::repository::Repository;
use crate::settings::Settings;
use crate::states::{State, StateHandler};
pub struct App {
    pub state: State, // Holds all states and data

    pub settings: Settings,

    pub repository: Repository, // Data CRUD interface

    pub registers: Registers, // Controls key event translation to StateMessages. Uses the State pattern.

    pub rendering_state: RenderingState,
}

// initialization
impl App {
    pub async fn new(settings: Settings) -> Result<Self, TGVError> {
        let mut state = State::new(&settings)?;
        let (repository, sequence_cache) = Repository::new(&settings).await?;
        if let Some(sequence_cache) = sequence_cache {
            // This is needed in local mode.
            // sequence_cache holds the 2bit IO buffers and is mutable.
            // But repository is immutable at runtime. So, the cache needs to be assigned to state.
            state.sequence_cache = sequence_cache;
        }

        Ok(Self {
            state,
            settings: settings.clone(),
            //state_handler: StateHandler::new(&settings).await?,
            repository,
            registers: Registers::new()?,
            rendering_state: RenderingState::new(),
        })
    }
}

// event handling
impl App {
    /// Main loop
    pub async fn run<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<(), TGVError> {
        while !self.state.exit {
            let frame_area = terminal.get_frame().area();
            self.state.update_frame_area(frame_area);

            if !self.state.initialized() {
                StateHandler::initialize(&mut self.state, &self.repository, &self.settings).await?;
            }

            self.registers.update_state(&self.state)?;

            // Prepare rendering
            self.rendering_state.update(&self.state)?;

            if self.rendering_state.needs_refresh() {
                let _ = terminal.clear();
            }

            // Render

            terminal
                .draw(|frame| {
                    self.draw(frame);
                })
                .unwrap();

            if self.settings.test_mode {
                break;
            }

            // handle events
            match event::read() {
                Ok(Event::Key(key_event)) if key_event.kind == KeyEventKind::Press => {
                    let state_messages = self.registers.update_key_event(key_event, &self.state)?;
                    StateHandler::handle(
                        &mut self.state,
                        &self.repository,
                        &self.settings,
                        state_messages,
                    )
                    .await?;
                }

                Ok(Event::Resize(_width, _height)) => {
                    self.state.self_correct_viewing_window();
                }

                _ => {}
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
        self.repository.close().await?;
        Ok(())
    }
}
const MIN_AREA_WIDTH: u16 = 10;
const MIN_AREA_HEIGHT: u16 = 6;
impl Widget for &App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.rendering_state
            .render(area, buf, &self.state, &self.registers)
            .unwrap()
    }
}

/// The main app object
///
use crossterm::event;
use ratatui::{
    buffer::Buffer,
    layout::{Layout, Rect},
    prelude::Backend,
    widgets::Widget,
    Frame, Terminal,
};

use crate::error::TGVError;
use crate::models::register::{Register, RegisterEnum, Registers};
use crate::rendering::RenderingState;
use crate::repository::Repository;
use crate::settings::Settings;
use crate::states::{State, StateHandler};
pub struct App {
    pub state: State, // Holds all states and data

    pub settings: Settings,

    //pub state_handler: StateHandler, // Update states accourding from state messages
    pub repository: Repository, // Data CRUD interface

    pub registers: Registers, // Controls key event translation to StateMessages. Uses the State pattern.

    pub rendering_state: RenderingState,
}

// initialization
impl App {
    pub async fn new(settings: Settings) -> Result<Self, TGVError> {
        Ok(Self {
            state: State::new(&settings)?,
            settings: settings.clone(),
            //state_handler: StateHandler::new(&settings).await?,
            repository: Repository::new(&settings).await?,
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

            // handle events
            if !self.settings.test_mode {
                if let Ok(event) = event::read() {
                    let state_messages = self.registers.get(&self.state)?.update(event)?;
                    StateHandler::handle(
                        &mut self.state,
                        &self.repository,
                        &self.settings,
                        state_messages,
                    )
                    .await?;
                }
            }

            // Prepare rendering
            self.rendering_state.update(&self.state)?;

            if self.rendering_state.needs_refresh() {
                let _ = terminal.clear();
            }

            if self.settings.test_mode {
                break;
            }

            // Render

            terminal
                .draw(|frame| {
                    self.draw(frame);
                })
                .unwrap();
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

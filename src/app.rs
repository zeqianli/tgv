/// The main app object
///
use crossterm::event::{self, Event, KeyEventKind};
use ratatui::{buffer::Buffer, layout::Rect, prelude::Backend, widgets::Widget, Frame, Terminal};

use crate::error::TGVError;
use crate::register::{MouseRegister, Register, Registers};
use crate::rendering::RenderingState;
use crate::repository::Repository;
use crate::settings::Settings;
use crate::states::{State, StateHandler};
pub struct App {
    pub state: State, // Holds all states and data

    pub settings: Settings,

    pub repository: Repository, // Data CRUD interface

    pub registers: Registers, // Controls key event translation to StateMessages. Uses the State pattern.

    pub mouse_register: MouseRegister,

    pub rendering_state: RenderingState,
}

// initialization
impl App {
    pub async fn new<B: Backend>(
        settings: Settings,
        terminal: &mut Terminal<B>,
    ) -> Result<Self, TGVError> {
        // Gather resources before initializing the state.

        let (repository, sequence_cache, track_cache, contig_header) =
            Repository::new(&settings).await?;

        let mut state = State::new(
            &settings,
            terminal.get_frame().area(),
            sequence_cache,
            track_cache,
            contig_header,
        )?;

        // Find the initial window
        StateHandler::handle_initial_messages(
            &mut state,
            &repository,
            &settings,
            settings.initial_state_messages.clone(),
        )
        .await?;

        let mouse_register = MouseRegister::new(&state.layout.root);

        Ok(Self {
            state,
            settings: settings.clone(),
            //state_handler: StateHandler::new(&settings).await?,
            repository,
            registers: Registers::new()?,
            mouse_register,
            rendering_state: RenderingState::new(),
        })
    }
}

impl App {
    /// Main loop
    pub async fn run<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<(), TGVError> {
        while !self.state.exit {
            self.registers.update_state(&self.state)?;

            // Prepare rendering
            self.rendering_state.update(&self.state)?;

            if self.rendering_state.needs_refresh() {
                let _ = terminal.clear();
            }

            // Render

            terminal
                .draw(|frame| {
                    let buffer = frame.buffer_mut();
                    self.state.set_area(buffer.area.clone()).unwrap();
                    self.rendering_state
                        .render(
                            buffer,
                            &self.state,
                            &self.registers,
                            &self.repository,
                            &self.settings.palette,
                        )
                        .unwrap()
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

                Ok(Event::Mouse(mouse_event)) => {
                    let state_messages = self
                        .mouse_register
                        .handle_mouse_event(&self.state, mouse_event)?;

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

    /// close connections
    pub async fn close(&mut self) -> Result<(), TGVError> {
        self.repository.close().await?;
        Ok(())
    }
}

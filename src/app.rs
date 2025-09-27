/// The main app object
///
use crossterm::event::{self, Event, KeyEventKind};
use ratatui::{prelude::Backend, Terminal};

use crate::error::TGVError;
use crate::register::{KeyRegister, MouseRegister, Registers};
use crate::rendering::Renderer;
use crate::repository::Repository;
use crate::settings::Settings;
use crate::states::{State, StateHandler};
pub struct App {
    pub state: State,
    pub settings: Settings,
    pub repository: Repository,
    pub registers: Registers,
    pub renderer: Renderer,
}

impl App {
    pub async fn new<B: Backend>(
        settings: Settings,
        terminal: &mut Terminal<B>,
    ) -> Result<Self, TGVError> {
        // Gather resources before initializing the state.
        let (mut repository, contig_header) = Repository::new(&settings).await?;

        let mut state = State::new(&settings, terminal.get_frame().area(), contig_header)?;

        StateHandler::handle_initial_messages(
            &mut state,
            &mut repository,
            &settings,
            settings.initial_state_messages.clone(),
        )
        .await?;

        let registers = Registers::new(&state)?;

        Ok(Self {
            state,
            settings: settings.clone(),
            repository,
            registers,
            renderer: Renderer::default(),
        })
    }
}

impl App {
    /// Main loop
    pub async fn run<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<(), TGVError> {
        while !self.state.exit {
            // Prepare rendering
            self.registers.update(&self.state)?;
            self.renderer.update(&self.state)?;
            if self.renderer.needs_refresh {
                let _ = terminal.clear();
            }

            // Render
            // FIXME: improve rendering performance. Not all sections need to be re-rendered at every loop.

            terminal
                .draw(|frame| {
                    let buffer = frame.buffer_mut();
                    self.state.set_area(buffer.area).unwrap();
                    self.renderer
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
                    let state_messages = self.registers.handle_key_event(key_event, &self.state)?;
                    StateHandler::handle(
                        &mut self.state,
                        &mut self.repository,
                        &mut self.registers,
                        &self.settings,
                        state_messages,
                    )
                    .await?;
                }

                Ok(Event::Mouse(mouse_event)) => {
                    let state_messages = self.registers.mouse_register.handle_mouse_event(
                        &self.state,
                        &self.repository,
                        mouse_event,
                    )?;

                    StateHandler::handle(
                        &mut self.state,
                        &mut self.repository,
                        &mut self.registers,
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
        self.repository.close().await
    }
}

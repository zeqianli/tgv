/// The main app object
///
use crossterm::event::{self, Event, KeyEventKind};
use ratatui::{Terminal, prelude::Backend};

use crate::message::Message;
use crate::register::{KeyRegisterType, Registers};
use crate::rendering::layout::MainLayout;
use crate::settings::Settings;
use gv_core::error::TGVError;
use gv_core::intervals::{Focus, GenomeInterval, Region};
use gv_core::register::{KeyRegister, MouseRegister, Registers};
use gv_core::rendering::Renderer;
use gv_core::repository::Repository;
use gv_core::settings::Settings;
use gv_core::state::State;

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Scene {
    Main,
    Help,
    ContigList,
}

pub struct App {
    pub exit: bool,

    pub layout: MainLayout,
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

        let state = State::new(settings.reference.clone(), contig_header)?;
        let focus = state.default_focus(repository).await?;

        // TODO: go to foucs?

        Ok(Self {
            exit: false,
            layout: MainLayout::new(&Settings, terminal.area(), focus),
            state,
            settings: settings.clone(),
            repository,
            registers: Registers::default(),
            renderer: Renderer::default(),
        })
    }
}

impl App {
    /// Main loop
    pub async fn run<B: Backend>(mut self, terminal: &mut Terminal<B>) -> Result<Self, TGVError> {
        while !self.state.exit {
            // Prepare rendering
            //self.registers.update(&self.state)?;
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
                    Self::handle(
                        &mut self.state,
                        &mut self.repository,
                        &mut self.registers,
                        &self.focus,
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

                    Self::handle(
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
        Ok(self)
    }

    /// close connections
    pub async fn close(mut self) -> Result<(), TGVError> {
        self.repository.close().await
    }
    /// Handle initial messages.
    /// This has different error handling strategy (loud) vs handle(...), which suppresses errors.
    pub async fn handle_initial_messages(
        state: &mut State,
        repository: &mut Repository,
        registers: &mut Registers,
        settings: &Settings,
        messages: Vec<Message>,
    ) -> Result<(), TGVError> {
        let mut data_messages = Vec::new();

        for message in messages {
            data_messages.extend(
                StateHandler::handle_state_message(state, repository, registers, settings, message)
                    .await?,
            );
        }

        let mut loaded_data = false;
        for data_message in data_messages {
            loaded_data = Self::handle_data_message(state, repository, data_message).await?;
        }

        Ok(())
    }

    /// Handle messages after initialization. This blocks any error messages instead of propagating them.
    pub async fn handle(
        state: &mut State,
        repository: &mut Repository,
        registers: &mut Registers,
        focus: &Focus,
        settings: &Settings,
        messages: Vec<Message>,
    ) -> Result<(), TGVError> {
        state.messages.clear();

        let mut data_messages: Vec<DataMessage> = Vec::new();

        for message in messages {
            match message {
                Message::Core(message) => {
                    // TODO
                }
                Message::SwitchScene(scene) => {
                    // TODO
                }
                Message::SwitchKeyRegister(register) => {
                    if register == KeyRegisterType::ContigList {
                        registers.contig_list_cursor = focus.contig_index
                    }
                    registers.current = register
                }
                Message::ClearAllKeyRegisters => registers.clear(),
                Message::Quit => self.exit = true,
            }
            match StateHandler::handle_state_message(
                state, repository, registers, settings, message,
            )
            .await
            {
                Ok(messages) => data_messages.extend(messages),
                Err(e) => {
                    state.add_message(e.to_string());
                    Ok(())
                }
            }
        }

        let data_messages = StateHandler::get_data_requirements(state, repository)?;

        for data_message in data_messages {
            match Self::handle_data_message(state, repository, data_message).await {
                Ok(_) => {}
                Err(e) => return StateHandler::add_message(state, e.to_string()),
            }
        }

        Ok(())
    }

    fn get_data_requirements(
        state: &State,
        repository: &mut Repository, // settings: &Settings,
    ) -> Result<Vec<DataMessage>, TGVError> {
        let mut data_messages = Vec::new();

        // It's important to load sequence first!
        // Alignment IO requires calculating mismatches with the reference sequence.

        if repository.sequence_service.is_some()
            && state.window.sequence_renderable()
            && !state.sequence.has_complete_data(&viewing_region)
        {
            let sequence_cache_region = Self::sequence_cache_region(state, &viewing_region)?;
            data_messages.push(DataMessage::RequiresCompleteSequences(
                sequence_cache_region,
            ));
        }
        if repository.alignment_repository.is_some()
            && state.window.alignment_renderable()
            && !state.alignment.has_complete_data(&viewing_region)
        {
            let alignment_cache_region = Self::alignment_cache_region(state, &viewing_region)?;
            data_messages.push(DataMessage::RequiresCompleteAlignments(
                alignment_cache_region,
            ));
        }

        if repository.track_service.is_some() {
            if !state.track.has_complete_data(&viewing_region) {
                // viewing_window.zoom <= Self::MAX_ZOOM_TO_DISPLAY_FEATURES is always true
                let track_cache_region = Self::track_cache_region(state, &viewing_region)?;
                data_messages.push(DataMessage::RequiresCompleteFeatures(track_cache_region));
            }

            // Cytobands
            data_messages.push(DataMessage::RequiresCytobands(state.contig_index()));
        }

        Ok(data_messages)
    }
}

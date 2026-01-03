/// The main app object
///
use crossterm::event::{self, Event, KeyEventKind};
use ratatui::{Terminal, buffer::Buffer, layout::Rect, prelude::Backend};

use crate::{
    layout::{AlignmentView, MainLayout},
    message::Message,
    mouse::MouseRegister,
    register::{KeyRegisterType, Registers},
    settings::Settings,
};
use gv_core::{
    error::TGVError,
    intervals::{Focus, GenomeInterval, Region},
    repository::Repository,
    state::State,
};

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

    pub alignment_view: AlignmentView,

    pub scene: Scene,
}

impl App {
    pub async fn new<B: Backend>(
        settings: Settings,
        terminal: &mut Terminal<B>,
    ) -> Result<Self, TGVError> {
        // Gather resources before initializing the state.
        let (mut repository, contig_header) = Repository::new(&settings.core).await?;

        let state = State::new(settings.core.reference.clone(), contig_header)?;
        let focus = state.default_focus(&mut repository).await?;

        // TODO: go to foucs?
        // TODO: handle initial message with stricter error handling

        Ok(Self {
            exit: false,
            layout: MainLayout::new(&settings),
            alignment_view: AlignmentView::new(focus),
            state,
            settings: settings.clone(),
            repository,
            registers: Registers::default(),
            scene: Scene::Main,
        })
    }
}

impl App {
    /// Main loop
    pub async fn run<B: Backend>(mut self, terminal: &mut Terminal<B>) -> Result<Self, TGVError> {
        while !self.exit {
            // Render
            // FIXME: improve rendering performance. Not all sections need to be re-rendered at every loop.
            //
            let mut refresh_terminal = false;

            terminal
                .draw(|frame| {
                    let buffer = frame.buffer_mut();
                    refresh_terminal = self.layout.set_area(buffer.area);
                    self.render(buffer).unwrap()
                })
                .unwrap();

            if self.settings.test_mode {
                break;
            }

            // handle events
            match event::read() {
                Ok(Event::Key(key_event)) if key_event.kind == KeyEventKind::Press => {
                    let state_messages = self.registers.handle_key_event(key_event, &self.state)?;
                    self.handle(state_messages).await?; // TODO: distinguish th
                }

                Ok(Event::Mouse(mouse_event)) => {
                    let state_messages = self.mouse_register.handle_mouse_event(
                        &self.state,
                        &self.repository,
                        mouse_event,
                    )?;

                    self.handle(state_messages).await?;
                }

                Ok(Event::Resize(_width, _height)) => {
                    self.alignment_view.self_correct(
                        &self.layout.main_area,
                        self.state.contig_length(&self.alignment_view.focus)?,
                    );
                }

                _ => {}
            }

            // Clear terminal for the next loop if needed
            if refresh_terminal {
                terminal.clear();
            }
        }
        Ok(self)
    }

    /// close connections
    pub async fn close(mut self) -> Result<(), TGVError> {
        self.repository.close().await
    }

    /// Handle messages after initialization. This blocks any error messages instead of propagating them.
    pub async fn handle(&mut self, messages: Vec<Message>) -> Result<(), TGVError> {
        self.state.messages.clear();

        for message in messages {
            match message {
                Message::Core(gv_core::message::Message::Move(movement)) => {
                    let focus = self
                        .state
                        .movement(
                            self.alignment_view.focus.clone(),
                            &mut self.repository,
                            movement,
                        )
                        .await?;

                    self.alignment_view.focus = focus;
                }

                Message::Core(gv_core::message::Message::Quit) => self.exit = true,

                Message::Core(gv_core::message::Message::Scroll(scroll)) => {
                    self.alignment_view.scroll(scroll, &self.state.alignment);
                }

                Message::Core(gv_core::message::Message::Zoom(zoom)) => {
                    let contig_length = self.state.contig_length(&self.alignment_view.focus)?;
                    self.alignment_view
                        .zoom(zoom, &self.layout.main_area, contig_length); // TODO
                }

                Message::Core(gv_core::message::Message::SetAlignmentOption(options)) => {
                    self.state
                        .set_alignment_change(&self.alignment_view.focus, options)?;
                }

                Message::Core(gv_core::message::Message::Message(message)) => {
                    self.state.add_message(message);
                }

                Message::SwitchScene(scene) => {
                    // TODO
                }
                Message::SwitchKeyRegister(register) => {
                    if register == KeyRegisterType::ContigList {
                        self.registers.contig_list_cursor = self.alignment_view.focus.contig_index
                    }
                    self.registers.current = register
                }
                Message::ClearAllKeyRegisters => self.registers.clear(),
            }
        }

        self.load_data().await
    }

    async fn load_data(&mut self) -> Result<(), TGVError> {
        // TODO: return whether data were loaded?
        // It's important to load sequence first!
        // Alignment IO requires calculating mismatches with the reference sequence.
        //
        let region = self.alignment_view.region(&self.layout.main_area);

        if let Some(sequence_service) = self.repository.sequence_service.as_mut()
            && self.alignment_view.zoom <= Self::MAX_ZOOM_TO_DISPLAY_SEQUENCES
            && !self.state.sequence.has_complete_data(&region)
        {
            self.state
                .load_sequence_data(
                    &self.state.sequence_cache_region(region.clone()),
                    sequence_service,
                )
                .await?;
        }

        if let Some(alignment_repository) = self.repository.alignment_repository.as_mut()
            && self.alignment_view.zoom <= Self::MAX_ZOOM_TO_DISPLAY_ALIGNMENTS
            && !self.state.alignment.has_complete_data(&region)
        {
            self.state
                .load_alignment_data(
                    &self.state.alignment_cache_region(region.clone()),
                    alignment_repository,
                )
                .await?;
        }

        if let Some(track_service) = self.repository.track_service.as_mut()
            && !self.state.track.has_complete_data(&region)
        {
            // viewing_window.zoom <= Self::MAX_ZOOM_TO_DISPLAY_FEATURES is always true

            self.state
                .load_track_data(
                    &self.state.track_cache_region(region.clone()),
                    track_service,
                )
                .await?;
        }

        if let Some(variant_repository) = self.repository.variant_repository.as_mut()
            && !self.state.variant_loaded
        {
            self.state
                .load_variant_data(&region, variant_repository)
                .await?;
        }

        if let Some(bed_repository) = self.repository.bed_repository.as_mut()
            && !self.state.bed_loaded
        {
            self.state.load_bed_data(&region, bed_repository).await?;
        }

        // Cytobands
        // TODO
        //
        Ok(())
    }

    pub const MAX_ZOOM_TO_DISPLAY_ALIGNMENTS: u64 = 32;
    pub const MAX_ZOOM_TO_DISPLAY_SEQUENCES: u64 = 2;

    pub fn render(&self, buf: &mut Buffer) -> Result<(), TGVError> {
        use crate::rendering::{render_contig_list, render_help, render_main};
        match &self.scene {
            Scene::Main => render_main(buf, &self.state, &self.registers, &self.settings.palette),
            Scene::Help => render_help(&self.layout.main_area, buf),
            Scene::ContigList => render_contig_list(
                &self.layout.main_area,
                buf,
                &self.state,
                &self.registers,
                &self.settings.palette,
            ),
        }
    }
}

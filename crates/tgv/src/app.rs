/// The main app object
///
use crossterm::event::{self, Event, KeyEventKind};
use ratatui::{Terminal, buffer::Buffer, prelude::Backend};

use crate::{
    layout::{AlignmentView, MainLayout},
    message::Message,
    mouse::MouseRegister,
    register::{KeyRegisterType, Registers},
    session::SessionFile,
    settings::Settings,
};
use gv_core::{error::TGVError, repository::Repository, settings::FilePath, state::State};
use std::{path::PathBuf, time::Instant};

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Scene {
    Main,
    Help,
    ContigList,
}

pub struct App {
    pub exit: bool,
    pub session_path: PathBuf,

    pub layout: MainLayout,
    pub state: State,
    pub settings: Settings,
    pub repository: Repository,
    pub registers: Registers,
    pub mouse_register: MouseRegister,

    pub alignment_view: AlignmentView,

    pub scene: Scene,
}

impl App {
    pub async fn new(settings: Settings, session_path: PathBuf) -> Result<Self, TGVError> {
        let app_init_started = Instant::now();

        // Gather resources before initializing the state.
        log::info!(
            "Initializing the app with session {}",
            session_path.display()
        );

        let (mut repository, contig_header, repository_file_indexes) =
            Repository::new(&settings.core).await?;

        let mut state = State::new(settings.core.reference.clone(), contig_header)?;

        // Initiate empty track data
        settings.core.file_paths.iter().for_each(|path| match path {
            FilePath::AlignmentPath(_) => state.add_alignment_track(),
            FilePath::VariantPath(_) => state.add_variant_track(),
            FilePath::BedPath(_) => state.add_bed_track(),
        });

        let focus = state.default_focus(&mut repository).await?;

        let mut alignment_view = AlignmentView::new(focus, state.alignments.len());
        if let Some(zoom) = settings.zoom {
            alignment_view.zoom = zoom;
        }
        log::info!(
            "App state initialized: reference={} contigs={} alignment_tracks={} variant_tracks={} bed_tracks={} default_focus={:?} initial_zoom={} elapsed_ms={}",
            settings.core.reference,
            state.contig_header.contigs.len(),
            state.alignments.len(),
            state.variants.len(),
            state.bed_intervals.len(),
            alignment_view.focus,
            alignment_view.zoom,
            app_init_started.elapsed().as_millis(),
        );

        Ok(Self {
            exit: false,
            session_path,
            layout: MainLayout::new(&settings, &repository_file_indexes),
            alignment_view,
            state,
            settings: settings.clone(),
            repository,
            registers: Registers::default(),
            mouse_register: MouseRegister::default(),
            scene: Scene::Main,
        })
    }
}

impl App {
    /// Main loop
    pub async fn run<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<(), TGVError> {
        log::info!("Starting the app event loop");
        terminal
            .draw(|frame| {
                let _ = self.layout.set_area(frame.area());
            })
            .map_err(|e| TGVError::IOError(format!("Failed to draw the terminal: {e}")))?;

        self.handle(self.settings.initial_state_messages.clone())
            .await?;

        self.alignment_view.self_correct(
            &self.layout.main_area,
            self.state.contig_length(&self.alignment_view.focus)?,
        );

        while !self.exit {
            // Render
            // FIXME: improve rendering performance. Not all sections need to be re-rendered at every loop.
            //
            let mut refresh_terminal = false;
            let mut render_result = Ok(());

            terminal
                .draw(|frame| {
                    let buffer = frame.buffer_mut();
                    refresh_terminal = self.layout.set_area(buffer.area);
                    render_result = self.render(buffer);
                })
                .map_err(|e| TGVError::IOError(format!("Failed to draw the terminal: {e}")))?;
            render_result?;

            if self.settings.test_mode {
                break;
            }

            // handle events
            match {
                match event::read() {
                    Ok(Event::Key(key_event)) if key_event.kind == KeyEventKind::Press => {
                        let state_messages =
                            self.registers.handle_key_event(key_event, &self.state)?;
                        self.handle(state_messages).await // TODO: this should not error out?
                    }

                    Ok(Event::Mouse(mouse_event)) => {
                        let state_messages = self.mouse_register.handle_mouse_event(
                            &self.state,
                            &mut self.layout,
                            &self.alignment_view,
                            mouse_event,
                        )?;

                        self.handle(state_messages).await // TODO: this should not error out?
                    }

                    Ok(Event::Resize(_width, _height)) => {
                        log::debug!("Terminal resized to {_width}x{_height}");
                        self.alignment_view.self_correct(
                            &self.layout.main_area,
                            self.state.contig_length(&self.alignment_view.focus)?,
                        );
                        Ok(())
                    }

                    _ => Ok(()),
                }
            } {
                Ok(_) => {}
                Err(e) => {
                    log::warn!("Error while handling event: {e}");
                    self.state.add_message(format!("{e}"));
                }
            }

            self.alignment_view.self_correct(
                &self.layout.main_area,
                self.state.contig_length(&self.alignment_view.focus)?,
            );

            // Clear terminal for the next loop if needed
            if refresh_terminal {
                terminal.clear()?;
            }
        }
        log::info!("The app event loop exited");
        Ok(())
    }

    /// close connections
    pub async fn close(mut self) -> Result<(), TGVError> {
        self.repository.close().await
    }

    fn save_session_to_path(&mut self, path: PathBuf) -> Result<(), TGVError> {
        SessionFile::try_from(&*self).and_then(|s| s.write_to_path(&path))?;
        self.session_path = path;
        Ok(())
    }

    /// Handle messages after initialization. This blocks any error messages instead of propagating them.
    pub async fn handle(&mut self, messages: Vec<Message>) -> Result<(), TGVError> {
        self.state.messages.clear();

        for message in messages {
            match message {
                Message::Core(gv_core::message::Message::Move(movement)) => {
                    let previous_focus = self.alignment_view.focus.clone();
                    log::debug!(
                        "Handling movement: movement={:?} previous_focus={:?} zoom={}",
                        movement,
                        previous_focus,
                        self.alignment_view.zoom,
                    );
                    let focus = self
                        .state
                        .movement(
                            self.alignment_view.focus.clone(),
                            self.alignment_view.zoom,
                            &mut self.repository,
                            movement.clone(),
                        )
                        .await?;

                    log::debug!(
                        "Movement applied: movement={:?} previous_focus={:?} new_focus={:?}",
                        movement,
                        previous_focus,
                        focus,
                    );
                    self.alignment_view.focus = focus;
                    self.load_data().await?
                }

                Message::Core(gv_core::message::Message::Quit) => {
                    log::info!("Quit requested");
                    self.exit = true;
                }

                Message::Core(gv_core::message::Message::SaveSession(path)) => {
                    let explicit_path = path.is_some();
                    let path = path
                        .as_deref()
                        .map(SessionFile::resolve_path)
                        .unwrap_or_else(|| self.session_path.clone());
                    log::info!(
                        "Saving session: path={} explicit={}",
                        path.display(),
                        explicit_path,
                    );
                    match self.save_session_to_path(path.clone()) {
                        Ok(()) => {
                            log::info!("Session saved: path={}", path.display());
                            self.state
                                .add_message(format!("Session saved to {}", path.display()));
                        }
                        Err(e) => {
                            log::warn!("Failed to save session: path={} error={e}", path.display());
                            self.state
                                .add_message(format!("Failed to save session: {e}"));
                        }
                    }
                }

                Message::Core(gv_core::message::Message::SaveAndQuit(path)) => {
                    let explicit_path = path.is_some();
                    let path = path
                        .as_deref()
                        .map(SessionFile::resolve_path)
                        .unwrap_or_else(|| self.session_path.clone());
                    log::info!(
                        "Saving session before quit: path={} explicit={}",
                        path.display(),
                        explicit_path,
                    );
                    match self.save_session_to_path(path) {
                        Ok(()) => {
                            log::info!("Session saved before quit");
                            self.exit = true;
                        }
                        Err(e) => {
                            log::warn!("Failed to save session before quit: {e}");
                            self.state
                                .add_message(format!("Failed to save session: {e}"));
                        }
                    }
                }

                Message::Core(gv_core::message::Message::Scroll(scroll)) => {
                    let previous_y = self.alignment_view.y.clone();
                    log::debug!(
                        "Handling scroll: scroll={:?} y_before={:?}",
                        scroll,
                        previous_y
                    );
                    self.alignment_view
                        .scroll(scroll.clone(), &self.state.alignments);
                    log::debug!(
                        "Scroll applied: scroll={:?} y_before={:?} y_after={:?}",
                        scroll,
                        previous_y,
                        self.alignment_view.y,
                    );
                }

                Message::Core(gv_core::message::Message::Zoom(zoom)) => {
                    let contig_length = self.state.contig_length(&self.alignment_view.focus)?;
                    let previous_zoom = self.alignment_view.zoom;
                    log::debug!(
                        "Handling zoom: zoom={:?} previous_zoom={} focus={:?}",
                        zoom,
                        previous_zoom,
                        self.alignment_view.focus,
                    );
                    self.alignment_view.zoom(
                        zoom.clone(),
                        &self.layout.main_area,
                        contig_length,
                    )?; // TODO
                    log::debug!(
                        "Zoom applied: zoom={:?} previous_zoom={} new_zoom={} focus={:?}",
                        zoom,
                        previous_zoom,
                        self.alignment_view.zoom,
                        self.alignment_view.focus,
                    );
                    self.load_data().await?
                }

                Message::Core(gv_core::message::Message::SetAlignmentOption(options)) => {
                    log::debug!(
                        "Setting alignment options: alignment_count={} options={:?}",
                        self.state.alignments.len(),
                        options,
                    );
                    // TODO: introduce focus. Only apply option to the alignment in focus
                    for index in 0..self.state.alignments.len() {
                        self.state.set_alignment_options(
                            index,
                            &self.alignment_view.focus,
                            options.clone(),
                        )?;
                    }
                }

                Message::Core(gv_core::message::Message::Message(message)) => {
                    log::trace!("Adding transient status message: bytes={}", message.len());
                    self.state.add_message(message);
                }

                Message::SwitchScene(scene) => {
                    let previous_scene = self.scene.clone();
                    log::debug!("Switching scene: from={:?} to={:?}", previous_scene, scene);
                    self.scene = scene;
                }
                Message::SwitchKeyRegister(register) => {
                    let previous_register = self.registers.current.clone();
                    if register == KeyRegisterType::ContigList {
                        self.registers.contig_list_cursor = self.alignment_view.focus.contig_index
                    }
                    self.registers.current = register;
                    log::debug!(
                        "Switching key register: from={:?} to={:?}",
                        previous_register,
                        self.registers.current,
                    );
                }
                Message::ClearAllKeyRegisters => {
                    log::debug!("Clearing all key registers");
                    self.registers.clear();
                }
            }
        }

        Ok(())
    }

    async fn load_data(&mut self) -> Result<(), TGVError> {
        // TODO: return whether data were loaded?
        // It's important to load sequence first!
        // Alignment IO requires calculating mismatches with the reference sequence.
        //
        let region = self.alignment_view.region(&self.layout.main_area);
        log::debug!(
            "Evaluating data loads: display_region={:?} zoom={} focus={:?}",
            region,
            self.alignment_view.zoom,
            self.alignment_view.focus,
        );

        if let Some(sequence_service) = self.repository.sequence_service.as_mut()
            && self.alignment_view.zoom <= AlignmentView::MAX_ZOOM_TO_DISPLAY_SEQUENCES
            && !self.state.sequence.has_complete_data(&region)
        {
            let cache_region = self.alignment_view.sequence_cache_region(region.clone());
            log::trace!(
                "Sequence cache miss; requesting data load: display_region={:?} cache_region={:?} zoom={}",
                region,
                cache_region,
                self.alignment_view.zoom,
            );
            self.state
                .load_sequence_data(&cache_region, sequence_service)
                .await?;
        }

        if self.alignment_view.zoom <= AlignmentView::MAX_ZOOM_TO_DISPLAY_ALIGNMENTS {
            for (index, alignment_repository) in self
                .repository
                .alignment_repositories
                .iter_mut()
                .enumerate()
            {
                if !self.state.alignments[index].has_complete_data(&region) {
                    let cache_region = self.alignment_view.alignment_cache_region(region.clone());
                    log::trace!(
                        "Alignment cache miss; requesting data load: track={} display_region={:?} cache_region={:?} zoom={}",
                        index,
                        region,
                        cache_region,
                        self.alignment_view.zoom,
                    );
                    self.state
                        .load_alignment_data(index, &cache_region, alignment_repository)
                        .await?;
                } else {
                    log::trace!(
                        "Skipping alignment data load because cached data is complete: track={} display_region={:?}",
                        index,
                        region,
                    );
                }
            }
        } else {
            log::trace!(
                "Skipping alignment data loads because zoom={} exceeds max_zoom={}",
                self.alignment_view.zoom,
                AlignmentView::MAX_ZOOM_TO_DISPLAY_ALIGNMENTS,
            );
        }

        if let Some(track_service) = self.repository.track_service.as_mut()
            && !self.state.track.has_complete_data(&region)
        {
            // viewing_window.zoom <= Self::MAX_ZOOM_TO_DISPLAY_FEATURES is always true
            let cache_region = self.alignment_view.track_cache_region(region.clone());
            log::trace!(
                "Reference track cache miss; requesting data load: display_region={:?} cache_region={:?}",
                region,
                cache_region,
            );
            self.state
                .load_track_data(&cache_region, track_service)
                .await?;
        }

        for (index, variant_repository) in
            self.repository.variant_repositories.iter_mut().enumerate()
        {
            if !self
                .state
                .variant_loaded
                .get(index)
                .copied()
                .unwrap_or(false)
            {
                log::trace!(
                    "Variant data not loaded; requesting data load: track={} display_region={:?}",
                    index,
                    region,
                );
                self.state
                    .load_variant_data(index, &region, variant_repository)
                    .await?;
            }
        }

        for (index, bed_repository) in self.repository.bed_repositories.iter_mut().enumerate() {
            if !self.state.bed_loaded.get(index).copied().unwrap_or(false) {
                log::trace!(
                    "BED data not loaded; requesting data load: track={} display_region={:?}",
                    index,
                    region,
                );
                self.state
                    .load_bed_data(index, &region, bed_repository)
                    .await?;
            }
        }

        // Cytobands
        // TODO
        //
        log::debug!(
            "Finished evaluating data loads: display_region={:?}",
            region
        );
        Ok(())
    }

    pub fn render(&mut self, buf: &mut Buffer) -> Result<(), TGVError> {
        use crate::rendering::{render_contig_list, render_help, render_main};
        match &self.scene {
            Scene::Main => render_main(
                buf,
                &mut self.state,
                &self.registers,
                &self.layout,
                &self.alignment_view,
                &self.mouse_register,
                &self.settings.palette,
            ),
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

/// The main app object
///
use crossterm::event::{self, Event, KeyEventKind};
use ratatui::{Terminal, buffer::Buffer, layout::Rect, prelude::Backend};

use crate::{
    layout::{
        AlignmentView,
        AreaType::{self, Console},
        MainLayout, ResolvedMainLayout,
    },
    message::{Message, UpdateLayoutMessage},
    mouse::MouseRegister,
    register::{KeyRegisterType, Registers},
    session::SessionFile,
    settings::Settings,
    track_registry::TrackRegistry,
};
use gv_core::prelude::*;
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

const NO_ACTIVE_SESSION_MESSAGE: &str = "No active session. Use :w NAME or :w PATH to save.";

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Scene {
    Main,
    Help,
    ContigList,
}

pub struct App {
    pub exit: bool,

    pub layout: MainLayout,
    pub resolved_layout: ResolvedMainLayout,
    pub tracks: Arc<TrackRegistry>,
    pub state: State,
    pub settings: Settings,
    pub repository: Repository,
    pub registers: Registers,
    pub mouse_register: MouseRegister,

    pub alignment_view: AlignmentView,

    pub scene: Scene,
    render_buffer: Buffer,
}

/// After event handling, which areas needs re-rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderEvent {
    Area(AreaType),
    Sidebar,

    /// All track areas.
    AllTracks,

    /// All track areas and the sidebar
    All,
}

impl App {
    pub async fn new(settings: Settings) -> Result<Self, TGVError> {
        let app_init_started = Instant::now();

        // Gather resources before initializing the state.
        log::info!(
            "Initializing the app with session {:?}",
            settings.session_path
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

        let tracks = Arc::new(TrackRegistry::new(&repository_file_indexes));
        let track_ids = tracks
            .entries
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>();
        let layout = MainLayout::new(&settings, Arc::clone(&tracks), &track_ids);
        Ok(Self {
            exit: false,
            layout,
            resolved_layout: ResolvedMainLayout::default(),
            tracks,
            alignment_view,
            state,
            settings: settings.clone(),
            repository,
            registers: Registers::default(),
            mouse_register: MouseRegister::default(),
            scene: Scene::Main,
            render_buffer: Buffer::empty(Rect::default()),
        })
    }
}

impl App {
    /// Main loop
    pub async fn run<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<(), TGVError> {
        log::info!("Starting the app event loop");
        terminal
            .draw(|frame| {
                self.resolved_layout = self.layout.resolve(frame.area(), &self.repository);
            })
            .map_err(|e| TGVError::IOError(format!("Failed to draw the terminal: {e}")))?;

        self.handle(self.settings.initial_state_messages.clone())
            .await?;

        self.alignment_view.self_correct(
            &self.resolved_layout.main_area,
            self.state.contig_length(&self.alignment_view.focus)?,
        );

        let mut render_events: Vec<RenderEvent> = vec![RenderEvent::All];

        while !self.exit {
            let mut render_result = Ok(());

            if !render_events.is_empty() {
                terminal
                    .draw(|frame| {
                        let buffer = frame.buffer_mut();
                        render_result = self.render(buffer, &render_events);
                    })
                    .map_err(|e| TGVError::IOError(format!("Failed to draw the terminal: {e}")))?;
                render_result?;
            }
            render_events.clear();

            if self.settings.test_mode {
                break;
            }

            // Block for one event, then drain everything already queued. Bursts of key repeats,
            // wheel scrolls, and mouse motion then produce one frame instead of one per event.
            let mut events = vec![event::read()];
            while matches!(event::poll(Duration::ZERO), Ok(true)) {
                events.push(event::read());
            }

            for (index, event) in events.iter().enumerate() {
                // Only the last of consecutive motion events determines the hover feedback.
                // Drags are kept because each one moves the view relative to the previous.
                if matches!(event, Ok(Event::Mouse(event)) if event.kind == event::MouseEventKind::Moved)
                    && events.get(index + 1).is_some_and(|next| {
                        matches!(next, Ok(Event::Mouse(next)) if next.kind == event::MouseEventKind::Moved)
                    })
                {
                    continue;
                }

                let events = match event {
                    Ok(Event::Key(key_event)) if key_event.kind == KeyEventKind::Press => {
                        self.mouse_register.last_hover = None;
                        let state_messages =
                            self.registers.handle_key_event(*key_event, &self.state)?;
                        self.handle(state_messages).await // TODO: this should not error out?
                    }

                    Ok(Event::Mouse(mouse_event)) if self.scene == Scene::Main => {
                        let state_messages = self.mouse_register.handle_mouse_event(
                            &self.state,
                            &self.resolved_layout,
                            &self.alignment_view,
                            *mouse_event,
                        )?;

                        self.handle(state_messages).await // TODO: this should not error out?
                    }

                    Ok(Event::Resize(width, height)) => {
                        log::debug!("Terminal resized to {width}x{height}");
                        self.mouse_register.last_hover = None;
                        self.resolved_layout = self
                            .layout
                            .resolve(Rect::new(0, 0, *width, *height), &self.repository);
                        self.alignment_view.self_correct(
                            &self.resolved_layout.main_area,
                            self.state.contig_length(&self.alignment_view.focus)?,
                        );
                        self.load_data().await?;
                        Ok(vec![RenderEvent::All])
                    }

                    _ => Ok(Vec::new()),
                }
                .unwrap_or_else(|e| {
                    log::warn!("Error while handling event: {e}");
                    self.state.add_message(format!("{e}"));
                    vec![RenderEvent::Area(AreaType::Error)]
                });
                for event in events {
                    if !render_events.contains(&event) {
                        render_events.push(event);
                    }
                }

                self.alignment_view.self_correct(
                    &self.resolved_layout.main_area,
                    self.state.contig_length(&self.alignment_view.focus)?,
                );
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
        self.settings.session_path = Some(path);
        Ok(())
    }

    /// Handle messages after initialization. This blocks any error messages instead of propagating them.
    /// Returns a list of render evnet, indicating which areas in the layout needs re-rendering.
    pub async fn handle(&mut self, messages: Vec<Message>) -> Result<Vec<RenderEvent>, TGVError> {
        let mut render_events = Vec::new();
        if !self.state.messages.is_empty() {
            self.state.messages.clear();
            render_events.push(RenderEvent::Area(AreaType::Error));
        }

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
                    self.load_data().await?;
                    render_events.push(RenderEvent::AllTracks);
                }

                Message::Core(gv_core::message::Message::Quit) => {
                    log::info!("Quit requested");
                    self.exit = true;
                }

                Message::Core(gv_core::message::Message::SaveSession(path)) => {
                    let explicit_path = path.is_some();
                    let Some(path) = path
                        .as_deref()
                        .map(SessionFile::resolve_path)
                        .or_else(|| self.settings.session_path.clone())
                    else {
                        self.state.add_message(NO_ACTIVE_SESSION_MESSAGE.to_string());
                        continue;
                    };
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
                    let Some(path) = path
                        .as_deref()
                        .map(SessionFile::resolve_path)
                        .or_else(|| self.settings.session_path.clone())
                    else {
                        self.state.add_message(NO_ACTIVE_SESSION_MESSAGE.to_string());
                        continue;
                    };
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
                    if !self.state.alignments.is_empty() {
                        let index = scroll.index();
                        let depth = match &self.state.paired_alignments[index] {
                            Some(paired) => paired.depth()?,
                            None => self.state.alignments[index].depth()?,
                        };
                        self.alignment_view.scroll(scroll.clone(), depth);
                    }
                    log::debug!(
                        "Scroll applied: scroll={:?} y_before={:?} y_after={:?}",
                        scroll,
                        previous_y,
                        self.alignment_view.y,
                    );
                    render_events.push(RenderEvent::Area(AreaType::Alignment(
                        self.tracks.alignment_id(scroll.index()),
                    )))
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
                        &self.resolved_layout.main_area,
                        contig_length,
                    )?; // TODO
                    log::debug!(
                        "Zoom applied: zoom={:?} previous_zoom={} new_zoom={} focus={:?}",
                        zoom,
                        previous_zoom,
                        self.alignment_view.zoom,
                        self.alignment_view.focus,
                    );
                    self.load_data().await?;
                    render_events.push(RenderEvent::AllTracks)
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

                    render_events.push(RenderEvent::AllTracks)
                }

                Message::Core(gv_core::message::Message::Message(message)) => {
                    log::trace!("Adding transient status message: bytes={}", message.len());
                    self.state.add_message(message);
                    render_events.push(RenderEvent::Area(AreaType::Error))
                }

                Message::SelectContig(index) => {
                    if self.scene == Scene::ContigList
                        && index < self.state.contig_header.contigs.len()
                        && index != self.registers.contig_list_cursor
                    {
                        self.registers.contig_list_cursor = index;
                        render_events.push(RenderEvent::All);
                    }
                }
                Message::SwitchScene(scene) => {
                    let previous_scene = self.scene.clone();
                    log::debug!("Switching scene: from={:?} to={:?}", previous_scene, scene);
                    self.scene = scene;
                    self.mouse_register = MouseRegister::default();
                    render_events.push(RenderEvent::All)
                }
                Message::CommandChanged => {
                    render_events.push(RenderEvent::Area(AreaType::Console));
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
                    render_events.push(RenderEvent::Area(AreaType::Console))
                }
                Message::UpdateLayout(update) => {
                    let previous_width = self.resolved_layout.main_area.width;
                    match update {
                        UpdateLayoutMessage::ToggleSidebar => self.layout.toggle_sidebar(),
                        UpdateLayoutMessage::SetSidebarWidth(column) => self
                            .layout
                            .resize_sidebar_to(column, self.resolved_layout.terminal_area),
                        UpdateLayoutMessage::ResizeAlignmentPair {
                            upper,
                            lower,
                            delta_rows,
                        } => self.layout.resize_alignment_pair(
                            upper,
                            lower,
                            delta_rows,
                            &self.resolved_layout,
                        ),
                    }
                    self.resolved_layout = self
                        .layout
                        .resolve(self.resolved_layout.terminal_area, &self.repository);
                    if self.resolved_layout.main_area.width != previous_width {
                        self.alignment_view.self_correct(
                            &self.resolved_layout.main_area,
                            self.state.contig_length(&self.alignment_view.focus)?,
                        );
                        self.load_data().await?;
                    }
                    render_events.push(RenderEvent::All)
                }
                Message::ClearAllKeyRegisters => {
                    log::debug!("Clearing all key registers");
                    self.registers.clear();
                    render_events.push(RenderEvent::Area(AreaType::Console))
                }
            }
        }

        // TODO: reduce this list

        Ok(render_events)
    }

    async fn load_data(&mut self) -> Result<(), TGVError> {
        if self.resolved_layout.main_area.width == 0 {
            return Ok(());
        }
        // TODO: return whether data were loaded?
        // It's important to load sequence first!
        // Alignment IO requires calculating mismatches with the reference sequence.
        //
        let region = self.alignment_view.region(&self.resolved_layout.main_area);
        log::debug!(
            "Evaluating data loads: display_region={:?} zoom={} focus={:?}",
            region,
            self.alignment_view.zoom,
            self.alignment_view.focus,
        );

        let show_alignments =
            self.alignment_view.zoom <= AlignmentView::MAX_ZOOM_TO_DISPLAY_ALIGNMENTS;
        if !show_alignments {
            log::trace!(
                "Skipping alignment data loads because zoom={} exceeds max_zoom={}",
                self.alignment_view.zoom,
                AlignmentView::MAX_ZOOM_TO_DISPLAY_ALIGNMENTS,
            );
        }
        let files: Vec<RepositoryFileIndex> = self
            .tracks
            .entries
            .iter()
            .map(|entry| entry.repository_index)
            .filter(|index| show_alignments || !matches!(index, RepositoryFileIndex::Alignment(_)))
            .collect();
        self.state
            .ensure_loaded(
                &region,
                &LoadRequest {
                    sequence: self.alignment_view.zoom
                        <= AlignmentView::MAX_ZOOM_TO_DISPLAY_SEQUENCES,
                    genes: true,
                    files: &files,
                    cache: CachePolicy::VIEWER,
                },
                &mut self.repository,
            )
            .await?;

        // Cytobands
        // TODO
        //
        log::debug!(
            "Finished evaluating data loads: display_region={:?}",
            region
        );
        Ok(())
    }

    pub fn render(
        &mut self,
        buf: &mut Buffer,
        render_events: &Vec<RenderEvent>,
    ) -> Result<(), TGVError> {
        use crate::rendering::{render_contig_list, render_help, render_main};
        let full_render = vec![RenderEvent::All];
        let render_events = if self.render_buffer.area != buf.area {
            self.render_buffer.resize(buf.area);
            &full_render
        } else {
            render_events
        };
        self.resolved_layout = self.layout.resolve(buf.area, &self.repository);
        if render_events.contains(&RenderEvent::All) || self.scene != Scene::Main {
            self.render_buffer.reset();
        }
        match &self.scene {
            Scene::Main => render_main(
                &mut self.render_buffer,
                &mut self.state,
                &self.registers,
                &self.resolved_layout,
                &self.alignment_view,
                &self.mouse_register,
                &self.settings.palette,
                render_events,
            ),
            Scene::Help => {
                render_help(&self.resolved_layout.terminal_area, &mut self.render_buffer)
            }
            Scene::ContigList => render_contig_list(
                &self.resolved_layout.terminal_area,
                &mut self.render_buffer,
                &self.state,
                &self.registers,
                &self.settings.palette,
            ),
        }?;
        // Ratatui expects a complete frame even when only some areas change.
        buf.clone_from(&self.render_buffer);
        Ok(())
    }
}

/// The main app object
///
use crossterm::event::{self, Event, EventStream, KeyEventKind};
use futures::StreamExt;
use ratatui::{Terminal, buffer::Buffer, layout::Rect, prelude::Backend};

use crate::{
    jumps::{JumpList, ViewPoint},
    layout::{
        AlignmentView,
        AreaType::{self, Console},
        MainLayout, OnScreenCoordinate, ResolvedMainLayout, file_name,
    },
    menu::ContextMenu,
    menu::MenuItem,
    message::{
        Action, AlignmentOptionUpdate, ColumnSpan, ContextMenuTarget, CursorAfter, Direction,
        FocusMove, FocusTarget, Motion, Operator, UpdateLayoutAction,
    },
    mouse::MouseRegister,
    popup::TextPopup,
    register::{KeyRegisterType, Registers},
    rendering::TrackFocus,
    session::SessionFile,
    settings::Settings,
};
use gv_core::{
    bed::BedEditKind,
    message::{AlignmentDisplayOption, FeatureSource, Movement, Zoom},
    prelude::*,
};
use gv_session::{
    BedWrite, Dataset, HighlightRequest, InspectInterval, NavigateRequest, Request, Requests,
    Session, SessionError, SessionHandle, ViewRequest, ViewState,
};
use itertools::Itertools;
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

const NO_ACTIVE_SESSION_MESSAGE: &str = "No active session. Use :ws NAME or :ws PATH to save.";

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Scene {
    Main,
    ContigList,
}

/// An interval that an agent marks in the view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Highlight {
    pub contig_index: usize,
    /// 1-based, inclusive.
    pub start: u64,
    /// 1-based, inclusive.
    pub end: u64,
}

pub struct App {
    pub exit: bool,

    pub layout: MainLayout,
    pub resolved_layout: ResolvedMainLayout,
    /// The displayed dataset. The view state holds what is drawn; agent requests use its
    /// separate query state.
    pub dataset: Dataset,
    pub settings: Settings,
    pub registers: Registers,
    pub mouse_register: MouseRegister,
    /// The open right-click menu. While it is open, it takes all mouse input.
    pub context_menu: Option<ContextMenu>,
    /// The open text popup. While it is open, it blocks all other input.
    pub popup: Option<TextPopup>,

    pub alignment_view: AlignmentView,
    pub jumps: JumpList,
    /// What word motions, insert mode, and `:w` act on. Normal mode scrolls the focused track
    /// when it is an alignment.
    pub focused: FocusTarget,
    /// The track that the track menu's "Paste" copies. Copying only remembers the track, so a
    /// paste takes its features as they are then.
    pub track_clipboard: Option<TrackId>,
    /// Whether to say, the first time insert mode starts, that the terminal can't tell
    /// Shift-Backspace from Backspace.
    pub warn_shift_backspace: bool,
    /// Whether the last action was a typing edit (`Space`, `Backspace`, or `Shift-Backspace`),
    /// so the next one joins its undo step.
    edit_typing: bool,
    pub highlights: Vec<Highlight>,
    /// Files from `:e` or a paste. They open after the next frame, which shows that they are
    /// opening.
    pub pending_open: Option<Vec<String>>,

    /// Sends requests to this viewer, for agents and tests.
    pub session: SessionHandle,
    requests: Requests,

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

        let mut dataset = Dataset::new(settings.core.clone(), settings.files.clone()).await?;
        let focus = dataset.view.default_focus(&mut dataset.repository).await?;

        let mut alignment_view = AlignmentView::new(focus, dataset.view.alignments.len());
        if let Some(zoom) = settings.zoom {
            alignment_view.zoom = zoom;
        }
        log::info!(
            "App state initialized: reference={} contigs={} alignment_tracks={} variant_tracks={} bed_tracks={} default_focus={:?} initial_zoom={} elapsed_ms={}",
            settings.core.reference,
            dataset.view.contig_header.contigs.len(),
            dataset.view.alignments.len(),
            dataset.view.variants.len(),
            dataset.view.bed_intervals.len(),
            alignment_view.focus,
            alignment_view.zoom,
            app_init_started.elapsed().as_millis(),
        );

        let track_ids = dataset
            .tracks
            .entries
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>();
        let layout = MainLayout::new(&settings, Arc::clone(&dataset.tracks), &track_ids);
        let (session, requests) = Session::channel();
        let mut app = Self {
            exit: false,
            layout,
            resolved_layout: ResolvedMainLayout::default(),
            dataset,
            alignment_view,
            jumps: JumpList::default(),
            // The reference has the focus at startup, so word motions move between exons.
            // Without reference rows, the first file track has it instead.
            focused: FocusTarget::Reference,
            track_clipboard: None,
            warn_shift_backspace: false,
            edit_typing: false,
            highlights: Vec::new(),
            pending_open: None,
            session,
            requests,
            settings: settings.clone(),
            registers: Registers::default(),
            mouse_register: MouseRegister::default(),
            context_menu: None,
            popup: None,
            scene: Scene::Main,
            render_buffer: Buffer::empty(Rect::default()),
        };
        app.ensure_focus_in_order();
        Ok(app)
    }
}

impl App {
    /// Main loop
    pub async fn run<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<(), TGVError> {
        log::info!("Starting the app event loop");
        terminal
            .draw(|frame| {
                self.resolved_layout = self.layout.resolve(frame.area(), &self.dataset.repository);
            })
            .map_err(|e| TGVError::IOError(format!("Failed to draw the terminal: {e}")))?;

        self.handle(self.settings.initial_actions.clone()).await?;
        // The startup locus is where history begins, not a jump to return from.
        self.jumps = JumpList::default();

        self.alignment_view.self_correct(
            &self.resolved_layout.main_area,
            self.dataset
                .view
                .contig_length(&self.alignment_view.focus)?,
        );

        let mut render_events: Vec<RenderEvent> = vec![RenderEvent::All];
        let mut terminal_events = EventStream::new();

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

            if self.pending_open.is_some() {
                // The frame just drawn shows that the files are opening while this waits.
                render_events = self.open_pending().await.unwrap_or_else(|e| {
                    log::warn!("Error while opening files: {e}");
                    self.dataset.view.messages = vec![format!("{e}")];
                    vec![RenderEvent::All]
                });
                continue;
            }

            if self.settings.test_mode {
                break;
            }

            // Wait for one terminal event or session request. After a terminal event, drain
            // everything already queued, so bursts of key repeats, wheel scrolls, and mouse
            // motion produce one frame instead of one per event.
            let mut events = tokio::select! {
                event = terminal_events.next() => match event {
                    Some(event) => vec![event],
                    None => break,
                },
                request = self.requests.recv() => {
                    if let Some(request) = request {
                        render_events.extend(self.serve(request).await);
                    }
                    continue;
                }
            };
            // A zero timeout polls the stream once with this task's waker. Polling it with a
            // no-op waker, as `now_or_never` does, leaves the stream unable to wake the loop.
            while let Ok(Some(event)) =
                tokio::time::timeout(Duration::ZERO, terminal_events.next()).await
            {
                events.push(event);
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
                        let actions = if self.popup.is_some() {
                            if key_event.code == event::KeyCode::Esc {
                                vec![Action::ClosePopup]
                            } else {
                                Vec::new()
                            }
                        } else if self.context_menu.is_some() {
                            // The menu is mouse-only. Escape closes it, and other keys wait.
                            if key_event.code == event::KeyCode::Esc {
                                vec![Action::CloseContextMenu]
                            } else {
                                Vec::new()
                            }
                        } else {
                            self.registers
                                .handle_key_event(*key_event, &self.dataset.view)?
                        };
                        self.handle(actions).await // TODO: this should not error out?
                    }

                    Ok(Event::Mouse(mouse_event)) if self.scene == Scene::Main => {
                        let actions = match &mut self.context_menu {
                            _ if self.popup.is_some() => Vec::new(),
                            Some(menu) => menu.handle_mouse_event(*mouse_event),
                            None => self.mouse_register.handle_mouse_event(
                                &self.dataset.view,
                                &self.resolved_layout,
                                &self.alignment_view,
                                *mouse_event,
                            )?,
                        };

                        self.handle(actions).await // TODO: this should not error out?
                    }

                    Ok(Event::Paste(text))
                        if self.popup.is_none() && self.context_menu.is_none() =>
                    {
                        let actions = self.registers.handle_paste(text);
                        self.handle(actions).await
                    }

                    Ok(Event::Resize(width, height)) => {
                        log::debug!("Terminal resized to {width}x{height}");
                        self.mouse_register.last_hover = None;
                        self.resolved_layout = self
                            .layout
                            .resolve(Rect::new(0, 0, *width, *height), &self.dataset.repository);
                        self.alignment_view.self_correct(
                            &self.resolved_layout.main_area,
                            self.dataset
                                .view
                                .contig_length(&self.alignment_view.focus)?,
                        );
                        self.load_data().await?;
                        Ok(vec![RenderEvent::All])
                    }

                    _ => Ok(Vec::new()),
                }
                .unwrap_or_else(|e| {
                    log::warn!("Error while handling event: {e}");
                    self.dataset.view.messages = vec![format!("{e}")];
                    vec![RenderEvent::Area(AreaType::Error)]
                });
                for event in events {
                    if !render_events.contains(&event) {
                        render_events.push(event);
                    }
                }

                self.alignment_view.self_correct(
                    &self.resolved_layout.main_area,
                    self.dataset
                        .view
                        .contig_length(&self.alignment_view.focus)?,
                );
            }
        }
        log::info!("The app event loop exited");
        Ok(())
    }

    /// Serves the next session request, for hosts that drive the app without `run`.
    pub async fn serve_next_request(&mut self) -> Vec<RenderEvent> {
        match self.requests.recv().await {
            Some(request) => self.serve(request).await,
            None => Vec::new(),
        }
    }

    /// Serves a session request and returns the areas to redraw.
    async fn serve(&mut self, request: Request) -> Vec<RenderEvent> {
        match request {
            Request::Data(request) => {
                self.dataset.serve(request).await;
                Vec::new()
            }
            Request::LoadDataset(_, reply) => {
                reply.respond(Err(SessionError::DatasetFixed));
                Vec::new()
            }
            Request::View(request) => self.serve_view(request).await,
            // The user decides when the viewer quits.
            Request::Shutdown => Vec::new(),
        }
    }

    /// Serves a view request by translating it into actions and applying them like input.
    async fn serve_view(&mut self, request: ViewRequest) -> Vec<RenderEvent> {
        match request {
            ViewRequest::Navigate(request, reply) => match self.navigate_actions(&request) {
                Ok(actions) => {
                    let (render_events, result) = self.apply_for_agent(actions).await;
                    reply.respond(result.and_then(|()| self.view_state()));
                    render_events
                }
                Err(error) => {
                    reply.respond(Err(error));
                    Vec::new()
                }
            },
            ViewRequest::Highlight(request, reply) => match self.highlight_actions(request) {
                Ok(actions) => {
                    let (render_events, result) = self.apply_for_agent(actions).await;
                    reply.respond(result);
                    render_events
                }
                Err(error) => {
                    reply.respond(Err(error));
                    Vec::new()
                }
            },
            ViewRequest::ClearHighlights(reply) => {
                let (render_events, result) =
                    self.apply_for_agent(vec![Action::ClearHighlights]).await;
                reply.respond(result);
                render_events
            }
            ViewRequest::Current(reply) => {
                reply.respond(self.view_state());
                Vec::new()
            }
        }
    }

    /// Applies actions from an agent. A failure is shown to the user and returned to the agent.
    async fn apply_for_agent(
        &mut self,
        actions: Vec<Action>,
    ) -> (Vec<RenderEvent>, Result<(), SessionError>) {
        match self.handle(actions).await {
            Ok(render_events) => (render_events, Ok(())),
            Err(error) => {
                self.dataset.view.messages = vec![format!("{error}")];
                (vec![RenderEvent::Area(AreaType::Error)], Err(error.into()))
            }
        }
    }

    /// Resolves an interval against the dataset's contigs, without a width limit.
    fn resolve_interval(&self, interval: &InspectInterval) -> Result<Region, SessionError> {
        Region::try_from_contig_names_and_bounds(
            &interval.contig,
            interval.start,
            interval.end,
            &self.dataset.view.contig_header,
            None,
        )
        .map_err(|error| SessionError::InvalidInput {
            field: "region",
            message: error.to_string(),
        })
    }

    /// Centers the region, fits it to the track area, and tells the user how to return.
    fn navigate_actions(&self, request: &NavigateRequest) -> Result<Vec<Action>, SessionError> {
        let region = self.resolve_interval(&request.region)?;
        let contigs = &self.dataset.view.contig_header;
        let previous = self.alignment_view.focus.to_locus_str(contigs)?;
        let target = region.focus.to_locus_str(contigs)?;
        Ok(vec![
            Movement::ContigNamePosition(
                contigs.contigs[region.contig_index()].name.clone(),
                region.focus.position,
            )
            .into(),
            Action::Core(
                Zoom::Fit {
                    bases: region.end() - region.start() + 1,
                }
                .into(),
            ),
            Action::message(format!(
                "An agent moved the view to {target}. Press u to go back to {previous}."
            )),
        ])
    }

    /// Replaces the highlights and tells the user what was marked.
    fn highlight_actions(&self, request: HighlightRequest) -> Result<Vec<Action>, SessionError> {
        let highlights = request
            .intervals
            .iter()
            .map(|interval| {
                let region = self.resolve_interval(interval)?;
                Ok(Highlight {
                    contig_index: region.contig_index(),
                    start: region.start(),
                    end: region.end(),
                })
            })
            .collect::<Result<Vec<_>, SessionError>>()?;
        let label = request
            .label
            .map_or_else(String::new, |label| format!(": {label}"));
        let notice = format!(
            "An agent highlighted {} intervals{label}.",
            highlights.len()
        );
        Ok(vec![
            Action::SetHighlights(highlights),
            Action::message(notice),
        ])
    }

    /// Reports the displayed interval and zoom.
    fn view_state(&self) -> Result<ViewState, SessionError> {
        let region = self.alignment_view.region(&self.resolved_layout.main_area);
        Ok(ViewState {
            region: InspectInterval {
                contig: self.dataset.view.contig_name(&region.focus)?.clone(),
                start: region.start(),
                end: region.end(),
            },
            zoom: self.alignment_view.zoom,
        })
    }

    /// close connections
    pub async fn close(mut self) -> Result<(), TGVError> {
        self.dataset.repository.close().await
    }

    fn save_session_to_path(&mut self, path: PathBuf) -> Result<(), TGVError> {
        SessionFile::try_from(&*self).and_then(|s| s.write_to_path(&path))?;
        self.settings.session_path = Some(path);
        Ok(())
    }

    /// Moves the view, records jumps in the history, and loads the new region.
    async fn apply_movement(&mut self, movement: Movement) -> Result<(), TGVError> {
        let previous_focus = self.alignment_view.focus.clone();
        log::debug!(
            "Handling movement: movement={:?} previous_focus={:?} zoom={}",
            movement,
            previous_focus,
            self.alignment_view.zoom,
        );
        let focus = self
            .dataset
            .view
            .movement(
                self.alignment_view.focus.clone(),
                self.alignment_view.zoom,
                &mut self.dataset.repository,
                movement.clone(),
            )
            .await?;

        log::debug!(
            "Movement applied: movement={:?} previous_focus={:?} new_focus={:?}",
            movement,
            previous_focus,
            focus,
        );
        let jump = matches!(
            movement,
            Movement::Position(_)
                | Movement::ContigNamePosition(..)
                | Movement::ContigIndex(_)
                | Movement::NextContig(_)
                | Movement::PreviousContig(_)
                | Movement::Gene(_)
                | Movement::Default
        );
        if jump && focus != previous_focus {
            self.jumps.record(ViewPoint {
                focus: previous_focus,
                zoom: self.alignment_view.zoom,
            });
        }
        self.alignment_view.focus = focus;
        self.load_data().await
    }

    /// Applies actions after initialization and returns the areas that need redrawing.
    pub async fn handle(&mut self, actions: Vec<Action>) -> Result<Vec<RenderEvent>, TGVError> {
        let mut render_events = Vec::new();
        // Messages from this batch replace the displayed ones. Batches without messages, such
        // as repeated motion within a cell, keep the current messages on screen.
        let mut messages = Vec::new();

        for action in actions {
            if !matches!(action, Action::EditColumns { typing: true, .. }) {
                self.edit_typing = false;
            }
            match action {
                Action::Core(gv_core::message::Message::Move(movement)) => {
                    self.apply_movement(movement).await?;
                    render_events.push(RenderEvent::AllTracks);
                    render_events.push(RenderEvent::Sidebar);
                }

                Action::JumpBack | Action::JumpForward => {
                    let current = ViewPoint {
                        focus: self.alignment_view.focus.clone(),
                        zoom: self.alignment_view.zoom,
                    };
                    let (target, edge) = if action == Action::JumpBack {
                        (
                            self.jumps.back(current),
                            "No earlier position to go back to.",
                        )
                    } else {
                        (
                            self.jumps.forward(current),
                            "No later position to go forward to.",
                        )
                    };
                    match target {
                        Some(target) => {
                            self.alignment_view.focus = target.focus;
                            self.alignment_view.zoom = target.zoom;
                            self.alignment_view.self_correct(
                                &self.resolved_layout.main_area,
                                self.dataset
                                    .view
                                    .contig_length(&self.alignment_view.focus)?,
                            );
                            self.load_data().await?;
                            render_events.push(RenderEvent::AllTracks);
                            render_events.push(RenderEvent::Sidebar);
                        }
                        None => messages.push(edge.to_string()),
                    }
                }

                Action::Core(gv_core::message::Message::Quit) => {
                    log::info!("Quit requested");
                    match self.unsaved_changes_message() {
                        Some(message) => messages.push(message),
                        None => self.exit = true,
                    }
                }

                Action::Core(gv_core::message::Message::ForceQuit) => {
                    log::info!("Quit without writing requested");
                    self.exit = true;
                }

                Action::Core(gv_core::message::Message::WriteTrack(path)) => {
                    let (Ok(message) | Err(message)) = self.write_track(path.as_deref());
                    messages.push(message);
                    render_events.push(RenderEvent::All);
                }

                Action::Core(gv_core::message::Message::WriteAll) => {
                    let (Ok(message) | Err(message)) = self.write_all();
                    messages.push(message);
                    render_events.push(RenderEvent::All);
                }

                Action::Core(
                    ref message @ (gv_core::message::Message::WriteAndQuit(_)
                    | gv_core::message::Message::WriteAllAndQuit),
                ) => {
                    let written = match message {
                        gv_core::message::Message::WriteAndQuit(path) => {
                            self.write_track(path.as_deref())
                        }
                        _ => self.write_all(),
                    };
                    render_events.push(RenderEvent::All);
                    match written {
                        Ok(written) => match self.unsaved_changes_message() {
                            Some(unsaved) => messages.extend([written, unsaved]),
                            None => self.exit = true,
                        },
                        Err(error) => messages.push(error),
                    }
                }

                Action::Core(gv_core::message::Message::NewBedTrack) => {
                    let position = match self.focused {
                        FocusTarget::Track(focused) => self.display_position(focused) + 1,
                        FocusTarget::Reference => self.layout.track_order.len(),
                    };
                    messages.push(self.new_track_at(position).await?);
                    render_events.push(RenderEvent::All);
                }

                Action::NewTrackBeside { track, below } => {
                    let position = self.display_position(track) + usize::from(below);
                    messages.push(self.new_track_at(position).await?);
                    render_events.push(RenderEvent::All);
                }

                Action::Core(gv_core::message::Message::SaveSession(path)) => {
                    let explicit_path = path.is_some();
                    let Some(path) = path
                        .as_deref()
                        .map(SessionFile::resolve_path)
                        .or_else(|| self.settings.session_path.clone())
                    else {
                        messages.push(NO_ACTIVE_SESSION_MESSAGE.to_string());
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
                            messages.push(format!("Session saved to {}.", path.display()));
                            messages.extend(self.session_notes());
                        }
                        Err(e) => {
                            log::warn!("Failed to save session: path={} error={e}", path.display());
                            messages.push(format!("Failed to save session: {e}"));
                        }
                    }
                }

                Action::Core(gv_core::message::Message::SaveAndQuit(path)) => {
                    if let Some(unsaved) = self.unsaved_changes_message() {
                        messages.push(unsaved);
                        continue;
                    }
                    let explicit_path = path.is_some();
                    let Some(path) = path
                        .as_deref()
                        .map(SessionFile::resolve_path)
                        .or_else(|| self.settings.session_path.clone())
                    else {
                        messages.push(NO_ACTIVE_SESSION_MESSAGE.to_string());
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
                            messages.push(format!("Failed to save session: {e}"));
                        }
                    }
                }

                Action::Core(gv_core::message::Message::Scroll(scroll)) => {
                    // Normal mode scrolls the focused track when it is an alignment.
                    if let FocusTarget::Track(id) = self.focused
                        && let RepositoryFileIndex::Alignment(_) =
                            self.dataset.tracks.get(id).repository_index
                    {
                        render_events.extend(self.scroll_alignment(id, scroll)?);
                    }
                }

                Action::ScrollAlignment { track, scroll } => {
                    render_events.extend(self.scroll_alignment(track, scroll)?);
                }

                Action::Core(gv_core::message::Message::Zoom(zoom)) => {
                    let contig_length = self
                        .dataset
                        .view
                        .contig_length(&self.alignment_view.focus)?;
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
                    render_events.push(RenderEvent::AllTracks);
                    render_events.push(RenderEvent::Sidebar);
                }

                Action::Core(gv_core::message::Message::SetAlignmentOption(options)) => {
                    log::debug!(
                        "Setting alignment options: alignment_count={} options={:?}",
                        self.dataset.view.alignments.len(),
                        options,
                    );
                    // TODO: introduce focus. Only apply option to the alignment in focus
                    for index in 0..self.dataset.view.alignments.len() {
                        self.dataset.view.set_alignment_options(
                            index,
                            &self.alignment_view.focus,
                            options.clone(),
                        )?;
                    }

                    render_events.push(RenderEvent::AllTracks);
                    render_events.push(RenderEvent::Sidebar);
                }

                Action::Core(gv_core::message::Message::OpenFiles(paths)) => {
                    let names = paths
                        .iter()
                        .map(|path| file_name(path))
                        .collect::<Vec<_>>()
                        .join(", ");
                    messages.push(format!("Opening {names}…"));
                    self.pending_open = Some(paths);
                }

                Action::RemoveTrack { track, force } => {
                    messages.push(self.remove_track(track, force).await?);
                    render_events.push(RenderEvent::All);
                }

                Action::CopyTrack(id) => {
                    self.track_clipboard = Some(id);
                    messages.push(format!("Copied {}.", self.track_name(id)));
                }

                Action::PasteTrackBelow(target) => {
                    let Some(source) = self.track_clipboard else {
                        continue;
                    };
                    let id = self.dataset.duplicate_bed(source).await?;
                    let position = self.display_position(target) + 1;
                    self.layout.track_order.insert(position, id);
                    log::info!("Pasted track: source={source} id={id} position={position}");
                    self.apply_track_change().await?;
                    render_events.extend(self.focus(FocusTarget::Track(id)));
                    messages.push(format!(
                        "Pasted {} as {}.",
                        self.track_name(source),
                        self.track_name(id)
                    ));
                    render_events.push(RenderEvent::All);
                }

                Action::Core(gv_core::message::Message::Message(message)) => {
                    log::trace!("Adding transient status message: bytes={}", message.len());
                    messages.push(message);
                }

                Action::SelectContig(index) => {
                    if self.scene == Scene::ContigList
                        && index < self.dataset.view.contig_header.contigs.len()
                        && index != self.registers.contig_list_cursor
                    {
                        self.registers.contig_list_cursor = index;
                        render_events.push(RenderEvent::All);
                    }
                }
                Action::SwitchScene(scene) => {
                    let previous_scene = self.scene.clone();
                    log::debug!("Switching scene: from={:?} to={:?}", previous_scene, scene);
                    self.scene = scene;
                    self.mouse_register = MouseRegister::default();
                    render_events.push(RenderEvent::All)
                }
                Action::CommandChanged => {
                    render_events.push(RenderEvent::Area(AreaType::Console));
                }
                Action::SwitchKeyRegister(register) => {
                    let previous_register = self.registers.current.clone();
                    if register == KeyRegisterType::ContigList {
                        self.registers.contig_list_cursor = self.alignment_view.focus.contig_index
                    }
                    if matches!(register, KeyRegisterType::Command | KeyRegisterType::Search)
                        && matches!(
                            previous_register,
                            KeyRegisterType::Normal | KeyRegisterType::Insert { .. }
                        )
                    {
                        self.registers.return_to = previous_register.clone();
                    }
                    self.registers.current = register;
                    log::debug!(
                        "Switching key register: from={:?} to={:?}",
                        previous_register,
                        self.registers.current,
                    );
                    // The mode shows in the sidebar fill, the console, and the insert cursor.
                    render_events.push(RenderEvent::All);
                }
                Action::UpdateLayout(update) => {
                    let previous_width = self.resolved_layout.main_area.width;
                    match update {
                        UpdateLayoutAction::ToggleSidebar => self.layout.toggle_sidebar(),
                        UpdateLayoutAction::SetSidebarWidth(column) => self
                            .layout
                            .resize_sidebar_to(column, self.resolved_layout.terminal_area),
                        UpdateLayoutAction::ResizeAlignmentPair {
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
                        .resolve(self.resolved_layout.terminal_area, &self.dataset.repository);
                    if self.resolved_layout.main_area.width != previous_width {
                        self.alignment_view.self_correct(
                            &self.resolved_layout.main_area,
                            self.dataset
                                .view
                                .contig_length(&self.alignment_view.focus)?,
                        );
                        self.load_data().await?;
                    }
                    render_events.push(RenderEvent::All)
                }
                Action::Focus(target) => render_events.extend(self.focus(target)),
                Action::MoveFocus(movement) => {
                    let order = self.focus_order();
                    let last = order.len() - 1;
                    let current = order
                        .iter()
                        .position(|&target| target == self.focused)
                        .expect("the focus is in the focus order");
                    let target = match movement {
                        FocusMove::Next(n) => current.saturating_add(n).min(last),
                        FocusMove::Previous(n) => current.saturating_sub(n),
                        FocusMove::NextWrap => (current + 1) % order.len(),
                        FocusMove::PreviousWrap => (current + last) % order.len(),
                        FocusMove::First => 0,
                        FocusMove::Last => last,
                    };
                    render_events.extend(self.focus(order[target]));
                }
                Action::EnterInsertMode { append } => {
                    let FocusTarget::Track(id) = self.focused else {
                        messages.push("Insert mode is only available for BED tracks.".to_string());
                        continue;
                    };
                    if !matches!(
                        self.dataset.tracks.get(id).repository_index,
                        RepositoryFileIndex::Bed(_)
                    ) {
                        messages.push("Insert mode is only available for BED tracks.".to_string());
                        continue;
                    }
                    // Read-only BED files refuse here, with a message naming the file.
                    if let Err(error) = self.dataset.begin_edit_step(id) {
                        messages.push(error.to_string());
                        continue;
                    }
                    if append {
                        self.apply_movement(Movement::Right(1)).await?;
                    }
                    self.registers.current = KeyRegisterType::Insert { track: id };
                    if self.warn_shift_backspace {
                        self.warn_shift_backspace = false;
                        messages.push(
                            "This terminal sends Shift-Backspace as Backspace. Paint leftward with h and Space."
                                .to_string(),
                        );
                    }
                    render_events.push(RenderEvent::All);
                }
                Action::Motion(motion) => {
                    let movement = self.resolve_motion(motion);
                    match self.apply_movement(movement).await {
                        Ok(()) => {
                            render_events.push(RenderEvent::AllTracks);
                            render_events.push(RenderEvent::Sidebar);
                        }
                        Err(error @ TGVError::NoMoreFeatures { .. }) => {
                            messages.push(error.to_string())
                        }
                        Err(error) => return Err(error),
                    }
                }
                Action::MoveBedLane { track, scroll } => {
                    self.dataset.move_bed_lane(
                        track,
                        self.alignment_view.focus.contig_index,
                        scroll,
                    )?;
                    let index = self.dataset.tracks.bed_index(track)?;
                    self.dataset.view.bed_display_modes[index] =
                        gv_core::intervals::IntervalDisplayMode::Expanded;
                    self.load_data().await?;
                    self.mouse_register.last_hover = None;
                    render_events.push(RenderEvent::All);
                }
                Action::InsertBedLanes {
                    track,
                    below,
                    count,
                } => {
                    self.dataset.begin_edit_step(track)?;
                    self.dataset.insert_bed_lanes(
                        track,
                        self.alignment_view.focus.contig_index,
                        below,
                        count,
                    )?;
                    let index = self.dataset.tracks.bed_index(track)?;
                    self.dataset.view.bed_display_modes[index] =
                        gv_core::intervals::IntervalDisplayMode::Expanded;
                    self.load_data().await?;
                    self.mouse_register.last_hover = None;
                    render_events.push(RenderEvent::All);
                }
                Action::EditColumns {
                    track,
                    edit,
                    span,
                    cursor,
                    typing,
                } => {
                    if !(typing && self.edit_typing) {
                        self.dataset.begin_edit_step(track)?;
                    }
                    self.edit_typing = typing;
                    render_events.extend(self.edit_columns(track, edit, span, cursor).await?);
                }
                Action::Operate {
                    track,
                    operator: Operator::Erase,
                    motion,
                } => {
                    self.dataset.begin_edit_step(track)?;
                    match motion {
                        Motion::Columns { direction, count } => {
                            let (span, cursor) = match direction {
                                Direction::Left => {
                                    (ColumnSpan::Left(count), CursorAfter::SpanStart)
                                }
                                Direction::Right => {
                                    // Genomic columns stay fixed after erasure, so advance past the gap.
                                    (ColumnSpan::Right(count), CursorAfter::PastSpan)
                                }
                            };
                            render_events.extend(
                                self.edit_columns(track, BedEditKind::Remove, span, cursor)
                                    .await?,
                            );
                        }
                        Motion::Word { .. } => match self.erase_to_word(track, motion).await? {
                            Ok(events) => render_events.extend(events),
                            Err(message) => messages.push(message),
                        },
                    }
                }
                Action::Undo(id) | Action::Redo(id) => {
                    let (result, edge) = if matches!(action, Action::Undo(_)) {
                        (self.dataset.undo(id), "Already at oldest change")
                    } else {
                        (self.dataset.redo(id), "Already at newest change")
                    };
                    match result {
                        Ok(true) => {
                            self.load_data().await?;
                            render_events.push(RenderEvent::All);
                        }
                        Ok(false) => messages.push(edge.to_string()),
                        Err(error) => messages.push(error.to_string()),
                    }
                }
                Action::UpdateAlignmentOptions { track, update } => {
                    let index = self.dataset.tracks.alignment_index(track)?;
                    let mut options = self.dataset.view.alignment_options[index].clone();
                    match update {
                        AlignmentOptionUpdate::Sort(sort) => {
                            options.retain(|option| {
                                !matches!(option, AlignmentDisplayOption::Sort(_))
                            });
                            options.push(AlignmentDisplayOption::Sort(sort));
                        }
                        AlignmentOptionUpdate::Filter(filter) => {
                            options.retain(|option| {
                                !matches!(option, AlignmentDisplayOption::Filter(_))
                            });
                            options.push(AlignmentDisplayOption::Filter(filter));
                        }
                        AlignmentOptionUpdate::TogglePaired => {
                            if options.contains(&AlignmentDisplayOption::ViewAsPairs) {
                                options.retain(|option| {
                                    *option != AlignmentDisplayOption::ViewAsPairs
                                });
                            } else {
                                options.push(AlignmentDisplayOption::ViewAsPairs);
                            }
                        }
                        AlignmentOptionUpdate::Reset => options.clear(),
                    }
                    // Options apply in order. Filtering re-stacks the reads, so it goes before
                    // sorting.
                    options.sort_by_key(|option| match option {
                        AlignmentDisplayOption::Filter(_) => 0,
                        AlignmentDisplayOption::Sort(_) => 1,
                        AlignmentDisplayOption::ViewAsPairs => 2,
                    });
                    log::debug!("Updating alignment options: track={track} options={options:?}");
                    self.dataset.view.set_alignment_options(
                        index,
                        &self.alignment_view.focus,
                        options,
                    )?;
                    render_events.push(RenderEvent::AllTracks);
                    render_events.push(RenderEvent::Sidebar);
                }
                Action::SetIntervalDisplayMode { track, mode } => {
                    match self.dataset.tracks.get(track).repository_index {
                        RepositoryFileIndex::Bed(index) => {
                            self.dataset.view.bed_display_modes[index] = mode
                        }
                        RepositoryFileIndex::Variant(index) => {
                            self.dataset.view.variant_display_modes[index] = mode
                        }
                        RepositoryFileIndex::Alignment(_) => continue,
                    }
                    self.mouse_register.last_hover = None;
                    render_events.push(RenderEvent::All);
                }
                Action::OpenContextMenu {
                    target,
                    column,
                    row,
                } => {
                    let items = match target {
                        ContextMenuTarget::Alignment { track, position } => {
                            let index = self.dataset.tracks.alignment_index(track)?;
                            let mut items = ContextMenu::alignment_items(
                                track,
                                position,
                                &self.dataset.view.alignments[index],
                                &self.dataset.view.alignment_options[index],
                            )?;
                            items.extend(self.track_menu_items(track));
                            items
                        }
                        ContextMenuTarget::Sidebar { track } => ContextMenu::sidebar_items(
                            track.map(|id| (id, self.track_menu_items(id))),
                        ),
                        ContextMenuTarget::Track { track } => {
                            let mut items = self.track_menu_items(track);
                            if let Some((_, area)) = self.resolved_layout.areas.iter().find(|(kind, _)|
                                matches!(kind, AreaType::Bed(id) | AreaType::Variant(id) if *id == track))
                                && let Some(description) = crate::mouse::interval_at(
                                    &self.dataset.view, &self.resolved_layout, &self.alignment_view,
                                    track, area, column, row,
                                )?
                            {
                                items.insert(0, MenuItem::Entry { label: description, action: None });
                            }
                            items
                        }
                    };
                    self.context_menu = Some(ContextMenu::new(
                        items,
                        column,
                        row,
                        self.resolved_layout.terminal_area,
                    ));
                    render_events.push(RenderEvent::All);
                }
                Action::OpenReadDetails { track, read_id } => {
                    let index = self.dataset.tracks.alignment_index(track)?;
                    self.popup = Some(TextPopup::read_details(
                        self.dataset.repository.alignment_repositories[index]
                            .source
                            .header(),
                        &self.dataset.view.alignments[index].records[read_id],
                    )?);
                    render_events.push(RenderEvent::All);
                }
                Action::ClosePopup => {
                    if self.popup.take().is_some() {
                        self.mouse_register = MouseRegister::default();
                        render_events.push(RenderEvent::All);
                    }
                }
                Action::ContextMenuChanged => render_events.push(RenderEvent::All),
                Action::CloseContextMenu => {
                    if self.context_menu.take().is_some() {
                        // The cell under the cursor may show something new after the menu closes.
                        self.mouse_register.last_hover = None;
                        render_events.push(RenderEvent::All);
                    }
                }
                Action::ClearAllKeyRegisters => {
                    log::debug!("Clearing all key registers");
                    self.registers.clear();
                    render_events.push(RenderEvent::Area(AreaType::Console))
                }
                Action::SetHighlights(highlights) => {
                    log::debug!("Setting highlights: count={}", highlights.len());
                    self.highlights = highlights;
                    render_events.push(RenderEvent::All)
                }
                Action::ClearHighlights => {
                    self.highlights.clear();
                    render_events.push(RenderEvent::All)
                }
            }
        }

        if !messages.is_empty() {
            self.dataset.view.messages = messages;
            render_events.push(RenderEvent::Area(AreaType::Error));
        }

        Ok(render_events)
    }

    /// The focus targets in display order: the file tracks, then the reference, which is drawn
    /// at the bottom. Without sequence or gene rows, the reference has nothing to draw its focus
    /// on, so it is left out unless there are no file tracks.
    fn focus_order(&self) -> Vec<FocusTarget> {
        let reference_shown = self
            .layout
            .tracks
            .iter()
            .any(|area| matches!(area, AreaType::Sequence | AreaType::GeneTrack));
        let tracks = self
            .layout
            .track_order
            .iter()
            .map(|&id| FocusTarget::Track(id));
        if reference_shown || self.layout.track_order.is_empty() {
            tracks.chain([FocusTarget::Reference]).collect()
        } else {
            tracks.collect()
        }
    }

    /// Moves the focus to the first target when the focused one is no longer in the focus
    /// order, as after removing tracks.
    fn ensure_focus_in_order(&mut self) {
        let order = self.focus_order();
        if !order.contains(&self.focused) {
            self.focused = order[0];
        }
    }

    /// A file track's position in the display order.
    fn display_position(&self, id: TrackId) -> usize {
        self.layout
            .track_order
            .iter()
            .position(|&track| track == id)
            .expect("file tracks are in the display order")
    }

    /// Moves the focus and returns the areas to redraw. Insert mode stays on its track, so
    /// focusing anything else leaves it.
    fn focus(&mut self, target: FocusTarget) -> Vec<RenderEvent> {
        if self.focused == target {
            return Vec::new();
        }
        self.focused = target;
        if let KeyRegisterType::Insert { track } = self.registers.current
            && target != FocusTarget::Track(track)
        {
            self.registers.current = KeyRegisterType::Normal;
            self.registers.pending.clear();
        }
        // Command input started in insert mode returns to normal mode instead.
        if let KeyRegisterType::Insert { track } = self.registers.return_to
            && target != FocusTarget::Track(track)
        {
            self.registers.return_to = KeyRegisterType::Normal;
        }
        vec![RenderEvent::All]
    }

    /// Turns a motion into a movement. Word motions move between the focused track's features:
    /// exons and genes for the reference and alignments, features for BED tracks, and variants
    /// for variant tracks.
    fn resolve_motion(&self, motion: Motion) -> Movement {
        match motion {
            Motion::Columns {
                direction: Direction::Left,
                count,
            } => Movement::Left(count),
            Motion::Columns {
                direction: Direction::Right,
                count,
            } => Movement::Right(count),
            Motion::Word { motion, big, count } => {
                let reference = if big {
                    FeatureSource::Genes
                } else {
                    FeatureSource::Exons
                };
                let source = match self.focused {
                    FocusTarget::Reference => reference,
                    FocusTarget::Track(id) => match self.dataset.tracks.get(id).repository_index {
                        RepositoryFileIndex::Alignment(_) => reference,
                        // Uppercase motions do the same as lowercase ones on BED and variant
                        // tracks, for now.
                        RepositoryFileIndex::Bed(index) => FeatureSource::Bed(index),
                        RepositoryFileIndex::Variant(index) => FeatureSource::Variants(index),
                    },
                };
                Movement::Feature {
                    source,
                    motion,
                    count,
                }
            }
        }
    }

    /// Adds or removes the bases of whole columns next to the cursor, then moves the cursor.
    /// One column is one base, or `zoom` bases when zoomed out.
    async fn edit_columns(
        &mut self,
        track: TrackId,
        edit: BedEditKind,
        span: ColumnSpan,
        cursor: CursorAfter,
    ) -> Result<Vec<RenderEvent>, TGVError> {
        let area = self.resolved_layout.main_area;
        let focus = self.alignment_view.focus.clone();
        let OnScreenCoordinate::OnScreen(column) = self
            .alignment_view
            .onscreen_x_coordinate(focus.position, &area)
        else {
            return Ok(Vec::new());
        };
        let Some((column_start, column_end)) = self
            .alignment_view
            .coordinates_of_onscreen_x(area.x + column as u16, &area)
        else {
            return Ok(Vec::new());
        };
        let zoom = self.alignment_view.zoom;
        let (start, end) = match span {
            ColumnSpan::Right(n) => (
                column_start,
                column_end.saturating_add(n.saturating_sub(1).saturating_mul(zoom)),
            ),
            ColumnSpan::Left(n) => (
                column_start.saturating_sub(n.saturating_mul(zoom)).max(1),
                column_start - 1,
            ),
        };
        let mut render_events = self.edit_bed_range(track, edit, start, end).await?;
        let movement = match (span, cursor) {
            (ColumnSpan::Right(n), CursorAfter::PastSpan) => Some(Movement::Right(n)),
            (ColumnSpan::Left(n), CursorAfter::SpanStart) => Some(Movement::Left(n)),
            (ColumnSpan::Right(_), CursorAfter::SpanStart)
            | (ColumnSpan::Left(_), CursorAfter::PastSpan) => None,
        };
        if let Some(movement) = movement {
            self.apply_movement(movement).await?;
            render_events.push(RenderEvent::AllTracks);
            render_events.push(RenderEvent::Sidebar);
        }
        Ok(render_events)
    }

    /// Erases from the cursor to a word motion's target, as vim's `d` does: an exclusive motion
    /// leaves out the target, and an inclusive one takes it. The cursor moves to the start of
    /// the range. Returns the message to show when nothing is erased.
    async fn erase_to_word(
        &mut self,
        track: TrackId,
        motion: Motion,
    ) -> Result<Result<Vec<RenderEvent>, String>, TGVError> {
        let focus = self.alignment_view.focus.clone();
        let movement = self.resolve_motion(motion);
        let target = match self
            .dataset
            .view
            .movement(
                focus.clone(),
                self.alignment_view.zoom,
                &mut self.dataset.repository,
                movement,
            )
            .await
        {
            Ok(target) => target,
            Err(error @ TGVError::NoMoreFeatures { .. }) => return Ok(Err(error.to_string())),
            Err(error) => return Err(error),
        };
        if target.contig_index != focus.contig_index {
            return Ok(Err(
                "The motion leaves the contig, so nothing is erased.".to_string()
            ));
        }
        let (cursor, target) = (focus.position, target.position);
        let inclusive = motion.inclusive();
        let (start, end) = match target.cmp(&cursor) {
            std::cmp::Ordering::Equal => return Ok(Ok(Vec::new())),
            std::cmp::Ordering::Greater if inclusive => (cursor, target),
            std::cmp::Ordering::Greater => (cursor, target - 1),
            std::cmp::Ordering::Less if inclusive => (target, cursor),
            std::cmp::Ordering::Less => (target, cursor - 1),
        };
        let mut render_events = self
            .edit_bed_range(track, BedEditKind::Remove, start, end)
            .await?;
        if start != cursor {
            // Moving the cursor within an edit isn't a jump to return to.
            self.alignment_view.focus = focus.move_to(start);
            self.load_data().await?;
            render_events.push(RenderEvent::AllTracks);
            render_events.push(RenderEvent::Sidebar);
        }
        Ok(Ok(render_events))
    }

    /// Adds or removes bases on the focused contig, 1-based and inclusive, and returns the
    /// areas to redraw.
    async fn edit_bed_range(
        &mut self,
        track: TrackId,
        edit: BedEditKind,
        start: u64,
        end: u64,
    ) -> Result<Vec<RenderEvent>, TGVError> {
        let focus = self.alignment_view.focus.clone();
        let end = match self.dataset.view.contig_length(&focus)? {
            Some(length) => end.min(length),
            None => end,
        };
        if start > end {
            return Ok(Vec::new());
        }
        let was_modified = self.dataset.is_modified(track);
        if !self
            .dataset
            .edit_selected_bed_lane(track, edit, focus.contig_index, start, end)?
        {
            return Ok(Vec::new());
        }
        self.load_data().await?;
        Ok(if self.dataset.is_modified(track) != was_modified {
            // The sidebar marker can change the label's height.
            vec![RenderEvent::All]
        } else {
            vec![
                RenderEvent::Area(AreaType::Bed(track)),
                RenderEvent::Area(AreaType::Console),
            ]
        })
    }

    /// Creates an untitled BED track at a position in the display order, focuses it, and
    /// returns the message to show.
    async fn new_track_at(&mut self, position: usize) -> Result<String, TGVError> {
        let id = self.dataset.new_bed_track().await?;
        self.layout.track_order.insert(position, id);
        log::info!("Created track: id={id} position={position}");
        self.apply_track_change().await?;
        self.focus(FocusTarget::Track(id));
        Ok(format!(
            "Created {}. Press i to paint features.",
            self.track_name(id)
        ))
    }

    /// The track menu's items: copying, pasting below, and removing a track.
    fn track_menu_items(&self, id: TrackId) -> Vec<MenuItem> {
        let index = self.dataset.tracks.get(id).repository_index;
        let copyable = matches!(index, RepositoryFileIndex::Bed(bed)
            if !self.dataset.repository.bed_repositories[bed].is_indexed());
        let mut items = ContextMenu::track_items(
            id,
            &self.track_name(id),
            copyable,
            self.dataset.is_modified(id),
            self.track_clipboard
                .map(|copied| self.track_name(copied))
                .as_deref(),
        );
        let mode = match index {
            RepositoryFileIndex::Bed(index) => Some(self.dataset.view.bed_display_modes[index]),
            RepositoryFileIndex::Variant(index) => {
                Some(self.dataset.view.variant_display_modes[index])
            }
            RepositoryFileIndex::Alignment(_) => None,
        };
        if let Some(mode) = mode {
            items.extend(ContextMenu::interval_items(id, mode));
        }
        items
    }

    /// The name a track shows, such as `peaks.bed` or `untitled-1`.
    fn track_name(&self, id: TrackId) -> String {
        file_name(&self.dataset.tracks.get(id).file_path.to_string()).to_owned()
    }

    /// The tracks with unsaved changes, in display order.
    fn modified_tracks(&self) -> Vec<TrackId> {
        self.layout
            .track_order
            .iter()
            .copied()
            .filter(|&id| self.dataset.is_modified(id))
            .collect()
    }

    /// Why quitting has to wait, if any track has unsaved changes.
    fn unsaved_changes_message(&self) -> Option<String> {
        let names = self
            .modified_tracks()
            .into_iter()
            .map(|id| self.track_name(id))
            .collect_vec();
        match names.as_slice() {
            [] => None,
            [name] => Some(format!(
                "{name} has unsaved changes. Use :w, or :q! to quit without writing."
            )),
            names => Some(format!(
                "{} have unsaved changes. Use :wa, or :q! to quit without writing.",
                join_names(names)
            )),
        }
    }

    /// Writes the focused BED track to its file, or to `path`. Returns the message to show,
    /// as an error when nothing was written.
    fn write_track(&mut self, path: Option<&str>) -> Result<String, String> {
        let not_bed = ":w writes BED tracks. Use :ws to save the session.".to_string();
        let FocusTarget::Track(id) = self.focused else {
            return Err(not_bed);
        };
        if !matches!(
            self.dataset.tracks.get(id).repository_index,
            RepositoryFileIndex::Bed(_)
        ) {
            return Err(not_bed);
        }
        let written = self
            .dataset
            .write_bed(id, path)
            .map_err(|error| error.to_string())?;
        let name = self.track_name(id);
        log::info!("Wrote track: id={id} name={name} result={written:?}");
        Ok(match written {
            BedWrite::Saved { features } => {
                format!("{name}: {} written.", feature_count(features))
            }
            BedWrite::Copy { path, .. } if self.dataset.is_modified(id) => {
                format!("Wrote a copy to {path}. {name} still has unsaved changes.")
            }
            BedWrite::Copy { path, .. } => format!("Wrote a copy to {path}."),
        })
    }

    /// Writes every modified BED track that has a file. Returns the message to show, as an
    /// error when a track is still modified afterwards.
    fn write_all(&mut self) -> Result<String, String> {
        let modified = self.modified_tracks();
        if modified.is_empty() {
            return Ok("No tracks have unsaved changes.".to_string());
        }
        let mut written = Vec::new();
        let mut untitled = Vec::new();
        let mut errors = Vec::new();
        for id in modified {
            let name = self.track_name(id);
            if self.dataset.tracks.get(id).file_path.path().is_none() {
                untitled.push(name);
                continue;
            }
            match self.dataset.write_bed(id, None) {
                Ok(_) => written.push(name),
                Err(error) => errors.push(error.to_string()),
            }
        }
        let mut parts = Vec::new();
        if !written.is_empty() {
            parts.push(format!("Wrote {}.", join_names(&written)));
        }
        match untitled.as_slice() {
            [] => {}
            [name] => parts.push(format!("{name} has no file name. Use :w <path>.")),
            names => parts.push(format!(
                "{} have no file names. Use :w <path> on each.",
                join_names(names)
            )),
        }
        let failed = !untitled.is_empty() || !errors.is_empty();
        parts.extend(errors);
        let message = parts.join(" ");
        if failed { Err(message) } else { Ok(message) }
    }

    /// Notes about edited tracks for a saved session: untitled tracks are left out, and
    /// modified tracks are saved by path without their edits.
    fn session_notes(&self) -> Vec<String> {
        let (untitled, modified): (Vec<_>, Vec<_>) = self
            .modified_tracks()
            .into_iter()
            .partition(|&id| self.dataset.tracks.get(id).file_path.path().is_none());
        let names = |ids: Vec<TrackId>| ids.into_iter().map(|id| self.track_name(id)).collect_vec();
        let mut notes = Vec::new();
        match names(untitled).as_slice() {
            [] => {}
            [name] => notes.push(format!("{name} is left out because it has no file.")),
            names => notes.push(format!(
                "{} are left out because they have no files.",
                join_names(names)
            )),
        }
        let modified = names(modified);
        if !modified.is_empty() {
            notes.push(format!(
                "The session points to {} on disk, without the unsaved changes.",
                join_names(&modified)
            ));
        }
        notes
    }

    /// Scrolls an alignment track and returns the areas to redraw.
    fn scroll_alignment(
        &mut self,
        id: TrackId,
        scroll: gv_core::message::Scroll,
    ) -> Result<Vec<RenderEvent>, TGVError> {
        let index = self.dataset.tracks.alignment_index(id)?;
        let depth = match &self.dataset.view.paired_alignments[index] {
            Some(paired) => paired.depth()?,
            None => self.dataset.view.alignments[index].depth()?,
        };
        let previous_y = self.alignment_view.y[index];
        self.alignment_view.scroll(index, scroll.clone(), depth);
        log::debug!(
            "Scroll applied: track={id} scroll={scroll:?} y_before={previous_y} y_after={}",
            self.alignment_view.y[index],
        );
        Ok(vec![
            RenderEvent::Area(AreaType::Alignment(id)),
            RenderEvent::Sidebar,
        ])
    }

    /// Removes a track and returns the message to show. Without `force`, a track with unsaved
    /// changes stays. The focus moves to the next track in display order, or else the previous
    /// one, or else the reference.
    async fn remove_track(&mut self, id: TrackId, force: bool) -> Result<String, TGVError> {
        let name = self.track_name(id);
        if !force && self.dataset.is_modified(id) {
            return Ok(format!(
                "{name} has unsaved changes. Write it with :w first."
            ));
        }
        if self.focused == FocusTarget::Track(id) {
            let order = &self.layout.track_order;
            let position = self.display_position(id);
            self.focused = order
                .get(position + 1)
                .or_else(|| position.checked_sub(1).map(|previous| &order[previous]))
                .map_or(FocusTarget::Reference, |&track| FocusTarget::Track(track));
        }
        if self.track_clipboard == Some(id) {
            self.track_clipboard = None;
        }
        if self.registers.current == (KeyRegisterType::Insert { track: id }) {
            self.registers.current = KeyRegisterType::Normal;
        }
        if self.registers.return_to == (KeyRegisterType::Insert { track: id }) {
            self.registers.return_to = KeyRegisterType::Normal;
        }
        let entry = self.dataset.remove_track(id)?;
        if let RepositoryFileIndex::Alignment(index) = entry.repository_index {
            self.alignment_view.y.remove(index);
        }
        log::info!("Removed track: id={id} file={name}");
        self.apply_track_change().await?;
        Ok(format!("Removed {name}."))
    }

    /// Opens the files in `pending_open` and returns the areas to redraw. An error opening them
    /// becomes the status message, and the tracks stay as they were.
    pub async fn open_pending(&mut self) -> Result<Vec<RenderEvent>, TGVError> {
        let Some(paths) = self.pending_open.take() else {
            return Ok(Vec::new());
        };
        let ids = match self.dataset.add_files(&paths).await {
            Ok(ids) => ids,
            Err(error) => {
                log::warn!("Failed to open files: paths={paths:?} error={error}");
                self.dataset.view.messages = vec![format!("{error}")];
                return Ok(vec![RenderEvent::Area(AreaType::Error)]);
            }
        };
        log::info!("Opened tracks: ids={ids:?} paths={paths:?}");
        self.alignment_view
            .y
            .resize(self.dataset.view.alignments.len(), 0);
        self.apply_track_change().await?;
        self.dataset.view.messages = vec![match paths.as_slice() {
            [path] => format!("Opened {}.", file_name(path)),
            paths => format!("Opened {} files.", paths.len()),
        }];
        Ok(vec![RenderEvent::All])
    }

    /// Rebuilds the layout after tracks were added or removed, and loads the visible region.
    async fn apply_track_change(&mut self) -> Result<(), TGVError> {
        // Divider state names tracks, which may no longer be adjacent or present.
        self.mouse_register = MouseRegister::default();
        self.layout
            .set_tracks(&self.settings, Arc::clone(&self.dataset.tracks));
        self.ensure_focus_in_order();
        self.resolved_layout = self
            .layout
            .resolve(self.resolved_layout.terminal_area, &self.dataset.repository);
        self.alignment_view.self_correct(
            &self.resolved_layout.main_area,
            self.dataset
                .view
                .contig_length(&self.alignment_view.focus)?,
        );
        self.load_data().await
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
        let show_indexed_features =
            self.alignment_view.zoom <= AlignmentView::MAX_ZOOM_TO_DISPLAY_INDEXED_FEATURES;
        let files: Vec<RepositoryFileIndex> = self
            .dataset
            .tracks
            .entries
            .iter()
            .map(|entry| entry.repository_index)
            .filter(|index| match index {
                RepositoryFileIndex::Alignment(_) => show_alignments,
                RepositoryFileIndex::Variant(i) => {
                    show_indexed_features
                        || !self.dataset.repository.variant_repositories[*i].is_indexed()
                }
                RepositoryFileIndex::Bed(i) => {
                    show_indexed_features
                        || !self.dataset.repository.bed_repositories[*i].is_indexed()
                }
            })
            .collect();
        self.dataset
            .view
            .ensure_loaded(
                &region,
                &LoadRequest {
                    sequence: self.alignment_view.zoom
                        <= AlignmentView::MAX_ZOOM_TO_DISPLAY_SEQUENCES,
                    genes: true,
                    // Cytobands load once for every contig, whatever the zoom.
                    cytobands: true,
                    files: &files,
                    cache: CachePolicy::VIEWER,
                },
                &mut self.dataset.repository,
            )
            .await?;

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
        use crate::rendering::{render_contig_list, render_main};
        let full_render = vec![RenderEvent::All];
        let region = self.alignment_view.region(&self.resolved_layout.main_area);
        let layout_changed = self
            .layout
            .update_interval_heights(&self.dataset.view, &region)?;
        let render_events = if layout_changed || self.render_buffer.area != buf.area {
            self.render_buffer.resize(buf.area);
            &full_render
        } else {
            render_events
        };
        self.resolved_layout = self.layout.resolve(buf.area, &self.dataset.repository);
        let focus = TrackFocus {
            target: self.focused,
            name: match self.focused {
                FocusTarget::Reference => self.dataset.view.reference.to_string(),
                FocusTarget::Track(id) => self.track_name(id),
            },
            modified: matches!(self.focused, FocusTarget::Track(id) if self.dataset.is_modified(id)),
        };
        if render_events.contains(&RenderEvent::All) || self.scene != Scene::Main {
            self.render_buffer.reset();
        }
        match &self.scene {
            Scene::Main => render_main(
                &mut self.render_buffer,
                &mut self.dataset.view,
                &self.registers,
                &self.resolved_layout,
                &self.alignment_view,
                &self.mouse_register,
                &self.highlights,
                &focus,
                self.context_menu.as_ref(),
                self.popup.as_ref(),
                &self.settings.palette,
                render_events,
            ),
            Scene::ContigList => render_contig_list(
                &self.resolved_layout.terminal_area,
                &mut self.render_buffer,
                &self.dataset.view,
                &self.registers,
                &self.settings.palette,
            ),
        }?;
        // Ratatui expects a complete frame even when only some areas change.
        buf.clone_from(&self.render_buffer);
        Ok(())
    }
}

/// Joins names with commas and "and", using the Oxford comma.
fn join_names(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [name] => name.clone(),
        [first, second] => format!("{first} and {second}"),
        [rest @ .., last] => format!("{}, and {last}", rest.join(", ")),
    }
}

/// "1 feature" or "42 features".
fn feature_count(features: usize) -> String {
    match features {
        1 => "1 feature".to_string(),
        features => format!("{features} features"),
    }
}

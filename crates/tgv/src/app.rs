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
        MainLayout, ResolvedMainLayout,
    },
    menu::ContextMenu,
    message::{Action, AlignmentOptionUpdate, ContextMenuTarget, UpdateLayoutAction},
    mouse::MouseRegister,
    popup::TextPopup,
    register::{KeyRegisterType, Registers},
    session::SessionFile,
    settings::Settings,
};
use gv_core::{
    message::{AlignmentDisplayOption, Movement, Zoom},
    prelude::*,
};
use gv_session::{
    Dataset, HighlightRequest, InspectInterval, NavigateRequest, Request, Requests, Session,
    SessionError, SessionHandle, ViewRequest, ViewState,
};
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

const NO_ACTIVE_SESSION_MESSAGE: &str = "No active session. Use :w NAME or :w PATH to save.";

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
    /// The alignment index that scrolls apply to: the one the mouse was last over.
    pub focused_alignment: usize,
    pub highlights: Vec<Highlight>,

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

        let mut dataset = Dataset::new(settings.core.clone()).await?;
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
        Ok(Self {
            exit: false,
            layout,
            resolved_layout: ResolvedMainLayout::default(),
            dataset,
            alignment_view,
            jumps: JumpList::default(),
            focused_alignment: 0,
            highlights: Vec::new(),
            session,
            requests,
            settings: settings.clone(),
            registers: Registers::default(),
            mouse_register: MouseRegister::default(),
            context_menu: None,
            popup: None,
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

    /// Applies actions after initialization and returns the areas that need redrawing.
    pub async fn handle(&mut self, actions: Vec<Action>) -> Result<Vec<RenderEvent>, TGVError> {
        let mut render_events = Vec::new();
        // Messages from this batch replace the displayed ones. Batches without messages, such
        // as repeated motion within a cell, keep the current messages on screen.
        let mut messages = Vec::new();

        for action in actions {
            match action {
                Action::Core(gv_core::message::Message::Move(movement)) => {
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
                    self.load_data().await?;
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
                    self.exit = true;
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
                            messages.push(format!("Session saved to {}", path.display()));
                        }
                        Err(e) => {
                            log::warn!("Failed to save session: path={} error={e}", path.display());
                            messages.push(format!("Failed to save session: {e}"));
                        }
                    }
                }

                Action::Core(gv_core::message::Message::SaveAndQuit(path)) => {
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
                    let previous_y = self.alignment_view.y.clone();
                    log::debug!(
                        "Handling scroll: scroll={:?} y_before={:?}",
                        scroll,
                        previous_y
                    );
                    let index = self.focused_alignment;
                    if !self.dataset.view.alignments.is_empty() {
                        let depth = match &self.dataset.view.paired_alignments[index] {
                            Some(paired) => paired.depth()?,
                            None => self.dataset.view.alignments[index].depth()?,
                        };
                        self.alignment_view.scroll(index, scroll.clone(), depth);
                        render_events.push(RenderEvent::Area(AreaType::Alignment(
                            self.dataset.tracks.alignment_id(index),
                        )));
                        render_events.push(RenderEvent::Sidebar);
                    }
                    log::debug!(
                        "Scroll applied: scroll={:?} y_before={:?} y_after={:?}",
                        scroll,
                        previous_y,
                        self.alignment_view.y,
                    );
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
                    self.registers.current = register;
                    log::debug!(
                        "Switching key register: from={:?} to={:?}",
                        previous_register,
                        self.registers.current,
                    );
                    render_events.push(RenderEvent::Area(AreaType::Console))
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
                Action::FocusAlignment(id) => {
                    let index = self.dataset.tracks.alignment_index(id)?;
                    if index != self.focused_alignment {
                        self.focused_alignment = index;
                        render_events.push(RenderEvent::Sidebar);
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
                Action::OpenContextMenu {
                    target,
                    column,
                    row,
                } => {
                    let items = match target {
                        ContextMenuTarget::Alignment { track, position } => {
                            let index = self.dataset.tracks.alignment_index(track)?;
                            ContextMenu::alignment_items(
                                track,
                                position,
                                &self.dataset.view.alignments[index],
                                &self.dataset.view.alignment_options[index],
                            )?
                        }
                        ContextMenuTarget::Sidebar => ContextMenu::sidebar_items(),
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
                    files: &files,
                    cache: CachePolicy::VIEWER,
                },
                &mut self.dataset.repository,
            )
            .await?;

        // The cytoband is queried once per contig, whatever the zoom.
        self.dataset
            .view
            .ensure_complete_cytoband_data(&region, &mut self.dataset.repository)
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
        let render_events = if self.render_buffer.area != buf.area {
            self.render_buffer.resize(buf.area);
            &full_render
        } else {
            render_events
        };
        self.resolved_layout = self.layout.resolve(buf.area, &self.dataset.repository);
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

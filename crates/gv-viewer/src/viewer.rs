//! The viewer: the dataset a front end shows, and everything about how it is shown that doesn't
//! depend on the screen.
//!
//! [`Viewer`] owns the [`Dataset`], the viewport, the focus, the modes, and the jump history.
//! It applies [`Action`]s from keys, the mouse, and agents, loads the
//! data the viewport needs, and serves session requests. It also hosts the session, so agents
//! reach it through its [`SessionHandle`].
//!
//! A front end tells the viewer how wide its track area is, in its own units, and applies the
//! [`Effect`]s the viewer leaves for it, such as a layout change or a context menu to show.

use crate::message::{
    EditAction, InputAction, InteractionAction, NavigationAction, SessionAction, TrackAction,
};

use crate::{
    jumps::{JumpList, ViewPoint},
    layout::{AreaType, RenderEvent},
    menu::{self, MenuItem},
    message::{
        Action, AlignmentOptionUpdate, Anchor, ColumnSpan, ContextMenuTarget, CursorAfter,
        Direction, FocusMove, FocusTarget, Motion, Operator, UiAction,
    },
    popup::TextPopup,
    register::{KeyRegisterType, Registers},
    session::SessionFile,
    settings::Settings,
    viewport::{Scale, Viewport},
};
use gv_core::{
    bed::BedEditKind,
    message::{AlignmentDisplayOption, FeatureSource, Movement, Zoom},
    prelude::*,
    settings::file_name,
};
use gv_session::{
    BedWrite, Dataset, InspectInterval, NavigateRequest, Request, Requests, Session, SessionError,
    SessionHandle, ViewRequest, ViewState, ViewerInfo, ViewerKind,
};
use itertools::Itertools;
use std::{
    path::PathBuf,
    time::{Instant, SystemTime},
};

const NO_ACTIVE_SESSION_MESSAGE: &str = "No active session. Use :ws NAME or :ws PATH to save.";

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Scene {
    Main,
    ContigList,
}

/// Something only the front end can do, left by the viewer after it applies actions.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// An action that changes the front end's own layout or widgets.
    Ui(UiAction),
    /// Shows a context menu at a point on the screen.
    ShowContextMenu {
        items: Vec<MenuItem>,
        anchor: Anchor,
    },
    /// Shows a text popup over the view.
    ShowPopup(TextPopup),
    /// File tracks were added, removed, or reordered. The front end rebuilds its layout from
    /// `Viewer::track_order`, and forgets pointer state that names tracks.
    TracksChanged,
    /// The scene changed, so pointer state from the old scene is stale.
    SceneChanged,
}

/// Files to open after the next frame, and where their tracks go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingOpen {
    pub paths: Vec<String>,
    /// The position in the display order. `None` adds the tracks after the others.
    pub position: Option<usize>,
}

pub struct Viewer<Z: Scale> {
    pub exit: bool,
    /// Which front end shows the viewer, as agents see it.
    pub kind: ViewerKind,
    /// When the viewer started, so agents can tell viewers apart.
    started: SystemTime,

    /// The displayed dataset. The view state holds what is drawn; agent requests use its
    /// separate query state.
    pub dataset: Dataset,
    pub settings: Settings,
    pub registers: Registers,

    pub viewport: Viewport<Z>,
    /// The width of the track area, in the front end's units. Zero until the front end lays
    /// out its screen.
    pub width: f64,
    pub jumps: JumpList<Z>,
    /// What word motions, insert mode, and `:w` act on. Normal mode scrolls the focused track
    /// when it is an alignment.
    pub focused: FocusTarget,
    /// File tracks in display order. The registry stays in ID order for lookups, so the order
    /// lives here.
    pub track_order: Vec<TrackId>,
    /// The track that the track menu's "Paste" copies. Copying only remembers the track, so a
    /// paste takes its features as they are then.
    pub track_clipboard: Option<TrackId>,
    /// Whether to say, the first time insert mode starts, that the terminal can't tell
    /// Shift-Backspace from Backspace.
    pub warn_shift_backspace: bool,
    /// Whether the last action was a typing edit (`Space`, `Backspace`, or `Shift-Backspace`),
    /// so the next one joins its undo step.
    edit_typing: bool,
    /// Files from `:e`, a paste, or a drop. They open after the next frame, which shows that
    /// they are opening.
    pub pending_open: Option<PendingOpen>,

    /// Sends requests to this viewer, for agents and tests.
    pub session: SessionHandle,
    requests: Requests,

    pub scene: Scene,

    /// What the front end still has to do, in order. See [`Viewer::take_effects`].
    effects: Vec<Effect>,
}

impl<Z: Scale> Viewer<Z> {
    pub async fn new(settings: Settings, kind: ViewerKind) -> Result<Self, TGVError> {
        let viewer_init_started = Instant::now();

        // Gather resources before initializing the state.
        log::info!(
            "Initializing the viewer with session {:?}",
            settings.session_path
        );

        let mut dataset = Dataset::new(settings.core.clone(), settings.files.clone()).await?;
        let focus = dataset.view.default_focus(&mut dataset.repository).await?;

        let zoom = settings
            .zoom
            .map_or_else(Z::default, Z::from_bases_per_unit);
        let viewport = Viewport::new(focus, zoom, dataset.view.alignments.len());
        log::info!(
            "Viewer state initialized: reference={} contigs={} alignment_tracks={} variant_tracks={} bed_tracks={} default_focus={:?} initial_zoom={:?} elapsed_ms={}",
            settings.core.reference,
            dataset.view.contig_header.contigs.len(),
            dataset.view.alignments.len(),
            dataset.view.variants.len(),
            dataset.view.bed_intervals.len(),
            viewport.focus,
            viewport.zoom,
            viewer_init_started.elapsed().as_millis(),
        );

        let track_order = dataset
            .tracks
            .entries
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>();
        let (session, requests) = Session::channel();
        let mut viewer = Self {
            exit: false,
            kind,
            started: SystemTime::now(),
            dataset,
            viewport,
            width: 0.0,
            jumps: JumpList::default(),
            // The reference has the focus at startup, so word motions move between exons.
            // Without reference rows, the first file track has it instead.
            focused: FocusTarget::Reference,
            track_order,
            track_clipboard: None,
            warn_shift_backspace: false,
            edit_typing: false,
            pending_open: None,
            session,
            requests,
            settings,
            registers: Registers::default(),
            scene: Scene::Main,
            effects: Vec::new(),
        };
        viewer.ensure_focus_in_order();
        Ok(viewer)
    }

    /// Applies the startup actions once the front end has set the track width. The startup
    /// locus is where history begins, not a jump to return from.
    pub async fn start(&mut self) -> Result<Vec<RenderEvent>, TGVError> {
        let render_events = self.handle(self.settings.initial_actions.clone()).await?;
        self.jumps = JumpList::default();
        self.self_correct()?;
        Ok(render_events)
    }

    /// Takes what the front end still has to do after the last calls, in order.
    pub fn take_effects(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.effects)
    }

    /// Keeps the viewport within the focused contig.
    pub fn self_correct(&mut self) -> Result<(), TGVError> {
        let contig_length = self.dataset.view.contig_length(&self.viewport.focus)?;
        self.viewport.self_correct(self.width, contig_length);
        Ok(())
    }

    /// Sets the track area's width after the front end's layout changed, and loads the region
    /// that is now shown.
    pub async fn resize(&mut self, width: f64) -> Result<(), TGVError> {
        if width == self.width {
            return Ok(());
        }
        self.width = width;
        self.self_correct()?;
        self.load_data().await
    }

    /// The region shown in the track area.
    pub fn region(&self) -> Region {
        self.viewport.region(self.width)
    }

    /// Waits for the next session request.
    pub async fn next_request(&mut self) -> Option<Request> {
        self.requests.recv().await
    }

    /// Serves the next session request, for hosts that drive the viewer without an event loop.
    pub async fn serve_next_request(&mut self) -> Vec<RenderEvent> {
        match self.requests.recv().await {
            Some(request) => self.serve(request).await,
            None => Vec::new(),
        }
    }

    /// Serves a session request and returns the areas to redraw.
    pub async fn serve(&mut self, request: Request) -> Vec<RenderEvent> {
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
            ViewRequest::Current(reply) => {
                reply.respond(self.view_state());
                Vec::new()
            }
            ViewRequest::Info(reply) => {
                reply.respond(self.viewer_info());
                Vec::new()
            }
        }
    }

    /// Describes the viewer for agents choosing between viewers.
    fn viewer_info(&self) -> Result<ViewerInfo, SessionError> {
        Ok(ViewerInfo {
            pid: std::process::id(),
            kind: self.kind,
            reference: self.settings.core.reference.to_string(),
            files: self
                .track_order
                .iter()
                .map(|&id| self.dataset.tracks.get(id).file_path.to_string())
                .collect(),
            locus: self
                .viewport
                .focus
                .to_locus_str(&self.dataset.view.contig_header)?,
            started: self
                .started
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0, |since| since.as_secs()),
        })
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
        let previous = self.viewport.focus.to_locus_str(contigs)?;
        let target = region.focus.to_locus_str(contigs)?;
        Ok(vec![
            Movement::ContigNamePosition(
                contigs.contigs[region.contig_index()].name.clone(),
                region.focus.position,
            )
            .into(),
            Action::from(Zoom::Fit {
                bases: region.end() - region.start() + 1,
            }),
            Action::message(format!(
                "An agent moved the view to {target}. Press u to go back to {previous}."
            )),
        ])
    }

    /// Reports the displayed interval and zoom.
    fn view_state(&self) -> Result<ViewState, SessionError> {
        let region = self.region();
        Ok(ViewState {
            region: InspectInterval {
                contig: self.dataset.view.contig_name(&region.focus)?.clone(),
                start: region.start(),
                end: region.end(),
            },
            zoom: self.viewport.zoom.bases_per_unit(),
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
        let previous_focus = self.viewport.focus.clone();
        log::debug!(
            "Handling movement: movement={:?} previous_focus={:?} zoom={:?}",
            movement,
            previous_focus,
            self.viewport.zoom,
        );
        let focus = self
            .dataset
            .view
            .movement(
                self.viewport.focus.clone(),
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
                zoom: self.viewport.zoom,
            });
        }
        self.viewport.focus = focus;
        self.load_data().await
    }

    /// Applies actions and returns the areas that need redrawing. Actions that only the front
    /// end can apply become effects; see [`Viewer::take_effects`].
    pub async fn handle(&mut self, actions: Vec<Action>) -> Result<Vec<RenderEvent>, TGVError> {
        let mut render_events = Vec::new();
        // Messages from this batch replace the displayed ones. Batches without messages, such
        // as repeated motion within a cell, keep the current messages on screen.
        let mut messages = Vec::new();

        for action in actions {
            if !matches!(
                action,
                Action::Edit(EditAction::EditColumns { typing: true, .. })
            ) {
                self.edit_typing = false;
            }
            match action {
                Action::Navigate(NavigationAction::Move(movement)) => {
                    self.apply_movement(movement).await?;
                    render_events.push(RenderEvent::AllTracks);
                    render_events.push(RenderEvent::Sidebar);
                }

                Action::Navigate(NavigationAction::JumpBack)
                | Action::Navigate(NavigationAction::JumpForward) => {
                    let current = ViewPoint {
                        focus: self.viewport.focus.clone(),
                        zoom: self.viewport.zoom,
                    };
                    let (target, edge) = if action == Action::Navigate(NavigationAction::JumpBack) {
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
                            self.viewport.focus = target.focus;
                            self.viewport.zoom = target.zoom;
                            self.self_correct()?;
                            self.load_data().await?;
                            render_events.push(RenderEvent::AllTracks);
                            render_events.push(RenderEvent::Sidebar);
                        }
                        None => messages.push(edge.to_string()),
                    }
                }

                Action::Session(SessionAction::Quit) => {
                    log::info!("Quit requested");
                    match self.unsaved_changes_message() {
                        Some(message) => messages.push(message),
                        None => self.exit = true,
                    }
                }

                Action::Session(SessionAction::ForceQuit) => {
                    log::info!("Quit without writing requested");
                    self.exit = true;
                }

                Action::Session(SessionAction::WriteTrack(path)) => {
                    let (Ok(message) | Err(message)) = self.write_track(path.as_deref());
                    messages.push(message);
                    render_events.push(RenderEvent::All);
                }

                Action::Session(SessionAction::WriteAll) => {
                    let (Ok(message) | Err(message)) = self.write_all();
                    messages.push(message);
                    render_events.push(RenderEvent::All);
                }

                Action::Session(
                    ref message @ (SessionAction::WriteAndQuit(_) | SessionAction::WriteAllAndQuit),
                ) => {
                    let written = match message {
                        SessionAction::WriteAndQuit(path) => self.write_track(path.as_deref()),
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

                Action::Track(TrackAction::NewBedTrack) => {
                    let position = match self.focused {
                        FocusTarget::Track(focused) => self.display_position(focused) + 1,
                        FocusTarget::Reference => self.track_order.len(),
                    };
                    messages.push(self.new_track_at(position).await?);
                    render_events.push(RenderEvent::All);
                }

                Action::Track(TrackAction::NewTrackBeside { track, below }) => {
                    let position = self.display_position(track) + usize::from(below);
                    messages.push(self.new_track_at(position).await?);
                    render_events.push(RenderEvent::All);
                }

                Action::Session(SessionAction::SaveSession(path)) => {
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

                Action::Session(SessionAction::SaveAndQuit(path)) => {
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

                Action::Navigate(NavigationAction::Scroll(scroll)) => {
                    // Normal mode scrolls the focused track when it is an alignment.
                    if let FocusTarget::Track(id) = self.focused
                        && let RepositoryFileIndex::Alignment(_) =
                            self.dataset.tracks.get(id).repository_index
                    {
                        render_events.extend(self.scroll_alignment(id, scroll)?);
                    }
                }

                Action::Navigate(NavigationAction::ScrollAlignment { track, scroll }) => {
                    render_events.extend(self.scroll_alignment(track, scroll)?);
                }

                Action::Navigate(NavigationAction::Zoom(zoom)) => {
                    let contig_length = self.dataset.view.contig_length(&self.viewport.focus)?;
                    let previous_zoom = self.viewport.zoom;
                    log::debug!(
                        "Handling zoom: zoom={:?} previous_zoom={:?} focus={:?}",
                        zoom,
                        previous_zoom,
                        self.viewport.focus,
                    );
                    self.viewport
                        .zoom(zoom.clone(), self.width, contig_length)?;
                    if matches!(self.registers.current, KeyRegisterType::Insert { .. }) {
                        self.viewport.zoom = self.viewport.zoom.for_insert_mode();
                        self.self_correct()?;
                    }
                    log::debug!(
                        "Zoom applied: zoom={:?} previous_zoom={:?} new_zoom={:?} focus={:?}",
                        zoom,
                        previous_zoom,
                        self.viewport.zoom,
                        self.viewport.focus,
                    );
                    self.load_data().await?;
                    render_events.push(RenderEvent::AllTracks);
                    render_events.push(RenderEvent::Sidebar);
                }

                Action::Track(TrackAction::SetAlignmentOption(options)) => {
                    log::debug!(
                        "Setting alignment options: alignment_count={} options={:?}",
                        self.dataset.view.alignments.len(),
                        options,
                    );
                    // TODO: introduce focus. Only apply option to the alignment in focus
                    for index in 0..self.dataset.view.alignments.len() {
                        self.dataset.view.set_alignment_options(
                            index,
                            &self.viewport.focus,
                            options.clone(),
                        )?;
                    }

                    render_events.push(RenderEvent::AllTracks);
                    render_events.push(RenderEvent::Sidebar);
                }

                Action::Track(TrackAction::OpenFiles(paths)) => {
                    messages.push(opening_message(&paths));
                    self.pending_open = Some(PendingOpen {
                        paths,
                        position: None,
                    });
                }

                Action::Track(TrackAction::OpenFilesAt { paths, position }) => {
                    messages.push(opening_message(&paths));
                    self.pending_open = Some(PendingOpen {
                        paths,
                        position: Some(position),
                    });
                }

                Action::Track(TrackAction::RemoveTrack { track, force }) => {
                    messages.push(self.remove_track(track, force).await?);
                    render_events.push(RenderEvent::All);
                }

                Action::Track(TrackAction::CopyTrack(id)) => {
                    self.track_clipboard = Some(id);
                    messages.push(format!("Copied {}.", self.track_name(id)));
                }

                Action::Track(TrackAction::PasteTrackBelow(target)) => {
                    let Some(source) = self.track_clipboard else {
                        continue;
                    };
                    let id = self.dataset.duplicate_bed(source).await?;
                    let position = self.display_position(target) + 1;
                    self.track_order.insert(position, id);
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

                Action::Message(message) => {
                    log::trace!("Adding transient status message: bytes={}", message.len());
                    messages.push(message);
                }

                Action::Navigate(NavigationAction::SelectContig(index)) => {
                    if self.scene == Scene::ContigList
                        && index < self.dataset.view.contig_header.contigs.len()
                        && index != self.registers.contig_list_cursor
                    {
                        self.registers.contig_list_cursor = index;
                        render_events.push(RenderEvent::All);
                    }
                }
                Action::Input(InputAction::SwitchScene(scene)) => {
                    let previous_scene = self.scene.clone();
                    log::debug!("Switching scene: from={:?} to={:?}", previous_scene, scene);
                    self.scene = scene;
                    self.effects.push(Effect::SceneChanged);
                    render_events.push(RenderEvent::All)
                }
                Action::Input(InputAction::CommandChanged) => {
                    render_events.push(RenderEvent::Area(AreaType::Console));
                }
                Action::Input(InputAction::SwitchKeyRegister(register)) => {
                    let previous_register = self.registers.current.clone();
                    if register == KeyRegisterType::ContigList {
                        self.registers.contig_list_cursor = self.viewport.focus.contig_index
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
                Action::Input(InputAction::Focus(target)) => {
                    render_events.extend(self.focus(target))
                }
                Action::Input(InputAction::MoveFocus(movement)) => {
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
                Action::Edit(EditAction::EnterInsertMode { append }) => {
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
                    // Every column of insert mode is whole bases.
                    let insert_zoom = self.viewport.zoom.for_insert_mode();
                    if insert_zoom != self.viewport.zoom {
                        self.viewport.zoom = insert_zoom;
                        self.self_correct()?;
                        self.load_data().await?;
                    }
                    if append {
                        self.apply_movement(Movement::Right(self.viewport.zoom.column_bases()))
                            .await?;
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
                Action::Edit(EditAction::Motion(motion)) => {
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
                Action::Edit(EditAction::EditColumns {
                    track,
                    edit,
                    span,
                    cursor,
                    typing,
                }) => {
                    if !(typing && self.edit_typing) {
                        self.dataset.begin_edit_step(track)?;
                    }
                    self.edit_typing = typing;
                    render_events.extend(self.edit_columns(track, edit, span, cursor).await?);
                }
                Action::Edit(EditAction::Operate {
                    track,
                    operator: Operator::Erase,
                    motion,
                }) => {
                    self.dataset.begin_edit_step(track)?;
                    match motion {
                        Motion::Columns { direction, count } => {
                            let span = match direction {
                                Direction::Left => ColumnSpan::Left(count),
                                Direction::Right => ColumnSpan::Right(count),
                            };
                            render_events.extend(
                                self.edit_columns(
                                    track,
                                    BedEditKind::Remove,
                                    span,
                                    CursorAfter::SpanStart,
                                )
                                .await?,
                            );
                        }
                        Motion::Word { .. } => match self.erase_to_word(track, motion).await? {
                            Ok(events) => render_events.extend(events),
                            Err(message) => messages.push(message),
                        },
                    }
                }
                Action::Edit(EditAction::Undo(id)) | Action::Edit(EditAction::Redo(id)) => {
                    let (result, edge) = if matches!(action, Action::Edit(EditAction::Undo(_))) {
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
                Action::Track(TrackAction::UpdateAlignmentOptions { track, update }) => {
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
                        &self.viewport.focus,
                        options,
                    )?;
                    render_events.push(RenderEvent::AllTracks);
                    render_events.push(RenderEvent::Sidebar);
                }
                Action::Interaction(InteractionAction::OpenContextMenu { target, anchor }) => {
                    let items = self.menu_items(target)?;
                    self.effects.push(Effect::ShowContextMenu { items, anchor });
                    render_events.push(RenderEvent::All);
                }
                Action::Interaction(InteractionAction::OpenReadDetails { track, read_id }) => {
                    let index = self.dataset.tracks.alignment_index(track)?;
                    let popup = TextPopup::read_details(
                        self.dataset.repository.alignment_repositories[index]
                            .source
                            .header(),
                        &self.dataset.view.alignments[index].records[read_id],
                    )?;
                    self.effects.push(Effect::ShowPopup(popup));
                    render_events.push(RenderEvent::All);
                }
                Action::Input(InputAction::ClearAllKeyRegisters) => {
                    log::debug!("Clearing all key registers");
                    self.registers.clear();
                    render_events.push(RenderEvent::Area(AreaType::Console))
                }
                Action::Ui(action) => self.effects.push(Effect::Ui(action)),
            }
        }

        if !messages.is_empty() {
            self.dataset.view.messages = messages;
            render_events.push(RenderEvent::Area(AreaType::Error));
        }

        Ok(render_events)
    }

    /// The items of the context menu for a clicked target.
    pub fn menu_items(&self, target: ContextMenuTarget) -> Result<Vec<MenuItem>, TGVError> {
        Ok(match target {
            ContextMenuTarget::Alignment { track, position } => {
                let index = self.dataset.tracks.alignment_index(track)?;
                let mut items = menu::alignment_items(
                    track,
                    position,
                    &self.dataset.view.alignments[index],
                    &self.dataset.view.alignment_options[index],
                )?;
                items.extend(self.track_menu_items(track));
                items
            }
            ContextMenuTarget::Sidebar { track } => {
                menu::sidebar_items(track.map(|id| (id, self.track_menu_items(id))))
            }
            ContextMenuTarget::Track { track } => self.track_menu_items(track),
        })
    }

    /// The focus targets in display order: the file tracks, then the reference, which is drawn
    /// at the bottom. Without sequence or gene rows, the reference has nothing to draw its focus
    /// on, so it is left out unless there are no file tracks.
    fn focus_order(&self) -> Vec<FocusTarget> {
        let reference = &self.settings.core.reference;
        let reference_shown = reference.needs_sequence() || reference.needs_track();
        let tracks = self.track_order.iter().map(|&id| FocusTarget::Track(id));
        if reference_shown || self.track_order.is_empty() {
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
        self.track_order
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
            } => Movement::Left(count.saturating_mul(self.viewport.zoom.column_bases())),
            Motion::Columns {
                direction: Direction::Right,
                count,
            } => Movement::Right(count.saturating_mul(self.viewport.zoom.column_bases())),
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
    /// A column is the bases that `Scale::column_bases` gives.
    async fn edit_columns(
        &mut self,
        track: TrackId,
        edit: BedEditKind,
        span: ColumnSpan,
        cursor: CursorAfter,
    ) -> Result<Vec<RenderEvent>, TGVError> {
        if self.width <= 0.0 {
            return Ok(Vec::new());
        }
        let focus = self.viewport.focus.clone();
        // The cursor is at the center of the track area, so its column is always shown.
        let (column_start, column_end) = self
            .viewport
            .zoom
            .column_at(self.region().start(), focus.position);
        let column = self.viewport.zoom.column_bases();
        let (start, end) = match span {
            ColumnSpan::Right(n) => (
                column_start,
                column_end.saturating_add(n.saturating_sub(1).saturating_mul(column)),
            ),
            ColumnSpan::Left(n) => (
                column_start.saturating_sub(n.saturating_mul(column)).max(1),
                column_start - 1,
            ),
        };
        let mut render_events = self.edit_bed_range(track, edit, start, end).await?;
        let movement = match (span, cursor) {
            (ColumnSpan::Right(n), CursorAfter::PastSpan) => {
                Some(Movement::Right(n.saturating_mul(column)))
            }
            (ColumnSpan::Left(n), CursorAfter::SpanStart) => {
                Some(Movement::Left(n.saturating_mul(column)))
            }
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
        let focus = self.viewport.focus.clone();
        let movement = self.resolve_motion(motion);
        let target = match self
            .dataset
            .view
            .movement(focus.clone(), &mut self.dataset.repository, movement)
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
            self.viewport.focus = focus.move_to(start);
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
        let focus = self.viewport.focus.clone();
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
            .edit_bed(track, edit, focus.contig_index, start, end)?
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
        self.track_order.insert(position, id);
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
        menu::track_items(
            id,
            &self.track_name(id),
            copyable,
            self.dataset.is_modified(id),
            self.track_clipboard
                .map(|copied| self.track_name(copied))
                .as_deref(),
        )
    }

    /// The name a track shows, such as `peaks.bed` or `untitled-1`.
    pub fn track_name(&self, id: TrackId) -> String {
        file_name(&self.dataset.tracks.get(id).file_path.to_string()).to_owned()
    }

    /// The name of what has the focus: the reference or a file track.
    pub fn focus_name(&self) -> String {
        match self.focused {
            FocusTarget::Reference => self.dataset.view.reference.to_string(),
            FocusTarget::Track(id) => self.track_name(id),
        }
    }

    /// The tracks with unsaved changes, in display order.
    pub fn modified_tracks(&self) -> Vec<TrackId> {
        self.track_order
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

    /// The number of rows of an alignment track, as it is shown: by pairs or by reads.
    pub fn alignment_depth(&self, index: usize) -> Result<usize, TGVError> {
        match &self.dataset.view.paired_alignments[index] {
            Some(paired) => paired.depth(),
            None => self.dataset.view.alignments[index].depth(),
        }
    }

    /// Scrolls an alignment track and returns the areas to redraw.
    fn scroll_alignment(
        &mut self,
        id: TrackId,
        scroll: gv_core::message::Scroll,
    ) -> Result<Vec<RenderEvent>, TGVError> {
        let index = self.dataset.tracks.alignment_index(id)?;
        let depth = self.alignment_depth(index)?;
        let previous_y = self.viewport.y[index];
        self.viewport.scroll(index, scroll.clone(), depth);
        log::debug!(
            "Scroll applied: track={id} scroll={scroll:?} y_before={previous_y} y_after={}",
            self.viewport.y[index],
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
            let order = &self.track_order;
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
            self.viewport.y.remove(index);
        }
        log::info!("Removed track: id={id} file={name}");
        self.apply_track_change().await?;
        Ok(format!("Removed {name}."))
    }

    /// Opens the files in `pending_open` and returns the areas to redraw. An error opening them
    /// becomes the status message, and the tracks stay as they were.
    pub async fn open_pending(&mut self) -> Result<Vec<RenderEvent>, TGVError> {
        let Some(PendingOpen { paths, position }) = self.pending_open.take() else {
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
        log::info!("Opened tracks: ids={ids:?} paths={paths:?} position={position:?}");
        if let Some(position) = position {
            let position = position.min(self.track_order.len());
            self.track_order
                .splice(position..position, ids.iter().copied());
        }
        self.viewport
            .y
            .resize(self.dataset.view.alignments.len(), 0);
        self.apply_track_change().await?;
        self.dataset.view.messages = vec![match paths.as_slice() {
            [path] => format!("Opened {}.", file_name(path)),
            paths => format!("Opened {} files.", paths.len()),
        }];
        Ok(vec![RenderEvent::All])
    }

    /// Updates the display order after tracks were added or removed, and loads the visible
    /// region. Tracks that remain keep their order, and new ones go after the others.
    async fn apply_track_change(&mut self) -> Result<(), TGVError> {
        let registry = &self.dataset.tracks;
        self.track_order.retain(|&id| registry.contains(id));
        for entry in &registry.entries {
            if !self.track_order.contains(&entry.id) {
                self.track_order.push(entry.id);
            }
        }
        self.effects.push(Effect::TracksChanged);
        self.ensure_focus_in_order();
        self.self_correct()?;
        self.load_data().await
    }

    /// Loads what the viewport shows, unless it is already loaded.
    pub async fn load_data(&mut self) -> Result<(), TGVError> {
        if self.width <= 0.0 {
            return Ok(());
        }
        // TODO: return whether data were loaded?
        // It's important to load sequence first!
        // Alignment IO requires calculating mismatches with the reference sequence.
        //
        let region = self.region();
        let zoom = self.viewport.zoom;
        log::debug!(
            "Evaluating data loads: display_region={:?} zoom={:?} focus={:?}",
            region,
            zoom,
            self.viewport.focus,
        );

        let show_alignments = zoom.shows_alignments();
        if !show_alignments {
            log::trace!("Skipping alignment data loads because zoom={zoom:?} is too far out");
        }
        let show_indexed_features = zoom.shows_indexed_features();
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
                    sequence: zoom.shows_sequence(),
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
}

/// Says which files are opening, such as "Opening a.bam, b.bed…".
fn opening_message(paths: &[String]) -> String {
    let names = paths
        .iter()
        .map(|path| file_name(path))
        .collect::<Vec<_>>()
        .join(", ");
    format!("Opening {names}…")
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

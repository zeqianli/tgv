//! The terminal app: the shared viewer, and the terminal's layout, mouse, menus, and drawing.
//!
//! The `Viewer` applies actions to the dataset and the viewport. `App` turns terminal events
//! into actions, applies the effects the viewer leaves for the terminal, and draws.

use crossterm::event::{self, Event, EventStream, KeyEventKind};
use futures::StreamExt;
use ratatui::{Terminal, buffer::Buffer, layout::Rect, prelude::Backend};

use crate::{
    keys::key_from_event,
    layout::{AreaType, MainLayout, ResolvedMainLayout},
    menu::ContextMenu,
    mouse::MouseRegister,
    rendering::TrackFocus,
    settings::Settings,
};
use gv_core::prelude::*;
use gv_session::ViewerKind;
use gv_viewer::{
    layout::RenderEvent,
    message::{FocusTarget, UiAction, UpdateLayoutAction},
    popup::TextPopup,
    prelude::*,
};
use std::{sync::Arc, time::Duration};

pub struct App {
    /// The dataset, the viewport, the focus, and the modes, shared with the GUI.
    pub viewer: Viewer<ColumnScale>,

    pub layout: MainLayout,
    pub resolved_layout: ResolvedMainLayout,
    pub mouse_register: MouseRegister,
    /// The open right-click menu. While it is open, it takes all mouse input.
    pub context_menu: Option<ContextMenu>,
    /// The open text popup. While it is open, it blocks all other input.
    pub popup: Option<TextPopup>,

    render_buffer: Buffer,
}

impl App {
    pub async fn new(settings: Settings) -> Result<Self, TGVError> {
        let viewer = Viewer::new(settings, ViewerKind::Tui).await?;
        let layout = MainLayout::new(
            &viewer.settings,
            Arc::clone(&viewer.dataset.tracks),
            &viewer.track_order,
        );
        Ok(Self {
            viewer,
            layout,
            resolved_layout: ResolvedMainLayout::default(),
            mouse_register: MouseRegister::default(),
            context_menu: None,
            popup: None,
            render_buffer: Buffer::empty(Rect::default()),
        })
    }
}

impl App {
    /// Main loop
    pub async fn run<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<(), TGVError> {
        log::info!("Starting the app event loop");
        terminal
            .draw(|frame| self.resolve_layout(frame.area()))
            .map_err(|e| TGVError::IOError(format!("Failed to draw the terminal: {e}")))?;

        self.start().await?;

        let mut render_events: Vec<RenderEvent> = vec![RenderEvent::All];
        let mut terminal_events = EventStream::new();

        while !self.viewer.exit {
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

            if self.viewer.pending_open.is_some() {
                // The frame just drawn shows that the files are opening while this waits.
                render_events = self.open_pending().await.unwrap_or_else(|e| {
                    log::warn!("Error while opening files: {e}");
                    self.viewer.dataset.view.messages = vec![format!("{e}")];
                    vec![RenderEvent::All]
                });
                continue;
            }

            if self.viewer.settings.test_mode {
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
                request = self.viewer.next_request() => {
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
                                vec![UiAction::ClosePopup.into()]
                            } else {
                                Vec::new()
                            }
                        } else if self.context_menu.is_some() {
                            // The menu is mouse-only. Escape closes it, and other keys wait.
                            if key_event.code == event::KeyCode::Esc {
                                vec![UiAction::CloseContextMenu.into()]
                            } else {
                                Vec::new()
                            }
                        } else {
                            match key_from_event(*key_event) {
                                Some(key) => self
                                    .viewer
                                    .registers
                                    .handle_key_event(key, &self.viewer.dataset.view)?,
                                None => Vec::new(),
                            }
                        };
                        self.handle(actions).await // TODO: this should not error out?
                    }

                    Ok(Event::Mouse(mouse_event)) if self.viewer.scene == Scene::Main => {
                        let actions = match &mut self.context_menu {
                            _ if self.popup.is_some() => Vec::new(),
                            Some(menu) => menu.handle_mouse_event(*mouse_event),
                            None => self.mouse_register.handle_mouse_event(
                                &self.viewer.dataset.view,
                                &self.resolved_layout,
                                &self.viewer.viewport,
                                *mouse_event,
                            )?,
                        };

                        self.handle(actions).await // TODO: this should not error out?
                    }

                    Ok(Event::Paste(text))
                        if self.popup.is_none() && self.context_menu.is_none() =>
                    {
                        let actions = self.viewer.registers.handle_paste(text);
                        self.handle(actions).await
                    }

                    Ok(Event::Resize(width, height)) => {
                        log::debug!("Terminal resized to {width}x{height}");
                        self.mouse_register.last_hover = None;
                        self.resolve_layout(Rect::new(0, 0, *width, *height));
                        self.viewer.self_correct()?;
                        self.viewer.load_data().await?;
                        Ok(vec![RenderEvent::All])
                    }

                    _ => Ok(Vec::new()),
                }
                .unwrap_or_else(|e| {
                    log::warn!("Error while handling event: {e}");
                    self.viewer.dataset.view.messages = vec![format!("{e}")];
                    vec![RenderEvent::Area(AreaType::Error)]
                });
                for event in events {
                    if !render_events.contains(&event) {
                        render_events.push(event);
                    }
                }

                self.viewer.self_correct()?;
            }
        }
        log::info!("The app event loop exited");
        Ok(())
    }

    /// Applies the startup actions after the first layout.
    pub async fn start(&mut self) -> Result<Vec<RenderEvent>, TGVError> {
        let mut render_events = self.viewer.start().await?;
        render_events.extend(self.apply_effects().await?);
        Ok(render_events)
    }

    /// Resolves the layout for a terminal size, and gives the viewer the new track width.
    pub fn resolve_layout(&mut self, terminal_area: Rect) {
        self.resolved_layout = self
            .layout
            .resolve(terminal_area, &self.viewer.dataset.repository);
        self.viewer.width = f64::from(self.resolved_layout.main_area.width);
    }

    /// Serves the next session request, for hosts that drive the app without `run`.
    pub async fn serve_next_request(&mut self) -> Vec<RenderEvent> {
        match self.viewer.next_request().await {
            Some(request) => self.serve(request).await,
            None => Vec::new(),
        }
    }

    /// Serves a session request and returns the areas to redraw.
    async fn serve(&mut self, request: gv_session::Request) -> Vec<RenderEvent> {
        let mut render_events = self.viewer.serve(request).await;
        match self.apply_effects().await {
            Ok(events) => render_events.extend(events),
            Err(error) => {
                self.viewer.dataset.view.messages = vec![format!("{error}")];
                render_events.push(RenderEvent::Area(AreaType::Error));
            }
        }
        render_events
    }

    /// close connections
    pub async fn close(self) -> Result<(), TGVError> {
        self.viewer.close().await
    }

    /// Applies actions after initialization and returns the areas that need redrawing.
    pub async fn handle(&mut self, actions: Vec<Action>) -> Result<Vec<RenderEvent>, TGVError> {
        let mut render_events = self.viewer.handle(actions).await?;
        render_events.extend(self.apply_effects().await?);
        Ok(render_events)
    }

    /// Opens the files that `:e` requested, and returns the areas to redraw.
    pub async fn open_pending(&mut self) -> Result<Vec<RenderEvent>, TGVError> {
        let mut render_events = self.viewer.open_pending().await?;
        render_events.extend(self.apply_effects().await?);
        Ok(render_events)
    }

    /// Applies what the viewer left for the terminal: layout changes, menus, and popups.
    async fn apply_effects(&mut self) -> Result<Vec<RenderEvent>, TGVError> {
        let mut render_events = Vec::new();
        for effect in self.viewer.take_effects() {
            match effect {
                Effect::Ui(UiAction::UpdateLayout(update)) => {
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
                    self.resolved_layout = self.layout.resolve(
                        self.resolved_layout.terminal_area,
                        &self.viewer.dataset.repository,
                    );
                    self.viewer
                        .resize(f64::from(self.resolved_layout.main_area.width))
                        .await?;
                    render_events.push(RenderEvent::All)
                }
                Effect::Ui(UiAction::ContextMenuChanged) => render_events.push(RenderEvent::All),
                Effect::Ui(UiAction::CloseContextMenu) => {
                    if self.context_menu.take().is_some() {
                        // The cell under the cursor may show something new after the menu closes.
                        self.mouse_register.last_hover = None;
                        render_events.push(RenderEvent::All);
                    }
                }
                Effect::Ui(UiAction::ClosePopup) => {
                    if self.popup.take().is_some() {
                        self.mouse_register = MouseRegister::default();
                        render_events.push(RenderEvent::All);
                    }
                }
                Effect::ShowContextMenu { items, anchor } => {
                    self.context_menu = Some(ContextMenu::new(
                        items,
                        anchor.x as u16,
                        anchor.y as u16,
                        self.resolved_layout.terminal_area,
                    ));
                }
                Effect::ShowPopup(popup) => self.popup = Some(popup),
                Effect::TracksChanged => {
                    // Divider state names tracks, which may no longer be adjacent or present.
                    self.mouse_register = MouseRegister::default();
                    self.layout.set_tracks(
                        &self.viewer.settings,
                        Arc::clone(&self.viewer.dataset.tracks),
                        &self.viewer.track_order,
                    );
                    self.resolve_layout(self.resolved_layout.terminal_area);
                }
                Effect::SceneChanged => self.mouse_register = MouseRegister::default(),
            }
        }
        Ok(render_events)
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
        self.resolve_layout(buf.area);
        let focus = TrackFocus {
            target: self.viewer.focused,
            name: self.viewer.focus_name(),
            modified: matches!(self.viewer.focused, FocusTarget::Track(id) if self.viewer.dataset.is_modified(id)),
        };
        if render_events.contains(&RenderEvent::All) || self.viewer.scene != Scene::Main {
            self.render_buffer.reset();
        }
        match &self.viewer.scene {
            Scene::Main => render_main(
                &mut self.render_buffer,
                &mut self.viewer.dataset.view,
                &self.viewer.registers,
                &self.resolved_layout,
                &self.viewer.viewport,
                &self.mouse_register,
                &focus,
                self.context_menu.as_ref(),
                self.popup.as_ref(),
                &self.viewer.settings.palette,
                render_events,
            ),
            Scene::ContigList => render_contig_list(
                &self.resolved_layout.terminal_area,
                &mut self.render_buffer,
                &self.viewer.dataset.view,
                &self.viewer.registers,
                &self.viewer.settings.palette,
            ),
        }?;
        // Ratatui expects a complete frame even when only some areas change.
        buf.clone_from(&self.render_buffer);
        Ok(())
    }
}

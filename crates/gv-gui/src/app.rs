//! The window app: the frames the worker sends, and the window's layout, mouse, menus, and
//! drawing.
//!
//! The worker owns the `Viewer`. `App` turns window events into actions and keys for it,
//! applies the effects it leaves for the window, and draws the latest frame. Pans and zooms
//! are predicted with the shared viewport code, so the window draws them at once, before the
//! worker answers.

use crate::{
    keys::key_from_keystroke,
    layout::{MainLayout, PixelView, Rect, ResolvedMainLayout},
    menu::ContextMenu,
    message::GuiAction,
    mouse::{MouseButton, MouseRegister, zoom_step},
    rendering::{
        self, Painter, Status, TrackFocus,
        colors::{Palette, ToHsla},
    },
    worker::{Frame, WorkerEvent, WorkerHandle, WorkerMessage},
};
use gpui::{
    Context, ExternalPaths, FocusHandle, InteractiveElement, IntoElement, KeyDownEvent,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, PathPromptOptions, Render,
    ScrollWheelEvent, Styled, Window, canvas, div, px,
};
use gv_core::message::{Message, Movement, Zoom};
use gv_viewer::{
    message::{Action, UiAction, UpdateLayoutAction},
    popup::TextPopup,
    settings::Settings,
    viewer::{Effect, Scene},
    viewport::{PixelScale, Viewport},
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::mpsc::UnboundedReceiver;

/// How long the window waits for the worker before it says that it is loading.
const LOADING_NOTICE_DELAY: Duration = Duration::from_millis(150);

pub struct App {
    worker: WorkerHandle,
    /// The latest frame from the worker. `None` until the dataset opens.
    pub frame: Option<Arc<Frame>>,
    /// Why the dataset couldn't open.
    failure: Option<String>,
    settings: Settings,

    /// The viewport after the pans and zooms sent to the worker, until a frame covers them.
    preview: Option<Viewport<PixelScale>>,
    /// Pixels of a pan that haven't added up to a whole base.
    offset: f32,

    pub layout: Option<MainLayout>,
    pub resolved_layout: Option<ResolvedMainLayout>,
    pub mouse_register: MouseRegister,
    /// The open right-click menu. While it is open, it takes all mouse input.
    pub context_menu: Option<ContextMenu>,
    /// The open text popup. While it is open, it blocks all other input.
    pub popup: Option<TextPopup>,
    /// What the message line says about the data under the pointer.
    pub hover: Option<String>,
    /// When the window started waiting for the worker, if it waits.
    waiting_since: Option<Instant>,
    /// The track width the worker was told.
    width_sent: f64,
    pub focus_handle: FocusHandle,
}

impl App {
    pub fn new(
        settings: Settings,
        worker: WorkerHandle,
        mut events: UnboundedReceiver<WorkerEvent>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.spawn(async move |this, cx| {
            while let Some(event) = events.recv().await {
                if this.update(cx, |app, cx| app.receive(event, cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        Self {
            worker,
            frame: None,
            failure: None,
            settings,
            preview: None,
            offset: 0.0,
            layout: None,
            resolved_layout: None,
            mouse_register: MouseRegister::default(),
            context_menu: None,
            popup: None,
            hover: None,
            waiting_since: None,
            width_sent: 0.0,
            focus_handle: cx.focus_handle(),
        }
    }

    /// Takes a frame or a failure from the worker.
    fn receive(&mut self, event: WorkerEvent, cx: &mut Context<Self>) {
        match event {
            WorkerEvent::Failed(error) => self.failure = Some(error),
            WorkerEvent::Frame(frame) => {
                if self.layout.is_none() {
                    self.layout = Some(MainLayout::new(
                        &self.settings,
                        Arc::clone(&frame.tracks),
                        &frame.track_order,
                    ));
                }
                if frame.revision >= self.worker.sent {
                    // The frame covers every message sent, so it replaces the prediction.
                    self.preview = None;
                    self.waiting_since = None;
                }
                let exit = frame.exit;
                let effects = frame.effects.clone();
                self.frame = Some(frame);
                self.apply_effects(effects);
                if exit {
                    cx.quit();
                }
            }
        }
        cx.notify();
    }

    /// Sends actions to the viewer, and waits for its frame.
    fn send(&mut self, message: WorkerMessage, cx: &mut Context<Self>) {
        self.worker.send(message);
        if self.waiting_since.is_none() {
            self.waiting_since = Some(Instant::now());
            // Redraw once the loading notice is due, in case the worker is still busy.
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(LOADING_NOTICE_DELAY + Duration::from_millis(10))
                    .await;
                let _ = this.update(cx, |_, cx| cx.notify());
            })
            .detach();
        }
    }

    /// The view as drawn: the predicted viewport, or the frame's.
    fn view(&self) -> Option<PixelView> {
        let frame = self.frame.as_ref()?;
        Some(PixelView {
            viewport: self
                .preview
                .clone()
                .unwrap_or_else(|| frame.viewport.clone()),
            offset: self.offset,
        })
    }

    /// Applies window actions: the window's own changes here, and the viewer's in one batch.
    fn handle(&mut self, actions: Vec<GuiAction>, cx: &mut Context<Self>) {
        let mut batch = Vec::new();
        for action in actions {
            match action {
                GuiAction::Viewer(Action::Ui(action)) => {
                    self.apply_effects(vec![Effect::Ui(action)])
                }
                GuiAction::Viewer(action) => batch.push(action),
                GuiAction::Pan { pixels } => self.pan(pixels, &mut batch),
                GuiAction::ZoomAround { factor, x } => self.zoom_around(factor, x, &mut batch),
                GuiAction::Hover(hover) => self.hover = hover,
            }
        }
        if !batch.is_empty() {
            self.send(WorkerMessage::Actions(batch), cx);
        }
        cx.notify();
    }

    /// Predicts a pan, and sends the viewer the whole bases it adds up to.
    fn pan(&mut self, pixels: f32, batch: &mut Vec<Action>) {
        let (Some(frame), Some(resolved), Some(view)) = (
            self.frame.clone(),
            self.resolved_layout.as_ref(),
            self.view(),
        ) else {
            return;
        };
        let mut viewport = view.viewport;
        let pixels_per_base = viewport.zoom.pixels_per_base() as f32;
        let total = self.offset + pixels;
        // Moving the content right moves the view left.
        let bases = (total / pixels_per_base).trunc();
        self.offset = total - bases * pixels_per_base;
        if bases == 0.0 {
            return;
        }
        let movement = if bases > 0.0 {
            Movement::Left(bases as u64)
        } else {
            Movement::Right(-bases as u64)
        };
        let expected = match movement {
            Movement::Left(n) => viewport.focus.clone().move_left(n),
            _ => viewport.focus.clone().move_right(-bases as u64),
        };
        viewport.focus = expected.clone();
        let contig_length = frame.state.contig_length(&viewport.focus).ok().flatten();
        viewport.self_correct(resolved.track_width(), contig_length);
        if viewport.focus != expected {
            // The view stopped at the contig's edge, so the content doesn't move further.
            self.offset = 0.0;
        }
        self.preview = Some(viewport);
        batch.push(movement.into());
    }

    /// Predicts a zoom around an x position, and sends the viewer the zoom and the move that
    /// keeps the base under the pointer in place.
    fn zoom_around(&mut self, factor: f64, x: f32, batch: &mut Vec<Action>) {
        let (Some(frame), Some(resolved), Some(view)) = (
            self.frame.clone(),
            self.resolved_layout.as_ref(),
            self.view(),
        ) else {
            return;
        };
        let area = resolved.main_area;
        let width = resolved.track_width();
        let mut viewport = view.viewport;
        let before = viewport.zoom.bases_per_pixel;
        let bases = ((width * before / factor).round() as u64).max(1);
        let contig_length = frame.state.contig_length(&viewport.focus).ok().flatten();
        let zoom = Zoom::Fit { bases };
        if viewport.zoom(zoom.clone(), width, contig_length).is_err() {
            return;
        }
        let after = viewport.zoom.bases_per_pixel;
        let pointer = f64::from(x - (area.x + area.width / 2.0 + self.offset));
        let shift = (pointer * (before - after)).round() as i64;
        batch.push(Action::Core(zoom.into()));
        self.offset = 0.0;
        if shift != 0 {
            let movement = if shift > 0 {
                Movement::Right(shift as u64)
            } else {
                Movement::Left(shift.unsigned_abs())
            };
            viewport.focus = match movement {
                Movement::Right(n) => viewport.focus.move_right(n),
                _ => viewport.focus.move_left(shift.unsigned_abs()),
            };
            viewport.self_correct(width, contig_length);
            batch.push(movement.into());
        }
        self.preview = Some(viewport);
    }

    /// Applies what the viewer left for the window: layout changes, menus, and popups.
    fn apply_effects(&mut self, effects: Vec<Effect>) {
        for effect in effects {
            match effect {
                Effect::Ui(UiAction::UpdateLayout(update)) => {
                    let (Some(layout), Some(resolved)) =
                        (self.layout.as_mut(), self.resolved_layout.as_ref())
                    else {
                        continue;
                    };
                    match update {
                        UpdateLayoutAction::ToggleSidebar => layout.toggle_sidebar(),
                        UpdateLayoutAction::SetSidebarWidth(x) => {
                            layout.resize_sidebar_to(f32::from(x), resolved.window)
                        }
                        UpdateLayoutAction::ResizeAlignmentPair {
                            upper,
                            lower,
                            delta_rows,
                        } => {
                            layout.resize_alignment_pair(upper, lower, delta_rows as f32, resolved)
                        }
                    }
                }
                Effect::Ui(UiAction::ContextMenuChanged) => {}
                Effect::Ui(UiAction::CloseContextMenu) => self.context_menu = None,
                Effect::Ui(UiAction::ClosePopup) => {
                    if self.popup.take().is_some() {
                        self.mouse_register = MouseRegister::default();
                    }
                }
                Effect::ShowContextMenu { items, anchor } => {
                    if let Some(resolved) = &self.resolved_layout {
                        self.context_menu =
                            Some(ContextMenu::new(items, anchor.x, anchor.y, resolved.window));
                    }
                }
                Effect::ShowPopup(popup) => self.popup = Some(popup),
                Effect::TracksChanged => {
                    // Divider state names tracks, which may no longer be adjacent or present.
                    self.mouse_register = MouseRegister::default();
                    if let (Some(layout), Some(frame)) = (self.layout.as_mut(), &self.frame) {
                        layout.set_tracks(
                            &self.settings,
                            Arc::clone(&frame.tracks),
                            &frame.track_order,
                        );
                    }
                }
                Effect::SceneChanged => self.mouse_register = MouseRegister::default(),
            }
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if keystroke.modifiers.platform {
            self.on_shortcut(&keystroke.key, window, cx);
            return;
        }
        let escape = keystroke.key == "escape";
        if self.popup.is_some() {
            if escape {
                self.handle(vec![Action::from(UiAction::ClosePopup).into()], cx);
            }
            return;
        }
        if self.context_menu.is_some() {
            // The menu is mouse-only. Escape closes it, and other keys wait.
            if escape {
                self.handle(vec![Action::from(UiAction::CloseContextMenu).into()], cx);
            }
            return;
        }
        if let Some(key) = key_from_keystroke(keystroke) {
            self.hover = None;
            self.send(WorkerMessage::Keys(vec![key]), cx);
        }
    }

    /// Command shortcuts, as macOS apps have them.
    fn on_shortcut(&mut self, key: &str, _window: &mut Window, cx: &mut Context<Self>) {
        match key {
            "q" | "w" => self.handle(vec![Action::Core(Message::Quit).into()], cx),
            "s" => self.handle(vec![Action::Core(Message::WriteTrack(None)).into()], cx),
            "=" | "+" => self.handle(vec![zoom_step(true).into()], cx),
            "-" => self.handle(vec![zoom_step(false).into()], cx),
            "v" => {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    self.send(WorkerMessage::Paste(text), cx);
                }
            }
            "o" => {
                let paths = cx.prompt_for_paths(PathPromptOptions {
                    files: true,
                    directories: false,
                    multiple: true,
                    prompt: Some("Open".into()),
                });
                cx.spawn(async move |this, cx| {
                    if let Ok(Ok(Some(paths))) = paths.await {
                        let paths = paths
                            .iter()
                            .map(|path| path.to_string_lossy().into_owned())
                            .collect();
                        let _ = this.update(cx, |app, cx| {
                            app.handle(vec![Action::Core(Message::OpenFiles(paths)).into()], cx)
                        });
                    }
                })
                .detach();
            }
            _ => {}
        }
    }

    fn on_mouse_down(
        &mut self,
        button: MouseButton,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        let (x, y) = (f32::from(event.position.x), f32::from(event.position.y));
        if self.popup.is_some() {
            // A click closes the popup, which blocks all other input.
            self.handle(vec![Action::from(UiAction::ClosePopup).into()], cx);
            return;
        }
        if let Some(menu) = &mut self.context_menu {
            let actions = menu.handle_mouse_down(x, y);
            self.handle(actions.into_iter().map(GuiAction::from).collect(), cx);
            return;
        }
        let (Some(frame), Some(resolved), Some(view)) = (
            self.frame.clone(),
            self.resolved_layout.clone(),
            self.view(),
        ) else {
            return;
        };
        if frame.scene != Scene::Main {
            return;
        }
        match self.mouse_register.handle_mouse_down(
            button,
            x,
            y,
            event.click_count,
            &frame,
            &resolved,
            &view,
        ) {
            Ok(actions) => self.handle(actions, cx),
            Err(error) => log::warn!("Error while handling a click: {error}"),
        }
    }

    fn on_mouse_up(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        let (Some(frame), Some(resolved), Some(view)) = (
            self.frame.clone(),
            self.resolved_layout.clone(),
            self.view(),
        ) else {
            return;
        };
        if self.popup.is_some() || self.context_menu.is_some() {
            return;
        }
        let (x, y) = (f32::from(event.position.x), f32::from(event.position.y));
        match self
            .mouse_register
            .handle_mouse_up(x, y, &frame, &resolved, &view)
        {
            Ok(actions) => self.handle(actions, cx),
            Err(error) => log::warn!("Error while handling a click: {error}"),
        }
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let (x, y) = (f32::from(event.position.x), f32::from(event.position.y));
        if self.popup.is_some() {
            return;
        }
        if let Some(menu) = &mut self.context_menu {
            let actions = menu.handle_mouse_move(x, y);
            if !actions.is_empty() {
                self.handle(actions.into_iter().map(GuiAction::from).collect(), cx);
            }
            return;
        }
        let (Some(frame), Some(resolved), Some(view)) = (
            self.frame.clone(),
            self.resolved_layout.clone(),
            self.view(),
        ) else {
            return;
        };
        if frame.scene != Scene::Main {
            return;
        }
        let left_pressed = event.pressed_button == Some(gpui::MouseButton::Left);
        match self
            .mouse_register
            .handle_mouse_move(x, y, left_pressed, &frame, &resolved, &view)
        {
            Ok(actions) => self.handle(actions, cx),
            Err(error) => log::warn!("Error while handling the pointer: {error}"),
        }
    }

    fn on_scroll_wheel(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        if self.popup.is_some() || self.context_menu.is_some() {
            return;
        }
        let Some(resolved) = self.resolved_layout.clone() else {
            return;
        };
        if self
            .frame
            .as_ref()
            .is_none_or(|frame| frame.scene != Scene::Main)
        {
            return;
        }
        let delta = event.delta.pixel_delta(px(rendering::LINE_HEIGHT));
        let modifiers = event.modifiers;
        let actions = self.mouse_register.handle_scroll(
            f32::from(delta.x),
            f32::from(delta.y),
            f32::from(event.position.x),
            f32::from(event.position.y),
            modifiers.control || modifiers.platform,
            &resolved,
        );
        self.handle(actions, cx);
    }

    fn on_drop(&mut self, paths: &ExternalPaths, cx: &mut Context<Self>) {
        let paths = paths
            .paths()
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        self.handle(vec![Action::Core(Message::OpenFiles(paths)).into()], cx);
    }

    /// Asks the viewer to quit. It refuses while tracks have unsaved changes, and says so.
    pub fn request_quit(&mut self, cx: &mut Context<Self>) -> bool {
        if self.frame.is_none() {
            // Nothing is open yet, or the dataset failed to open.
            return true;
        }
        self.handle(vec![Action::Core(Message::Quit).into()], cx);
        false
    }
}

impl Render for App {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let size = window.viewport_size();
        let window_rect = Rect::new(0.0, 0.0, f32::from(size.width), f32::from(size.height));
        let palette: Palette = self.settings.palette;

        if let (Some(frame), Some(layout)) = (self.frame.clone(), &self.layout) {
            let resolved = layout.resolve(window_rect, &frame.track_labels);
            let width = resolved.track_width();
            self.resolved_layout = Some(resolved);
            if width != self.width_sent {
                self.width_sent = width;
                self.send(WorkerMessage::Resize(width), cx);
            }
            if let Ok(locus) = frame
                .viewport
                .focus
                .to_locus_str(&frame.state.contig_header)
            {
                window.set_window_title(&format!("TGV — {locus}"));
            }
        }

        let waiting = self
            .waiting_since
            .is_some_and(|since| since.elapsed() >= LOADING_NOTICE_DELAY);
        let paint = match (&self.frame, &self.resolved_layout, self.view()) {
            (Some(frame), Some(resolved), Some(view)) => Some((
                Arc::clone(frame),
                resolved.clone(),
                view,
                self.mouse_register.clone(),
                self.context_menu.clone(),
                self.popup.clone(),
                self.hover.clone(),
            )),
            _ => None,
        };
        let failure = self.failure.clone();
        let opening = self
            .settings
            .files
            .iter()
            .map(|file| file.to_string())
            .collect::<Vec<_>>()
            .join(", ");

        div()
            .size_full()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|app, event: &KeyDownEvent, window, cx| {
                app.on_key_down(event, window, cx)
            }))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|app, event: &MouseDownEvent, _, cx| {
                    app.on_mouse_down(MouseButton::Left, event, cx)
                }),
            )
            .on_mouse_down(
                gpui::MouseButton::Right,
                cx.listener(|app, event: &MouseDownEvent, _, cx| {
                    app.on_mouse_down(MouseButton::Right, event, cx)
                }),
            )
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(|app, event: &MouseUpEvent, _, cx| app.on_mouse_up(event, cx)),
            )
            .on_mouse_move(
                cx.listener(|app, event: &MouseMoveEvent, _, cx| app.on_mouse_move(event, cx)),
            )
            .on_scroll_wheel(
                cx.listener(|app, event: &ScrollWheelEvent, _, cx| app.on_scroll_wheel(event, cx)),
            )
            .on_drop(cx.listener(|app, paths: &ExternalPaths, _, cx| app.on_drop(paths, cx)))
            .child(
                canvas(
                    |_, _, _| {},
                    move |_, (), window, cx| {
                        let mut painter = Painter::new(window, cx);
                        let Some((frame, resolved, view, mouse_register, menu, popup, hover)) =
                            paint
                        else {
                            painter.fill(window_rect, palette.background.hsla());
                            let text = match failure {
                                Some(failure) => {
                                    format!("TGV couldn't open the dataset: {failure}")
                                }
                                None if opening.is_empty() => "Opening…".to_string(),
                                None => format!("Opening {opening}…"),
                            };
                            painter.text(16.0, 16.0, &text, rendering::colors::TEXT);
                            return;
                        };
                        let result = match frame.scene {
                            Scene::ContigList => rendering::render_contig_list(
                                &mut painter,
                                &resolved.window,
                                &frame.state,
                                &frame.registers,
                                &palette,
                            ),
                            Scene::Main => rendering::render_main(
                                &mut painter,
                                &frame,
                                &resolved,
                                &view,
                                &mouse_register,
                                &TrackFocus {
                                    target: frame.focused,
                                    name: frame.focus_name.clone(),
                                    modified: frame.focus_modified,
                                },
                                menu.as_ref(),
                                popup.as_ref(),
                                &Status {
                                    hover: hover.as_deref(),
                                    waiting,
                                },
                                &palette,
                            ),
                        };
                        if let Err(error) = result {
                            log::warn!("Failed to draw the window: {error}");
                        }
                    },
                )
                .size_full(),
            )
    }
}

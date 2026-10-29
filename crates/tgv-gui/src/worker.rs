//! The dataset worker: a thread that owns the viewer, so loads never block the window.
//!
//! The window sends [`WorkerMessage`]s. The worker applies every message already queued, loads
//! what the view needs, and sends back a [`Frame`]: a cheap snapshot of everything the window
//! draws. The worker also hosts the session, so agents reach it through the session socket
//! while the window keeps drawing.
//!
//! The terminal UI does the same work inline in its event loop. Repository readers aren't
//! `Send`, so the viewer is created on this thread and never leaves it.

use gv_core::prelude::*;
use gv_session::{SessionSocket, ViewerKind};
use gv_viewer::{
    keys::Key,
    message::{FocusTarget, Highlight},
    register::Registers,
    settings::Settings,
    viewer::{Effect, Scene, Viewer},
    viewport::{PixelScale, Viewport},
};
use std::{collections::HashMap, sync::Arc, thread::JoinHandle};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

/// Input for the viewer, from the window.
#[derive(Debug)]
pub enum WorkerMessage {
    /// Keys for the vim grammar and the command line.
    Keys(Vec<Key>),
    Actions(Vec<gv_viewer::message::Action>),
    /// Text pasted into the window.
    Paste(String),
    /// The track area's new width, in pixels.
    Resize(f64),
}

/// What the worker tells the window.
pub enum WorkerEvent {
    Frame(Arc<Frame>),
    /// The dataset couldn't be opened, so there is nothing to show.
    Failed(String),
}

/// Everything the window draws, as of `revision` applied messages.
pub struct Frame {
    /// How many messages the worker has applied. The window drops its prediction of the
    /// viewport once a frame covers every message it sent.
    pub revision: u64,
    pub state: State,
    pub tracks: Arc<TrackRegistry>,
    pub track_order: Vec<TrackId>,
    /// Sidebar labels by track: the file name, with `● ` before tracks with unsaved changes.
    pub track_labels: HashMap<TrackId, String>,
    pub viewport: Viewport<PixelScale>,
    pub registers: Registers,
    pub focused: FocusTarget,
    pub focus_name: String,
    pub focus_modified: bool,
    pub highlights: Vec<Highlight>,
    pub scene: Scene,
    /// What the window still has to do, in order, since the last frame.
    pub effects: Vec<Effect>,
    pub exit: bool,
}

impl Frame {
    fn new(viewer: &Viewer<PixelScale>, revision: u64, effects: Vec<Effect>) -> Self {
        let track_labels = viewer
            .dataset
            .tracks
            .entries
            .iter()
            .map(|entry| {
                let name = viewer.track_name(entry.id);
                let label = if viewer.dataset.is_modified(entry.id) {
                    format!("● {name}")
                } else {
                    name
                };
                (entry.id, label)
            })
            .collect();
        Self {
            revision,
            state: viewer.dataset.view.clone(),
            tracks: Arc::clone(&viewer.dataset.tracks),
            track_order: viewer.track_order.clone(),
            track_labels,
            viewport: viewer.viewport.clone(),
            registers: viewer.registers.clone(),
            focused: viewer.focused,
            focus_name: viewer.focus_name(),
            focus_modified: matches!(viewer.focused, FocusTarget::Track(id) if viewer.dataset.is_modified(id)),
            highlights: viewer.highlights.clone(),
            scene: viewer.scene.clone(),
            effects,
            exit: viewer.exit,
        }
    }
}

/// The window's end of the worker.
pub struct WorkerHandle {
    sender: UnboundedSender<WorkerMessage>,
    /// Messages sent so far. A frame whose revision reaches it covers all of them.
    pub sent: u64,
    pub thread: JoinHandle<()>,
}

impl WorkerHandle {
    /// Starts the worker on its own thread, and returns its handle and its events.
    pub fn spawn(settings: Settings) -> (Self, UnboundedReceiver<WorkerEvent>) {
        let (sender, inbox) = unbounded_channel();
        let (events, receiver) = unbounded_channel();
        let thread = std::thread::Builder::new()
            .name("tgv-gui dataset".to_string())
            .spawn(move || {
                // A multi-threaded runtime serves the session socket while the worker loads.
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .expect("the worker starts a Tokio runtime");
                runtime.block_on(run(settings, inbox, events));
            })
            .expect("the worker thread starts");
        (
            Self {
                sender,
                sent: 0,
                thread,
            },
            receiver,
        )
    }

    /// Sends a message to the viewer. Messages after the worker stopped are dropped.
    pub fn send(&mut self, message: WorkerMessage) {
        if self.sender.send(message).is_ok() {
            self.sent += 1;
        }
    }
}

/// What woke the worker.
enum Input {
    Message(Option<WorkerMessage>),
    Request(Option<gv_session::Request>),
}

async fn run(
    settings: Settings,
    mut inbox: UnboundedReceiver<WorkerMessage>,
    events: UnboundedSender<WorkerEvent>,
) {
    let mut viewer = match Viewer::<PixelScale>::new(settings, ViewerKind::Gui).await {
        Ok(viewer) => viewer,
        Err(error) => {
            log::error!("Failed to open the dataset: {error}");
            let _ = events.send(WorkerEvent::Failed(error.to_string()));
            return;
        }
    };
    // Agents reach the viewer through `tgv mcp`; without the socket, the viewer still works.
    let socket = SessionSocket::bind(viewer.session.clone())
        .inspect_err(|e| log::warn!("Agents can't connect to this viewer: {e}"))
        .ok();

    let mut revision = 0;
    let mut started = false;
    let publish = |viewer: &mut Viewer<PixelScale>, revision: u64| {
        let effects = viewer.take_effects();
        events
            .send(WorkerEvent::Frame(Arc::new(Frame::new(
                viewer, revision, effects,
            ))))
            .is_ok()
    };
    publish(&mut viewer, revision);

    while !viewer.exit {
        // The select ends before the event is handled, so handling can borrow the viewer.
        let event = tokio::select! {
            message = inbox.recv() => Input::Message(message),
            request = viewer.next_request() => Input::Request(request),
        };
        match event {
            Input::Message(None) | Input::Request(None) => break,
            Input::Message(Some(message)) => {
                // Apply everything already queued, so a burst of input loads once.
                let mut messages = vec![message];
                while let Ok(message) = inbox.try_recv() {
                    messages.push(message);
                }
                for message in messages {
                    revision += 1;
                    if let Err(error) = apply(&mut viewer, &mut started, message).await {
                        log::warn!("Error while handling input: {error}");
                        viewer.dataset.view.messages = vec![format!("{error}")];
                    }
                }
            }
            Input::Request(Some(request)) => {
                viewer.serve(request).await;
            }
        }
        if let Err(error) = viewer.self_correct() {
            log::warn!("Error while correcting the view: {error}");
        }
        if viewer.pending_open.is_some() {
            // The frame shows that the files are opening while this waits.
            if !publish(&mut viewer, revision) {
                break;
            }
            if let Err(error) = viewer.open_pending().await {
                log::warn!("Error while opening files: {error}");
                viewer.dataset.view.messages = vec![format!("{error}")];
            }
        }
        if !publish(&mut viewer, revision) {
            break;
        }
    }
    drop(socket);
    if let Err(error) = viewer.close().await {
        log::warn!("Error while closing the dataset: {error}");
    }
    log::info!("The dataset worker stopped");
}

/// Applies one message from the window.
async fn apply(
    viewer: &mut Viewer<PixelScale>,
    started: &mut bool,
    message: WorkerMessage,
) -> Result<(), TGVError> {
    match message {
        WorkerMessage::Keys(keys) => {
            for key in keys {
                let actions = viewer
                    .registers
                    .handle_key_event(key, &viewer.dataset.view)?;
                viewer.handle(actions).await?;
            }
        }
        WorkerMessage::Actions(actions) => {
            viewer.handle(actions).await?;
        }
        WorkerMessage::Paste(text) => {
            let actions = viewer.registers.handle_paste(&text);
            viewer.handle(actions).await?;
        }
        WorkerMessage::Resize(width) => {
            if *started {
                viewer.resize(width).await?;
            } else {
                // The startup locus needs the track width, so it waits for the first layout.
                viewer.width = width;
                viewer.start().await?;
                *started = true;
            }
        }
    }
    Ok(())
}

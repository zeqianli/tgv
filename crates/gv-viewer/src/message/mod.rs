//! Actions that change a viewer, whatever front end or agent they come from.

pub mod edit;
pub mod input;
pub mod interaction;
pub mod navigation;
pub mod session;
pub mod track;
pub mod ui;

pub use edit::{ColumnSpan, CursorAfter, Direction, EditAction, Motion, Operator};
use gv_core::message::Zoom;
pub use gv_core::message::{Movement, Scroll};
pub use input::{FocusMove, FocusTarget, InputAction};
pub use interaction::{Anchor, ContextMenuTarget, InteractionAction};
pub use navigation::NavigationAction;
pub use session::SessionAction;
use strum::Display;
pub use track::{AlignmentOptionUpdate, TrackAction};
pub use ui::{UiAction, UpdateLayoutAction};

/// A change to a viewer's view or input state, applied by `Viewer::handle`.
///
/// Key and mouse input produce actions, and so do agent view requests, so every change to what
/// the screen shows goes through one path. Actions that only a front end can apply, such as a
/// layout change, are [`UiAction`]s, which `Viewer::handle` hands back to the front end.
#[derive(Debug, Clone, Eq, PartialEq, Display)]
pub enum Action {
    Session(SessionAction),
    Message(String),
    Navigate(NavigationAction),
    Input(InputAction),
    Edit(EditAction),
    Track(TrackAction),
    Interaction(InteractionAction),
    /// An action that only the front end applies.
    Ui(UiAction),
}

impl Action {
    /// Wraps a status message for the message line.
    pub fn message(s: String) -> Self {
        Action::Message(s)
    }
}

impl From<gv_core::message::Movement> for Action {
    fn from(movement: gv_core::message::Movement) -> Self {
        Action::Navigate(NavigationAction::Move(movement))
    }
}

impl From<gv_core::message::Scroll> for Action {
    fn from(scroll: gv_core::message::Scroll) -> Self {
        Action::Navigate(NavigationAction::Scroll(scroll))
    }
}

impl From<Zoom> for Action {
    fn from(zoom: Zoom) -> Self {
        Action::Navigate(NavigationAction::Zoom(zoom))
    }
}

//! Front-end layout and widget actions.

use super::Action;
use gv_core::track_registry::TrackId;
use strum::Display;

/// An action that only a front end can apply, because it changes the front end's own layout or
/// widgets.
#[derive(Debug, Clone, Eq, PartialEq, Display)]
pub enum UiAction {
    UpdateLayout(UpdateLayoutAction),

    /// The open context menu changed its hover or submenu state.
    ContextMenuChanged,

    CloseContextMenu,

    ClosePopup,
}

/// Layout changes, in the front end's units: terminal cells, or pixels.
#[derive(Debug, Clone, Eq, PartialEq, Display)]
pub enum UpdateLayoutAction {
    ToggleSidebar,
    /// Moves the sidebar's right edge to an x position.
    SetSidebarWidth(u16),
    ResizeAlignmentPair {
        upper: TrackId,
        lower: TrackId,
        delta_rows: i32,
    },
}

impl From<UpdateLayoutAction> for Action {
    fn from(update: UpdateLayoutAction) -> Self {
        Action::Ui(UiAction::UpdateLayout(update))
    }
}

impl From<UiAction> for Action {
    fn from(action: UiAction) -> Self {
        Action::Ui(action)
    }
}

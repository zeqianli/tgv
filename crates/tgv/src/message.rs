use crate::{
    app::{Highlight, Scene},
    register::KeyRegisterType,
};
use gv_core::message::{AlignmentFilter, AlignmentSort};
pub use gv_core::message::{Movement, Scroll};
use gv_core::track_registry::TrackId;
use strum::Display;

/// A change to the TUI's view or input state, applied by `App::handle`.
///
/// Key and mouse input produce actions, and so do agent view requests, so every change to what
/// the screen shows goes through one path.
#[derive(Debug, Clone, Eq, PartialEq, Display)]
pub enum Action {
    Core(gv_core::message::Message),

    SwitchScene(Scene),

    SelectContig(usize),

    CommandChanged,

    SwitchKeyRegister(KeyRegisterType),

    UpdateLayout(UpdateLayoutAction),

    /// Returns to the view before the last jump.
    JumpBack,

    /// Returns to the view that the last `JumpBack` left.
    JumpForward,

    /// Makes the alignment track the target of later scrolls.
    FocusAlignment(TrackId),

    /// Changes one display option of an alignment track and keeps the others.
    UpdateAlignmentOptions {
        track: TrackId,
        update: AlignmentOptionUpdate,
    },

    /// Opens the context menu for `target`, anchored at a terminal cell.
    OpenContextMenu {
        target: ContextMenuTarget,
        column: u16,
        row: u16,
    },

    /// Shows the SAM record of a read in a popup.
    OpenReadDetails {
        track: TrackId,
        read_id: usize,
    },

    ClosePopup,

    /// The open context menu changed its hover or submenu state.
    ContextMenuChanged,

    CloseContextMenu,

    ClearAllKeyRegisters,

    /// Replaces the highlighted intervals.
    SetHighlights(Vec<Highlight>),

    ClearHighlights,
}

/// A change to one alignment track's display options.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum AlignmentOptionUpdate {
    /// Replaces the sort.
    Sort(AlignmentSort),
    /// Replaces the filter.
    Filter(AlignmentFilter),
    TogglePaired,
    Reset,
}

/// What a right click opened the context menu on.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ContextMenuTarget {
    /// An alignment track's reads or coverage. `position` is the clicked base, or `None` when
    /// a column covers several bases.
    Alignment {
        track: TrackId,
        position: Option<u64>,
    },
    Sidebar,
}

/// Layout changes.
#[derive(Debug, Clone, Eq, PartialEq, Display)]
pub enum UpdateLayoutAction {
    ToggleSidebar,
    SetSidebarWidth(u16),
    ResizeAlignmentPair {
        upper: TrackId,
        lower: TrackId,
        delta_rows: i32,
    },
}

impl Action {
    /// Wraps a status message for the message line.
    pub fn message(s: String) -> Self {
        Action::Core(gv_core::message::Message::Message(s))
    }
}

impl From<gv_core::message::Message> for Action {
    fn from(m: gv_core::message::Message) -> Self {
        Action::Core(m)
    }
}

impl From<gv_core::message::Movement> for Action {
    fn from(movement: gv_core::message::Movement) -> Self {
        gv_core::message::Message::Move(movement).into()
    }
}

impl From<gv_core::message::Scroll> for Action {
    fn from(scroll: gv_core::message::Scroll) -> Self {
        gv_core::message::Message::Scroll(scroll).into()
    }
}

use crate::{
    app::{Highlight, Scene},
    register::KeyRegisterType,
};
use gv_core::bed::BedEditKind;
use gv_core::message::{AlignmentFilter, AlignmentSort, WordMotion};
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

    /// Focuses the reference or a file track: what word motions, insert mode, and `:w` act on.
    Focus(FocusTarget),

    /// Moves the focus along the file tracks and the reference, in display order.
    MoveFocus(FocusMove),

    /// Scrolls one alignment track, whatever has focus.
    ScrollAlignment {
        track: TrackId,
        scroll: Scroll,
    },

    /// Moves the cursor. `App` resolves word motions against the focused track.
    Motion(Motion),

    /// Enters insert mode on the focused track. `append` first moves the cursor right by one
    /// column, like vim's `a`.
    EnterInsertMode {
        append: bool,
    },

    /// Adds or removes the bases of whole columns next to the cursor on a BED track.
    EditColumns {
        track: TrackId,
        edit: BedEditKind,
        span: ColumnSpan,
        cursor: CursorAfter,
        /// Whether the key is a typing key (`Space`, `Backspace`, or `Shift-Backspace`). A run of
        /// typing keys is one undo step, like a run of typed text in vim.
        typing: bool,
    },

    /// Applies an operator to the range between the cursor and the motion's target.
    Operate {
        track: TrackId,
        operator: Operator,
        motion: Motion,
    },

    /// Undoes a BED track's last edit step.
    Undo(TrackId),

    /// Redoes a BED track's last undone step.
    Redo(TrackId),

    /// Creates an untitled BED track above or below a track, and focuses it.
    NewTrackBeside {
        track: TrackId,
        below: bool,
    },

    /// Remembers a BED track for pasting. The features are read when it is pasted.
    CopyTrack(TrackId),

    /// Creates an untitled BED track below a track, with the copied track's features.
    PasteTrackBelow(TrackId),

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

    /// Removes a file track from the dataset and the layout. Without `force`, a track with
    /// unsaved changes stays.
    RemoveTrack {
        track: TrackId,
        force: bool,
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

/// What has the focus: the reference (the sequence and gene rows) or a file track.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FocusTarget {
    Reference,
    Track(TrackId),
}

/// A focus move along the file tracks and the reference, in display order.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FocusMove {
    /// Stops at the last target.
    Next(usize),
    /// Stops at the first target.
    Previous(usize),
    /// Wraps around to the first target.
    NextWrap,
    /// Wraps around to the last target.
    PreviousWrap,
    First,
    Last,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Direction {
    Left,
    Right,
}

/// A cursor motion, before `App` resolves it against the focus.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Motion {
    /// `h`, `l`, `H`, and `L`: whole columns, which are `zoom` bases each.
    Columns { direction: Direction, count: u64 },
    /// `w`, `b`, `e`, and `ge`, with `big` for their uppercase forms.
    Word {
        motion: WordMotion,
        big: bool,
        count: usize,
    },
}

impl Motion {
    /// Whether an operator includes the motion's target, as vim's `e`, `ge`, `E`, and `gE` do.
    pub fn inclusive(&self) -> bool {
        matches!(
            self,
            Motion::Word {
                motion: WordMotion::NextEnd | WordMotion::PreviousEnd,
                ..
            }
        )
    }
}

/// An operator that waits for a motion, such as `d` in `d3l`.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Operator {
    /// `d`: removes the bases of the range.
    Erase,
}

/// Whole columns next to the cursor.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ColumnSpan {
    /// The cursor column and the `n - 1` columns to its right.
    Right(u64),
    /// The `n` columns to the left of the cursor.
    Left(u64),
}

/// Where the cursor goes after an edit of a column span.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum CursorAfter {
    /// The first column of the span. For a span to the right, the cursor doesn't move.
    SpanStart,
    /// The column after the span, as typing advances the cursor.
    PastSpan,
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
    /// The sidebar, and the file track whose section was clicked, if any.
    Sidebar { track: Option<TrackId> },
    /// A BED or variant track's row.
    Track { track: TrackId },
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

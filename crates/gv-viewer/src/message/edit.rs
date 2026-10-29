//! Cursor motions and edits to BED tracks.

use gv_core::{bed::BedEditKind, message::WordMotion, track_registry::TrackId};
use strum::Display;

/// Cursor motions and BED track edits.
#[derive(Debug, Clone, Eq, PartialEq, Display)]
pub enum EditAction {
    /// Moves the cursor. `App` resolves word motions against the focused track.
    Motion(Motion),

    /// Enters insert mode on the focused track. `append` first moves the cursor right by one
    /// column, like vim's `a`.
    EnterInsertMode { append: bool },

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
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Direction {
    Left,
    Right,
}

/// A cursor motion, before `Viewer` resolves it against the focus.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Motion {
    /// `h`, `l`, `H`, and `L`: whole columns, which `Scale::column_bases` turns into bases.
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

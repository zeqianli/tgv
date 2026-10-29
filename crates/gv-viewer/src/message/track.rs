//! File track operations and display options.

use gv_core::{
    message::{AlignmentDisplayOption, AlignmentFilter, AlignmentSort},
    track_registry::TrackId,
};
use strum::Display;

/// Changes to file tracks and their display options.
#[derive(Debug, Clone, Eq, PartialEq, Display)]
pub enum TrackAction {
    NewBedTrack,
    SetAlignmentOption(Vec<AlignmentDisplayOption>),
    OpenFiles(Vec<String>),
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

    /// Adds file tracks from these paths at a position in the display order, as a drop between
    /// two tracks does. Position 0 is above the first track.
    OpenFilesAt {
        paths: Vec<String>,
        position: usize,
    },

    /// Removes a file track from the dataset and the layout. Without `force`, a track with
    /// unsaved changes stays.
    RemoveTrack {
        track: TrackId,
        force: bool,
    },
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

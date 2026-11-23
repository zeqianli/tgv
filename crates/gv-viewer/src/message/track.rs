use crate::{message::FocusTarget, viewport::RowLayout};
use gv_core::{
    message::{AlignmentDisplayOption, AlignmentFilter, AlignmentSort},
    track_registry::TrackId,
};
use strum::Display;

#[derive(Debug, Clone, Eq, PartialEq, Display)]
pub enum TrackAction {
    NewBedTrack,
    SetAlignmentOption(Vec<AlignmentDisplayOption>),
    OpenFiles(Vec<String>),
    NewTrackBeside {
        track: TrackId,
        below: bool,
    },

    CopyTrack(TrackId),

    PasteTrackBelow(TrackId),

    UpdateAlignmentOptions {
        track: TrackId,
        update: AlignmentOptionUpdate,
    },

    OpenFilesAt {
        paths: Vec<String>,
        position: usize,
    },

    SetRowLayout {
        target: FocusTarget,
        rows: RowLayout,
    },

    RemoveTrack {
        track: TrackId,
        force: bool,
    },
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum AlignmentOptionUpdate {
    Sort(AlignmentSort),
    Filter(AlignmentFilter),
    TogglePaired,
    Reset,
}

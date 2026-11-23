use crate::message::FocusTarget;
use gv_core::{prelude::*, reference::Reference};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AreaType {
    Cytoband,
    Coordinate,
    Coverage(TrackId),
    Alignment(TrackId),
    Divider(FocusTarget),
    Sequence,
    GeneTrack,
    Console,
    Error,
    Variant(TrackId),
    Bed(TrackId),
    Fill,
}

impl AreaType {
    /// The areas for a reference and file tracks in display order: the cytoband and the ruler,
    /// each file track, the reference rows, and then the console and the message line.
    pub fn for_tracks(
        reference: &Reference,
        track_registry: &TrackRegistry,
        track_order: &[TrackId],
    ) -> Vec<AreaType> {
        let mut areas = vec![];
        if reference.needs_track() {
            areas.push(AreaType::Cytoband);
        }

        areas.push(AreaType::Coordinate);

        let mut has_alignment = false;
        for &id in track_order {
            match track_registry.get(id).repository_index {
                RepositoryFileIndex::Alignment(_) => {
                    areas.push(AreaType::Coverage(id));
                    areas.push(AreaType::Alignment(id));
                    has_alignment = true;
                }
                RepositoryFileIndex::Variant(_) => areas.push(AreaType::Variant(id)),
                RepositoryFileIndex::Bed(_) => areas.push(AreaType::Bed(id)),
            }
            areas.push(AreaType::Divider(FocusTarget::Track(id)));
        }

        if !has_alignment {
            areas.push(AreaType::Fill);
        }
        if reference.needs_sequence() {
            areas.push(AreaType::Sequence);
        }
        if reference.needs_track() {
            areas.push(AreaType::GeneTrack);
            areas.push(AreaType::Divider(FocusTarget::Reference));
        }

        areas.push(AreaType::Console);
        areas.push(AreaType::Error);
        areas
    }
}

impl AreaType {
    pub fn focus_target(&self) -> Option<FocusTarget> {
        match self {
            AreaType::Coverage(id)
            | AreaType::Alignment(id)
            | AreaType::Variant(id)
            | AreaType::Bed(id) => Some(FocusTarget::Track(*id)),
            AreaType::Sequence | AreaType::GeneTrack => Some(FocusTarget::Reference),
            _ => None,
        }
    }

    pub fn rows_target(&self) -> Option<FocusTarget> {
        match self {
            AreaType::Alignment(id) | AreaType::Variant(id) | AreaType::Bed(id) => {
                Some(FocusTarget::Track(*id))
            }
            AreaType::GeneTrack => Some(FocusTarget::Reference),
            _ => None,
        }
    }
}

/// After event handling, which areas needs re-rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderEvent {
    Area(AreaType),
    Sidebar,

    /// All track areas.
    AllTracks,

    /// All track areas and the sidebar
    All,
}

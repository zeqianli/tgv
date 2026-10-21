//! The areas a viewer shows, from top to bottom, and which of them need redrawing.
//!
//! Both front ends show the same areas in the same order. Each front end decides how tall they
//! are and where they go on its screen.

use gv_core::{prelude::*, reference::Reference};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AreaType {
    Cytoband,
    Coordinate,
    Coverage(TrackId),
    Alignment(TrackId),
    AlignmentDivider { upper: TrackId, lower: TrackId },
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

        let mut last_alignment_id = None;
        for &id in track_order {
            match track_registry.get(id).repository_index {
                RepositoryFileIndex::Alignment(_) => {
                    if let Some(upper) = last_alignment_id {
                        areas.push(AreaType::AlignmentDivider { upper, lower: id });
                    }
                    areas.push(AreaType::Coverage(id));
                    areas.push(AreaType::Alignment(id));
                    last_alignment_id = Some(id);
                }
                RepositoryFileIndex::Variant(_) => areas.push(AreaType::Variant(id)),
                RepositoryFileIndex::Bed(_) => areas.push(AreaType::Bed(id)),
            }
        }

        if last_alignment_id.is_none() {
            areas.push(AreaType::Fill);
        }
        if reference.needs_sequence() {
            areas.push(AreaType::Sequence);
        }
        if reference.needs_track() {
            areas.push(AreaType::GeneTrack);
        }

        areas.push(AreaType::Console);
        areas.push(AreaType::Error);
        areas
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

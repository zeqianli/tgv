use crate::{
    message::FocusTarget,
    viewport::{RowLayout, Scale, Viewport},
};
use gv_core::prelude::*;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrackRows {
    pub layout: RowLayout,
    pub depth: usize,
}

impl TrackRows {
    pub fn units(&self, cap: usize) -> usize {
        match self.layout {
            RowLayout::Collapsed => 1,
            RowLayout::Expanded => self.depth.clamp(1, cap),
        }
    }
}

pub fn depth(
    state: &State,
    tracks: &TrackRegistry,
    target: FocusTarget,
) -> Result<usize, crate::Error> {
    Ok(match target {
        FocusTarget::Reference => state.track.depth()?,
        FocusTarget::Track(id) => match tracks.get(id).repository_index {
            RepositoryFileIndex::Alignment(index) => match &state.paired_alignments[index] {
                Some(paired) => paired.depth()?,
                None => state.alignments[index].depth()?,
            },
            RepositoryFileIndex::Variant(index) => state.variants[index].depth()?,
            RepositoryFileIndex::Bed(index) => state.bed_intervals[index].depth()?,
        },
    })
}

pub fn track_rows<Z: Scale>(
    state: &State,
    tracks: &TrackRegistry,
    viewport: &Viewport<Z>,
) -> Result<HashMap<FocusTarget, TrackRows>, crate::Error> {
    viewport
        .tracks
        .iter()
        .map(|(&target, view)| {
            Ok((
                target,
                TrackRows {
                    layout: view.rows,
                    depth: depth(state, tracks, target)?,
                },
            ))
        })
        .collect()
}

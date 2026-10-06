//! Stable file-track identities and their repository indexes, shared by all front ends.

use crate::{error::TGVError, repository::RepositoryFileIndex};

pub type TrackId = usize;

#[derive(Debug)]
pub struct TrackEntry {
    pub id: TrackId,
    pub repository_index: RepositoryFileIndex,
}

#[derive(Debug, Default)]
pub struct TrackRegistry {
    pub entries: Vec<TrackEntry>,
}

impl TrackRegistry {
    pub fn new(indexes: &[RepositoryFileIndex]) -> Self {
        let entries = indexes
            .iter()
            .copied()
            .enumerate()
            .map(|(id, repository_index)| TrackEntry {
                id,
                repository_index,
            })
            .collect();
        Self { entries }
    }

    pub fn get(&self, id: TrackId) -> &TrackEntry {
        &self.entries[id]
    }

    pub fn alignment_id(&self, index: usize) -> TrackId {
        self.entries
            .iter()
            .find(|entry| entry.repository_index == RepositoryFileIndex::Alignment(index))
            .map(|entry| entry.id)
            .expect("an alignment index has a TUI track")
    }

    pub fn alignment_index(&self, id: TrackId) -> Result<usize, TGVError> {
        match self.get(id).repository_index {
            RepositoryFileIndex::Alignment(index) => Ok(index),
            _ => Err(TGVError::StateError(format!(
                "Track {id} is not an alignment track."
            ))),
        }
    }

    pub fn variant_index(&self, id: TrackId) -> Result<usize, TGVError> {
        match self.get(id).repository_index {
            RepositoryFileIndex::Variant(index) => Ok(index),
            _ => Err(TGVError::StateError(format!(
                "Track {id} is not a variant track."
            ))),
        }
    }

    pub fn bed_index(&self, id: TrackId) -> Result<usize, TGVError> {
        match self.get(id).repository_index {
            RepositoryFileIndex::Bed(index) => Ok(index),
            _ => Err(TGVError::StateError(format!(
                "Track {id} is not a BED track."
            ))),
        }
    }
}

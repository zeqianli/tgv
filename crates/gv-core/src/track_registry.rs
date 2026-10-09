//! Stable file-track identities and their repository indexes, shared by all front ends.
//!
//! Track IDs are never reused, so front ends and agents can keep referring to a track after
//! others are removed. Entries stay in dataset order, which is also ascending ID order.

use crate::{error::TGVError, repository::RepositoryFileIndex, settings::FilePath};

pub type TrackId = usize;

#[derive(Debug, Clone)]
pub struct TrackEntry {
    pub id: TrackId,
    pub repository_index: RepositoryFileIndex,
    /// The file the track shows.
    pub file_path: FilePath,
}

#[derive(Debug, Default, Clone)]
pub struct TrackRegistry {
    pub entries: Vec<TrackEntry>,
    next_id: TrackId,
}

impl TrackRegistry {
    /// The entry's position in dataset order, if the track exists.
    fn position(&self, id: TrackId) -> Option<usize> {
        self.entries
            .binary_search_by_key(&id, |entry| entry.id)
            .ok()
    }

    pub fn contains(&self, id: TrackId) -> bool {
        self.position(id).is_some()
    }

    pub fn get(&self, id: TrackId) -> &TrackEntry {
        &self.entries[self
            .position(id)
            .expect("a track ID names a registered track")]
    }

    /// Appends a track with a fresh ID. IDs only increase, so removed IDs are never reused.
    pub fn push(&mut self, repository_index: RepositoryFileIndex, file_path: FilePath) -> TrackId {
        let id = self.next_id;
        self.next_id += 1;
        self.entries.push(TrackEntry {
            id,
            repository_index,
            file_path,
        });
        id
    }

    /// Removes a track. The other tracks keep their IDs; only the repository indexes of later
    /// tracks of the same kind shift down, as removing the track from the per-kind vectors does.
    pub fn remove(&mut self, id: TrackId) -> Option<TrackEntry> {
        let removed = self.entries.remove(self.position(id)?);
        for entry in &mut self.entries {
            entry.repository_index = entry
                .repository_index
                .after_removal(removed.repository_index)
                .expect("only the removed entry has the removed index");
        }
        Some(removed)
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

#[cfg(test)]
mod tests {
    use super::*;
    use RepositoryFileIndex::{Alignment, Bed, Variant};
    use rstest::rstest;

    fn registry(indexes: &[RepositoryFileIndex]) -> TrackRegistry {
        let mut registry = TrackRegistry::default();
        for &index in indexes {
            let file_path = match index {
                Alignment(i) => FilePath::AlignmentPath(crate::settings::AlignmentPath::Bam {
                    path: format!("{i}.bam"),
                    index: format!("{i}.bam.bai"),
                    source: crate::settings::BamSource::Local,
                }),
                Variant(i) => FilePath::VariantPath(format!("{i}.vcf")),
                Bed(i) => FilePath::BedPath(format!("{i}.bed")),
            };
            registry.push(index, file_path);
        }
        registry
    }

    fn indexes(registry: &TrackRegistry) -> Vec<(TrackId, RepositoryFileIndex)> {
        registry
            .entries
            .iter()
            .map(|entry| (entry.id, entry.repository_index))
            .collect()
    }

    #[rstest]
    #[case::first_alignment(0, vec![(1, Variant(0)), (2, Alignment(0)), (3, Bed(0)), (4, Alignment(1))])]
    #[case::middle_alignment(2, vec![(0, Alignment(0)), (1, Variant(0)), (3, Bed(0)), (4, Alignment(1))])]
    #[case::variant(1, vec![(0, Alignment(0)), (2, Alignment(1)), (3, Bed(0)), (4, Alignment(2))])]
    #[case::last(4, vec![(0, Alignment(0)), (1, Variant(0)), (2, Alignment(1)), (3, Bed(0))])]
    fn remove_shifts_later_tracks_of_the_same_kind(
        #[case] id: TrackId,
        #[case] expected: Vec<(TrackId, RepositoryFileIndex)>,
    ) {
        let mut registry =
            registry(&[Alignment(0), Variant(0), Alignment(1), Bed(0), Alignment(2)]);
        assert_eq!(registry.remove(id).map(|entry| entry.id), Some(id));
        assert_eq!(indexes(&registry), expected);
        assert!(!registry.contains(id));
    }

    #[test]
    fn push_never_reuses_ids() {
        let mut registry = registry(&[Alignment(0), Bed(0)]);
        registry.remove(1);
        assert_eq!(
            registry.push(Bed(0), FilePath::BedPath("2.bed".to_owned())),
            2
        );
        assert!(registry.remove(7).is_none());
        assert_eq!(indexes(&registry), vec![(0, Alignment(0)), (2, Bed(0))]);
        assert_eq!(registry.get(2).repository_index, Bed(0));
    }
}

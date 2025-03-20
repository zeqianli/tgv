use crate::models::{
    alignment::Alignment,
    message::DataMessage,
    region::Region,
    sequence::Sequence,
    services::{sequences::SequenceService, tracks::TrackService},
    track::Track,
};
use crate::settings::Settings;
use std::io;

/// Holds all data in the session.
pub struct Data {
    /// Alignment segments.
    pub alignment: Option<Alignment>,
    pub bam_path: String,

    /// Tracks.
    pub track: Option<Track>,
    pub track_service: TrackService,

    /// Sequences.
    pub sequence: Option<Sequence>,
    pub sequence_service: SequenceService,
    // TODO: in the first implementation, refresh all data when the viewing window is near the boundary.
}

impl Data {
    pub async fn new(settings: &Settings) -> Self {
        let bam_path = settings.bam_path.clone().unwrap();
        let sequence_service = SequenceService::new(settings.reference.clone()).unwrap();

        Self {
            alignment: None,
            track: None,
            sequence: None,

            bam_path,
            track_service: TrackService::new(settings.reference.clone()).await.unwrap(),
            sequence_service,
        }
    }

    // TODO: async
    pub async fn has_complete_data_or_load(
        &mut self,
        data_message: DataMessage,
    ) -> io::Result<bool> {
        let mut loaded_data = false;

        match data_message {
            DataMessage::RequiresCompleteAlignments(region) => {
                if !self.has_complete_alignment(&region) {
                    let cache_region = Self::cache_region(region); // TODO: calculated three times. Not efficient.
                    self.alignment = Some(
                        Alignment::from_bam_path(self.bam_path.clone(), cache_region).unwrap(),
                    );
                    loaded_data = true;
                }
            }
            DataMessage::RequiresCompleteFeatures(region) => {
                if !self.has_complete_track(&region) {
                    let cache_region = Self::cache_region(region);
                    self.track = Some(
                        self.track_service
                            .query_feature_track(&cache_region)
                            .await
                            .unwrap(),
                    );
                    loaded_data = true;
                }
            }
            DataMessage::RequiresCompleteSequences(region) => {
                if !self.has_complete_sequence(&region) {
                    let cache_region = Self::cache_region(region);
                    match self.sequence_service.query_sequence(&cache_region).await {
                        Ok(sequence) => {
                            self.sequence = Some(sequence);
                            loaded_data = true;
                        }
                        Err(e) => {
                            eprintln!("Failed to query sequence: {}", e);
                            return Err(io::Error::new(io::ErrorKind::Other, e.to_string()));
                        }
                    }
                }
            }
        }

        Ok(loaded_data)
    }

    pub async fn load_all_data(&mut self, region: Region) -> io::Result<bool> {
        let loaded_alignment = self
            .has_complete_data_or_load(DataMessage::RequiresCompleteAlignments(region.clone()))
            .await
            .unwrap();
        let loaded_track = self
            .has_complete_data_or_load(DataMessage::RequiresCompleteFeatures(region.clone()))
            .await
            .unwrap();
        let loaded_sequence = self
            .has_complete_data_or_load(DataMessage::RequiresCompleteSequences(region.clone()))
            .await
            .unwrap();
        Ok(loaded_alignment || loaded_track || loaded_sequence)
    }

    pub fn has_complete_alignment(&self, region: &Region) -> bool {
        self.alignment.is_some() && self.alignment.as_ref().unwrap().has_complete_data(region)
    }

    pub fn has_complete_track(&self, region: &Region) -> bool {
        self.track.is_some() && self.track.as_ref().unwrap().has_complete_data(region)
    }

    pub fn has_complete_sequence(&self, region: &Region) -> bool {
        self.sequence.is_some() && self.sequence.as_ref().unwrap().has_complete_data(region)
    }

    const DATA_CACHE_RATIO: usize = 3;

    fn cache_region(region: Region) -> Region {
        let left = region
            .start
            .saturating_sub(Data::DATA_CACHE_RATIO * region.width() / 2)
            .max(1);
        let right = region
            .end
            .saturating_add(Data::DATA_CACHE_RATIO * region.width() / 2)
            .min(usize::MAX);
        Region {
            contig: region.contig.clone(),
            start: left,
            end: right,
        }
    }
}

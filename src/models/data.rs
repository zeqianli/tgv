use crate::error::TGVError;
use crate::helpers::is_url;
use crate::models::{
    alignment::Alignment,
    message::DataMessage,
    region::Region,
    sequence::Sequence,
    services::{
        sequences::SequenceService,
        tracks::{TrackService, UcscApiTrackService, UcscDbTrackService},
    },
    track::{
        feature::{Gene, SubGeneFeature},
        track::Track,
    },
};
use crate::settings::Settings;
use std::path::Path;
/// Holds all data in the session.
pub struct Data {
    /// Alignment segments.
    pub alignment: Option<Alignment>,
    pub bam_path: Option<String>,
    pub bai_path: Option<String>,

    /// Tracks.
    pub track: Option<Track<Gene>>,
    pub track_service: Option<UcscApiTrackService>,

    /// Sequences.
    pub sequence: Option<Sequence>,
    pub sequence_service: Option<SequenceService>,
    // TODO: in the first implementation, refresh all data when the viewing window is near the boundary.
}

impl Data {
    pub async fn new(settings: &Settings) -> Result<Self, TGVError> {
        let bam_path = match settings.bam_path.clone() {
            Some(bam_path) => {
                if !is_url(&bam_path) {
                    if !Path::new(&bam_path).exists() {
                        return Err(TGVError::IOError(format!(
                            "BAM file {} not found",
                            bam_path
                        )));
                    }
                    match settings.bai_path.clone() {
                        Some(bai_path) => {
                            if !Path::new(&bai_path).exists() {
                                return Err(TGVError::IOError(format!(
                                "BAM index file {} not found. Only indexed BAM files are supported.",
                                bai_path
                            )));
                            }
                        }
                        None => {
                            if !Path::new(&format!("{}.bai", bam_path)).exists() {
                                return Err(TGVError::IOError(format!(
                                "BAM index file {}.bai not found. Only indexed BAM files are supported.",
                                bam_path
                            )));
                            }
                        }
                    }
                }
                Some(bam_path)
            }
            None => None,
        };

        let (track_service, sequence_service) = match settings.reference.as_ref() {
            Some(reference) => (
                Some(UcscApiTrackService::new(reference.clone()).unwrap()),
                Some(SequenceService::new(reference.clone()).unwrap()),
            ),
            None => (None, None),
        };

        Ok(Self {
            alignment: None,
            bam_path,
            bai_path: settings.bai_path.clone(),
            track: None,
            track_service,
            sequence: None,
            sequence_service,
        })
    }

    pub async fn close(&mut self) -> Result<(), TGVError> {
        if self.track_service.is_some() {
            self.track_service.as_ref().unwrap().close().await?;
        }
        if self.sequence_service.is_some() {
            self.sequence_service.as_ref().unwrap().close().await?;
        }
        Ok(())
    }

    pub async fn handle_data_messages(
        &mut self,
        data_messages: Vec<DataMessage>,
    ) -> Result<bool, TGVError> {
        let mut loaded_data = false;
        for data_message in data_messages {
            loaded_data = self.handle_data_message(data_message).await?;
        }
        Ok(loaded_data)
    }

    // TODO: async
    pub async fn handle_data_message(
        &mut self,
        data_message: DataMessage,
    ) -> Result<bool, TGVError> {
        let mut loaded_data = false;

        match data_message {
            DataMessage::RequiresCompleteAlignments(region) => {
                if self.bam_path.is_none() {
                    return Err(TGVError::IOError("BAM file not found".to_string()));
                }

                let bam_path = self.bam_path.as_ref().unwrap();

                if !self.has_complete_alignment(&region) {
                    self.alignment = Some(
                        Alignment::from_bam_path(bam_path, self.bai_path.as_ref(), &region)
                            .unwrap(),
                    );
                    loaded_data = true;
                }
            }
            DataMessage::RequiresCompleteFeatures(region) => {
                if self.track_service.is_none() {
                    return Err(TGVError::IOError("Track service not found".to_string()));
                }
                let track_service = self.track_service.as_ref().unwrap();

                if !self.has_complete_track(&region) {
                    self.track = Some(track_service.query_gene_track(&region).await.unwrap());
                    loaded_data = true;
                }
            }
            DataMessage::RequiresCompleteSequences(region) => {
                if self.sequence_service.is_none() {
                    return Err(TGVError::IOError("Sequence service not found".to_string()));
                }
                let sequence_service = self.sequence_service.as_ref().unwrap();

                if !self.has_complete_sequence(&region) {
                    match sequence_service.query_sequence(&region).await {
                        Ok(sequence) => {
                            self.sequence = Some(sequence);
                            loaded_data = true;
                        }
                        Err(_) => {
                            return Err(TGVError::IOError("Sequence service error".to_string()));
                        }
                    }
                }
            }
        }

        Ok(loaded_data)
    }

    pub async fn load_all_data(&mut self, region: Region) -> Result<bool, TGVError> {
        let loaded_alignment = self
            .handle_data_message(DataMessage::RequiresCompleteAlignments(region.clone()))
            .await?;
        let loaded_track = self
            .handle_data_message(DataMessage::RequiresCompleteFeatures(region.clone()))
            .await?;
        let loaded_sequence = self
            .handle_data_message(DataMessage::RequiresCompleteSequences(region.clone()))
            .await?;
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
}

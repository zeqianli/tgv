use crate::error::TGVError;
use crate::helpers::is_url;
use crate::repository::{AlignmentRepository, AlignmentRepositoryEnum};
use crate::models::{
    alignment::Alignment,
    contig_collection::ContigCollection,
    cytoband::Cytoband,
    message::DataMessage,
    reference::Reference,
    region::Region,
    sequence::Sequence,
    services::{
        sequences::SequenceService,
        track_service::{
            TrackCache, TrackService, TrackServiceEnum, UcscApiTrackService, UcscDbTrackService,
        },
    },
    track::{feature::Gene, track::Track},
};

use crate::settings::{BackendType, Settings};
/// Holds all data in the session.
pub struct Data {
    /// Alignment segments.
    pub alignment: Option<Alignment>,
    pub alignment_repository: AlignmentRepositoryEnum,

    /// Tracks.
    pub track: Option<Track<Gene>>,
    pub track_cache: TrackCache,
    pub track_service: Option<TrackServiceEnum>,

    /// Sequences.
    pub sequence: Option<Sequence>,
    pub sequence_service: Option<SequenceService>,

    // TODO: in the first implementation, refresh all data when the viewing window is near the boundary.
    /// Contigs in the BAM header
    pub contigs: ContigCollection,
}

impl Data {
    pub async fn new(settings: &Settings) -> Result<Self, TGVError> {

        let alignment_repository = AlignmentRepositoryEnum::from(settings)?;

        let (track_service, sequence_service): (Option<TrackServiceEnum>, Option<SequenceService>) =
            match settings.reference.as_ref() {
                Some(reference) => {
                    let ts = match settings.backend {
                        BackendType::Api => TrackServiceEnum::Api(UcscApiTrackService::new()?),
                        BackendType::Db => {
                            TrackServiceEnum::Db(UcscDbTrackService::new(reference).await?)
                        }
                    };
                    let ss = SequenceService::new(reference.clone())?;
                    (Some(ts), Some(ss))
                }
                None => (None, None),
            };

        let contigs = load_contig_data(
            settings.reference.as_ref(),
            track_service.as_ref(),
            &alignment_repository
        )
        .await?;

        Ok(Self {
            alignment: None,
            alignment_repository,
            track: None,
            track_cache: TrackCache::new(),
            track_service,
            sequence: None,
            sequence_service,
            contigs,
        })
    }

    pub async fn close(&mut self) -> Result<(), TGVError> {
        if let Some(ts) = self.track_service.as_mut() {
            ts.close().await?;
        }
        if let Some(ss) = self.sequence_service.as_mut() {
            ss.close().await?;
        }
        Ok(())
    }

    pub async fn handle_data_messages(
        &mut self,
        reference: Option<&Reference>, // TODO: improve this.
        data_messages: Vec<DataMessage>,
    ) -> Result<bool, TGVError> {
        let mut loaded_data = false;
        for data_message in data_messages {
            loaded_data = self.handle_data_message(reference, data_message).await?;
        }
        Ok(loaded_data)
    }

    // TODO: async
    pub async fn handle_data_message(
        &mut self,
        reference: Option<&Reference>, // TODO: improve this.
        data_message: DataMessage,
    ) -> Result<bool, TGVError> {
        let mut loaded_data = false;

        match data_message {
            DataMessage::RequiresCompleteAlignments(region) => {

                if !self.has_complete_alignment(&region) {
                    self.alignment =Some(self.alignment_repository.read_alignment(&region)?); // TODO
                    loaded_data = true;
                }
            }
            DataMessage::RequiresCompleteFeatures(region) => {
                let has_complete_track = self.has_complete_track(&region);
                if let (Some(reference), Some(track_service)) =
                    (reference, self.track_service.as_mut())
                {
                    if !has_complete_track {
                        if let Ok(track) = track_service
                            .query_gene_track(reference, &region, &mut self.track_cache)
                            .await
                        {
                            self.track = Some(track);
                            loaded_data = true;
                        } else {
                            // Do nothing (track not found). TODO: fix this shit properly.
                        }
                    }
                } else if reference.is_none() {
                    // No reference provided, cannot load features
                } else {
                    return Err(TGVError::StateError(
                        "Track service not initialized".to_string(),
                    ));
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

            DataMessage::RequiresCytobands(contig) => {
                if self.contigs.cytoband_is_loaded(&contig)? {
                    return Ok(false);
                }

                if let (Some(reference), Some(track_service)) =
                    (reference, self.track_service.as_ref())
                {
                    let cytoband = track_service.get_cytoband(reference, &contig).await?;
                    self.contigs.update_cytoband(&contig, cytoband);
                    loaded_data = true;
                } else if reference.is_none() {
                    // Cannot load cytobands without reference
                } else {
                    // track service not available
                }
            }
        }

        Ok(loaded_data)
    }

    pub async fn load_all_data(
        &mut self,
        reference: Option<&Reference>, // TODO: improve this.
        region: Region,
    ) -> Result<bool, TGVError> {
        let loaded_alignment = self
            .handle_data_message(
                reference,
                DataMessage::RequiresCompleteAlignments(region.clone()),
            )
            .await?;
        let loaded_track = self
            .handle_data_message(
                reference,
                DataMessage::RequiresCompleteFeatures(region.clone()),
            )
            .await?;
        let loaded_sequence = self
            .handle_data_message(
                reference,
                DataMessage::RequiresCompleteSequences(region.clone()),
            )
            .await?;
        Ok(loaded_alignment || loaded_track || loaded_sequence)
    }

    pub fn has_complete_alignment(&self, region: &Region) -> bool {
        self.alignment.is_some() && self.alignment.as_ref().unwrap().has_complete_data(region)
    }

    pub fn has_complete_track(&self, region: &Region) -> bool {
        // self.track_cache.get_track(region.contig()) == Some(None)
        self.track.is_some() && self.track.as_ref().unwrap().has_complete_data(region)
    }

    pub fn has_complete_sequence(&self, region: &Region) -> bool {
        self.sequence.is_some() && self.sequence.as_ref().unwrap().has_complete_data(region)
    }

    pub fn track_checked(&self) -> Result<&Track<Gene>, TGVError> {
        self.track.as_ref().ok_or(TGVError::StateError("Track is not initialized".to_string()))
    }
}

/// Contig data looading


pub async fn load_contig_data(
    reference: Option<&Reference>,
    track_service: Option<&TrackServiceEnum>,
    repository: &AlignmentRepositoryEnum,
) -> Result<ContigCollection, TGVError> {
    let mut contig_data = ContigCollection::new(reference.cloned());

    if let (Some(reference), Some(track_service)) = (reference, track_service) {
        for (contig, length) in track_service.get_all_contigs(reference).await? {
            contig_data
                .update_or_add_contig(contig, Some(length))
                .unwrap();
        }
    }

    if let Some(reference) = reference {
        match &reference {
            Reference::Hg19 | Reference::Hg38 => {
                for cytoband in Cytoband::from_human_reference(reference)?.iter() {
                    contig_data.update_cytoband(&cytoband.contig, Some(cytoband.clone()));
                }
            }
            _ => {}
        }
    }

    if  !matches!(repository, AlignmentRepositoryEnum::None) {
        contig_data
            .update_from_bam(reference, repository)
            .unwrap();
    }

    Ok(contig_data)
}

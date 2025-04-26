use crate::error::TGVError;
use crate::helpers::is_url;
use crate::models::{
    alignment::Alignment,
    contig::Contig,
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
    track::{
        feature::{Gene, SubGeneFeature},
        track::Track,
    },
};
use crate::settings::{BackendType, Settings};
use std::path::Path;
/// Holds all data in the session.
pub struct Data {
    /// Alignment segments.
    pub alignment: Option<Alignment>,
    pub bam_path: Option<String>,
    pub bai_path: Option<String>,

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

        let contigs = Data::load_contig_data(
            settings.reference.as_ref(),
            track_service.as_ref(),
            settings.bam_path.as_ref(),
            settings.bai_path.as_ref(),
        )
        .await?;

        Ok(Self {
            alignment: None,
            bam_path,
            bai_path: settings.bai_path.clone(),
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
                let has_complete_track = self.has_complete_track(&region);
                if let (Some(reference), Some(track_service)) =
                    (reference, self.track_service.as_mut())
                {
                    if !has_complete_track {
                        match track_service {
                            TrackServiceEnum::Api(service) => {
                                service
                                    .check_or_load_contig(reference, &region.contig)
                                    .await?;
                            }
                            _ => {}
                        }
                        self.track = Some(track_service.query_gene_track(&region).await?);
                        loaded_data = true;
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
        self.track.is_some() && self.track.as_ref().unwrap().has_complete_data(region)
    }

    pub fn has_complete_sequence(&self, region: &Region) -> bool {
        self.sequence.is_some() && self.sequence.as_ref().unwrap().has_complete_data(region)
    }
}

/// Contig data looading

impl Data {
    pub async fn load_contig_data(
        reference: Option<&Reference>,
        track_service: Option<&TrackServiceEnum>,
        bam_path: Option<&String>,
        bai_path: Option<&String>,
    ) -> Result<ContigCollection, TGVError> {
        let mut contig_data = ContigCollection::new(reference.cloned());

        if let (Some(reference), Some(track_service)) = (reference, track_service) {
            for (contig, length) in track_service.get_all_contigs(reference).await? {
                contig_data
                    .update_or_add_contig(contig, Some(length))
                    .unwrap();
            }
        }

        match reference {
            Some(reference) => match &reference {
                Reference::Hg19 | Reference::Hg38 => {
                    for cytoband in Cytoband::from_human_reference(&reference)?.iter() {
                        contig_data.update_cytoband(&cytoband.contig, Some(cytoband.clone()));
                    }
                }
                _ => {}
            },
            _ => {}
        }

        if let Some(bam_path) = bam_path {
            contig_data
                .update_from_bam(bam_path, bai_path, reference)
                .unwrap();
        }

        Ok(contig_data)
    }
}

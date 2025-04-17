use crate::{
    error::TGVError,
    models::{
        contig::Contig,
        reference::Reference,
        track::{Feature, Gene, Track},
        services::{
            tracks::TrackService
        }
    },
};
use std::collections::HashMap;

use reqwest;
use serde_json;

pub struct UcscApiTrackService {
    reference: Reference,
    cached_tracks: HashMap<String, Track>,
    
    gene_name_lookup: HashMap<String, (String, usize)>, // gene_name -> (contig.full_name(), start)
}

impl UcscApiTrackService {
    pub fn new(reference: Reference) -> Self {
        Self {
            reference,
            cached_tracks: HashMap::new(),
            gene_name_lookup: HashMap::new(),
        }
    }


    /// Check if the contig's track is already cached. If not, load it.
    /// Return true if loading is performed.
    pub async fn check_or_load_contig(&mut self, contig: &Contig) -> Result<bool, TGVError> {
        if self.cached_tracks.contains_key(&contig.full_name()) {
            return Ok(false);
        }

        let track = self.query_feature_track(contig).await?;
        self.cached_tracks
            .insert(contig.full_name().to_string(), track);

        // Populate gene_name_lookup
        for gene in track.genes {
            self.gene_name_lookup
                .insert(gene.name.clone(), (contig.full_name().to_string(), gene.transcription_start));
        }
    
        Ok(true)
    }

    async fn get_prefered_track(&self) -> Result<String, TGVError> {
        match self.reference.clone() {
            Reference::Hg19 | Reference::Hg38 => Ok("ncbiRefSeqSelect".to_string()),
            Reference::UcscGenome(genome) => {
                let query_url =
                    format!("https://api.genome.ucsc.edu/list/tracks?genome={}", genome);
                let response = reqwest::get(query_url)
                    .await?
                    .json::<serde_json::Value>()
                    .await?;
                let allowed_keys = response
                    .get(genome)
                    .ok_or(TGVError::IOError(
                        "Failed to get genome from UCSC API".to_string(),
                    ))?
                    .as_object()
                    .ok_or(TGVError::IOError(
                        "Failed to get genome from UCSC API".to_string(),
                    ))?
                    .keys();

                let mut prefered_track = None;

                for track in allowed_keys {
                    if track == "ncbiRefSeqSelect" {
                        prefered_track = Some(track.clone());
                        break;
                    } else if track == "ncbiRefSeqCurated" {
                        prefered_track = Some(track.clone());
                    } else if track == "ncbiRefSeq"
                        && prefered_track != Some("ncbiRefSeqCurated".to_string())
                    {
                        prefered_track = Some(track.clone());
                    } else if track == "refGene"
                        && (prefered_track != Some("ncbiRefSeqCurated".to_string())
                            && prefered_track != Some("ncbiRefSeq".to_string()))
                    {
                        prefered_track = Some(track.clone());
                    }
                }
                Ok(prefered_track
                    .ok_or(TGVError::IOError(
                        "Failed to get prefered track from UCSC API".to_string(),
                    ))?
                    .clone())
            }
            _ => Err(TGVError::IOError(
                "Failed to get prefered track from UCSC API".to_string(),
            )),
        }
    }
}

impl TrackService for UcscApiTrackService {
    async fn query_feature_track(&self, contig: &Contig) -> Result<Track, TGVError> {
        let track = self.query_feature_track(contig).await?;
        Ok(track)
    }

    async fn 
}

use crate::models::services::tracks::helpers::get_prefered_track;
use crate::{
    error::TGVError,
    models::{
        contig::Contig,
        reference::Reference,
        track::{Feature, Gene, Track},
    },
};
use std::collections::HashMap;

use reqwest;
use serde_json;

pub struct UcscApiTrackService {
    reference: Reference,
    cached_tracks: HashMap<String, Track>,
}

impl UcscApiTrackService {
    pub fn new(reference: Reference) -> Self {
        Self {
            reference,
            cached_tracks: HashMap::new(),
        }
    }

    pub async fn load_contig(&mut self, contig: &Contig) -> Result<(), TGVError> {
        if self.cached_tracks.contains_key(&contig.full_name()) {
            return Ok(());
        }

        let track = self.query_feature_track(contig).await?;
        self.cached_tracks
            .insert(contig.full_name().to_string(), track);
        Ok(())
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
                    } else if (track == "ncbiRefSeqCurated") {
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

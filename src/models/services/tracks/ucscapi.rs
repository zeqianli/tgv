use crate::{
    error::TGVError,
    models::{
        contig::Contig,
        cytoband::{Cytoband, CytobandSegment},
        reference::Reference,
        region::Region,
        services::tracks::TrackService,
        strand::Strand,
        track::{
            feature::{Gene, SubGeneFeature},
            track::Track,
        },
    },
    traits::GenomeInterval,
};
use std::collections::HashMap;

use reqwest::{Client, StatusCode};
use serde::de::Error as _;
use serde_json;

// TODO: improved pattern:
// Service doesn't save anything. No reference, no cache.
// Ask these things to be passed in. And return them to store in the state.

pub struct UcscApiTrackService {
    cached_tracks: HashMap<String, Track<Gene>>,
    gene_name_lookup: HashMap<String, (Contig, usize)>, // gene_name -> (contig.full_name(), start)

    client: Client,
}

impl UcscApiTrackService {
    pub fn new() -> Result<Self, TGVError> {
        Ok(Self {
            cached_tracks: HashMap::new(),
            gene_name_lookup: HashMap::new(),
            client: Client::new(),
        })
    }

    /// Check if the contig's track is already cached. If not, load it.
    /// Return true if loading is performed.
    pub async fn check_or_load_contig(
        &mut self,
        reference: &Reference,
        contig: &Contig,
    ) -> Result<bool, TGVError> {
        if self.cached_tracks.contains_key(&contig.full_name()) {
            return Ok(false);
        }

        let preferred_track = self.get_prefered_track_name(reference).await?;
        let query_url = self.get_track_data_url(reference, contig, preferred_track.clone())?;

        let response = self.client.get(query_url).send().await?.text().await?;

        let mut value: serde_json::Value = serde_json::from_str(&response)?;

        // Extract the genes array from the response
        let genes_value = value[preferred_track].take();
        let mut genes: Vec<Gene> = serde_json::from_value(genes_value)?;
        for gene in genes.iter_mut() {
            gene.contig = contig.clone();
        }

        let track = Track::from_genes(genes, contig.clone())?;

        for (i, gene) in track.genes().iter().enumerate() {
            self.gene_name_lookup
                .insert(gene.name.clone(), (contig.clone(), i));
        }

        self.cached_tracks.insert(contig.full_name(), track);

        Ok(true)
    }

    /// Load all contigs until the gene is found.
    pub async fn check_or_load_gene(
        &mut self,
        reference: &Reference,
        gene_name: &String,
    ) -> Result<bool, TGVError> {
        if self.gene_name_lookup.contains_key(gene_name) {
            return Ok(false);
        }

        for (contig, _) in self.get_all_contigs(reference).await?.iter() {
            self.check_or_load_contig(reference, contig).await?;
            if self.gene_name_lookup.contains_key(gene_name) {
                return Ok(true);
            }
        }

        Err(TGVError::IOError(format!(
            "Gene {} not found in contigs",
            gene_name
        )))
    }

    /// Return

    const CYTOBAND_TRACK: &str = "cytoBandIdeo";

    fn get_track_data_url(
        &self,
        reference: &Reference,
        contig: &Contig,
        track_name: String,
    ) -> Result<String, TGVError> {
        match reference {
            Reference::Hg19 | Reference::Hg38 | Reference::UcscGenome(_) => Ok(format!(
                "https://api.genome.ucsc.edu/getData/track?genome={}&track={}&chrom={}",
                reference.to_string(),
                track_name,
                contig.full_name()
            )),
            _ => Err(TGVError::IOError("Unsupported reference".to_string())),
        }
    }
}
/// Get the preferred track name recursively.
fn get_prefered_track_name(
    key: &str,
    content: &serde_json::Value,
    preferred_track_name: Option<String>,
    is_top_level: bool,
) -> Result<Option<String>, TGVError> {
    let mut prefered_track_name = preferred_track_name;

    let err = TGVError::IOError(format!("Failed to get genome from UCSC API"));

    if content.get("compositeContainer").is_some() || is_top_level {
        // do this recursively
        for (track_name, track_content) in content.as_object().ok_or(err)?.iter() {
            if track_content.is_object() {
                prefered_track_name =
                    get_prefered_track_name(track_name, track_content, prefered_track_name, false)?;
            }
        }
    } else {
        if prefered_track_name == Some("ncbiRefSeqSelect".to_string()) {
        } else if key == "ncbiRefSeqSelect".to_string() {
            prefered_track_name = Some(key.to_string());
        } else if key == "ncbiRefSeqCurated".to_string() {
            prefered_track_name = Some(key.to_string());
        } else if *key == "ncbiRefSeq".to_string()
            && prefered_track_name != Some("ncbiRefSeqCurated".to_string())
        {
            prefered_track_name = Some(key.to_string());
        } else if *key == "refGene".to_string()
            && (prefered_track_name != Some("ncbiRefSeqCurated".to_string())
                && prefered_track_name != Some("ncbiRefSeq".to_string()))
        {
            prefered_track_name = Some(key.to_string());
        }
    }

    Ok(prefered_track_name)
}

impl TrackService for UcscApiTrackService {
    async fn close(&self) -> Result<(), TGVError> {
        // reqwest client dones't need closing
        Ok(())
    }

    async fn get_all_contigs(
        &self,
        reference: &Reference,
    ) -> Result<Vec<(Contig, usize)>, TGVError> {
        match reference {
            Reference::Hg19 | Reference::Hg38 => reference.contigs_and_lengths(), // TODO: query
            Reference::UcscGenome(genome) => {
                let query_url = format!(
                    "https://api.genome.ucsc.edu/list/chromosomes?genome={}",
                    genome
                );

                let response = self.client.get(query_url).send().await?.text().await?;

                let err =
                    TGVError::IOError(format!("Failed to deserialize chromosomes for {}", genome));

                // schema: {..., "chromosomes": [{"__name__", len}]}

                let value: serde_json::Value = serde_json::from_str(&response)?;

                let mut output = Vec::new();
                for (k, v) in value["chromosomes"].as_object().ok_or(err)?.iter() {
                    // TODO: save length
                    output.push((Contig::chrom(k), v.as_u64().unwrap() as usize));
                }

                Ok(output)
            }
            _ => Err(TGVError::IOError("Unsupported reference".to_string())),
        }
    }

    async fn get_cytoband(
        &self,
        reference: &Reference,
        contig: &Contig,
    ) -> Result<Option<Cytoband>, TGVError> {
        let query_url = format!(
            "https://api.genome.ucsc.edu/getData/track?genome={}&track={}&chrom={}",
            reference.to_string(),
            Self::CYTOBAND_TRACK,
            contig.full_name()
        );

        let response = self.client.get(query_url).send().await?;

        if response.status() != StatusCode::OK {
            return Ok(None); // Some genome doesn't have cytobands
        }

        let response_text = response.text().await?;
        let value: serde_json::Value =
            serde_json::from_str(&response_text).map_err(TGVError::JsonSerializationError)?;

        // Extract the array of segments from the "cytoBandIdeo" field
        let segments_value = value.get(Self::CYTOBAND_TRACK).ok_or_else(|| {
            TGVError::JsonSerializationError(serde_json::Error::custom(format!(
                "Missing '{}' field in UCSC API response",
                Self::CYTOBAND_TRACK
            )))
        })?;

        // Deserialize the segments array
        let segments: Vec<CytobandSegment> = serde_json::from_value(segments_value.clone())
            .map_err(TGVError::JsonSerializationError)?;

        if segments.is_empty() {
            return Ok(None);
        }

        // Construct the *single* Cytoband object for this contig
        let cytoband = Cytoband {
            reference: Some(reference.clone()),
            contig: contig.clone(),
            segments,
        };

        // Return the single Cytoband wrapped in Option
        Ok(Some(cytoband))
    }

    async fn get_prefered_track_name(&self, reference: &Reference) -> Result<String, TGVError> {
        match reference.clone() {
            Reference::Hg19 | Reference::Hg38 => Ok("ncbiRefSeqSelect".to_string()),
            Reference::UcscGenome(genome) => {
                let query_url =
                    format!("https://api.genome.ucsc.edu/list/tracks?genome={}", genome);
                let response = reqwest::get(query_url)
                    .await?
                    .json::<serde_json::Value>()
                    .await?;
                // Schema:
                // {
                //     "genome": {
                //         "track1":{
                //             "compositeContainer": "TRUE",
                //             "track2": {...}
                //             ...
                //         },
                //         "track3": {...},
                //     }
                // }
                // (Composite tracks can be nested.)

                let prefered_track = get_prefered_track_name(
                    genome.as_str(),
                    response
                        .get(genome.clone())
                        .ok_or(TGVError::IOError(format!(
                            "Failed to get genome from UCSC API"
                        )))?,
                    None,
                    true,
                )?;

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

    async fn query_genes_overlapping(&self, region: &Region) -> Result<Vec<Gene>, TGVError> {
        if let Some(track) = self.cached_tracks.get(&region.contig().full_name()) {
            Ok(track
                .get_features_overlapping(region)
                .iter()
                .map(|g| (*g).clone())
                .collect())
        } else {
            Err(TGVError::IOError(format!(
                "Track not found for contig {}",
                region.contig().full_name()
            )))
        }
    }

    async fn query_gene_covering(
        &self,
        contig: &Contig,
        position: usize,
    ) -> Result<Option<Gene>, TGVError> {
        if let Some(track) = self.cached_tracks.get(&contig.full_name()) {
            Ok(track.get_gene_at(position).map(|g| (*g).clone()))
        } else {
            Err(TGVError::IOError(format!(
                "Track not found for contig {}",
                contig.full_name()
            )))
        }
    }

    async fn query_gene_name(&self, name: &String) -> Result<Gene, TGVError> {
        if let Some((contig, gene_index)) = self.gene_name_lookup.get(name) {
            return Ok(self
                .cached_tracks
                .get(&contig.full_name())
                .unwrap() // should never error out
                .genes()[*gene_index]
                .clone());
        } else {
            Err(TGVError::IOError("Gene not found".to_string()))
        }
    }

    async fn query_k_genes_after(
        &self,
        contig: &Contig,
        position: usize,
        k: usize,
    ) -> Result<Gene, TGVError> {
        if let Some(track) = self.cached_tracks.get(&contig.full_name()) {
            Ok(track
                .get_saturating_k_genes_after(position, k)
                .ok_or(TGVError::IOError("No genes found".to_string()))?
                .clone())
        } else {
            Err(TGVError::IOError(format!(
                "Track not found for contig {}",
                contig.full_name()
            )))
        }
    }

    async fn query_k_genes_before(
        &self,
        contig: &Contig,
        position: usize,
        k: usize,
    ) -> Result<Gene, TGVError> {
        if let Some(track) = self.cached_tracks.get(&contig.full_name()) {
            Ok(track
                .get_saturating_k_genes_before(position, k)
                .ok_or(TGVError::IOError("No genes found".to_string()))?
                .clone())
        } else {
            Err(TGVError::IOError(format!(
                "Track not found for contig {}",
                contig.full_name()
            )))
        }
    }

    async fn query_k_exons_after(
        &self,
        contig: &Contig,
        position: usize,
        k: usize,
    ) -> Result<SubGeneFeature, TGVError> {
        if let Some(track) = self.cached_tracks.get(&contig.full_name()) {
            Ok(track
                .get_saturating_k_exons_after(position, k)
                .ok_or(TGVError::IOError("No exons found".to_string()))?)
        } else {
            Err(TGVError::IOError(format!(
                "Track not found for contig {}",
                contig.full_name()
            )))
        }
    }

    async fn query_k_exons_before(
        &self,
        contig: &Contig,
        position: usize,
        k: usize,
    ) -> Result<SubGeneFeature, TGVError> {
        if let Some(track) = self.cached_tracks.get(&contig.full_name()) {
            Ok(track
                .get_saturating_k_exons_before(position, k)
                .ok_or(TGVError::IOError("No exons found".to_string()))?)
        } else {
            Err(TGVError::IOError(format!(
                "Track not found for contig {}",
                contig.full_name()
            )))
        }
    }
}

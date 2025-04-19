use crate::{
    error::TGVError,
    models::{
        contig::Contig,
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

use reqwest::Client;
use serde_json;

// TODO: improved pattern:
// Service doesn't save anything. No reference, no cache.
// Ask these things to be passed in. And return them to store in the state.

pub struct UcscApiTrackService {
    reference: Reference,
    cached_tracks: HashMap<String, Track<Gene>>,
    gene_name_lookup: HashMap<String, (Contig, usize)>, // gene_name -> (contig.full_name(), start)

    client: Client,
}

impl UcscApiTrackService {
    pub fn new(reference: Reference) -> Result<Self, TGVError> {
        Ok(Self {
            reference,
            cached_tracks: HashMap::new(),
            gene_name_lookup: HashMap::new(),
            client: Client::new(),
        })
    }

    /// Check if the contig's track is already cached. If not, load it.
    /// Return true if loading is performed.
    pub async fn check_or_load_contig(&mut self, contig: &Contig) -> Result<bool, TGVError> {
        if self.cached_tracks.contains_key(&contig.full_name()) {
            return Ok(false);
        }

        let preferred_track = self.get_prefered_track_name().await?;
        let query_url = self.get_track_data_url(contig, preferred_track.clone())?;

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
    pub async fn check_or_load_gene(&mut self, gene_name: &String) -> Result<bool, TGVError> {
        if self.gene_name_lookup.contains_key(gene_name) {
            return Ok(false);
        }

        for (contig, _) in self.get_all_contigs().await?.iter() {
            self.check_or_load_contig(contig).await?;
            if self.gene_name_lookup.contains_key(gene_name) {
                return Ok(true);
            }
        }

        Err(TGVError::IOError(format!(
            "Gene {} not found in contigs",
            gene_name
        )))
    }

    pub async fn get_all_contigs(&self) -> Result<Vec<(Contig, usize)>, TGVError> {
        match &self.reference {
            Reference::Hg19 | Reference::Hg38 => self.reference.contigs_and_lengths(), // TODO: query
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

    fn get_track_data_url(&self, contig: &Contig, track_name: String) -> Result<String, TGVError> {
        match &self.reference {
            Reference::Hg19 | Reference::Hg38 | Reference::UcscGenome(_) => Ok(format!(
                "https://api.genome.ucsc.edu/getData/track?genome={}&track={}&chrom={}",
                self.reference.to_string(),
                track_name,
                contig.full_name()
            )),
            _ => Err(TGVError::IOError("Unsupported reference".to_string())),
        }
    }

    async fn get_prefered_track_name(&self) -> Result<String, TGVError> {
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
    async fn close(&self) -> Result<(), TGVError> {
        // reqwest client dones't need closing
        Ok(())
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

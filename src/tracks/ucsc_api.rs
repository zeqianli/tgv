use crate::cytoband;
use crate::tracks::{TrackCache, TrackService, TRACK_PREFERENCES};
use crate::{
    contig_collection::{Contig, ContigHeader},
    cytoband::{Cytoband, CytobandSegment},
    error::TGVError,
    feature::{Gene, SubGeneFeature},
    intervals::GenomeInterval,
    reference::Reference,
    region::Region,
    track::Track,
    tracks::schema::*,
};
use async_trait::async_trait;
use reqwest::Client;

// TODO: improved pattern:
// Service doesn't save anything. No reference, no cache.
// Ask these things to be passed in. And return them to store in the state.

#[derive(Debug)]
pub struct UcscApiTrackService {
    client: Client,
}

impl UcscApiTrackService {
    pub fn new() -> Result<Self, TGVError> {
        Ok(Self {
            client: Client::new(),
        })
    }

    /// Query the API to download the gene track data for a contig.
    pub async fn query_track_by_contig(
        &self,
        reference: &Reference,
        contig: &Contig,
        cache: &mut TrackCache,
    ) -> Result<Track<Gene>, TGVError> {
        let preferred_track = match cache.get_preferred_track_name() {
            None => {
                {
                    let preferred_track = self
                        .get_preferred_track_name(reference, cache)
                        .await?
                        .ok_or(TGVError::IOError(format!(
                            "Failed to get prefered track for {} from UCSC API",
                            contig.name
                        )))?; // TODO: proper handling

                    cache.set_preferred_track_name(Some(preferred_track));
                    preferred_track
                }
            }

            Some(preferred_track) => preferred_track.ok_or(TGVError::IOError(format!(
                "Failed to get prefered track for {} from UCSC API",
                contig.name
            )))?,
        };

        let query_url = match reference {
            Reference::Hg19 | Reference::Hg38 | Reference::UcscGenome(_) => format!(
                "https://api.genome.ucsc.edu/getData/track?genome={}&track={}&chrom={}",
                reference.to_string(),
                preferred_track,
                contig.name
            ),
            Reference::UcscAccession(genome) => {
                let hub_url = cache.hub_url.clone().unwrap_or({
                    let hub_url = self.get_hub_url_for_genark_accession(genome).await?;
                    cache.hub_url = Some(hub_url.clone());
                    hub_url
                });
                format!(
                    "https://api.genome.ucsc.edu/getData/track?hubUrl={}&genome={}&track={}&chrom={}",
                    hub_url, genome, preferred_track, contig.name
                )
            }
        };

        let response: UcscApiListGeneResponse =
            self.client.get(query_url).send().await?.json().await?;

        response.to_track(preferred_track)
    }

    pub async fn get_hub_url_for_genark_accession(
        &self,
        accession: &str,
    ) -> Result<String, TGVError> {
        let response = self
            .client
            .get(format!(
                "https://api.genome.ucsc.edu/list/genarkGenomes?genome={}",
                accession
            ))
            .send()
            .await?
            .json::<UcscApiHubUrlResponse>()
            .await?;

        response.get_hub_url(accession)
    }
}

#[async_trait]
impl TrackService for UcscApiTrackService {
    async fn close(&self) -> Result<(), TGVError> {
        // reqwest client dones't need closing
        Ok(())
    }

    async fn get_all_contigs(
        &self,
        reference: &Reference,
        track_cache: &mut TrackCache,
    ) -> Result<Vec<Contig>, TGVError> {
        let query_url = match reference {
            Reference::Hg19 | Reference::Hg38 | Reference::UcscGenome(_) => {
                format!(
                    "https://api.genome.ucsc.edu/list/chromosomes?genome={}",
                    reference.to_string()
                )
            }
            Reference::UcscAccession(genome) => {
                let hub_url = track_cache.hub_url.clone().unwrap_or({
                    let hub_url = self.get_hub_url_for_genark_accession(genome).await?;
                    track_cache.hub_url = Some(hub_url.clone());
                    hub_url
                });

                format!(
                    "https://api.genome.ucsc.edu/list/chromosomes?hubUrl={};genome={}",
                    hub_url, genome
                )
            }
        };

        let response = self
            .client
            .get(query_url)
            .send()
            .await?
            .json::<UcscListChromosomeResponse>()
            .await?;

        let mut output = Vec::new();
        for (name_string, length) in response.chromosomes.into_iter() {
            // TODO: save length
            output.push(Contig::new(&name_string, Some(length)));
        }

        output.sort_by(|a, b| {
            if a.name.starts_with("chr") || b.name.starts_with("chr") {
                Contig::contigs_compare(a, b)
            } else {
                b.length.cmp(&a.length) // Sort by length in descending order
            }
        });

        Ok(output)
    }

    async fn get_cytoband(
        &self,
        reference: &Reference,
        contig: &Contig,
        cache: &mut TrackCache,
        contig_header: &ContigHeader,
    ) -> Result<Option<Cytoband>, TGVError> {
        let query_url = match reference {
            Reference::Hg19 | Reference::Hg38 | Reference::UcscGenome(_) => format!(
                "https://api.genome.ucsc.edu/getData/track?genome={}&track=cytoBandIdeo&chrom={}",
                reference.to_string(),
                contig.name
            ),
            Reference::UcscAccession(genome) => {
                if cache.hub_url.is_none() {
                    let hub_url = self.get_hub_url_for_genark_accession(genome).await?;
                    cache.hub_url = Some(hub_url);
                }
                let hub_url = cache.hub_url.as_ref().unwrap();
                format!(
                    "https://api.genome.ucsc.edu/getData/track?hubUrl={}&genome={}&track=cytoBandIdeo&chrom={}",
                    hub_url, genome, contig.name
                )
            }
        };

        let response: UcscApiCytobandResponse = self
            .client
            .get(query_url)
            .send()
            .await?
            .json()
            .await
            .unwrap_or_default();

        response.to_cytoband(reference, contig, contig_header)
    }

    async fn get_preferred_track_name(
        &self,
        reference: &Reference,
        cache: &mut TrackCache,
    ) -> Result<Option<String>, TGVError> {
        let query_url = match reference {
            Reference::Hg19 | Reference::Hg38 | Reference::UcscGenome(_) => format!(
                "https://api.genome.ucsc.edu/list/tracks?trackLeavesOnly=1;genome={}",
                reference.to_string(),
            ),
            Reference::UcscAccession(genome) => {
                if cache.hub_url.is_none() {
                    let hub_url = self.get_hub_url_for_genark_accession(genome).await?;
                    cache.hub_url = Some(hub_url);
                }
                let hub_url = cache.hub_url.as_ref().unwrap();
                format!(
                    "https://api.genome.ucsc.edu/list/tracks?trackLeavesOnly=1;hubUrl={};genome={}",
                    hub_url, genome
                )
            }
        };
        match reference.clone() {
            Reference::Hg19 | Reference::Hg38 => Ok(Some("ncbiRefSeqSelect".to_string())),
            reference => {
                let response = reqwest::get(query_url)
                    .await?
                    .json::<serde_json::Value>()
                    .await?;

                let track_names = response
                    .get(reference.to_string())
                    .ok_or(TGVError::IOError(
                        "Failed to get genome from UCSC API".to_string(),
                    ))?
                    .as_object()
                    .ok_or(TGVError::IOError(
                        "Failed to get genome from UCSC API".to_string(),
                    ))?
                    .keys()
                    .cloned()
                    .collect::<Vec<String>>();

                for pref in TRACK_PREFERENCES {
                    if track_names.contains(&pref.to_string()) {
                        return Ok(Some(pref.to_string()));
                    }
                }

                Ok(None)
            }
        }
    }

    async fn query_genes_overlapping(
        &self,
        reference: &Reference,
        region: &Region,
        cache: &mut TrackCache,
    ) -> Result<Vec<Gene>, TGVError> {
        if !cache.includes_contig(region.contig()) {
            let track = self
                .query_track_by_contig(reference, region.contig(), cache)
                .await?;
            cache.add_track(region.contig(), Some(track));
        }
        // TODO: now I don't really handle empty query results

        if let Some(Some(track)) = cache.get_track(region.contig()) {
            Ok(track
                .get_features_overlapping(region)
                .iter()
                .map(|g| (*g).clone())
                .collect())
        } else {
            Err(TGVError::IOError(format!(
                "Track not found for contig {}",
                region.contig().name
            )))
        }
    }

    async fn query_gene_covering(
        &self,
        reference: &Reference,
        contig: &Contig,
        position: usize,
        cache: &mut TrackCache,
    ) -> Result<Option<Gene>, TGVError> {
        if !cache.includes_contig(contig) {
            let track = self.query_track_by_contig(reference, contig, cache).await?;
            cache.add_track(contig, Some(track));
        }
        if let Some(Some(track)) = cache.get_track(contig) {
            Ok(track.get_gene_at(position).map(|g| (*g).clone()))
        } else {
            Err(TGVError::IOError(format!(
                "Track not found for contig {}",
                contig.name
            )))
        }
    }

    async fn query_gene_name(
        &self,
        reference: &Reference,
        name: &String,
        cache: &mut TrackCache,
    ) -> Result<Gene, TGVError> {
        if !cache.includes_gene(name) {
            // query all possible tracks until the gene is found
            for contig in self.get_all_contigs(reference, cache).await? {
                let track = self
                    .query_track_by_contig(reference, &contig, cache)
                    .await?;
                cache.add_track(&contig, Some(track));

                if let Some(Some(gene)) = cache.get_gene(name) {
                    break;
                }
            }
        }

        if let Some(Some(gene)) = cache.get_gene(name) {
            Ok(gene.clone())
        } else {
            Err(TGVError::IOError(format!("Gene {} not found", name)))
        }
    }

    async fn query_k_genes_after(
        &self,
        reference: &Reference,
        contig: &Contig,
        coord: usize,
        k: usize,
        cache: &mut TrackCache,
    ) -> Result<Gene, TGVError> {
        if !cache.includes_contig(contig) {
            let track = self.query_track_by_contig(reference, contig, cache).await?;
            cache.add_track(contig, Some(track));
        }

        if let Some(Some(track)) = cache.get_track(contig) {
            Ok(track
                .get_saturating_k_genes_after(coord, k)
                .ok_or(TGVError::IOError("No genes found".to_string()))?
                .clone())
        } else {
            Err(TGVError::IOError(format!(
                "Track not found for contig {}",
                contig.name
            )))
        }
    }

    async fn query_k_genes_before(
        &self,
        reference: &Reference,
        contig: &Contig,
        coord: usize,
        k: usize,
        cache: &mut TrackCache,
    ) -> Result<Gene, TGVError> {
        if !cache.includes_contig(contig) {
            let track = self.query_track_by_contig(reference, contig, cache).await?;
            cache.add_track(contig, Some(track));
        }
        if let Some(Some(track)) = cache.get_track(contig) {
            Ok(track
                .get_saturating_k_genes_before(coord, k)
                .ok_or(TGVError::IOError("No genes found".to_string()))?
                .clone())
        } else {
            Err(TGVError::IOError(format!(
                "Track not found for contig {}",
                contig.name
            )))
        }
    }

    async fn query_k_exons_after(
        &self,
        reference: &Reference,
        contig: &Contig,
        coord: usize,
        k: usize,
        cache: &mut TrackCache,
    ) -> Result<SubGeneFeature, TGVError> {
        if !cache.includes_contig(contig) {
            let track = self.query_track_by_contig(reference, contig, cache).await?;
            cache.add_track(contig, Some(track));
        }
        if let Some(Some(track)) = cache.get_track(contig) {
            Ok(track
                .get_saturating_k_exons_after(coord, k)
                .ok_or(TGVError::IOError("No exons found".to_string()))?)
        } else {
            Err(TGVError::IOError(format!(
                "Track not found for contig {}",
                contig.name
            )))
        }
    }

    async fn query_k_exons_before(
        &self,
        reference: &Reference,
        contig: &Contig,
        coord: usize,
        k: usize,
        cache: &mut TrackCache,
    ) -> Result<SubGeneFeature, TGVError> {
        if !cache.includes_contig(contig) {
            let track = self.query_track_by_contig(reference, contig, cache).await?;
            cache.add_track(contig, Some(track));
        }
        if let Some(Some(track)) = cache.get_track(contig) {
            Ok(track
                .get_saturating_k_exons_before(coord, k)
                .ok_or(TGVError::IOError("No exons found".to_string()))?)
        } else {
            Err(TGVError::IOError(format!(
                "Track not found for contig {}",
                contig.name
            )))
        }
    }
}

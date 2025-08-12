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
    pub async fn query_track_if_not_cached(
        &self,
        reference: &Reference,
        contig_index: usize,
        cache: &mut TrackCache,
        contig_header: &ContigHeader,
    ) -> Result<(), TGVError> {
        if cache.contig_quried(&contig_index) {
            return Ok(());
        }

        let contig_name = contig_header.get_name(contig_index)?;

        let preferred_track = match &cache.preferred_track_name {
            None => {
                {
                    let preferred_track = self
                        .get_preferred_track_name(reference, cache)
                        .await?
                        .ok_or(TGVError::IOError(format!(
                            "Failed to get prefered track for {} from UCSC API",
                            contig_name
                        )))?; // TODO: proper handling

                    cache.set_preferred_track_name(Some(preferred_track.clone()));
                    preferred_track
                }
            }

            Some(preferred_track) => preferred_track.clone().ok_or(TGVError::IOError(format!(
                "Failed to get prefered track for {} from UCSC API",
                contig_name
            )))?,
        };

        let query_url = match reference {
            Reference::Hg19 | Reference::Hg38 | Reference::UcscGenome(_) => format!(
                "https://api.genome.ucsc.edu/getData/track?genome={}&track={}&chrom={}",
                reference.to_string(),
                preferred_track,
                contig_name
            ),
            Reference::UcscAccession(genome) => {
                let hub_url = cache.hub_url.clone().unwrap_or({
                    let hub_url = self.get_hub_url_for_genark_accession(genome).await?;
                    cache.hub_url = Some(hub_url.clone());
                    hub_url
                });
                format!(
                    "https://api.genome.ucsc.edu/getData/track?hubUrl={}&genome={}&track={}&chrom={}",
                    hub_url, genome, preferred_track, contig_name
                )
            }
        };

        let response: UcscApiListGeneResponse =
            self.client.get(query_url).send().await?.json().await?;

        cache.add_track(contig_index, response.to_track(&preferred_track)?);

        Ok(())
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
        contig_index: usize,
        cache: &mut TrackCache,
        contig_header: &ContigHeader,
    ) -> Result<Option<Cytoband>, TGVError> {
        let contig_name = contig_header.get_name(contig_index)?;
        let query_url = match reference {
            Reference::Hg19 | Reference::Hg38 | Reference::UcscGenome(_) => format!(
                "https://api.genome.ucsc.edu/getData/track?genome={}&track=cytoBandIdeo&chrom={}",
                reference.to_string(),
                contig_name
            ),
            Reference::UcscAccession(genome) => {
                if cache.hub_url.is_none() {
                    let hub_url = self.get_hub_url_for_genark_accession(genome).await?;
                    cache.hub_url = Some(hub_url);
                }
                let hub_url = cache.hub_url.as_ref().unwrap();
                format!(
                    "https://api.genome.ucsc.edu/getData/track?hubUrl={}&genome={}&track=cytoBandIdeo&chrom={}",
                    hub_url, genome, contig_name
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

        response.to_cytoband(reference, contig_index, contig_header)
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
        contig_header: &ContigHeader,
    ) -> Result<Vec<Gene>, TGVError> {
        self.query_track_if_not_cached(reference, region.contig(), cache, contig_header)
            .await?;

        // TODO: now I don't really handle empty query results

        Ok(cache
            .tracks
            .get(&region.contig())
            .ok_or(TGVError::IOError(format!(
                "Track not found for contig index {}",
                region.contig()
            )))?
            .get_features_overlapping(region)
            .iter()
            .map(|g| (*g).clone())
            .collect())
    }

    async fn query_gene_covering(
        &self,
        reference: &Reference,
        contig_index: usize,
        position: usize,
        cache: &mut TrackCache,
        contig_header: &ContigHeader,
    ) -> Result<Option<Gene>, TGVError> {
        self.query_track_if_not_cached(reference, contig_index, cache, contig_header)
            .await?;

        Ok(cache
            .tracks
            .get(&contig_index)
            .ok_or(TGVError::IOError(format!(
                "Track not found for contig index {}",
                contig_index
            )))?
            .get_gene_at(position)
            .cloned())
    }

    async fn query_gene_name(
        &self,
        reference: &Reference,
        gene_name: &String,
        cache: &mut TrackCache,
        contig_header: &ContigHeader,
    ) -> Result<Gene, TGVError> {
        if !cache.gene_quried(gene_name) {
            // query all possible tracks until the gene is found
            for contig_index in 0..contig_header.contigs.len() {
                self.query_track_if_not_cached(reference, contig_index, cache, contig_header)
                    .await?;

                if let Some(gene) = cache.get_gene(gene_name) {
                    return Ok(gene.clone());
                }
            }
        }

        Err(TGVError::IOError(format!("Gene {} not found", gene_name)))
    }

    async fn query_k_genes_after(
        &self,
        reference: &Reference,
        contig_index: usize,
        coord: usize,
        k: usize,
        cache: &mut TrackCache,
        contig_header: &ContigHeader,
    ) -> Result<Gene, TGVError> {
        self.query_track_if_not_cached(reference, contig_index, cache, contig_header)
            .await?;

        cache
            .tracks
            .get(&contig_index)
            .ok_or(TGVError::IOError(format!(
                "Track not found for contig {}",
                contig_index
            )))?
            .get_saturating_k_genes_after(coord, k)
            .ok_or(TGVError::IOError("No genes found".to_string()))
            .cloned()
    }

    async fn query_k_genes_before(
        &self,
        reference: &Reference,
        contig_index: usize,
        coord: usize,
        k: usize,
        cache: &mut TrackCache,
        contig_header: &ContigHeader,
    ) -> Result<Gene, TGVError> {
        self.query_track_if_not_cached(reference, contig_index, cache, contig_header)
            .await?;

        cache
            .tracks
            .get(&contig_index)
            .ok_or(TGVError::IOError(format!(
                "Track not found for contig {}",
                contig_index
            )))?
            .get_saturating_k_genes_before(coord, k)
            .ok_or(TGVError::IOError("No genes found".to_string()))
            .cloned()
    }

    async fn query_k_exons_after(
        &self,
        reference: &Reference,
        contig_index: usize,
        coord: usize,
        k: usize,
        cache: &mut TrackCache,
        contig_header: &ContigHeader,
    ) -> Result<SubGeneFeature, TGVError> {
        self.query_track_if_not_cached(reference, contig_index, cache, contig_header)
            .await?;

        cache
            .tracks
            .get(&contig_index)
            .ok_or(TGVError::IOError(format!(
                "Track not found for contig {}",
                contig_index
            )))?
            .get_saturating_k_exons_after(coord, k)
            .ok_or(TGVError::IOError("No exons found".to_string()))
    }

    async fn query_k_exons_before(
        &self,
        reference: &Reference,
        contig_index: usize,
        coord: usize,
        k: usize,
        cache: &mut TrackCache,
        contig_header: &ContigHeader,
    ) -> Result<SubGeneFeature, TGVError> {
        self.query_track_if_not_cached(reference, contig_index, cache, contig_header)
            .await?;

        cache
            .tracks
            .get(&contig_index)
            .ok_or(TGVError::IOError(format!(
                "Track not found for contig {}",
                contig_index
            )))?
            .get_saturating_k_exons_before(coord, k)
            .ok_or(TGVError::IOError("No exons found".to_string()))
    }
}

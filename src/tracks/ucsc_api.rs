use crate::tracks::{TrackCache, TrackService, TRACK_PREFERENCES};
use crate::{
    contig_header::{Contig, ContigHeader},
    cytoband::Cytoband,
    error::TGVError,
    feature::{Gene, SubGeneFeature},
    intervals::GenomeInterval,
    intervals::Region,
    reference::Reference,
    track::Track,
    tracks::schema::*,
};
use async_trait::async_trait;
use noodles::vcf::header::record::value::map::contig;
use reqwest::Client;

// TODO: improved pattern:
// Service doesn't save anything. No reference, no cache.
// Ask these things to be passed in. And return them to store in the state.

#[derive(Debug)]
pub struct UcscApiTrackService {
    client: Client,

    cache: TrackCache,

    /// hub_url for UCSC accessions.
    /// None: Not initialized.
    /// Some(url): Queried and found.
    hub_url: Option<String>,
}

impl UcscApiTrackService {
    pub fn new() -> Result<Self, TGVError> {
        Ok(Self {
            client: Client::new(),
            cache: TrackCache::default(),
            hub_url: None,
        })
    }

    /// Query the API to download the gene track data for a contig.
    pub async fn query_track_if_not_cached(
        &mut self,
        reference: &Reference,
        contig_name: &str,
        contig_index: usize,
    ) -> Result<(), TGVError> {
        if self.cache.contig_quried(&contig_index) {
            return Ok(());
        }

        let preferred_track = match &self.cache.preferred_track_name {
            None => {
                {
                    let preferred_track = self.get_preferred_track_name(reference).await?.ok_or(
                        TGVError::IOError(format!(
                            "Failed to get prefered track for {} from UCSC API",
                            contig_name
                        )),
                    )?; // TODO: proper handling

                    self.cache
                        .set_preferred_track_name(Some(preferred_track.clone()));
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
                let hub_url = self.hub_url.clone().unwrap_or({
                    let hub_url = self.get_hub_url_for_genark_accession(genome).await?;
                    self.hub_url = Some(hub_url.clone());
                    hub_url
                });
                format!(
                    "https://api.genome.ucsc.edu/getData/track?hubUrl={}&genome={}&track={}&chrom={}",
                    hub_url, genome, preferred_track, contig_name
                )
            }
            _ => {
                return Err(TGVError::StateError(
                    "UcscApi can only be used for UCSC reference genomes.".to_string(),
                ));
            }
        };

        let mut response: serde_json::Value =
            self.client.get(query_url).send().await?.json().await?;

        let response: UcscApiListGeneResponse =
            serde_json::from_value(response[preferred_track].take())?;

        self.cache.add_track(
            contig_index,
            Track::from_genes(
                response
                    .genes
                    .into_iter()
                    .map(|response| response.to_gene(contig_index))
                    .collect::<Result<Vec<Gene>, TGVError>>()?,
                contig_index,
            )?,
        );

        Ok(())
    }

    pub async fn get_hub_url_for_genark_accession(
        &mut self,
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
    async fn close(&mut self) -> Result<(), TGVError> {
        // reqwest client dones't need closing
        Ok(())
    }

    async fn get_all_contigs(&mut self, reference: &Reference) -> Result<Vec<Contig>, TGVError> {
        let query_url = match reference {
            Reference::Hg19 | Reference::Hg38 | Reference::UcscGenome(_) => {
                format!(
                    "https://api.genome.ucsc.edu/list/chromosomes?genome={}",
                    reference.to_string()
                )
            }
            Reference::UcscAccession(genome) => {
                let hub_url = self.hub_url.clone().unwrap_or({
                    let hub_url = self.get_hub_url_for_genark_accession(genome).await?;
                    self.hub_url = Some(hub_url.clone());
                    hub_url
                });

                format!(
                    "https://api.genome.ucsc.edu/list/chromosomes?hubUrl={};genome={}",
                    hub_url, genome
                )
            }
            _ => {
                return Err(TGVError::StateError(
                    "UcscApi tracks can only be used for UCSC reference genomes.".to_string(),
                ));
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
        &mut self,
        reference: &Reference,
        contig_index: usize,

        contig_header: &ContigHeader,
    ) -> Result<Option<Cytoband>, TGVError> {
        let contig_name = match contig_header.try_get(contig_index)?.get_track_name() {
            Some(contig_name) => contig_name,
            None => return Ok(None), // contig not included in the UCSC API
        };
        let query_url = match reference {
            Reference::Hg19 | Reference::Hg38 | Reference::UcscGenome(_) => format!(
                "https://api.genome.ucsc.edu/getData/track?genome={}&track=cytoBandIdeo&chrom={}",
                reference.to_string(),
                contig_name
            ),
            Reference::UcscAccession(genome) => {
                if self.hub_url.is_none() {
                    let hub_url = self.get_hub_url_for_genark_accession(genome).await?;
                    self.hub_url = Some(hub_url);
                }
                let hub_url = self.hub_url.as_ref().unwrap();
                format!(
                    "https://api.genome.ucsc.edu/getData/track?hubUrl={}&genome={}&track=cytoBandIdeo&chrom={}",
                    hub_url, genome, contig_name
                )
            }
            _ => {
                return Err(TGVError::StateError(
                    "UcscApi tracks can only be used for UCSC reference genomes.".to_string(),
                ));
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

        response.to_cytoband(reference, contig_index)
    }

    async fn get_preferred_track_name(
        &mut self,
        reference: &Reference,
    ) -> Result<Option<String>, TGVError> {
        let query_url = match reference {
            Reference::Hg19 | Reference::Hg38 | Reference::UcscGenome(_) => format!(
                "https://api.genome.ucsc.edu/list/tracks?trackLeavesOnly=1;genome={}",
                reference.to_string(),
            ),
            Reference::UcscAccession(genome) => {
                if self.hub_url.is_none() {
                    let hub_url = self.get_hub_url_for_genark_accession(genome).await?;
                    self.hub_url = Some(hub_url);
                }
                let hub_url = self.hub_url.as_ref().unwrap();
                format!(
                    "https://api.genome.ucsc.edu/list/tracks?trackLeavesOnly=1;hubUrl={};genome={}",
                    hub_url, genome
                )
            }
            _ => {
                return Err(TGVError::StateError(
                    "UcscApi tracks can only be used for UCSC reference genomes.".to_string(),
                ));
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
        &mut self,
        reference: &Reference,
        region: &Region,
        contig_header: &ContigHeader,
    ) -> Result<Vec<Gene>, TGVError> {
        let contig_name = match contig_header
            .try_get(region.contig_index())?
            .get_track_name()
        {
            Some(contig_name) => contig_name,
            None => return Ok(Vec::new()), // Contig doesn't have track data
        };
        self.query_track_if_not_cached(reference, contig_name, region.contig_index())
            .await?;

        // TODO: now I don't really handle empty query results

        Ok(self
            .cache
            .tracks
            .get(&region.contig_index())
            .ok_or(TGVError::IOError(format!(
                "Track not found for contig index {}",
                region.contig_index()
            )))?
            .get_features_overlapping(region)
            .iter()
            .map(|g| (*g).clone())
            .collect())
    }

    async fn query_gene_covering(
        &mut self,
        reference: &Reference,
        contig_index: usize,
        position: usize,

        contig_header: &ContigHeader,
    ) -> Result<Option<Gene>, TGVError> {
        let contig_name = match contig_header.try_get(contig_index)?.get_track_name() {
            Some(contig_name) => contig_name,
            None => return Ok(None), // Contig doesn't have track data
        };
        self.query_track_if_not_cached(reference, contig_name, contig_index)
            .await?;

        Ok(self
            .cache
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
        &mut self,
        reference: &Reference,
        gene_name: &String,
        contig_header: &ContigHeader,
    ) -> Result<Gene, TGVError> {
        if !self.cache.gene_quried(gene_name) {
            // query all possible tracks until the gene is found
            for (contig_index, contig) in contig_header.contigs.iter().enumerate() {
                if let Some(contig_name) = contig.get_track_name() {
                    self.query_track_if_not_cached(reference, contig_name, contig_index)
                        .await?;

                    if let Some(gene) = self.cache.get_gene(gene_name) {
                        return Ok(gene.clone());
                    }
                }
            }
        }

        Err(TGVError::IOError(format!("Gene {} not found", gene_name)))
    }

    async fn query_k_genes_after(
        &mut self,
        reference: &Reference,
        contig_index: usize,
        coord: usize,
        k: usize,
        contig_header: &ContigHeader,
    ) -> Result<Gene, TGVError> {
        let contig_name = match contig_header.try_get(contig_index)?.get_track_name() {
            Some(contig_name) => contig_name,
            None => {
                return Err(TGVError::StateError(format!(
                    "Contig {} (index = {}, aliases = {}) does not have track data.",
                    contig_header.contigs[contig_index].name,
                    contig_index,
                    contig_header.contigs[contig_index].aliases.join(",")
                )))
            }
        };
        self.query_track_if_not_cached(reference, contig_name, contig_index)
            .await?;

        self.cache
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
        &mut self,
        reference: &Reference,
        contig_index: usize,
        coord: usize,
        k: usize,

        contig_header: &ContigHeader,
    ) -> Result<Gene, TGVError> {
        let contig_name = match contig_header.try_get(contig_index)?.get_track_name() {
            Some(contig_name) => contig_name,
            None => {
                return Err(TGVError::StateError(format!(
                    "Contig {} (index = {}, aliases = {}) does not have track data.",
                    contig_header.contigs[contig_index].name,
                    contig_index,
                    contig_header.contigs[contig_index].aliases.join(",")
                )))
            }
        };
        self.query_track_if_not_cached(reference, contig_name, contig_index)
            .await?;

        self.cache
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
        &mut self,
        reference: &Reference,
        contig_index: usize,
        coord: usize,
        k: usize,

        contig_header: &ContigHeader,
    ) -> Result<SubGeneFeature, TGVError> {
        let contig_name = match contig_header.try_get(contig_index)?.get_track_name() {
            Some(contig_name) => contig_name,
            None => {
                return Err(TGVError::StateError(format!(
                    "Contig {} (index = {}, aliases = {}) does not have track data.",
                    contig_header.contigs[contig_index].name,
                    contig_index,
                    contig_header.contigs[contig_index].aliases.join(",")
                )))
            }
        };
        self.query_track_if_not_cached(reference, contig_name, contig_index)
            .await?;

        self.cache
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
        &mut self,
        reference: &Reference,
        contig_index: usize,
        coord: usize,
        k: usize,
        contig_header: &ContigHeader,
    ) -> Result<SubGeneFeature, TGVError> {
        let contig_name = match contig_header.try_get(contig_index)?.get_track_name() {
            Some(contig_name) => contig_name,
            None => {
                return Err(TGVError::StateError(format!(
                    "Contig {} (index = {}, aliases = {}) does not have track data.",
                    contig_header.contigs[contig_index].name,
                    contig_index,
                    contig_header.contigs[contig_index].aliases.join(",")
                )))
            }
        };
        self.query_track_if_not_cached(reference, contig_name, contig_index)
            .await?;

        self.cache
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

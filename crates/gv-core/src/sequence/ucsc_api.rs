use crate::contig_header::{Contig, ContigHeader};
use crate::error::TGVError;
use crate::intervals::{GenomeInterval, Region};
use crate::reference::Reference;
use crate::sequence::{Sequence, SequenceRepository};
use crate::tracks::{UcscHost, schema::*};
use reqwest::Client;
use serde::Deserialize;

#[derive(Debug)]
pub struct UCSCApiSequenceRepository {
    client: Client,
    reference: Reference,

    /// Used when using UCSC APIs and the reference is a UCSC Accession.
    /// None: Not queried yet
    /// Some(hub_url): Queried and cached.
    hub_url: Option<String>,
}

impl UCSCApiSequenceRepository {
    pub fn new(reference: &Reference, host: &UcscHost) -> Result<Self, TGVError> {
        // FIXME: decide API url based on host
        Ok(Self {
            client: Client::new(),
            reference: reference.clone(),
            hub_url: None,
        })
    }

    pub async fn close(&mut self) -> Result<(), TGVError> {
        // Reqwest client does not need to be closed.
        Ok(())
    }

    /// start / end: 1-based, inclusive.
    async fn get_api_url(
        &mut self,
        contig_name: &str,
        start: u64,
        end: u64,
    ) -> Result<String, TGVError> {
        match &self.reference {
            Reference::Hg19 | Reference::Hg38 | Reference::UcscGenome(_) => Ok(format!(
                "https://api.genome.ucsc.edu/getData/sequence?genome={};chrom={};start={};end={}",
                self.reference.to_string(),
                contig_name,
                start - 1, // start is 0-based, inclusive.
                end
            )),
            Reference::UcscAccession(genome) => {
                if self.hub_url.is_none() {
                    let hub_url = self.get_hub_url_for_genark_accession(genome).await?;
                    self.hub_url = Some(hub_url);
                }
                let hub_url = self.hub_url.as_ref().unwrap();
                Ok(format!(
                    "https://api.genome.ucsc.edu/getData/sequence?hubUrl={}&genome={};chrom={};start={};end={}",
                    hub_url,
                    genome,
                    contig_name,
                    start - 1,
                    end
                ))
            }
            _ => Err(TGVError::StateError(
                "UcscApi can only be used for UCSC reference genomes.".to_string(),
            )),
        }
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

#[derive(Debug, Deserialize)]
struct UcscResponse {
    dna: String,
}

impl SequenceRepository for UCSCApiSequenceRepository {
    async fn query_sequence(
        &mut self,
        region: &Region,
        contig_header: &ContigHeader,
    ) -> Result<Sequence, TGVError> {
        let contig_name = match contig_header
            .try_get(region.contig_index())?
            .get_sequence_name()
        {
            Some(contig_name) => contig_name,
            None => {
                return Ok(Sequence {
                    start: region.start(),
                    sequence: Vec::new(),
                    contig_index: region.contig_index(),
                });
            }
        };
        let url = self
            .get_api_url(contig_name, region.start(), region.end())
            .await?;

        let response: UcscResponse = self.client.get(&url).send().await?.json().await?;

        Ok(Sequence {
            start: region.start(),
            sequence: response.dna.into_bytes(),
            contig_index: region.contig_index(),
        })
    }

    async fn close(&mut self) -> Result<(), TGVError> {
        Ok(())
    }

    async fn get_all_contigs(&mut self) -> Result<Vec<Contig>, TGVError> {
        let query_url = match &self.reference {
            Reference::Hg19 | Reference::Hg38 | Reference::UcscGenome(_) => {
                format!(
                    "https://api.genome.ucsc.edu/list/chromosomes?genome={}",
                    self.reference.to_string()
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
                    "UcscApi sequence can only be used for UCSC reference genomes.".to_string(),
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
            output.push(Contig::new(&name_string, Some(length as u64)));
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
}

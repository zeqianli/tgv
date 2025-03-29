use crate::error::TGVError;
use crate::models::{contig::Contig, reference::Reference, region::Region, sequence::Sequence};
use reqwest::Client;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct UcscResponse {
    dna: String,
}

pub struct SequenceService {
    client: Client,
    reference: Reference,
}

impl SequenceService {
    pub fn new(reference: Reference) -> Result<Self, ()> {
        Ok(Self {
            client: Client::new(),
            reference,
        })
    }

    pub async fn close(&self) -> Result<(), TGVError> {
        // Reqwest client does not need to be closed.
        Ok(())
    }

    pub async fn query_sequence(&self, region: &Region) -> reqwest::Result<Sequence> {
        let url = self
            .get_api_url(&region.contig, region.start, region.end)
            .unwrap();

        let response: UcscResponse = self.client.get(&url).send().await?.json().await?;

        Ok(Sequence {
            start: region.start,
            sequence: response.dna,
            contig: region.contig.clone(),
        })
    }

    /// start / end: 1-based, inclusive.
    fn get_api_url(&self, chrom: &Contig, start: usize, end: usize) -> Result<String, ()> {
        match self.reference {
            Reference::Hg19 => Ok(format!(
                "https://api.genome.ucsc.edu/getData/sequence?genome=hg19;chrom={};start={};end={}",
                chrom.full_name(),
                start - 1, // start is 0-based, inclusive.
                end
            )),
            Reference::Hg38 => Ok(format!(
                "https://api.genome.ucsc.edu/getData/sequence?genome=hg38;chrom={};start={};end={}",
                chrom.full_name(),
                start - 1, // start is 0-based, inclusive.
                end
            )),
            _ => {
                Err(()) // TODO: error handling
            }
        }
    }
}

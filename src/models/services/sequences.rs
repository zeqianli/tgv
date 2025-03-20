use crate::models::{contig::Contig, region::Region, sequence::Sequence};
use reqwest::Client;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct UcscResponse {
    dna: String,
}

pub struct SequenceService {
    client: Client,
    reference: String,
}

impl SequenceService {
    pub fn new(reference: String) -> Result<Self, ()> {
        Ok(Self {
            client: Client::new(),
            reference,
        })
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

    fn get_api_url(&self, chrom: &Contig, start: usize, end: usize) -> Result<String, ()> {
        match self.reference.as_str() {
            "hg19" => Ok(format!(
                "https://api.genome.ucsc.edu/getData/sequence?genome=hg19;chrom={};start={};end={}",
                chrom.full_name(),
                start,
                end
            )),
            "hg38" => Ok(format!(
                "https://api.genome.ucsc.edu/getData/sequence?genome=hg38;chrom={};start={};end={}",
                chrom.full_name(),
                start,
                end
            )),
            _ => {
                Err(()) // TODO: error handling
            }
        }
    }
}

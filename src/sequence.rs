use crate::contig::Contig;
use crate::error::TGVError;
use crate::reference::Reference;
use crate::region::Region;
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;
use twobit::TwoBitFile; // Add twobit crate to Cargo.toml
use url::Url;
/// Sequences of a genome region.
pub struct Sequence {
    /// 1-based genome coordinate of sequence[0].
    /// 1-based, inclusive.
    pub start: usize,

    /// Genome sequence
    pub sequence: String,

    /// Contig name
    pub contig: Contig,
}

impl Sequence {
    pub fn new(start: usize, sequence: String, contig: Contig) -> Result<Self, ()> {
        if usize::MAX - start < sequence.len() {
            return Err(());
        }

        Ok(Self {
            contig,
            start,
            sequence,
        })
    }

    /// Sequence start. 1-based, inclusive.
    pub fn start(&self) -> usize {
        self.start
    }

    /// Sequence length.
    pub fn len(&self) -> usize {
        self.sequence.len()
    }

    /// Sequence end. 1-based, inclusive.
    pub fn end(&self) -> usize {
        self.start + self.sequence.len() - 1
    }
}

impl Sequence {
    /// Get the sequence in [left, right].
    /// 1-based, inclusive.
    pub fn get_sequence(&self, region: &Region) -> Option<String> {
        if !self.has_complete_data(region) {
            return None;
        }

        Some(
            self.sequence
                .get(region.start - self.start..region.end - self.start + 1)
                .unwrap()
                .to_string(),
        )
    }

    /// Whether the sequence has complete data in [left, right].
    /// 1-based, inclusive.
    pub fn has_complete_data(&self, region: &Region) -> bool {
        (region.contig == self.contig)
            && ((region.start >= self.start()) && (region.end <= self.end()))
    }
}

/// Sequence access cache.  
pub struct SequenceCache {
    /// Used when using UCSC APIs and the reference is a UCSC Accession.
    /// None: Not queried yet
    /// Some(hub_url): Queried and cached.
    hub_url: Option<String>,

    /// contig name -> 2bit buffer index in buffers. Used in local mode (TwoBitSequenceRepository).
    contig_to_buffer_index: HashMap<String, usize>,

    /// 2bit file buffers. Used in local mode (TwoBitSequenceRepository).
    buffers: Vec<TwoBitFile<std::io::BufReader<std::fs::File>>>,
}

impl Default for SequenceCache {
    fn default() -> Self {
        Self::new()
    }
}

impl SequenceCache {
    pub fn new() -> Self {
        Self {
            hub_url: None,
            contig_to_buffer_index: HashMap::new(),
            buffers: Vec::new(),
        }
    }
}

pub trait SequenceRepository {
    async fn query_sequence(
        &self,
        region: &Region,
        cache: &mut SequenceCache,
    ) -> Result<Sequence, TGVError>;

    async fn close(&self) -> Result<(), TGVError>;
}

pub enum SequenceRepositoryEnum {
    UCSCApi(UCSCApiSequenceRepository),
    TwoBit(TwoBitSequenceRepository),
}

impl SequenceRepositoryEnum {
    pub async fn query_sequence(
        &self,
        region: &Region,
        cache: &mut SequenceCache,
    ) -> Result<Sequence, TGVError> {
        match self {
            Self::UCSCApi(repo) => repo.query_sequence(region, cache).await,
            Self::TwoBit(repo) => repo.query_sequence(region, cache).await,
        }
    }

    pub async fn close(&self) -> Result<(), TGVError> {
        match self {
            Self::UCSCApi(repo) => repo.close().await,
            Self::TwoBit(repo) => repo.close().await,
        }
    }
}

pub struct UCSCApiSequenceRepository {
    client: Client,
    reference: Reference,
}

impl UCSCApiSequenceRepository {
    pub fn new(reference: Reference) -> Result<Self, TGVError> {
        Ok(Self {
            client: Client::new(),
            reference,
        })
    }

    pub async fn close(&self) -> Result<(), TGVError> {
        // Reqwest client does not need to be closed.
        Ok(())
    }

    /// start / end: 1-based, inclusive.
    async fn get_api_url(
        &self,
        contig: &Contig,
        start: usize,
        end: usize,
        cache: &mut SequenceCache,
    ) -> Result<String, TGVError> {
        match &self.reference {
            Reference::Hg19 | Reference::Hg38 | Reference::UcscGenome(_) => Ok(format!(
                "https://api.genome.ucsc.edu/getData/sequence?genome={};chrom={};start={};end={}",
                self.reference.to_string(),
                contig.name,
                start - 1, // start is 0-based, inclusive.
                end
            )),
            Reference::UcscAccession(genome) => {
                if cache.hub_url.is_none() {
                    let hub_url = self.get_hub_url_for_genark_accession(genome).await?;
                    cache.hub_url = Some(hub_url);
                }
                let hub_url = cache.hub_url.as_ref().unwrap();
                Ok(format!(
                    "https://api.genome.ucsc.edu/getData/sequence?hubUrl={}&genome={};chrom={};start={};end={}",
                    hub_url, genome, contig.name, start - 1, end
                ))
            }
        }
    }

    async fn get_hub_url_for_genark_accession(&self, accession: &str) -> Result<String, TGVError> {
        let query_url = format!(
            "https://api.genome.ucsc.edu/list/genarkGenomes?genome={}",
            accession
        );
        let response = self.client.get(query_url).send().await?;

        // Example response:
        // {
        //     "downloadTime": "2025:05:06T03:46:07Z",
        //     "downloadTimeStamp": 1746503167,
        //     "dataTime": "2025-04-29T10:42:00",
        //     "dataTimeStamp": 1745948520,
        //     "hubUrlPrefix": "/gbdb/genark",
        //     "genarkGenomes": {
        //       "GCF_028858775.2": {
        //         "hubUrl": "GCF/028/858/775/GCF_028858775.2/hub.txt",
        //         "asmName": "NHGRI_mPanTro3-v2.0_pri",
        //         "scientificName": "Pan troglodytes",
        //         "commonName": "chimpanzee (v2 AG18354 primary hap 2024 refseq)",
        //         "taxId": 9598,
        //         "priority": 138,
        //         "clade": "primates"
        //       }
        //     },
        //     "totalAssemblies": 5691,
        //     "itemsReturned": 1
        //   }

        let response_text = response.text().await?;
        let value: serde_json::Value = serde_json::from_str(&response_text)?;

        Ok(format!(
            "https://hgdownload.soe.ucsc.edu/hubs/{}",
            value["genarkGenomes"][accession]["hubUrl"]
                .as_str()
                .ok_or(TGVError::IOError(format!(
                    "Failed to get hub url for {}",
                    accession
                )))?
        ))
    }
}

#[derive(Debug, Deserialize)]
struct UcscResponse {
    dna: String,
}

impl SequenceRepository for UCSCApiSequenceRepository {
    async fn query_sequence(
        &self,
        region: &Region,
        cache: &mut SequenceCache,
    ) -> Result<Sequence, TGVError> {
        let url = self
            .get_api_url(&region.contig, region.start, region.end, cache)
            .await?;

        let response: UcscResponse = self.client.get(&url).send().await?.json().await?;

        Ok(Sequence {
            start: region.start,
            sequence: response.dna,
            contig: region.contig.clone(),
        })
    }

    async fn close(&self) -> Result<(), TGVError> {
        Ok(())
    }
}

/// Repository for reading sequences from 2bit files
pub struct TwoBitSequenceRepository {
    /// Reference genome.   
    reference: Reference,

    /// reference genome string -> 2bit file base name
    contig_to_file_name: HashMap<String, String>,

    /// Cache root directory. 2bit files are in cache_dir/_reference_name/*.2bit
    cache_dir: String,
}

impl TwoBitSequenceRepository {
    pub fn new(
        reference: Reference,
        contig_to_file_name: HashMap<String, String>,
        cache_dir: String,
    ) -> Result<(Self, SequenceCache), TGVError> {
        // Get the file path for this contig

        let mut buffers = Vec::new();
        let mut file_name_to_buffer_index = HashMap::new();
        let mut contig_to_buffer_index = HashMap::new();

        for (contig, file_name) in contig_to_file_name.iter() {
            let i_buffer = buffers.len();

            match file_name_to_buffer_index.get(file_name) {
                Some(i_buffer) => {
                    contig_to_buffer_index.insert(contig.clone(), *i_buffer);
                }
                None => {
                    file_name_to_buffer_index.insert(file_name.clone(), i_buffer);
                    contig_to_buffer_index.insert(contig.clone(), i_buffer + 1);

                    // add a new buffer
                    let file_path = format!("{}/{}/{}", &cache_dir, reference, file_name);
                    let mut tb: TwoBitFile<std::io::BufReader<std::fs::File>> =
                        twobit::TwoBitFile::open(&file_path).map_err(|e| {
                            TGVError::IOError(format!(
                                "Failed to open 2bit file {}: {}",
                                &file_path, e
                            ))
                        })?;
                    buffers.push(tb);
                }
            }
        }

        Ok((
            Self {
                reference,
                contig_to_file_name,
                cache_dir,
            },
            SequenceCache {
                hub_url: None,
                contig_to_buffer_index: contig_to_buffer_index,
                buffers,
            },
        ))
    }
}

impl SequenceRepository for TwoBitSequenceRepository {
    async fn query_sequence(
        &self,
        region: &Region,
        cache: &mut SequenceCache,
    ) -> Result<Sequence, TGVError> {
        let buffer = &mut cache.buffers[cache.contig_to_buffer_index[&region.contig.name]];
        let sequence_str = buffer.read_sequence(
            &region.contig.name,
            (region.start - 1)..region.end, // Convert to 0-based range
        )?;

        Ok(Sequence {
            start: region.start,
            sequence: sequence_str,
            contig: region.contig.clone(),
        })
    }

    async fn close(&self) -> Result<(), TGVError> {
        Ok(())
    }
}

use crate::contig_header::ContigHeader;
use crate::error::TGVError;
use crate::reference::Reference;
use crate::region::Region;
use crate::tracks::schema::*;
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;
use twobit::TwoBitFile; // Add twobit crate to Cargo.toml
/// Sequences of a genome region.
pub struct Sequence {
    /// 1-based genome coordinate of sequence[0].
    /// 1-based, inclusive.
    pub start: usize,

    /// Genome sequence
    pub sequence: Vec<u8>,

    /// Contig id
    pub contig_index: usize,
}

impl Sequence {
    pub fn new(start: usize, sequence: Vec<u8>, contig_index: usize) -> Result<Self, ()> {
        if usize::MAX - start < sequence.len() {
            return Err(());
        }

        Ok(Self {
            contig_index,
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
    pub fn get_sequence(&self, region: &Region) -> Option<Vec<u8>> {
        if !self.has_complete_data(region) {
            return None;
        }

        Some(
            self.sequence
                .get(region.start - self.start..region.end - self.start + 1)
                .unwrap()
                .to_vec(),
        )
    }

    pub fn base_at(&self, coordinate: usize) -> Option<u8> {
        if coordinate < self.start() {
            return None;
        }

        if coordinate > self.end() {
            return None;
        }

        Some(self.sequence[coordinate - self.start])
    }

    /// Whether the sequence has complete data in [left, right].
    /// 1-based, inclusive.
    pub fn has_complete_data(&self, region: &Region) -> bool {
        (region.contig_index == self.contig_index)
            && ((region.start >= self.start()) && (region.end <= self.end()))
    }
}

/// Sequence access cache.  
pub struct SequenceCache {
    /// Used when using UCSC APIs and the reference is a UCSC Accession.
    /// None: Not queried yet
    /// Some(hub_url): Queried and cached.
    hub_url: Option<String>,

    /// contig index -> 2bit buffer index in buffers. Used in local mode (TwoBitSequenceRepository).
    contig_to_buffer_index: HashMap<usize, usize>,

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
        contig_header: &ContigHeader,
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
        contig_header: &ContigHeader,
    ) -> Result<Sequence, TGVError> {
        match self {
            Self::UCSCApi(repo) => repo.query_sequence(region, cache, contig_header).await,
            Self::TwoBit(repo) => repo.query_sequence(region, cache, contig_header).await,
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
        contig_index: &usize,
        start: usize,
        end: usize,
        cache: &mut SequenceCache,
        contig_header: &ContigHeader,
    ) -> Result<String, TGVError> {
        let contig_name = contig_header.get_name(*contig_index)?;
        match &self.reference {
            Reference::Hg19 | Reference::Hg38 | Reference::UcscGenome(_) => Ok(format!(
                "https://api.genome.ucsc.edu/getData/sequence?genome={};chrom={};start={};end={}",
                self.reference.to_string(),
                contig_name,
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
                    hub_url, genome, contig_name, start - 1, end
                ))
            }
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
        &self,
        region: &Region,
        cache: &mut SequenceCache,
        contig_header: &ContigHeader,
    ) -> Result<Sequence, TGVError> {
        let url = self
            .get_api_url(
                &region.contig_index,
                region.start,
                region.end,
                cache,
                contig_header,
            )
            .await?;

        let response: UcscResponse = self.client.get(&url).send().await?.json().await?;

        Ok(Sequence {
            start: region.start,
            sequence: response.dna.into_bytes(),
            contig_index: region.contig_index,
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
    contig_to_file_name: HashMap<usize, String>,

    /// Cache root directory. 2bit files are in cache_dir/_reference_name/*.2bit
    cache_dir: String,
}

impl TwoBitSequenceRepository {
    pub fn new(
        reference: Reference,
        contig_to_file_name: HashMap<usize, Option<String>>,
        cache_dir: String,
    ) -> Result<(Self, SequenceCache), TGVError> {
        // Get the file path for this contig

        let mut buffers = Vec::new();
        let mut file_name_to_buffer_index = HashMap::new();
        let mut contig_to_buffer_index = HashMap::new();

        // Remove contigs that have no 2bit file.
        let contig_to_file_name: HashMap<usize, String> = contig_to_file_name
            .into_iter()
            .filter(|(_, file_name)| file_name.is_some())
            .map(|(contig, file_name)| (contig, file_name.unwrap()))
            .collect();

        for (contig, file_name) in contig_to_file_name.iter() {
            let i_buffer = buffers.len();

            match file_name_to_buffer_index.get(file_name) {
                Some(i_buffer) => {
                    contig_to_buffer_index.insert(contig.clone(), *i_buffer);
                }
                None => {
                    file_name_to_buffer_index.insert(file_name.clone(), i_buffer);
                    contig_to_buffer_index.insert(contig.clone(), i_buffer);

                    // add a new buffer
                    let file_path = format!("{}/{}/{}", &cache_dir, reference, file_name);
                    let tb: TwoBitFile<std::io::BufReader<std::fs::File>> =
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
                contig_to_buffer_index,
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
        contig_header: &ContigHeader,
    ) -> Result<Sequence, TGVError> {
        let contig_name = contig_header.get_name(region.contig_index)?;
        let buffer = &mut cache.buffers[cache.contig_to_buffer_index[&region.contig_index]];
        let sequence_str = buffer.read_sequence(
            &contig_name,
            (region.start - 1)..region.end, // Convert to 0-based range
        )?;

        Ok(Sequence {
            start: region.start,
            sequence: sequence_str.into_bytes(),
            contig_index: region.contig_index,
        })
    }

    async fn close(&self) -> Result<(), TGVError> {
        Ok(())
    }
}

use crate::{
    alignment::{Alignment, AlignmentBuilder},
    contig::Contig,
    error::TGVError,
    helpers::is_url,
    reference::Reference,
    region::Region,
    sequence::Sequence,
    settings::{BackendType, Settings},
    track_service::{TrackService, TrackServiceEnum, UcscApiTrackService, UcscDbTrackService},
};

use reqwest::Client;
use rust_htslib::bam;
use rust_htslib::bam::{Header, IndexedReader, Read};
use serde::Deserialize;
use std::path::Path;
use url::Url;

pub struct Repository {
    pub alignment_repository: AlignmentRepositoryEnum,

    pub track_service: Option<TrackServiceEnum>,

    pub sequence_service: Option<SequenceService>,
}

impl Repository {
    pub async fn new(settings: &Settings) -> Result<Self, TGVError> {
        let alignment_repository = AlignmentRepositoryEnum::from(settings)?;

        let (track_service, sequence_service): (Option<TrackServiceEnum>, Option<SequenceService>) =
            match settings.reference.as_ref() {
                Some(reference) => {
                    let ts = match (&settings.backend, reference) {
                        (BackendType::Db, Reference::UcscAccession(_)) => {
                            TrackServiceEnum::Api(UcscApiTrackService::new()?)
                        }
                        (BackendType::Db, _) => {
                            TrackServiceEnum::Db(UcscDbTrackService::new(reference).await?)
                        }
                        _ => {
                            return Err(TGVError::ValueError(format!(
                                "Unsupported reference: {}",
                                reference
                            )));
                        }
                    };
                    let ss = SequenceService::new(reference.clone())?;
                    (Some(ts), Some(ss))
                }
                None => (None, None),
            };

        Ok(Self {
            alignment_repository,
            track_service,
            sequence_service,
        })
    }

    pub fn track_service_checked(&self) -> Result<&TrackServiceEnum, TGVError> {
        match self.track_service {
            Some(ref track_service) => Ok(track_service),
            None => Err(TGVError::StateError(
                "Track service is not initialized".to_string(),
            )),
        }
    }

    pub fn sequence_service_checked(&self) -> Result<&SequenceService, TGVError> {
        match self.sequence_service {
            Some(ref sequence_service) => Ok(sequence_service),
            None => Err(TGVError::StateError(
                "Sequence service is not initialized".to_string(),
            )),
        }
    }

    pub async fn close(&mut self) -> Result<(), TGVError> {
        if let Some(ts) = self.track_service.as_mut() {
            ts.close().await?;
        }
        if let Some(ss) = self.sequence_service.as_mut() {
            ss.close().await?;
        }
        Ok(())
    }

    pub fn has_alignment(&self) -> bool {
        self.alignment_repository.has_alignment()
    }

    pub fn has_track(&self) -> bool {
        self.track_service.is_some()
    }

    pub fn has_sequence(&self) -> bool {
        self.sequence_service.is_some()
    }
}

#[derive(Debug)]
enum RemoteSource {
    S3,
    HTTP,
    GS,
}

impl RemoteSource {
    fn from(path: &String) -> Result<Self, TGVError> {
        if path.starts_with("s3://") {
            Ok(Self::S3)
        } else if path.starts_with("http://") || path.starts_with("https://") {
            Ok(Self::HTTP)
        } else if path.starts_with("gss://") {
            Ok(Self::GS)
        } else {
            Err(TGVError::ValueError(format!(
                "Unsupported remote path {}. Only S3, HTTP/HTTPS, and GS are supported.",
                path
            )))
        }
    }
}

pub trait AlignmentRepository {
    fn read_alignment(&self, region: &Region) -> Result<Alignment, TGVError>;

    fn read_header(&self) -> Result<Vec<(String, Option<usize>)>, TGVError>;
}

#[derive(Debug)]
pub struct BamRepository {
    bam_path: String,
    bai_path: Option<String>,
}

impl BamRepository {
    fn new(bam_path: String, bai_path: Option<String>) -> Result<Self, TGVError> {
        if is_url(&bam_path) {
            return Err(TGVError::IOError(format!(
                "{} is a remote path. Use RemoteBamRepository for remote BAM IO",
                bam_path
            )));
        }

        if !Path::new(&bam_path).exists() {
            return Err(TGVError::IOError(format!(
                "BAM file {} not found",
                bam_path
            )));
        }

        match &bai_path {
            Some(bai_path) => {
                if !Path::new(bai_path).exists() {
                    return Err(TGVError::IOError(format!(
                        "BAM index file {} not found. Only indexed BAM files are supported.",
                        bai_path
                    )));
                }
            }
            None => {
                if !Path::new(&format!("{}.bai", bam_path)).exists() {
                    return Err(TGVError::IOError(format!(
                        "BAM index file {}.bai not found. Only indexed BAM files are supported.",
                        bam_path
                    )));
                }
            }
        }

        Ok(Self { bam_path, bai_path })
    }
}

impl AlignmentRepository for BamRepository {
    fn read_alignment(&self, region: &Region) -> Result<Alignment, TGVError> {
        let mut bam = match self.bai_path.as_ref() {
            Some(bai_path) => {
                IndexedReader::from_path_and_index(self.bam_path.clone(), bai_path.clone())?
            }
            None => IndexedReader::from_path(self.bam_path.clone())?,
        };

        let header = bam::Header::from_template(bam.header());

        let query_contig_string = get_query_contig_string(&header, region)?;
        bam.fetch((
            &query_contig_string,
            region.start as i32 - 1,
            region.end as i32,
        ))
        .map_err(|e| TGVError::IOError(e.to_string()))?;

        let mut alignment_builder = AlignmentBuilder::new()?;

        for record in bam.records() {
            let read = record.map_err(|e| TGVError::IOError(e.to_string()))?;
            alignment_builder.add_read(read)?;
        }

        alignment_builder.region(region)?.build()
    }

    /// Read BAM headers and return contig namesa and lengths.
    /// Note that this function does not interprete the contig name as contg vs chromosome.
    fn read_header(&self) -> Result<Vec<(String, Option<usize>)>, TGVError> {
        let bam = match self.bai_path.as_ref() {
            Some(bai_path) => {
                IndexedReader::from_path_and_index(self.bam_path.clone(), bai_path.clone())?
            }
            None => IndexedReader::from_path(self.bam_path.clone())?,
        };

        let header = bam::Header::from_template(bam.header());
        get_contig_names_and_lengths_from_header(&header)
    }
}

#[derive(Debug)]
pub struct RemoteBamRepository {
    bam_path: String,
    source: RemoteSource,
}

impl RemoteBamRepository {
    pub fn new(bam_path: &String) -> Result<Self, TGVError> {
        Ok(Self {
            bam_path: bam_path.clone(),
            source: RemoteSource::from(bam_path)?,
        })
    }
}

impl AlignmentRepository for RemoteBamRepository {
    fn read_alignment(&self, region: &Region) -> Result<Alignment, TGVError> {
        let mut bam = IndexedReader::from_url(
            &Url::parse(&self.bam_path).map_err(|e| TGVError::IOError(e.to_string()))?,
        )?;

        let header = bam::Header::from_template(bam.header());

        let query_contig_string = get_query_contig_string(&header, region)?;
        bam.fetch((
            &query_contig_string,
            region.start as i32 - 1,
            region.end as i32,
        ))
        .map_err(|e| TGVError::IOError(e.to_string()))?;

        let mut alignment_builder = AlignmentBuilder::new()?;

        for record in bam.records() {
            let read = record.map_err(|e| TGVError::IOError(e.to_string()))?;
            alignment_builder.add_read(read)?;
        }

        alignment_builder.region(region)?.build()
    }

    fn read_header(&self) -> Result<Vec<(String, Option<usize>)>, TGVError> {
        let bam = IndexedReader::from_url(
            &Url::parse(&self.bam_path).map_err(|e| TGVError::IOError(e.to_string()))?,
        )?;

        let header = bam::Header::from_template(bam.header());
        get_contig_names_and_lengths_from_header(&header)
    }
}

// fn is_remote_path {
//     IndexedReader::from_url(
//         &Url::parse(bam_path).map_err(|e| TGVError::IOError(e.to_string()))?,
//     )
//     .unwrap();

// struct CRAMRepository {
//     cram_path: String,
// }

fn get_contig_names_and_lengths_from_header(
    header: &Header,
) -> Result<Vec<(String, Option<usize>)>, TGVError> {
    let mut output = Vec::new();

    for (_key, records) in header.to_hashmap().iter() {
        for record in records {
            if record.contains_key("SN") {
                let contig_name = record["SN"].to_string();
                let contig_length = if record.contains_key("LN") {
                    record["LN"].to_string().parse::<usize>().ok()
                } else {
                    None
                };

                output.push((contig_name, contig_length))
            }
        }
    }

    Ok(output)
}

/// Get the query string for a region.
/// Look through the header to decide if the bam file chromosome names are abbreviated or full.
fn get_query_contig_string(header: &Header, region: &Region) -> Result<String, TGVError> {
    let mut bam_headers = Vec::new();

    for (_key, records) in header.to_hashmap().iter() {
        for record in records {
            if record.contains_key("SN") {
                let reference_name = record["SN"].to_string();

                if reference_name == region.contig.name
                    || region.contig.aliases.contains(&reference_name)
                {
                    return Ok(reference_name);
                }

                bam_headers.push(reference_name);
            }
        }
    }

    Err(TGVError::IOError(format!(
        "Contig {} (aliases: {}) not found in the bam file header. BAM file has {} contigs: {}",
        region.contig.name,
        region.contig.aliases.join(", "),
        bam_headers.len(),
        bam_headers.join(", ")
    )))
}

#[derive(Debug)]
pub enum AlignmentRepositoryEnum {
    None,
    Bam(BamRepository),
    RemoteBam(RemoteBamRepository),
}

impl AlignmentRepositoryEnum {
    pub fn from(settings: &Settings) -> Result<Self, TGVError> {
        if settings.bam_path.is_none() {
            return Ok(AlignmentRepositoryEnum::None);
        }

        let bam_path = settings.bam_path.clone().unwrap();

        if is_url(&bam_path) {
            return Ok(AlignmentRepositoryEnum::RemoteBam(
                RemoteBamRepository::new(&bam_path)?,
            ));
        }

        Ok(AlignmentRepositoryEnum::Bam(BamRepository::new(
            bam_path,
            settings.bai_path.clone(),
        )?))
    }

    pub fn has_alignment(&self) -> bool {
        match self {
            AlignmentRepositoryEnum::Bam(_) => true,
            AlignmentRepositoryEnum::RemoteBam(_) => true,
            AlignmentRepositoryEnum::None => false,
        }
    }
}

impl AlignmentRepository for AlignmentRepositoryEnum {
    fn read_alignment(&self, region: &Region) -> Result<Alignment, TGVError> {
        match self {
            AlignmentRepositoryEnum::Bam(repository) => repository.read_alignment(region),
            AlignmentRepositoryEnum::RemoteBam(repository) => repository.read_alignment(region),
            AlignmentRepositoryEnum::None => Err(TGVError::IOError("No alignment".to_string())),
        }
    }

    fn read_header(&self) -> Result<Vec<(String, Option<usize>)>, TGVError> {
        match self {
            AlignmentRepositoryEnum::Bam(repository) => repository.read_header(),
            AlignmentRepositoryEnum::RemoteBam(repository) => repository.read_header(),
            AlignmentRepositoryEnum::None => Err(TGVError::IOError("No alignment".to_string())),
        }
    }
}

#[derive(Debug, Deserialize)]
struct UcscResponse {
    dna: String,
}

pub struct SequenceCache {
    /// hub_url for UCSC Accessions
    /// None: Not queried yet
    /// Some(hub_url): Queried
    hub_url: Option<String>,
}

impl Default for SequenceCache {
    fn default() -> Self {
        Self::new()
    }
}

impl SequenceCache {
    pub fn new() -> Self {
        Self { hub_url: None }
    }
}

pub struct SequenceService {
    client: Client,
    reference: Reference,
}

impl SequenceService {
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

    pub async fn query_sequence(
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

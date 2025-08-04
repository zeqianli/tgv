use crate::{
    alignment::{Alignment, AlignmentBuilder},
    bed::BEDIntervals,
    error::TGVError,
    helpers::is_url,
    reference::Reference,
    region::Region,
    sequence::{
        SequenceCache, SequenceRepositoryEnum, TwoBitSequenceRepository, UCSCApiSequenceRepository,
    },
    settings::{BackendType, Settings},
    tracks::{
        LocalDbTrackService, TrackService, TrackServiceEnum, UcscApiTrackService,
        UcscDbTrackService,
    },
    variant::VariantRepository,
};

use rust_htslib::bam;
use rust_htslib::bam::{Header, IndexedReader, Read};
use std::path::Path;
// Add twobit crate to Cargo.toml
use url::Url;

pub struct Repository {
    pub alignment_repository: AlignmentRepositoryEnum,

    pub variant_repository: Option<VariantRepository>,

    pub bed_intervals: Option<BEDIntervals>,

    pub track_service: Option<TrackServiceEnum>,

    pub sequence_service: Option<SequenceRepositoryEnum>,
}

impl Repository {
    pub async fn new(settings: &Settings) -> Result<(Self, Option<SequenceCache>), TGVError> {
        let alignment_repository = AlignmentRepositoryEnum::from(settings)?;

        let variant_repository = match &settings.vcf_path {
            Some(vcf_path) => Some(VariantRepository::from_vcf(vcf_path)?),
            None => None,
        };

        let bed_intervals = match &settings.bed_path {
            Some(bed_path) => Some(BEDIntervals::from_bed(bed_path)?),
            None => None,
        };

        let (track_service, sequence_service, sequence_cache): (
            Option<TrackServiceEnum>,
            Option<SequenceRepositoryEnum>,
            Option<SequenceCache>,
        ) = match settings.reference.as_ref() {
            Some(reference) => {
                let ts = match (&settings.backend, reference) {
                    (BackendType::Ucsc, Reference::UcscAccession(_)) => {
                        TrackServiceEnum::Api(UcscApiTrackService::new()?)
                    }
                    (BackendType::Ucsc, _) => TrackServiceEnum::Db(
                        UcscDbTrackService::new(reference, &settings.ucsc_host).await?,
                    ),
                    (BackendType::Local, _) => TrackServiceEnum::LocalDb(
                        LocalDbTrackService::new(reference, &settings.cache_dir).await?,
                    ),
                    (BackendType::Default, reference) => {
                        // If the local cache is available, use the local cache.
                        // Otherwise, use the UCSC DB / API.
                        match LocalDbTrackService::new(reference, &settings.cache_dir).await {
                            Ok(ts) => TrackServiceEnum::LocalDb(ts),
                            Err(TGVError::IOError(e)) => match reference {
                                Reference::UcscAccession(_) => {
                                    TrackServiceEnum::Api(UcscApiTrackService::new()?)
                                }
                                _ => TrackServiceEnum::Db(
                                    UcscDbTrackService::new(reference, &settings.ucsc_host).await?,
                                ),
                            },

                            Err(e) => return Err(e),
                        }
                    }
                    _ => {
                        return Err(TGVError::ValueError(format!(
                            "Unsupported reference: {}",
                            reference
                        )));
                    }
                };

                let use_ucsc_api_sequence =
                    matches!(ts, TrackServiceEnum::Api(_) | TrackServiceEnum::Db(_));

                let (ss, sc) = if use_ucsc_api_sequence {
                    (
                        SequenceRepositoryEnum::UCSCApi(UCSCApiSequenceRepository::new(
                            reference.clone(),
                        )?),
                        None,
                    )
                } else {
                    // query the chromInfo table to get the 2bit file path

                    let (ss, cache) = TwoBitSequenceRepository::new(
                        reference.clone(),
                        ts.get_contig_2bit_file_lookup(reference).await?,
                        settings.cache_dir.clone(),
                    )?;

                    (SequenceRepositoryEnum::TwoBit(ss), Some(cache))
                };
                (Some(ts), Some(ss), sc)
            }
            None => (None, None, None),
        };

        Ok((
            Self {
                alignment_repository,
                variant_repository,
                bed_intervals,
                track_service,
                sequence_service,
            },
            sequence_cache,
        ))
    }

    pub fn track_service_checked(&self) -> Result<&TrackServiceEnum, TGVError> {
        match self.track_service {
            Some(ref track_service) => Ok(track_service),
            None => Err(TGVError::StateError(
                "Track service is not initialized".to_string(),
            )),
        }
    }

    pub fn sequence_service_checked(&self) -> Result<&SequenceRepositoryEnum, TGVError> {
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

use crate::{
    alignment::{AlignedRead, Alignment},
    contig_header::ContigHeader,
    error::TGVError,
    helpers::is_url,
    intervals::Region,
    sequence::Sequence,
    settings::Settings,
};

use rust_htslib::bam::{self, Header, IndexedReader, Read};
use std::path::Path;
use url::Url;

#[derive(Debug)]
enum RemoteSource {
    S3,
    HTTP,
    GS,
}

impl RemoteSource {
    fn from(path: &str) -> Result<Self, TGVError> {
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
    fn read_alignment(
        &self,
        region: &Region,
        sequence: &Sequence,
        contig_header: &ContigHeader,
    ) -> Result<Alignment, TGVError>;

    fn read_header(&self) -> Result<Vec<(String, Option<usize>)>, TGVError>;
}

#[derive(Debug)]
pub struct BamRepository {
    bam_path: String,
    bai_path: Option<String>,
}

impl BamRepository {
    fn new(bam_path: &String, bai_path: Option<String>) -> Result<Self, TGVError> {
        if !Path::new(&bam_path).exists() {
            return Err(TGVError::IOError(format!(
                "BAM file {} not found",
                bam_path
            )));
        }

        match bai_path.as_ref() {
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

        Ok(Self {
            bam_path: bam_path.clone(),
            bai_path,
        })
    }
}

impl AlignmentRepository for BamRepository {
    /// Read an alignment from a BAM file.
    fn read_alignment(
        &self,
        region: &Region,
        sequence: &Sequence,
        contig_header: &ContigHeader,
    ) -> Result<Alignment, TGVError> {
        let mut bam = match self.bai_path.as_ref() {
            Some(bai_path) => {
                IndexedReader::from_path_and_index(self.bam_path.clone(), bai_path.clone())?
            }
            None => IndexedReader::from_path(self.bam_path.clone())?,
        };

        let header = bam::Header::from_template(bam.header());

        match get_query_contig_string(&header, region, contig_header)? {
            Some(query_contig_string) => {
                bam.fetch((
                    &query_contig_string,
                    region.start as i32 - 1,
                    region.end as i32,
                ))
                .map_err(|e| TGVError::IOError(e.to_string()))?;

                let reads = bam
                    .records()
                    .enumerate()
                    .map(|(i, record)| AlignedRead::from_bam_record(i, record?, sequence))
                    .collect::<Result<_, _>>()?;

                Alignment::from_aligned_reads(reads, region, sequence)
            }

            None => {
                // Contig indicated in region is not present in the BAM header.
                // Can happen when contigs in the reference and in the BAM header mismatches.
                Alignment::from_aligned_reads(Vec::new(), region, sequence)
            }
        }
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
    pub fn new(bam_path: &str) -> Result<Self, TGVError> {
        Ok(Self {
            bam_path: bam_path.to_string(),
            source: RemoteSource::from(bam_path)?,
        })
    }
}

impl AlignmentRepository for RemoteBamRepository {
    fn read_alignment(
        &self,
        region: &Region,
        sequence: &Sequence,
        contig_header: &ContigHeader,
    ) -> Result<Alignment, TGVError> {
        let mut bam = IndexedReader::from_url(
            &Url::parse(&self.bam_path).map_err(|e| TGVError::IOError(e.to_string()))?,
        )?;

        let header = bam::Header::from_template(bam.header());

        match get_query_contig_string(&header, region, contig_header)? {
            Some(query_contig_string) => {
                bam.fetch((
                    &query_contig_string,
                    region.start as i32 - 1,
                    region.end as i32,
                ))
                .map_err(|e| TGVError::IOError(e.to_string()))?;

                let reads = bam
                    .records()
                    .enumerate()
                    .map(|(i, record)| AlignedRead::from_bam_record(i, record?, sequence))
                    .collect::<Result<_, _>>()?;

                Alignment::from_aligned_reads(reads, region, sequence)
            }

            None => {
                // Contig indicated in region is not present in the BAM header.
                // Can happen when contigs in the reference and in the BAM header mismatches.
                Alignment::from_aligned_reads(Vec::new(), region, sequence)
            }
        }
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
fn get_query_contig_string(
    header: &Header,
    region: &Region,
    contig_header: &ContigHeader,
) -> Result<Option<String>, TGVError> {
    // FIXME:
    // BAM header is re-parsed for every alignment loading.
    // Parse this once and store it in the repository.
    let mut bam_headers = Vec::new();

    for (_key, records) in header.to_hashmap().iter() {
        for record in records {
            if record.contains_key("SN") {
                let reference_name = record["SN"].to_string();

                if reference_name == contig_header.get(region.contig_index)?.name
                    || contig_header
                        .get(region.contig_index)?
                        .aliases
                        .contains(&reference_name)
                {
                    return Ok(Some(reference_name));
                }

                bam_headers.push(reference_name);
            }
        }
    }

    // Contig is not in the BAM header.
    // This can happen when the reference contig names and the BAM contig names mismatch.
    Ok(None)
}

#[derive(Debug)]
pub enum AlignmentRepositoryEnum {
    Bam(BamRepository),
    RemoteBam(RemoteBamRepository),
}

impl AlignmentRepositoryEnum {
    pub fn new(settings: &Settings) -> Result<Option<Self>, TGVError> {
        match &settings.bam_path {
            None => Ok(None),
            Some(bam_path) => {
                if is_url(bam_path) {
                    Ok(Some(AlignmentRepositoryEnum::RemoteBam(
                        RemoteBamRepository::new(bam_path)?,
                    )))
                } else {
                    Ok(Some(AlignmentRepositoryEnum::Bam(BamRepository::new(
                        bam_path,
                        settings.bai_path.clone(),
                    )?)))
                }
            }
        }
    }

    pub fn has_alignment(&self) -> bool {
        match self {
            AlignmentRepositoryEnum::Bam(_) => true,
            AlignmentRepositoryEnum::RemoteBam(_) => true,
        }
    }
}

impl AlignmentRepository for AlignmentRepositoryEnum {
    fn read_alignment(
        &self,
        region: &Region,
        sequence: &Sequence,
        contig_header: &ContigHeader,
    ) -> Result<Alignment, TGVError> {
        match self {
            AlignmentRepositoryEnum::Bam(repository) => {
                repository.read_alignment(region, sequence, contig_header)
            }
            AlignmentRepositoryEnum::RemoteBam(repository) => {
                repository.read_alignment(region, sequence, contig_header)
            }
        }
    }

    fn read_header(&self) -> Result<Vec<(String, Option<usize>)>, TGVError> {
        match self {
            AlignmentRepositoryEnum::Bam(repository) => repository.read_header(),
            AlignmentRepositoryEnum::RemoteBam(repository) => repository.read_header(),
        }
    }
}

use crate::{
    alignment::{AlignedRead, Alignment},
    contig_header::ContigHeader,
    error::TGVError,
    intervals::{GenomeInterval, Region},
    sequence::Sequence,
    settings::Settings,
};

use async_compat::{Compat, CompatExt};
use itertools::Itertools;
use noodles::bam::{self, bai};
use noodles::cram::{self as cram, crai};
use noodles::fasta::{self as fasta, repository::adapters::IndexedReader as FastaIndexedReader};
use noodles::sam::Header;
use noodles::sam::alignment::RecordBuf;
use opendal::{FuturesAsyncReader, Operator, services};
use std::path::Path;
use tokio::fs::File;

pub struct BamRepository {
    bam_path: String,
    bai_path: String,

    index: bai::Index,

    header: Header,

    reader: bam::r#async::io::Reader<noodles::bgzf::r#async::io::Reader<File>>,
}

impl BamRepository {
    async fn new(bam_path: &str, bai_path: &str) -> Result<Self, TGVError> {
        use tokio::fs::File;

        let mut reader = File::open(bam_path)
            .await
            .map(bam::r#async::io::Reader::new)?;
        let header = reader.read_header().await?;

        let index = bai::r#async::fs::read(bai_path).await?;

        if !Path::new(&bam_path).exists() {
            return Err(TGVError::IOError(format!(
                "BAM file {} not found",
                bam_path
            )));
        }

        Ok(Self {
            bam_path: bam_path.to_string(),
            bai_path: bai_path.to_string(),

            index,
            header,
            reader,
        })
    }
}

pub struct CramRepository {
    cram_path: String,
    crai_path: String,
    fasta_path: String,
    fai_path: String,

    index: crai::Index,

    header: Header,

    reader: cram::r#async::io::Reader<File>,
}

impl CramRepository {
    async fn new(
        cram_path: &str,
        crai_path: &str,
        fasta_path: &str,
        fai_path: &str,
    ) -> Result<Self, TGVError> {
        if !Path::new(cram_path).exists() {
            return Err(TGVError::IOError(format!(
                "CRAM file {} not found",
                cram_path
            )));
        }

        let repository = fasta::io::indexed_reader::Builder::default()
            .build_from_path(fasta_path)
            .map(FastaIndexedReader::new)
            .map(fasta::Repository::new)?;

        let mut reader = cram::r#async::io::reader::Builder::default()
            .set_reference_sequence_repository(repository)
            .build_from_path(cram_path)
            .await?;

        let header = reader.read_header().await?;

        let index = crai::r#async::read(crai_path).await?;

        Ok(Self {
            cram_path: cram_path.to_string(),
            crai_path: crai_path.to_string(),
            fasta_path: fasta_path.to_string(),
            fai_path: fai_path.to_string(),
            index,
            header,
            reader,
        })
    }
}

pub struct RemoteBamRepository {
    bam_path: String,
    bai_path: String,

    index: bai::Index,

    header: Header,

    reader:
        bam::r#async::io::Reader<noodles::bgzf::r#async::io::Reader<Compat<FuturesAsyncReader>>>,
}

impl RemoteBamRepository {
    pub async fn new(s3_bam_path: &str, s3_bai_path: &str) -> Result<Self, TGVError> {
        let (bucket, name) = s3_bam_path
            .strip_prefix("s3://")
            .unwrap()
            .split_once("/")
            .unwrap();

        let builder = services::S3::default().bucket(bucket);

        let operator = Operator::new(builder)?.finish();

        let index = Self::read_index(s3_bai_path).await?;

        let stream = operator
            .reader(name)
            .await?
            .into_futures_async_read(..)
            .await?;

        let mut reader = bam::r#async::io::Reader::new(stream.compat());

        let header = reader.read_header().await?;

        let index = Self::read_index(s3_bai_path).await?;

        Ok(Self {
            bam_path: s3_bam_path.to_string(),
            bai_path: s3_bai_path.to_string(),

            index,

            header,
            reader,
        })
    }

    async fn read_index(s3_bai_path: &str) -> Result<bai::Index, TGVError> {
        let (bucket, name) = s3_bai_path
            .strip_prefix("s3://")
            .unwrap()
            .split_once("/")
            .unwrap();

        let builder = services::S3::default().bucket(bucket);

        let operator = Operator::new(builder)?.finish();

        let stream = operator
            .reader(name)
            .await?
            .into_futures_async_read(..)
            .await?;

        let mut reader = bai::r#async::io::Reader::new(stream.compat());

        Ok(reader.read_index().await?)
    }
}

fn get_contig_names_and_lengths_from_header(
    header: &Header,
) -> Result<Vec<(String, Option<usize>)>, TGVError> {
    Ok(header
        .reference_sequences()
        .iter()
        .map(|(contig_name, record)| (contig_name.to_string(), Some(record.length().get())))
        .collect_vec())
}

pub enum AlignmentRepositoryEnum {
    Bam(BamRepository),
    RemoteBam(RemoteBamRepository),
    Cram(CramRepository),
}

impl AlignmentRepositoryEnum {
    pub async fn new(
        alignment_path: &str,
        index_path: &str,
        fasta_path: Option<&str>,
    ) -> Result<Self, TGVError> {
        if is_url(alignment_path) {
            Ok(AlignmentRepositoryEnum::RemoteBam(
                RemoteBamRepository::new(alignment_path, index_path).await?,
            ))
        } else if alignment_path.ends_with(".cram") {
            let fasta = fasta_path.ok_or_else(|| {
                TGVError::IOError("a reference FASTA path is required for CRAM files".to_string())
            })?;
            let fai = format!("{}.fai", fasta.to_string());
            Ok(AlignmentRepositoryEnum::Cram(
                CramRepository::new(alignment_path, index_path, fasta, fai.as_ref()).await?,
            ))
        } else {
            Ok(AlignmentRepositoryEnum::Bam(
                BamRepository::new(alignment_path, index_path).await?,
            ))
        }
    }
}

impl AlignmentRepositoryEnum {
    pub async fn read_alignment(
        &mut self,
        region: &Region,
        reference_sequence: &Sequence,
        contig_header: &ContigHeader,
    ) -> Result<Alignment, TGVError> {
        use futures::TryStreamExt;

        let records = match region.alignment(contig_header)? {
            Some(region) => {
                let mut records = Vec::new();
                let mut index = 0;
                match self {
                    AlignmentRepositoryEnum::Bam(inner) => {
                        let mut query = inner.reader.query(&inner.header, &inner.index, &region)?;

                        while let Some(record) = query.try_next().await? {
                            records.push(AlignedRead::from_bam_record(
                                index,
                                record,
                                reference_sequence,
                            )?);
                            index += 1;
                        }
                    }
                    AlignmentRepositoryEnum::RemoteBam(inner) => {
                        let mut query = inner.reader.query(&inner.header, &inner.index, &region)?;

                        while let Some(record) = query.try_next().await? {
                            records.push(AlignedRead::from_bam_record(
                                index,
                                record,
                                reference_sequence,
                            )?);
                            index += 1;
                        }
                    }
                    AlignmentRepositoryEnum::Cram(inner) => {
                        let mut query = inner.reader.query(&inner.header, &inner.index, &region)?;

                        while let Some(record_buf) = query.try_next().await? {
                            let bam_record = cram_record_to_bam_record(&inner.header, &record_buf)?;
                            records.push(AlignedRead::from_bam_record(
                                index,
                                bam_record,
                                reference_sequence,
                            )?);
                            index += 1;
                        }
                    }
                };

                records
            }
            None => Vec::new(),
        };

        Alignment::from_aligned_reads(
            records,
            region.contig_index(),
            (region.start(), region.end()),
            reference_sequence,
        )
    }

    /// Read BAM headers and return contig namesa and lengths.
    /// Note that this function does not interprete the contig name as contg vs chromosome.
    pub fn read_header(&self) -> Result<Vec<(String, Option<usize>)>, TGVError> {
        let header = match self {
            AlignmentRepositoryEnum::Bam(inner) => &inner.header,
            AlignmentRepositoryEnum::RemoteBam(inner) => &inner.header,
            AlignmentRepositoryEnum::Cram(inner) => &inner.header,
        };
        get_contig_names_and_lengths_from_header(header)
    }
}

/// Convert a SAM `RecordBuf` (as returned by CRAM queries) to a `bam::Record`.
///
/// CRAM queries yield `RecordBuf` records, but `AlignedRead` is built around `bam::Record`.
/// This function bridges the two by round-tripping through an in-memory BAM encoding.
fn cram_record_to_bam_record(
    header: &Header,
    record_buf: &RecordBuf,
) -> Result<bam::Record, TGVError> {
    use noodles::sam::alignment::io::Write as AlignmentWrite;

    let mut buf = Vec::new();
    let mut writer = bam::io::Writer::from(&mut buf);
    writer.write_alignment_record(header, record_buf)?;
    drop(writer);

    let mut reader = bam::io::Reader::from(&buf[..]);
    let mut record = bam::Record::default();
    reader.read_record(&mut record)?;
    Ok(record)
}

pub fn is_url(path: &str) -> bool {
    path.starts_with("s3://")
        || path.starts_with("http://")
        || path.starts_with("https://")
        || path.starts_with("gs://")
}

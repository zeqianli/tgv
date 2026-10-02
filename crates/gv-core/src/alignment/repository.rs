use crate::{
    alignment::{Alignment, AlignmentTables},
    contig_header::ContigHeader,
    error::TGVError,
    intervals::{GenomeInterval, Region},
    sequence::Sequence,
    settings::{AlignmentPath, BamSource},
};

use async_compat::CompatExt;
use futures::TryStreamExt;
use itertools::Itertools;
use noodles::cram::{self as cram};
use noodles::fasta::{self as fasta, repository::adapters::IndexedReader as FastaIndexedReader};
use noodles::sam::Header;
use noodles::{
    bam::{self, bai},
    sam::alignment::RecordBuf,
};
use opendal::{Operator, services};
use std::fs;
use std::path::Path;
use std::time::Instant;
use tokio::fs::File;

const RECORD_BATCH_SIZE: usize = 1024;

pub struct BamRepository {
    bam_path: String,
    bai_path: String,

    index: bai::Index,

    header: Header,
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
        })
    }
}

pub struct CramRepository {
    cram_path: String,
    crai_path: String,
    fasta_path: String,
    fai_path: String,

    //index: crai::Index,
    header: Header,

    reader: cram::io::indexed_reader::IndexedReader<fs::File>,
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

        let mut reader = cram::io::indexed_reader::Builder::default()
            .set_reference_sequence_repository(repository)
            .build_from_path(cram_path)?;

        let header = reader.read_header()?;

        // let index = fs::File::open(fai_path)
        //     .map(crai::io::Reader::new)?
        //     .read_index()?;

        Ok(Self {
            cram_path: cram_path.to_string(),
            crai_path: crai_path.to_string(),
            fasta_path: fasta_path.to_string(),
            fai_path: fai_path.to_string(),
            //index,
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

    operator: Operator,
    key: String,
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

        let (index_bucket, index_name) = s3_bai_path
            .strip_prefix("s3://")
            .unwrap()
            .split_once("/")
            .unwrap();
        let index_operator = Operator::new(services::S3::default().bucket(index_bucket))?.finish();
        log::info!(
            "Object storage request: operation=read object_url={} bucket={} key={} context=remote BAM index",
            s3_bai_path,
            index_bucket,
            index_name
        );
        let index_stream = index_operator
            .reader(index_name)
            .await?
            .into_futures_async_read(..)
            .await?;
        let mut index_reader = bai::r#async::io::Reader::new(index_stream.compat());
        let index = index_reader.read_index().await?;

        log::info!(
            "Object storage request: operation=read object_url={} bucket={} key={} context=remote BAM header",
            s3_bam_path,
            bucket,
            name
        );
        let stream = operator
            .reader(name)
            .await?
            .into_futures_async_read(..)
            .await?;

        let mut reader = bam::r#async::io::Reader::new(stream.compat());

        let header = reader.read_header().await?;

        Ok(Self {
            bam_path: s3_bam_path.to_string(),
            bai_path: s3_bai_path.to_string(),

            index,

            header,
            operator,
            key: name.to_owned(),
        })
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
    pub fn path(&self) -> &str {
        match self {
            Self::Bam(inner) => &inner.bam_path,
            Self::RemoteBam(inner) => &inner.bam_path,
            Self::Cram(inner) => &inner.cram_path,
        }
    }

    pub async fn new(alignment_path: &AlignmentPath) -> Result<Self, TGVError> {
        match alignment_path {
            AlignmentPath::Bam {
                path,
                index,
                source: BamSource::S3,
            } => Ok(AlignmentRepositoryEnum::RemoteBam(
                RemoteBamRepository::new(path, index).await?,
            )),
            AlignmentPath::Bam {
                path,
                index,
                source: BamSource::Local,
            } => Ok(AlignmentRepositoryEnum::Bam(
                BamRepository::new(path, index).await?,
            )),
            AlignmentPath::Cram {
                path,
                crai,
                fasta,
                fai,
            } => Ok(AlignmentRepositoryEnum::Cram(
                CramRepository::new(path, crai, fasta, fai).await?,
            )),
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
        let started = Instant::now();
        let (source_kind, data_path, index_path) = match self {
            AlignmentRepositoryEnum::Bam(inner) => {
                ("BAM", inner.bam_path.clone(), inner.bai_path.clone())
            }
            AlignmentRepositoryEnum::RemoteBam(inner) => {
                ("remote BAM", inner.bam_path.clone(), inner.bai_path.clone())
            }
            AlignmentRepositoryEnum::Cram(inner) => {
                ("CRAM", inner.cram_path.clone(), inner.crai_path.clone())
            }
        };
        log::debug!(
            "Reading alignment records: source_type={} path={} index={} region={:?}",
            source_kind,
            data_path,
            index_path,
            region,
        );

        let query_region = match region.alignment(contig_header) {
            Ok(query_region) => query_region,
            Err(e) => {
                log::warn!(
                    "Failed to resolve alignment query region: source_type={} path={} index={} region={:?} elapsed_ms={} error={e}",
                    source_kind,
                    data_path,
                    index_path,
                    region,
                    started.elapsed().as_millis(),
                );
                return Err(e);
            }
        };

        let mut records = Vec::new();
        let mut batch = Vec::with_capacity(RECORD_BATCH_SIZE);
        let mut tables = AlignmentTables::default();
        match query_region {
            Some(query_region) => {
                match self {
                    AlignmentRepositoryEnum::Bam(inner) => {
                        // Reopen the reader because repeated queries at the same BGZF offset
                        // can otherwise reuse a completed seek and return no records.
                        let file = File::open(&inner.bam_path).await?;
                        let mut reader = bam::r#async::io::Reader::new(file);
                        let mut query = reader
                            .query(&inner.header, &inner.index, &query_region)?
                            .records();

                        while let Some(record) = query.try_next().await? {
                            batch.push(RecordBuf::try_from_alignment_record(
                                &inner.header,
                                &record,
                            )?);
                            if batch.len() == RECORD_BATCH_SIZE {
                                tables = tables.add_records(
                                    &batch,
                                    reference_sequence,
                                    region.contig_index(),
                                )?;
                                records.append(&mut batch);
                            }
                        }
                    }
                    AlignmentRepositoryEnum::RemoteBam(inner) => {
                        log::info!(
                            "Object storage request: operation=query object_url={} index_url={} region={:?} context=remote BAM records",
                            inner.bam_path,
                            inner.bai_path,
                            region
                        );
                        let stream = inner
                            .operator
                            .reader(&inner.key)
                            .await?
                            .into_futures_async_read(..)
                            .await?;
                        let mut reader = bam::r#async::io::Reader::new(stream.compat());
                        let mut query = reader
                            .query(&inner.header, &inner.index, &query_region)?
                            .records();

                        while let Some(record) = query.try_next().await? {
                            batch.push(RecordBuf::try_from_alignment_record(
                                &inner.header,
                                &record,
                            )?);
                            if batch.len() == RECORD_BATCH_SIZE {
                                tables = tables.add_records(
                                    &batch,
                                    reference_sequence,
                                    region.contig_index(),
                                )?;
                                records.append(&mut batch);
                            }
                        }
                    }
                    AlignmentRepositoryEnum::Cram(inner) => {
                        let query = inner.reader.query(&inner.header, &query_region)?;

                        for record in query {
                            batch.push(record?);
                            if batch.len() == RECORD_BATCH_SIZE {
                                tables = tables.add_records(
                                    &batch,
                                    reference_sequence,
                                    region.contig_index(),
                                )?;
                                records.append(&mut batch);
                            }
                        }
                    }
                };
            }
            None => {
                log::debug!(
                    "Skipped alignment record query because the region does not map to the alignment header: source_type={} path={} index={} region={:?} elapsed_ms={}",
                    source_kind,
                    data_path,
                    index_path,
                    region,
                    started.elapsed().as_millis(),
                );
            }
        };

        tables = tables.add_records(&batch, reference_sequence, region.contig_index())?;
        records.append(&mut batch);
        let record_count = records.len();
        let alignment = match Alignment::from_tables(
            records,
            tables,
            region.contig_index(),
            (region.start(), region.end()),
            reference_sequence,
        ) {
            Ok(alignment) => alignment,
            Err(e) => {
                log::warn!(
                    "Failed to build alignment from records: source_type={} path={} index={} region={:?} records={} elapsed_ms={} error={e}",
                    source_kind,
                    data_path,
                    index_path,
                    region,
                    record_count,
                    started.elapsed().as_millis(),
                );
                return Err(e);
            }
        };

        log::debug!(
            "Read alignment records: source_type={} path={} index={} region={:?} records={} elapsed_ms={}",
            source_kind,
            data_path,
            index_path,
            region,
            record_count,
            started.elapsed().as_millis(),
        );

        Ok(alignment)
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

pub fn is_url(path: &str) -> bool {
    path.starts_with("s3://")
        || path.starts_with("http://")
        || path.starts_with("https://")
        || path.starts_with("gs://")
}

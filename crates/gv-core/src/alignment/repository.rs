use crate::{
    alignment::{Alignment, AlignmentTables, QualityEncoding, QualityEncodingSetting},
    error::TGVError,
    intervals::Region,
    sequence::Sequence,
    settings::{AlignmentPath, BamSource},
};

use crate::object_storage::S3Object;
use futures::TryStreamExt;
use itertools::Itertools;
use noodles::cram::{self as cram};
use noodles::fasta::{self as fasta, repository::adapters::IndexedReader as FastaIndexedReader};
use noodles::sam::Header;
use noodles::{
    bam::{self, bai},
    sam::alignment::RecordBuf,
};
use opendal::Operator;
use std::fs;
use std::path::Path;
use std::time::Instant;
use tokio::fs::File;

const RECORD_BATCH_SIZE: usize = 1024;
const QUERY_REMOTE_BAM: &str = "query the BAM";

pub struct BamRepository {
    bam_path: String,
    bai_path: String,

    index: bai::Index,

    header: Header,
}

impl BamRepository {
    async fn new(bam_path: &str, bai_path: &str) -> Result<Self, TGVError> {
        use tokio::fs::File;

        if !Path::new(bam_path).exists() {
            return Err(TGVError::NotAFile(bam_path.to_string()));
        }
        if !Path::new(bai_path).exists() {
            return Err(TGVError::MissingIndex {
                file: bam_path.to_string(),
                index: bai_path.to_string(),
            });
        }

        let failed = |operation, path: &str, source| TGVError::FileOperationError {
            operation,
            path: path.into(),
            source,
        };
        let mut reader = File::open(bam_path)
            .await
            .map(bam::r#async::io::Reader::new)
            .map_err(|source| failed("open the BAM", bam_path, source))?;
        let header = reader
            .read_header()
            .await
            .map_err(|source| failed("read the BAM header", bam_path, source))?;

        let index = bai::r#async::fs::read(bai_path)
            .await
            .map_err(|source| failed("read the BAM index", bai_path, source))?;

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
    object: S3Object,
    index_object: S3Object,

    index: bai::Index,

    header: Header,

    operator: Operator,
}

impl RemoteBamRepository {
    pub async fn new(s3_bam_path: &str, s3_bai_path: &str) -> Result<Self, TGVError> {
        let object = S3Object::parse(s3_bam_path)?;
        let index_object = S3Object::parse(s3_bai_path)?;
        let operator = object.operator()?;
        // The index can live in another bucket.
        let index_operator = if index_object.bucket == object.bucket {
            operator.clone()
        } else {
            index_object.operator()?
        };

        const READ_INDEX: &str = "read the BAM index";
        let index = bai::r#async::io::Reader::new(
            index_object.open_read(&index_operator, READ_INDEX).await?,
        )
        .read_index()
        .await
        .map_err(index_object.read_error(READ_INDEX))?;

        const READ_HEADER: &str = "read the BAM header";
        let header = bam::r#async::io::Reader::new(object.open_read(&operator, READ_HEADER).await?)
            .read_header()
            .await
            .map_err(object.read_error(READ_HEADER))?;

        Ok(Self {
            object,
            index_object,
            index,
            header,
            operator,
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

/// An alignment file and the quality encoding its records use.
pub struct AlignmentRepository {
    pub source: AlignmentRepositoryEnum,
    /// An automatic setting resolves at the first load with qualities and stays fixed after.
    pub quality_encoding: QualityEncodingSetting,
}

impl AlignmentRepository {
    pub async fn new(
        alignment_path: &AlignmentPath,
        quality_encoding: QualityEncodingSetting,
    ) -> Result<Self, TGVError> {
        Ok(Self {
            source: AlignmentRepositoryEnum::new(alignment_path).await?,
            quality_encoding,
        })
    }

    /// Reads the alignments in a region. `contig_name` is the file's name for the region's
    /// contig, or `None` when the file doesn't have it, which reads nothing.
    pub async fn read_alignment(
        &mut self,
        region: &Region,
        contig_name: Option<&str>,
        reference_sequence: &Sequence,
    ) -> Result<Alignment, TGVError> {
        self.source
            .read_alignment(
                region,
                contig_name,
                reference_sequence,
                &mut self.quality_encoding,
            )
            .await
    }
}

pub enum AlignmentRepositoryEnum {
    Bam(BamRepository),
    RemoteBam(RemoteBamRepository),
    Cram(CramRepository),
}

impl AlignmentRepositoryEnum {
    /// The alignment file's header, which names the references that records point to.
    pub fn header(&self) -> &Header {
        match self {
            Self::Bam(inner) => &inner.header,
            Self::RemoteBam(inner) => &inner.header,
            Self::Cram(inner) => &inner.header,
        }
    }

    pub fn path(&self) -> &str {
        match self {
            Self::Bam(inner) => &inner.bam_path,
            Self::RemoteBam(inner) => &inner.object.url,
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
        contig_name: Option<&str>,
        reference_sequence: &Sequence,
        quality_encoding: &mut QualityEncodingSetting,
    ) -> Result<Alignment, TGVError> {
        let started = Instant::now();
        let (source_kind, data_path, index_path) = match self {
            AlignmentRepositoryEnum::Bam(inner) => {
                ("BAM", inner.bam_path.clone(), inner.bai_path.clone())
            }
            AlignmentRepositoryEnum::RemoteBam(inner) => (
                "remote BAM",
                inner.object.url.clone(),
                inner.index_object.url.clone(),
            ),
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

        let query_region = contig_name
            .map(|name| region.to_noodles_region(name))
            .transpose()?;

        let mut records = Vec::new();
        let mut batch = Vec::with_capacity(RECORD_BATCH_SIZE);
        let mut tables = AlignmentTables::default();
        match query_region {
            Some(query_region) => {
                match self {
                    AlignmentRepositoryEnum::Bam(inner) => {
                        // Reopen the reader because repeated queries at the same BGZF offset
                        // can otherwise reuse a completed seek and return no records.
                        let file = File::open(&inner.bam_path).await.map_err(|source| {
                            TGVError::FileOperationError {
                                operation: "open the BAM",
                                path: inner.bam_path.clone().into(),
                                source,
                            }
                        })?;
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
                                    quality_encoding.resolve(&batch),
                                    reference_sequence,
                                    region.contig_index(),
                                )?;
                                records.append(&mut batch);
                            }
                        }
                    }
                    AlignmentRepositoryEnum::RemoteBam(inner) => {
                        let stream = inner
                            .object
                            .open_read(&inner.operator, QUERY_REMOTE_BAM)
                            .await?;
                        let mut reader = bam::r#async::io::Reader::new(stream);
                        let read_error = inner.object.read_error(QUERY_REMOTE_BAM);
                        let mut query = reader
                            .query(&inner.header, &inner.index, &query_region)
                            .map_err(&read_error)?
                            .records();

                        while let Some(record) = query.try_next().await.map_err(&read_error)? {
                            batch.push(RecordBuf::try_from_alignment_record(
                                &inner.header,
                                &record,
                            )?);
                            if batch.len() == RECORD_BATCH_SIZE {
                                tables = tables.add_records(
                                    &batch,
                                    quality_encoding.resolve(&batch),
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
                                    quality_encoding.resolve(&batch),
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
                    "Skipped alignment record query because the file does not have the contig: source_type={} path={} index={} region={:?} elapsed_ms={}",
                    source_kind,
                    data_path,
                    index_path,
                    region,
                    started.elapsed().as_millis(),
                );
            }
        };

        tables = tables.add_records(
            &batch,
            quality_encoding.resolve(&batch),
            reference_sequence,
            region.contig_index(),
        )?;
        records.append(&mut batch);
        let record_count = records.len();
        // Records without qualities leave an automatic setting unresolved, and the encoding
        // does not affect them.
        let encoding = match *quality_encoding {
            QualityEncodingSetting::Phred64 => QualityEncoding::Phred64,
            QualityEncodingSetting::Phred33 | QualityEncodingSetting::Auto => {
                QualityEncoding::Phred33
            }
        };
        let alignment = match Alignment::from_tables(
            records,
            tables,
            encoding,
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

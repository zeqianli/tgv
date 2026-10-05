use crate::{
    alignment::{Alignment, AlignmentTables},
    contig_header::ContigHeader,
    error::TGVError,
    intervals::{GenomeInterval, Region},
    sequence::Sequence,
    settings::{AlignmentPath, BamSource},
};

use async_compat::CompatExt;
use futures::{StreamExt, TryStreamExt, stream};
use itertools::Itertools;
use noodles::cram::{self as cram};
use noodles::fasta::{self as fasta, repository::adapters::IndexedReader as FastaIndexedReader};
use noodles::sam::Header;
use noodles::{
    bam::{self, bai},
    bgzf::{self, VirtualPosition},
    core::region::Interval,
    csi::{BinningIndex, binning_index::index::reference_sequence::bin::Chunk},
    sam::alignment::{Record as _, RecordBuf},
};
use opendal::{Operator, services};
use std::fs;
use std::io::{Cursor, SeekFrom};
use std::num::NonZero;
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

/// Records per table-building task.
const RECORD_BATCH_SIZE: usize = 1024;

/// The largest BGZF block, including its header and footer.
///
/// A chunk's end virtual position names the block that holds its last record, so reading this
/// far past that block's start covers the whole block.
const MAX_BGZF_BLOCK_SIZE: u64 = 64 * 1024;

/// The number of range requests in flight for one remote BAM query.
const REMOTE_READ_CONCURRENCY: usize = 8;

/// The size of each remote range request.
///
/// A region query usually maps to one chunk of a few megabytes, so smaller requests spread it
/// across more connections. Object stores reach full per-connection throughput well below this.
const REMOTE_READ_CHUNK_SIZE: usize = 512 * 1024;

pub struct BamRepository {
    bam_path: String,
    bai_path: String,

    index: bai::Index,

    /// Shared with the table-building tasks of a query.
    header: Arc<Header>,
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
            header: Arc::new(header),
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

    /// Shared with the table-building tasks of a query.
    header: Arc<Header>,

    operator: Operator,
    key: String,

    /// The object size, which bounds range requests that extend past a chunk's last block.
    content_length: u64,
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
        let content_length = operator.stat(name).await?.content_length();

        Ok(Self {
            bam_path: s3_bam_path.to_string(),
            bai_path: s3_bai_path.to_string(),

            index,

            header: Arc::new(header),
            operator,
            key: name.to_owned(),
            content_length,
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

        let contig_index = region.contig_index();
        let (records, tables) = match query_region {
            Some(query_region) => match self {
                AlignmentRepositoryEnum::Bam(inner) => {
                    let query = ChunkQuery::new(&inner.header, &inner.index, &query_region)?;
                    let mut file = File::open(&inner.bam_path).await?;
                    let ranges = query.byte_ranges(file.metadata().await?.len());
                    let mut buffers = Vec::with_capacity(ranges.len());
                    for range in &ranges {
                        file.seek(SeekFrom::Start(range.start)).await?;
                        let mut buffer = vec![0; (range.end - range.start) as usize];
                        file.read_exact(&mut buffer).await?;
                        buffers.push(buffer);
                    }
                    let records = query.read_records(&ranges, buffers).await?;
                    let header = Arc::clone(&inner.header);
                    build_tables(
                        records,
                        move |record| Ok(RecordBuf::try_from_alignment_record(&header, &record)?),
                        reference_sequence,
                        contig_index,
                    )
                    .await?
                }
                AlignmentRepositoryEnum::RemoteBam(inner) => {
                    let query = ChunkQuery::new(&inner.header, &inner.index, &query_region)?;
                    let ranges = query.byte_ranges(inner.content_length);
                    log::info!(
                        "Object storage request: operation=query object_url={} index_url={} region={:?} ranges={} bytes={} context=remote BAM records",
                        inner.bam_path,
                        inner.bai_path,
                        region,
                        ranges.len(),
                        ranges.iter().map(|range| range.end - range.start).sum::<u64>(),
                    );
                    // `fetch` panics on an empty range list.
                    let buffers = if ranges.is_empty() {
                        Vec::new()
                    } else {
                        // Each range is split into concurrent chunk requests, and `fetch` also
                        // merges nearby ranges, so a region costs a few round trips in parallel
                        // instead of one sequential stream with a request per seek.
                        inner
                            .operator
                            .reader_with(&inner.key)
                            .concurrent(REMOTE_READ_CONCURRENCY)
                            .chunk(REMOTE_READ_CHUNK_SIZE)
                            .await?
                            .fetch(ranges.clone())
                            .await?
                            .into_iter()
                            .map(|buffer| buffer.to_vec())
                            .collect()
                    };
                    let records = query.read_records(&ranges, buffers).await?;
                    let header = Arc::clone(&inner.header);
                    build_tables(
                        records,
                        move |record| Ok(RecordBuf::try_from_alignment_record(&header, &record)?),
                        reference_sequence,
                        contig_index,
                    )
                    .await?
                }
                AlignmentRepositoryEnum::Cram(inner) => {
                    let records = inner
                        .reader
                        .query(&inner.header, &query_region)?
                        .collect::<Result<Vec<_>, _>>()?;
                    build_tables(records, Ok, reference_sequence, contig_index).await?
                }
            },
            None => {
                log::debug!(
                    "Skipped alignment record query because the region does not map to the alignment header: source_type={} path={} index={} region={:?} elapsed_ms={}",
                    source_kind,
                    data_path,
                    index_path,
                    region,
                    started.elapsed().as_millis(),
                );
                (Vec::new(), AlignmentTables::default())
            }
        };

        let record_count = records.len();
        let alignment = match Alignment::from_tables(
            records,
            tables,
            contig_index,
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
            AlignmentRepositoryEnum::Bam(inner) => inner.header.as_ref(),
            AlignmentRepositoryEnum::RemoteBam(inner) => inner.header.as_ref(),
            AlignmentRepositoryEnum::Cram(inner) => &inner.header,
        };
        get_contig_names_and_lengths_from_header(header)
    }
}

/// The index chunks of one BAM region query.
struct ChunkQuery {
    reference_sequence_id: usize,
    interval: Interval,

    /// Sorted, non-overlapping chunks, so every record is read at most once.
    chunks: Vec<Chunk>,
}

impl ChunkQuery {
    fn new(
        header: &Header,
        index: &bai::Index,
        region: &noodles::core::Region,
    ) -> Result<Self, TGVError> {
        let reference_sequence_id = header
            .reference_sequences()
            .get_index_of(region.name())
            .ok_or_else(|| {
                TGVError::IOError(format!(
                    "Reference sequence {} is not in the BAM header",
                    region.name()
                ))
            })?;
        let chunks = index.query(reference_sequence_id, region.interval())?;
        Ok(Self {
            reference_sequence_id,
            interval: region.interval(),
            chunks,
        })
    }

    /// Return the compressed byte range that holds each chunk, in chunk order.
    fn byte_ranges(&self, file_length: u64) -> Vec<Range<u64>> {
        self.chunks
            .iter()
            .map(|chunk| {
                chunk.start().compressed()
                    ..(chunk.end().compressed() + MAX_BGZF_BLOCK_SIZE).min(file_length)
            })
            .collect()
    }

    /// Decode the region's records from the bytes of each chunk's range, in file order.
    async fn read_records(
        &self,
        ranges: &[Range<u64>],
        buffers: Vec<Vec<u8>>,
    ) -> Result<Vec<bam::Record>, TGVError> {
        let chunks = futures::future::try_join_all(
            self.chunks
                .iter()
                .zip(ranges)
                .zip(buffers)
                .map(|((chunk, range), buffer)| self.read_chunk(*chunk, range.start, buffer)),
        )
        .await?;
        Ok(chunks.into_iter().flatten().collect())
    }

    async fn read_chunk(
        &self,
        chunk: Chunk,
        range_start: u64,
        buffer: Vec<u8>,
    ) -> Result<Vec<bam::Record>, TGVError> {
        // The buffer starts at `range_start`, so chunk positions are rebased onto it.
        let rebase = |position: VirtualPosition| {
            VirtualPosition::try_from((position.compressed() - range_start, position.uncompressed()))
                .map_err(|e| TGVError::IOError(format!("Invalid BAM chunk position: {e}")))
        };
        let (start, end) = (rebase(chunk.start())?, rebase(chunk.end())?);
        // The BGZF reader inflates blocks on the blocking pool in parallel.
        let mut reader =
            bam::r#async::io::Reader::from(bgzf::r#async::io::Reader::new(Cursor::new(buffer)));
        reader.get_mut().seek(start).await?;
        let mut records = Vec::new();
        while reader.get_ref().virtual_position() < end {
            let mut record = bam::Record::default();
            if reader.read_record(&mut record).await? == 0 {
                break;
            }
            if self.intersects(&record)? {
                records.push(record);
            }
        }
        Ok(records)
    }

    /// Chunks cover whole bins, so drop the records outside the queried interval.
    fn intersects(&self, record: &bam::Record) -> Result<bool, TGVError> {
        if record.reference_sequence_id().transpose()? != Some(self.reference_sequence_id) {
            return Ok(false);
        }
        Ok(
            match (
                record.alignment_start().transpose()?,
                record.alignment_end().transpose()?,
            ) {
                (Some(start), Some(end)) => self.interval.intersects((start..=end).into()),
                _ => false,
            },
        )
    }
}

/// Build the tables of record batches on the blocking pool, then join them in read ID order.
///
/// Batches are independent once their read ID offsets are known, so converting records and
/// building their tables runs on all cores instead of in sequence on the async task.
async fn build_tables<T, F>(
    records: Vec<T>,
    to_record_buf: F,
    reference_sequence: &Sequence,
    contig_index: usize,
) -> Result<(Vec<RecordBuf>, AlignmentTables), TGVError>
where
    T: Send + 'static,
    F: Fn(T) -> Result<RecordBuf, TGVError> + Clone + Send + 'static,
{
    let record_count = records.len();
    let reference_sequence = Arc::new(reference_sequence.clone());
    let batches = records
        .into_iter()
        .chunks(RECORD_BATCH_SIZE)
        .into_iter()
        .map(Iterator::collect::<Vec<_>>)
        .collect::<Vec<_>>();
    // Bound the tasks in flight so a large region does not grow the blocking pool past the core
    // count while BGZF inflation shares it.
    let parallelism = std::thread::available_parallelism().map_or(1, NonZero::get);
    let built = stream::iter(batches.into_iter().enumerate().map(|(index, batch)| {
        let to_record_buf = to_record_buf.clone();
        let reference_sequence = Arc::clone(&reference_sequence);
        async move {
            tokio::task::spawn_blocking(move || {
                let records = batch
                    .into_iter()
                    .map(to_record_buf)
                    .collect::<Result<Vec<_>, _>>()?;
                let tables = AlignmentTables::from_batch(
                    &records,
                    &reference_sequence,
                    contig_index,
                    (index * RECORD_BATCH_SIZE) as u64,
                )?;
                Ok::<_, TGVError>((records, tables))
            })
            .await?
        }
    }))
    .buffered(parallelism)
    .try_collect::<Vec<_>>()
    .await?;

    let mut records = Vec::with_capacity(record_count);
    let mut batches = Vec::with_capacity(built.len());
    for (mut batch_records, tables) in built {
        records.append(&mut batch_records);
        batches.push(tables);
    }
    Ok((records, AlignmentTables::concat(batches)?))
}

pub fn is_url(path: &str) -> bool {
    path.starts_with("s3://")
        || path.starts_with("http://")
        || path.starts_with("https://")
        || path.starts_with("gs://")
}

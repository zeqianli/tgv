use crate::{
    alignment::{AlignedRead, Alignment},
    contig_header::ContigHeader,
    error::TGVError,
    intervals::Region,
    sequence::Sequence,
    settings::Settings,
};

use async_compat::Compat;
use itertools::Itertools;
use noodles::sam::{self, Header};
use noodles::{
    bam::{self, bai},
    vcf::header::record::value::map::contig,
};
use opendal::{services, FuturesAsyncReader, Operator};
use std::path::Path;
use tokio::fs::File;
use tokio_util::{bytes::Bytes, io::StreamReader};
use url::Url;

// #[derive(Debug)]
// enum RemoteSource {
//     S3,
//     HTTP,
//     GS,
// }

// impl RemoteSource {
//     fn from(path: &str) -> Result<Self, TGVError> {
//         if path.starts_with("s3://") {
//             Ok(Self::S3)
//         } else if path.starts_with("http://") || path.starts_with("https://") {
//             Ok(Self::HTTP)
//         } else if path.starts_with("gss://") {
//             Ok(Self::GS)
//         } else {
//             Err(TGVError::ValueError(format!(
//                 "Unsupported remote path {}. Only S3, HTTP/HTTPS, and GS are supported.",
//                 path
//             )))
//         }
//     }
// }

// pub trait AlignmentRepository {

//     fn index(&self) -> &bai::Index;

//     fn header(&self) -> &Header;

// async fn read_alignment(
//     &mut self,
//     region: &Region,
//     sequence: &Sequence,
//     contig_header: &ContigHeader,
// ) -> Result<Alignment, TGVError>;

// fn read_header(&self) -> Result<Vec<(String, Option<usize>)>, TGVError>;

//}

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

            index: index,
            header: header,
            reader: reader,
        })
    }
}

// impl AlignmentRepository for BamRepository {
//     /// Read an alignment from a BAM file.

// }

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
        let future_reader = operator
            .reader(name)
            .await?
            .into_futures_async_read(..)
            .await?;

        let tokio_reader = Compat::new(future_reader);

        let mut reader = bam::r#async::io::Reader::new(tokio_reader);

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
        let stream = operator.reader(name).await?.into_bytes_stream(..).await?;

        let inner = StreamReader::new(stream);
        let mut reader = bai::r#async::io::Reader::new(inner);

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
}

impl AlignmentRepositoryEnum {
    pub async fn new(settings: &Settings) -> Result<Option<Self>, TGVError> {
        match &settings.bam_path {
            None => Ok(None),
            Some(bam_path) => {
                if is_url(bam_path) {
                    Ok(Some(AlignmentRepositoryEnum::RemoteBam(
                        RemoteBamRepository::new(bam_path, settings.bai_path.as_ref().unwrap())
                            .await?,
                    )))
                } else {
                    Ok(Some(AlignmentRepositoryEnum::Bam(
                        BamRepository::new(bam_path, settings.bai_path.as_ref().unwrap()).await?,
                    )))
                }
            }
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

        match contig_header
            .try_get(region.contig_index)?
            .get_alignment_name()
        {
            Some(region_str) => {
                let mut records = Vec::new();
                let mut index = 0;
                match self {
                    AlignmentRepositoryEnum::Bam(inner) => {
                        let mut query = inner.reader.query(
                            &inner.header,
                            &inner.index,
                            &region_str.parse()?,
                        )?;

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
                        let mut query = inner.reader.query(
                            &inner.header,
                            &inner.index,
                            &region_str.parse()?,
                        )?;

                        while let Some(record) = query.try_next().await? {
                            records.push(AlignedRead::from_bam_record(
                                index,
                                record,
                                reference_sequence,
                            )?);
                            index += 1;
                        }
                    }
                };

                Alignment::from_aligned_reads(records, region, reference_sequence)
            }
            None => Alignment::from_aligned_reads(Vec::new(), region, reference_sequence),
        }
    }

    /// Read BAM headers and return contig namesa and lengths.
    /// Note that this function does not interprete the contig name as contg vs chromosome.
    pub fn read_header(&self) -> Result<Vec<(String, Option<usize>)>, TGVError> {
        let header = match self {
            AlignmentRepositoryEnum::Bam(inner) => &inner.header,
            AlignmentRepositoryEnum::RemoteBam(inner) => &inner.header,
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

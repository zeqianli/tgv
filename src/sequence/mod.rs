mod fasta;
mod twobit;
mod ucsc_api;

pub use crate::sequence::{
    fasta::IndexedFastaSequenceRepository, twobit::TwoBitSequenceRepository,
    ucsc_api::UCSCApiSequenceRepository,
};
use crate::{
    contig_header::ContigHeader,
    error::TGVError,
    intervals::Region,
    reference::Reference,
    settings::{BackendType, Settings},
};
use std::path::Path;
/// Sequences of a genome region.
#[derive(Debug, Default)]
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
    pub fn new(start: usize, sequence: Vec<u8>, contig_index: usize) -> Self {
        Self {
            contig_index,
            start,
            sequence,
        }
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

pub trait SequenceRepository {
    async fn query_sequence(
        &mut self,
        region: &Region,
        // cache: &mut SequenceCache,
        contig_header: &ContigHeader,
    ) -> Result<Sequence, TGVError>;

    async fn close(&mut self) -> Result<(), TGVError>;
}

pub enum SequenceRepositoryEnum {
    UCSCApi(UCSCApiSequenceRepository),
    TwoBit(TwoBitSequenceRepository),
    IndexedFasta(IndexedFastaSequenceRepository),
}

impl SequenceRepositoryEnum {
    pub fn new(settings: &Settings) -> Result<Option<Self>, TGVError> {
        match (&settings.backend, &settings.reference) {
            (_, Reference::NoReference) => Ok(None),
            (_, Reference::BYOIndexedFasta(path)) => Ok(Some(Self::IndexedFasta(
                IndexedFastaSequenceRepository::new(path.clone())?,
            ))),

            (BackendType::Ucsc, _) => Ok(Some(Self::UCSCApi(UCSCApiSequenceRepository::new(
                &settings.reference,
                &settings.ucsc_host,
            )?))),
            (BackendType::Local, _) => Ok(Some(Self::TwoBit(TwoBitSequenceRepository::new(
                &settings.reference,
            )))), // add paths later
            (BackendType::Default, _) => {
                // If the local cache is available, use the local cache.
                // Otherwise, use the UCSC DB / API.
                if Path::new(&settings.reference.cache_dir(&settings.cache_dir)).exists() {
                    Ok(Some(Self::TwoBit(TwoBitSequenceRepository::new(
                        &settings.reference,
                    ))))
                } else {
                    Ok(Some(Self::UCSCApi(UCSCApiSequenceRepository::new(
                        &settings.reference,
                        &settings.ucsc_host,
                    )?)))
                }
            }
        }
    }
}

impl SequenceRepository for SequenceRepositoryEnum {
    async fn query_sequence(
        &mut self,
        region: &Region,
        contig_header: &ContigHeader,
    ) -> Result<Sequence, TGVError> {
        match self {
            Self::UCSCApi(repo) => repo.query_sequence(region, contig_header).await,
            Self::TwoBit(repo) => repo.query_sequence(region, contig_header).await,
            Self::IndexedFasta(repo) => repo.query_sequence(region, contig_header).await,
        }
    }

    async fn close(&mut self) -> Result<(), TGVError> {
        match self {
            Self::UCSCApi(repo) => repo.close().await,
            Self::TwoBit(repo) => repo.close().await,
            Self::IndexedFasta(repo) => repo.close().await,
        }
    }
}

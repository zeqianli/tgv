mod fasta;
mod twobit;
mod ucsc_api;

use crate::contig_header::ContigHeader;
use crate::error::TGVError;
use crate::intervals::Region;
pub use crate::sequence::{
    fasta::{IndexedFastaSequenceCache, IndexedFastaSequenceRepository},
    twobit::{TwoBitSequenceCache, TwoBitSequenceRepository},
    ucsc_api::{UCSCApiSequenceRepository, UcscApiSequenceCache},
};
use ::twobit::TwoBitFile;
use std::collections::HashMap;
/// Sequences of a genome region.
#[derive(Debug)]
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

pub enum SequenceCache {
    UcscApi(UcscApiSequenceCache),
    TwoBit(TwoBitSequenceCache),
    IndexedFasta(IndexedFastaSequenceCache),
    NoReference,
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

#[derive(Debug)]
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

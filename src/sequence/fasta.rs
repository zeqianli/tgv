use crate::{
    contig_header::{Contig, ContigHeader},
    error::TGVError,
    intervals::Region,
    sequence::{Sequence, SequenceCache, SequenceRepository},
};
use noodles_core::region::Region as noodlesRegion;
use noodles_fasta::{
    fai::{Index, Record},
    io::{
        indexed_reader::{Builder, IndexedReader},
        BufReader,
    },
};
use std::str::FromStr;

#[derive(Debug)]
pub struct IndexedFastaSequenceRepository {
    index: Index,
}

impl IndexedFastaSequenceRepository {
    pub fn new(path: String) -> Result<(Self, SequenceCache), TGVError> {
        let reader = Builder::default().build_from_path(path)?;
        let index = reader.index().clone();
        return Ok((
            Self { index },
            SequenceCache::IndexedFasta(IndexedFastaSequenceCache { reader }),
        ));
    }

    pub fn query_contigs(&self) -> Vec<Contig> {
        self.index
            .as_ref()
            .iter()
            .map(|record| {
                Contig::new(
                    record.name().to_string().as_ref(),
                    Some(record.length() as usize),
                )
            })
            .collect::<Vec<Contig>>()
    }
}

impl SequenceRepository for IndexedFastaSequenceRepository {
    async fn query_sequence(
        &self,
        region: &Region,
        cache: &mut SequenceCache,
        contig_header: &ContigHeader,
    ) -> Result<Sequence, TGVError> {
        let cache = match cache {
            SequenceCache::IndexedFasta(cache) => cache,
            _ => {
                return Err(TGVError::StateError(
                    "Expect SequenceCache::IndexFasta".to_string(),
                ));
            }
        };

        let region_string = format!(
            "{}{}-{}",
            contig_header.get(region.contig_index)?.name,
            region.start,
            region.end
        );

        Ok(Sequence {
            start: region.start,

            // FIXME: pre-allocate the sequence array to read more efficiently
            sequence: cache
                .reader
                .query(&noodlesRegion::from_str(&region_string)?)?
                .sequence()
                .as_ref()
                .to_vec(),
            contig_index: region.contig_index,
        })
    }

    async fn close(&self) -> Result<(), TGVError> {
        Ok(())
    }
}

// FIXME: I don't like this repository - cache setup. This is out of mutability concerns. Feels redundant.

pub struct IndexedFastaSequenceCache {
    reader: IndexedReader<BufReader<std::fs::File>>,
}

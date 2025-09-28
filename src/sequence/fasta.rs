use crate::{
    contig_header::{Contig, ContigHeader},
    error::TGVError,
    intervals::Region,
    sequence::{Sequence, SequenceRepository},
};
use noodles_core::region::Region as noodlesRegion;
use noodles_fasta::{
    fai::Index,
    io::{
        indexed_reader::{Builder, IndexedReader},
        BufReader,
    },
};
use std::str::FromStr;

pub struct IndexedFastaSequenceRepository {
    index: Index,

    reader: IndexedReader<BufReader<std::fs::File>>,
}

impl IndexedFastaSequenceRepository {
    pub fn new(path: String) -> Result<Self, TGVError> {
        let reader = Builder::default().build_from_path(path)?;
        let index = reader.index().clone();
        Ok(Self { index, reader })
    }
}

impl SequenceRepository for IndexedFastaSequenceRepository {
    async fn query_sequence(
        &mut self,
        region: &Region,
        contig_header: &ContigHeader,
    ) -> Result<Sequence, TGVError> {
        let region_string = format!(
            "{}:{}-{}",
            contig_header.try_get(region.contig_index)?.name,
            region.start,
            region.end
        );

        Ok(Sequence {
            start: region.start,

            // FIXME: pre-allocate the sequence array to read more efficiently
            sequence: self
                .reader
                .query(&noodlesRegion::from_str(&region_string)?)?
                .sequence()
                .as_ref()
                .to_vec(),
            contig_index: region.contig_index,
        })
    }

    async fn close(&mut self) -> Result<(), TGVError> {
        Ok(())
    }

    async fn get_all_contigs(&mut self) -> Result<Vec<Contig>, TGVError> {
        Ok(self
            .index
            .as_ref()
            .iter()
            .map(|record| {
                Contig::new(
                    record.name().to_string().as_ref(),
                    Some(record.length() as usize),
                )
            })
            .collect::<Vec<Contig>>())
    }
}

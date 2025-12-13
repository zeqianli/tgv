use crate::{
    contig_header::{Contig, ContigHeader},
    error::TGVError,
    intervals::Region,
    sequence::{Sequence, SequenceRepository},
};
use noodles::fasta::{
    fai::Index,
    io::{
        BufReader,
        indexed_reader::{Builder, IndexedReader},
    },
};
use noodles::{core::region::Region as noodlesRegion, vcf::header::record::value::map::contig};
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
        let sequence = if let Some(region) = region.noodles_sequence(contig_header)? {
            self.reader.query(&region)?.sequence().as_ref().to_vec()
        } else {
            vec![]
        };

        Ok(Sequence {
            start: region.start(),

            // FIXME: pre-allocate the sequence array to read more efficiently
            sequence: sequence,
            contig_index: region.contig_index(),
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

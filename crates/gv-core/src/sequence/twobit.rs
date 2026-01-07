use crate::contig_header::{Contig, ContigHeader, ContigSource};
use crate::error::TGVError;
use crate::intervals::{GenomeInterval, Region};
use crate::reference::Reference;
use crate::sequence::Sequence;
use std::collections::HashMap;
use twobit::TwoBitFile;

/// Repository for reading sequences from 2bit files

pub struct TwoBitSequenceRepository {
    /// Reference genome.
    reference: Reference,

    /// contig index -> 2bit buffer index in buffers.
    contig_to_buffer_index: HashMap<usize, usize>,

    /// 2bit file buffers.
    buffers: Vec<TwoBitFile<std::io::BufReader<std::fs::File>>>,
}
impl TwoBitSequenceRepository {
    pub fn new(reference: &Reference) -> Self {
        Self {
            reference: reference.clone(),
            contig_to_buffer_index: HashMap::new(),
            buffers: Vec::new(),
        }
    }

    pub fn add_contig_path(
        &mut self,
        path: &str,
        contig_header: &mut ContigHeader,
    ) -> Result<(), TGVError> {
        let tb: TwoBitFile<std::io::BufReader<std::fs::File>> = twobit::TwoBitFile::open(path)
            .map_err(|e| TGVError::IOError(format!("Failed to open 2bit file {}: {}", &path, e)))?;

        let buffer_index = self.buffers.len();

        tb.chrom_names()
            .into_iter()
            .zip(tb.chrom_sizes())
            .for_each(|(chrom_name, chrom_size)| {
                let index = contig_header.update_or_add_contig(
                    chrom_name,
                    Some(chrom_size as u64),
                    Vec::new(),
                    ContigSource::Sequence,
                );

                self.contig_to_buffer_index.insert(index, buffer_index);
            });

        self.buffers.push(tb);

        Ok(())
    }
}

impl TwoBitSequenceRepository {
    pub async fn query_sequence(
        &mut self,
        region: &Region,

        contig_header: &ContigHeader,
    ) -> Result<Sequence, TGVError> {
        let contig_name = contig_header
            .try_get(region.contig_index())?
            .get_sequence_name();

        match (
            self.contig_to_buffer_index.get(&region.contig_index()),
            contig_name,
        ) {
            (Some(buffer_index), Some(contig_name)) => {
                let buffer = &mut self.buffers[*buffer_index];
                let sequence_string = buffer.read_sequence(
                    contig_name,
                    ((region.start() as usize).saturating_sub(1))..region.end() as usize, // Convert to 0-based range
                )?;
                Ok(Sequence {
                    start: region.start(),
                    sequence: sequence_string.into_bytes(),
                    contig_index: region.contig_index(),
                })
            }
            _ => {
                // Going to a contig that's not in twobit file.
                // Can happen when there are contig mismatches between data sources (e.g. BAM and reference)
                Ok(Sequence {
                    start: region.start(),
                    sequence: "".to_string().into_bytes(),
                    contig_index: region.contig_index(),
                })
            }
        }
    }

    pub async fn close(&mut self) -> Result<(), TGVError> {
        Ok(())
    }

    pub async fn get_all_contigs(&mut self) -> Result<Vec<Contig>, TGVError> {
        todo!()
    }
}

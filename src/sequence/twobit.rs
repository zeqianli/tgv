use crate::contig_header::{Contig, ContigHeader};
use crate::error::TGVError;
use crate::intervals::Region;
use crate::reference::Reference;
use crate::sequence::{Sequence, SequenceRepository};
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
            .zip(tb.chrom_sizes().into_iter())
            .for_each(|(chrom_name, chrom_size)| {
                let index =
                    contig_header.update_or_add_contig(Contig::new(&chrom_name, Some(chrom_size)));

                self.contig_to_buffer_index.insert(index, buffer_index);
            });

        self.buffers.push(tb);

        Ok(())
    }
}

impl SequenceRepository for TwoBitSequenceRepository {
    async fn query_sequence(
        &mut self,
        region: &Region,

        contig_header: &ContigHeader,
    ) -> Result<Sequence, TGVError> {
        let contig_name = contig_header.get_name(region.contig_index)?;

        match self.contig_to_buffer_index.get(&region.contig_index) {
            Some(buffer_index) => {
                let buffer = &mut self.buffers[*buffer_index];
                let sequence_string = buffer.read_sequence(
                    contig_name,
                    (region.start - 1)..region.end, // Convert to 0-based range
                )?;
                Ok(Sequence {
                    start: region.start,
                    sequence: sequence_string.into_bytes(),
                    contig_index: region.contig_index,
                })
            }
            None => {
                // Going to a contig that's not in twobit file.
                // Can happen when there are contig mismatches between data sources (e.g. BAM and reference)
                Ok(Sequence {
                    start: region.start,
                    sequence: "".to_string().into_bytes(),
                    contig_index: region.contig_index,
                })
            }
        }
    }

    async fn close(&mut self) -> Result<(), TGVError> {
        Ok(())
    }

    async fn get_all_contigs(&mut self) -> Result<Vec<Contig>, TGVError> {
        todo!()
    }
}

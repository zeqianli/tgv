use crate::contig_header::ContigHeader;
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

    file_name_to_buffer_index: HashMap<String, usize>,

    /// contig index -> 2bit buffer index in buffers. Used in local mode (TwoBitSequenceRepository).
    contig_to_buffer_index: HashMap<usize, usize>,

    /// 2bit file buffers. Used in local mode (TwoBitSequenceRepository).
    buffers: Vec<TwoBitFile<std::io::BufReader<std::fs::File>>>,
}
impl TwoBitSequenceRepository {
    pub fn new(reference: Reference) -> Self {
        Self {
            reference,
            file_name_to_buffer_index: HashMap::new(),
            contig_to_buffer_index: HashMap::new(),
            buffers: Vec::new(),
        }
    }

    pub fn add_contig_path(mut self, contig_index: usize, path: &String) -> Result<Self, TGVError> {
        // contig_to_file_name: HashMap<usize, Option<String>>,
        // cache_dir: String,
        // Remove contigs that have no 2bit file.

        match self.file_name_to_buffer_index.get(path) {
            Some(i_buffer) => {
                self.contig_to_buffer_index.insert(contig_index, *i_buffer);
            }
            None => {
                let i_buffer = self.buffers.len();
                self.file_name_to_buffer_index
                    .insert(path.clone(), i_buffer);
                self.contig_to_buffer_index.insert(contig_index, i_buffer);

                // add a new buffer
                //let file_path = format!("{}/{}/{}", &cache_dir, reference.to_string(), file_name);
                let tb: TwoBitFile<std::io::BufReader<std::fs::File>> =
                    twobit::TwoBitFile::open(&path).map_err(|e| {
                        TGVError::IOError(format!("Failed to open 2bit file {}: {}", &path, e))
                    })?;
                self.buffers.push(tb);
            }
        }

        Ok(self)
    }
}

impl SequenceRepository for TwoBitSequenceRepository {
    async fn query_sequence(
        &mut self,
        region: &Region,

        contig_header: &ContigHeader,
    ) -> Result<Sequence, TGVError> {
        let contig_name = contig_header.get_name(region.contig_index)?;
        let buffer = &mut self.buffers[self.contig_to_buffer_index[&region.contig_index]];
        let sequence_str = buffer.read_sequence(
            contig_name,
            (region.start - 1)..region.end, // Convert to 0-based range
        )?;

        Ok(Sequence {
            start: region.start,
            sequence: sequence_str.into_bytes(),
            contig_index: region.contig_index,
        })
    }

    async fn close(&mut self) -> Result<(), TGVError> {
        Ok(())
    }
}

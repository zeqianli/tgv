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

    /// reference genome string -> 2bit file base name
    contig_to_file_name: HashMap<usize, String>,

    /// Cache root directory. 2bit files are in cache_dir/_reference_name/*.2bit
    cache_dir: String,

    /// contig index -> 2bit buffer index in buffers. Used in local mode (TwoBitSequenceRepository).
    contig_to_buffer_index: HashMap<usize, usize>,

    /// 2bit file buffers. Used in local mode (TwoBitSequenceRepository).
    buffers: Vec<TwoBitFile<std::io::BufReader<std::fs::File>>>,
}

impl TwoBitSequenceRepository {
    pub fn new(
        reference: Reference,
        contig_to_file_name: HashMap<usize, Option<String>>,
        cache_dir: String,
    ) -> Result<Self, TGVError> {
        // Get the file path for this contig

        let mut buffers = Vec::new();
        let mut file_name_to_buffer_index = HashMap::new();
        let mut contig_to_buffer_index = HashMap::new();

        // Remove contigs that have no 2bit file.
        let contig_to_file_name: HashMap<usize, String> = contig_to_file_name
            .into_iter()
            .filter(|(_, file_name)| file_name.is_some())
            .map(|(contig, file_name)| (contig, file_name.unwrap()))
            .collect();

        for (contig, file_name) in contig_to_file_name.iter() {
            let i_buffer = buffers.len();

            match file_name_to_buffer_index.get(file_name) {
                Some(i_buffer) => {
                    contig_to_buffer_index.insert(*contig, *i_buffer);
                }
                None => {
                    file_name_to_buffer_index.insert(file_name.clone(), i_buffer);
                    contig_to_buffer_index.insert(*contig, i_buffer);

                    // add a new buffer
                    let file_path =
                        format!("{}/{}/{}", &cache_dir, reference.to_string(), file_name);
                    let tb: TwoBitFile<std::io::BufReader<std::fs::File>> =
                        twobit::TwoBitFile::open(&file_path).map_err(|e| {
                            TGVError::IOError(format!(
                                "Failed to open 2bit file {}: {}",
                                &file_path, e
                            ))
                        })?;
                    buffers.push(tb);
                }
            }
        }

        Ok(Self {
            reference,
            contig_to_file_name,
            cache_dir,
            contig_to_buffer_index,
            buffers,
        })
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

use crate::error::TGVError;
use crate::helpers::is_url;
use crate::models::{
    contig::Contig,
    reference::Reference
};

use rust_htslib::bam::{self, IndexedReader, Read};
use std::collections::HashMap;
use url::Url;



/// A collection of contigs. This helps relative contig movements.
pub struct ContigCollection {
    contigs: Vec<Contig>,
    contig_lengths: Vec<Option<usize>>,

    contig_index: HashMap<String, usize>,
}

impl ContigCollection {
    pub fn new(contigs: Vec<Contig>, contig_lengths: Vec<Option<usize>>) -> Result<Self, TGVError> {
        // check that contigs do not have duplicated full names
        let mut contig_index = HashMap::new();
        for (i, contig) in contigs.iter().enumerate() {
            if contig_index.contains_key(&contig.full_name()) {
                return Err(TGVError::StateError(format!(
                    "Duplicate contig names {}. Is your BAM file header correct?",
                    contig.full_name()
                )));
            }
            contig_index.insert(contig.full_name(), i);
        }

        Ok(Self {
            contigs,
            contig_lengths,
            contig_index,
        })
    }

    pub fn first(&self) -> Result<&Contig, TGVError> {
        Ok(&self.contigs[0])
    }

    #[allow(dead_code)]
    pub fn last(&self) -> Result<&Contig, TGVError> {
        Ok(&self.contigs[self.contigs.len() - 1])
    }

    pub fn from_bam(
        path: &String,
        bai_path: Option<&String>,
        reference: Option<&Reference>,
    ) -> Result<Self, TGVError> {
        // Use the indexed_reader::Builder pattern as shown in alignment.rs
        let is_remote_path = is_url(path);
        let bam = match bai_path {
            Some(bai_path) => {
                if is_remote_path {
                    return Err(TGVError::IOError(
                        "Custom .bai path for remote BAM files are not supported yet.".to_string(),
                    ));
                }
                IndexedReader::from_path_and_index(path, bai_path)?
            }
            None => {
                if is_remote_path {
                    IndexedReader::from_url(
                        &Url::parse(path).map_err(|e| TGVError::IOError(e.to_string()))?,
                    )
                    .unwrap()
                } else {
                    IndexedReader::from_path(path)?
                }
            }
        };

        let header = bam::Header::from_template(bam.header());

        let mut contigs = Vec::new();
        let mut contig_lengths: Vec<Option<usize>> = Vec::new();
        for (_key, records) in header.to_hashmap().iter() {
            for record in records {
                if record.contains_key("SN") {
                    let contig_name = record["SN"].to_string();
                    match reference {
                        // If the reference is human, interpret contig names as chromosomes. This allows abbreviated matching (chr1 <-> 1).
                        Some(Reference::Hg19) => contigs.push(Contig::chrom(&contig_name)),
                        Some(Reference::Hg38) => contigs.push(Contig::chrom(&contig_name)),

                        // Otherwise, interpret contig names as contigs. This does not allow abbreviated matching.
                        _ => contigs.push(Contig::contig(&contig_name)),
                    }

                    if record.contains_key("LN") {
                        contig_lengths.push(record["LN"].to_string().parse::<usize>().ok());
                    } else {
                        contig_lengths.push(None);
                    }
                }
            }
        }

        Self::new(contigs, contig_lengths)
    }

    pub fn contains(&self, contig: &Contig) -> bool {
        self.contig_index.contains_key(&contig.full_name())
    }

    pub fn length(&self, contig: &Contig) -> Option<usize> {
        let index = self.contig_index.get(&contig.full_name())?;
        self.contig_lengths[*index]
    }

    #[allow(dead_code)]
    pub fn next(&self, contig: &Contig, k: usize) -> Result<Contig, TGVError> {
        let index = self.contig_index[&contig.full_name()];
        let next_index = (index + k) % self.contigs.len();
        Ok(self.contigs[next_index].clone())
    }

    #[allow(dead_code)]
    pub fn previous(&self, contig: &Contig, k: usize) -> Result<Contig, TGVError> {
        let index = self.contig_index[&contig.full_name()];
        let previous_index =
            (index + self.contigs.len() - k % self.contigs.len()) % self.contigs.len();
        Ok(self.contigs[previous_index].clone())
    }
}

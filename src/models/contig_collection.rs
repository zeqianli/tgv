use crate::error::TGVError;
use crate::helpers::is_url;
use crate::models::{contig::Contig, cytoband::Cytoband, reference::Reference};
use rust_htslib::bam::{self, IndexedReader, Read};
use std::collections::HashMap;
use url::Url;

pub struct ContigDatum {
    contig: Contig,             // Name
    length: Option<usize>,      // Length
    cytoband: Option<Cytoband>, // Cytoband
}

/// A collection of contigs. This helps relative contig movements.
pub struct ContigCollection {
    reference: Option<Reference>,
    contigs: Vec<ContigDatum>,

    contig_index: HashMap<String, usize>,
}

impl ContigCollection {
    pub fn new(reference: Option<Reference>) -> Self {
        Self {
            reference,
            contigs: Vec::new(),
            contig_index: HashMap::new(),
        }
    }

    pub fn first(&self) -> Result<&Contig, TGVError> {
        Ok(&self.contigs[0].contig)
    }

    #[allow(dead_code)]
    pub fn last(&self) -> Result<&Contig, TGVError> {
        Ok(&self.contigs[self.contigs.len() - 1].contig)
    }

    pub fn update_cytoband(&mut self, contig: &Contig, cytoband: Cytoband) -> Result<(), TGVError> {
        let index = self
            .contig_index
            .get(&contig.full_name())
            .ok_or(TGVError::StateError(format!(
                "Contig {} not found",
                contig.full_name()
            )))?;
        self.contigs[*index].cytoband = Some(cytoband);
        Ok(())
    }

    pub fn update_or_add_contig(
        &mut self,
        contig: Contig,
        length: Option<usize>,
    ) -> Result<(), TGVError> {
        let index = self.contig_index.get(&contig.full_name());
        if let Some(index) = index {
            self.contigs[*index].length = length;
        } else {
            let contig_name = contig.full_name();
            self.contigs.push(ContigDatum {
                contig,
                length,
                cytoband: None,
            });
            self.contig_index
                .insert(contig_name, self.contigs.len() - 1);
        }
        Ok(())
    }

    pub fn update_from_bam(
        &mut self,
        path: &String,
        bai_path: Option<&String>,
        reference: Option<&Reference>,
    ) -> Result<(), TGVError> {
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

        for (_key, records) in header.to_hashmap().iter() {
            for record in records {
                if record.contains_key("SN") {
                    let contig_name = record["SN"].to_string();
                    let contig = match reference {
                        // If the reference is human, interpret contig names as chromosomes. This allows abbreviated matching (chr1 <-> 1).
                        Some(Reference::Hg19) => Contig::chrom(&contig_name),
                        Some(Reference::Hg38) => Contig::chrom(&contig_name),
                        Some(Reference::UcscGenome(genome)) => Contig::chrom(genome),

                        // Otherwise, interpret contig names as contigs. This does not allow abbreviated matching.
                        _ => Contig::contig(&contig_name),
                    };

                    let contig_length = if record.contains_key("LN") {
                        record["LN"].to_string().parse::<usize>().ok()
                    } else {
                        None
                    };

                    self.update_or_add_contig(contig, contig_length)?;
                }
            }
        }

        Ok(())
    }

    pub fn contains(&self, contig: &Contig) -> bool {
        self.contig_index.contains_key(&contig.full_name())
    }

    pub fn length(&self, contig: &Contig) -> Option<usize> {
        let index = self.contig_index.get(&contig.full_name())?;
        self.contigs[*index].length
    }

    pub fn next(&self, contig: &Contig, k: usize) -> Result<Contig, TGVError> {
        let index = self
            .contig_index
            .get(&contig.full_name())
            .ok_or(TGVError::StateError(format!(
                "Contig {} not found",
                contig.full_name()
            )))?;
        let next_index = (index + k) % self.contigs.len();
        Ok(self.contigs[next_index].contig.clone())
    }

    pub fn previous(&self, contig: &Contig, k: usize) -> Result<Contig, TGVError> {
        let index = self
            .contig_index
            .get(&contig.full_name())
            .ok_or(TGVError::StateError(format!(
                "Contig {} not found",
                contig.full_name()
            )))?;
        let previous_index =
            (index + self.contigs.len() - k % self.contigs.len()) % self.contigs.len();
        Ok(self.contigs[previous_index].contig.clone())
    }
}

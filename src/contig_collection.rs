use crate::error::TGVError;
use crate::repository::{AlignmentRepository, AlignmentRepositoryEnum};
use crate::{contig::Contig, cytoband::Cytoband, reference::Reference};
use std::collections::HashMap;
use std::fmt;
use std::fmt::Display;

#[derive(Debug)]
pub struct ContigDatum {
    contig: Contig,             // Name
    length: Option<usize>,      // Length
    cytoband: Option<Cytoband>, // Cytoband

    cytoband_loaded: bool, // Whether this contig's cytoband has been quried.
}

/// A collection of contigs. This helps relative contig movements.
#[derive(Debug)]
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

    pub fn get_index(&self, contig: &Contig) -> Option<usize> {
        match self.contig_index.get(&contig.name) {
            Some(index) => Some(*index),
            None => {
                for alias in contig.aliases.iter() {
                    if let Some(index) = self.contig_index.get(alias) {
                        return Some(*index);
                    }
                }
                None
            }
        }
    }

    pub fn get_contig_by_str(&self, s: &str) -> Option<&Contig> {
        match self.contig_index.get(s) {
            Some(index) => Some(&self.contigs[*index].contig),
            None => None,
        }
    }

    pub fn update_cytoband(
        &mut self,
        contig: &Contig,
        cytoband: Option<Cytoband>,
    ) -> Result<(), TGVError> {
        match self.get_index(contig) {
            Some(index) => {
                self.contigs[index].cytoband = cytoband;
                self.contigs[index].cytoband_loaded = true; // can be None
                Ok(())
            }
            None => Err(TGVError::StateError(format!(
                "Contig {} not found",
                contig.name
            ))),
        }
    }

    pub fn update_or_add_contig(
        &mut self,
        contig: Contig,
        length: Option<usize>,
    ) -> Result<(), TGVError> {
        match self.get_index(&contig) {
            Some(index) => {
                self.contigs[index].length = length;
            }
            None => {
                self.contig_index
                    .insert(contig.name.clone(), self.contigs.len());
                for alias in contig.aliases.iter() {
                    self.contig_index.insert(alias.clone(), self.contigs.len());
                }
                self.contigs.push(ContigDatum {
                    contig,
                    length,
                    cytoband: None,
                    cytoband_loaded: false,
                });
            }
        }
        Ok(())
    }

    pub fn update_from_bam(
        &mut self,
        reference: Option<&Reference>,
        bam: &AlignmentRepositoryEnum,
    ) -> Result<(), TGVError> {
        // Use the indexed_reader::Builder pattern as shown in alignment.rs

        for (contig_name, contig_length) in bam.read_header()? {
            let contig = match reference {
                // If the reference is human, interpret contig names as chromosomes. This allows abbreviated matching (chr1 <-> 1).
                Some(Reference::Hg19) | Some(Reference::Hg38) | Some(Reference::UcscGenome(_)) => {
                    Contig::new(&contig_name)
                }

                // Otherwise, interpret contig names as contigs. This does not allow abbreviated matching.
                _ => Contig::new(&contig_name),
            };

            self.update_or_add_contig(contig, contig_length)?;
        }

        Ok(())
    }

    pub fn contains(&self, contig: &Contig) -> bool {
        self.get_index(contig).is_some()
    }

    pub fn length(&self, contig: &Contig) -> Option<usize> {
        let index = self.get_index(contig)?;
        self.contigs[index].length
    }

    pub fn next(&self, contig: &Contig, k: usize) -> Result<Contig, TGVError> {
        match self.get_index(contig) {
            Some(index) => {
                let next_index = (index + k) % self.contigs.len();
                Ok(self.contigs[next_index].contig.clone())
            }
            None => Err(TGVError::StateError(format!(
                "Contig {} not found",
                contig.name.clone()
            ))),
        }
    }

    pub fn previous(&self, contig: &Contig, k: usize) -> Result<Contig, TGVError> {
        match self.get_index(contig) {
            Some(index) => {
                let previous_index =
                    (index + self.contigs.len() - k % self.contigs.len()) % self.contigs.len();
                Ok(self.contigs[previous_index].contig.clone())
            }
            None => Err(TGVError::StateError(format!(
                "Contig {} not found",
                contig.name.clone()
            ))),
        }
    }

    pub fn cytoband(&self, contig: &Contig) -> Option<&Cytoband> {
        let index = self.get_index(contig)?;
        self.contigs[index].cytoband.as_ref()
    }

    pub fn cytoband_is_loaded(&self, contig: &Contig) -> Result<bool, TGVError> {
        let index = self.get_index(contig).ok_or(TGVError::StateError(format!(
            "Contig {} not found",
            contig.name.clone()
        )))?;
        Ok(self.contigs[index].cytoband_loaded)
    }
}

impl Display for ContigCollection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for contig in &self.contigs {
            writeln!(f, "{}: {:?}", contig.contig.name, contig.length)?;
        }
        Ok(())
    }
}

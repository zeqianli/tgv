use crate::error::TGVError;
use crate::helpers::is_url;
use crate::models::{contig::Contig, cytoband::Cytoband, reference::Reference};
use crate::repository::{AlignmentRepositoryEnum, AlignmentRepository};
use rust_htslib::bam::{self, IndexedReader, Read};
use std::collections::HashMap;
use url::Url;

pub struct ContigDatum {
    contig: Contig,             // Name
    length: Option<usize>,      // Length
    cytoband: Option<Cytoband>, // Cytoband

    cytoband_loaded: bool, // Whether this contig's cytoband has been quried.
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

    pub fn update_cytoband(
        &mut self,
        contig: &Contig,
        cytoband: Option<Cytoband>,
    ) -> Result<(), TGVError> {
        let index = self
            .contig_index
            .get(&contig.full_name())
            .ok_or(TGVError::StateError(format!(
                "Contig {} not found",
                contig.full_name()
            )))?;
        self.contigs[*index].cytoband = cytoband;
        self.contigs[*index].cytoband_loaded = true; // can be None
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
                cytoband_loaded: false,
            });
            self.contig_index
                .insert(contig_name, self.contigs.len() - 1);
        }
        Ok(())
    }

    pub fn update_from_bam(
        &mut self,
        reference: Option<&Reference>,
        bam: &AlignmentRepositoryEnum
    ) -> Result<(), TGVError> {
        // Use the indexed_reader::Builder pattern as shown in alignment.rs
        

        for (contig_name, contig_length) in bam.read_header()? {
            
            let contig = match reference {
                // If the reference is human, interpret contig names as chromosomes. This allows abbreviated matching (chr1 <-> 1).
                Some(Reference::Hg19) => Contig::chrom(&contig_name),
                Some(Reference::Hg38) => Contig::chrom(&contig_name),
                Some(Reference::UcscGenome(genome)) => Contig::chrom(genome),

                // Otherwise, interpret contig names as contigs. This does not allow abbreviated matching.
                _ => Contig::contig(&contig_name),
            };

            self.update_or_add_contig(contig, contig_length)?;
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

    pub fn cytoband(&self, contig: &Contig) -> Option<&Cytoband> {
        let index = self.contig_index.get(&contig.full_name())?;
        self.contigs[*index].cytoband.as_ref()
    }

    pub fn cytoband_is_loaded(&self, contig: &Contig) -> Result<bool, TGVError> {
        let index = self
            .contig_index
            .get(&contig.full_name())
            .ok_or(TGVError::StateError(format!(
                "Contig {} not found",
                contig.full_name()
            )))?;
        Ok(self.contigs[*index].cytoband_loaded)
    }
}

use crate::error::TGVError;
use crate::traits::GenomeInterval;
use crate::{contig::Contig, region::Region};
use rust_htslib::bcf::{Reader, Record};
use std::collections::{BTreeMap, HashMap};

pub struct Variant {
    /// Contig name. This is not stored in the record.
    pub contig: Contig,

    /// VCF record
    pub record: Record,
}

impl GenomeInterval for Variant {
    fn contig(&self) -> &Contig {
        &self.contig
    }

    fn start(&self) -> usize {
        // rust_htslib record is 0-based
        self.record.pos() as usize + 1
    }

    fn end(&self) -> usize {
        // rust_htslib record end is 0-based
        self.record.end() as usize + 1 // 1-based
    }
}
pub struct VariantCollection {
    variants: Vec<Variant>,

    /// {contig_name: {start: variant_index, ...}}
    variant_lookup: HashMap<String, BTreeMap<usize, usize>>,
}

pub struct VariantCollectionBuilder {
    variants: Vec<Variant>,

    /// {contig_name: {start: variant_index, ...}}
    variant_lookup: HashMap<String, BTreeMap<usize, usize>>,
}

impl VariantCollectionBuilder {
    pub fn new() -> Self {
        Self {
            variants: Vec::new(),
            variant_lookup: HashMap::new(),
        }
    }

    pub fn vcf(&mut self, path: &str) -> Result<&mut Self, TGVError> {
        // let mut reader = Reader::from_path(path)?;
        // let header = reader.header();
        // for record in reader.records() {
        //     self.variants.push(Variant::new(record));
        // }
        // Ok(self)
        todo!()
    }
}

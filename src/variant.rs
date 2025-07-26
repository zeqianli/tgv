use crate::error::TGVError;
use crate::intervals::{GenomeInterval, SortedIntervalCollection};
use crate::{contig::Contig, region::Region};
use rust_htslib::bcf::{Read, Reader, Record};
use std::collections::{BTreeMap, HashMap};
use std::ops::Bound::{Excluded, Included};
pub struct Variant {
    /// Contig name. This is not stored in the record.
    pub contig: Contig,

    /// VCF record
    pub record: Record,
}

impl Variant {
    pub fn new(record: Record) -> Result<Self, TGVError> {
        let contig_u8 = record
            .header()
            .rid2name(record.rid().ok_or(TGVError::ValueError(
                "VCF record {:?} doesn't have a valid contig.".to_string(),
            ))?)?;
        let contig = Contig::new(std::str::from_utf8(contig_u8).map_err(|_| {
            TGVError::ValueError("VCF record {:?} doesn't have a valid contig. ".to_string())
        })?);
        Ok(Self {
            contig: contig,
            record: record,
        })
    }
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
pub struct VariantRepository {
    pub variants: SortedIntervalCollection<Variant>,
}

impl VariantRepository {
    pub fn from_vcf(path: &str) -> Result<Self, TGVError> {
        let mut bcf = Reader::from_path(path)?;

        let variants: Result<Vec<Variant>, _> =
            bcf.records().map(|record| Variant::new(record?)).collect();
        let variants = variants?;

        // lookup

        let mut variant_lookup: HashMap<String, BTreeMap<usize, Vec<usize>>> = HashMap::new();

        for (i, variant) in variants.iter().enumerate() {
            variant_lookup
                .entry(variant.contig().name.clone())
                .and_modify(|vs| {
                    vs.entry(variant.start())
                        .and_modify(|vvs| vvs.push(i))
                        .or_insert(vec![i]);
                })
                .or_insert(BTreeMap::from([(variant.start(), vec![i])]));
        }

        Ok(VariantRepository {
            variants: SortedIntervalCollection::new(variants)?,
        })
    }
}

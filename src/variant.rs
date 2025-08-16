use crate::contig_header::ContigHeader;
use crate::error::TGVError;
use crate::intervals::{GenomeInterval, SortedIntervalCollection};
use rust_htslib::bcf::{Read, Reader, Record};
use std::collections::{BTreeMap, HashMap};
pub struct Variant {
    /// Contig id name. This is not stored in the record.
    pub contig_index: usize,

    pub index: usize,

    /// VCF record
    pub record: Record,
}

impl Variant {
    pub fn new(
        record: Record,
        index: usize,
        contig_header: &ContigHeader,
    ) -> Result<Self, TGVError> {
        let contig_u8 = record
            .header()
            .rid2name(record.rid().ok_or(TGVError::ValueError(
                "VCF record {:?} doesn't have a valid contig.".to_string(),
            ))?)?;
        let contig_index =
            contig_header.get_index_by_str(&std::str::from_utf8(contig_u8).map_err(|_| {
                TGVError::ValueError("VCF record {:?} doesn't have a valid contig.".to_string())
            })?)?;
        Ok(Self {
            index,
            contig_index,
            record,
        })
    }
}

impl GenomeInterval for Variant {
    fn contig_index(&self) -> usize {
        self.contig_index
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
    pub fn from_vcf(path: &str, contig_header: &ContigHeader) -> Result<Self, TGVError> {
        let mut bcf = Reader::from_path(path)?;

        let variants: Result<Vec<Variant>, _> = bcf
            .records()
            .enumerate()
            .map(|(index, record)| Variant::new(record?, index, contig_header))
            .collect();
        let variants = variants?;

        // lookup

        let mut variant_lookup: HashMap<usize, BTreeMap<usize, Vec<usize>>> = HashMap::new();

        for (i, variant) in variants.iter().enumerate() {
            variant_lookup
                .entry(variant.contig_index)
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

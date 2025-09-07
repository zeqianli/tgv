use crate::contig_header::ContigHeader;
use crate::error::TGVError;
use crate::intervals::{GenomeInterval, Region, SortedIntervalCollection};
use itertools::Itertools;
use noodles_vcf as vcf;
use noodles_vcf::variant::record::{AlternateBases, Filters};
use std::collections::{BTreeMap, HashMap};
pub struct Variant {
    /// Contig id name. This is not stored in the record.
    pub contig_index: usize,

    /// Variant start. 1-based, inclusive.
    start: usize,

    /// Index in the VCF file
    pub index: usize,

    /// VCF record
    pub record: vcf::Record,
}

impl Variant {
    pub fn new(
        record: vcf::Record,
        index: usize,
        contig_header: &ContigHeader,
    ) -> Result<Self, TGVError> {
        let contig_str = record.reference_sequence_name();
        let contig_index = contig_header.get_index_by_str(contig_str)?;

        let start = record
            .variant_start()
            .ok_or(TGVError::ValueError("VCF record parsing error".to_string()))??
            .get();

        Ok(Self {
            contig_index: contig_index,
            start: start,
            index: index,
            record: record,
        })
    }
}

impl Variant {
    pub fn describe(&self) -> String {
        // FIXME: display more fields.
        // Note that other fields (filter, info, sample) requires the VCF header.
        format!(
            "Variant: {}:{} {}>{} QUAL={}",
            self.record.reference_sequence_name(),
            self.start,
            self.record.reference_bases(),
            self.record
                .alternate_bases()
                .iter()
                .collect::<Result<Vec<&str>, _>>()
                .unwrap_or(vec!["?"; 1])
                .iter()
                .join(","),
            self.record
                .quality_score()
                .map(|score_result| match score_result {
                    Ok(score) => format!("{}", score),
                    _ => "?".to_string(),
                })
                .unwrap_or("?".to_string()),
        )
    }
}

impl GenomeInterval for Variant {
    fn contig_index(&self) -> usize {
        self.contig_index
    }

    fn start(&self) -> usize {
        self.start
    }

    fn end(&self) -> usize {
        self.start + self.record.reference_bases().len() - 1
    }
}
pub struct VariantRepository {
    pub variants: SortedIntervalCollection<Variant>,
    // FIXME: save VCF header. This is needed for retrieving VCF FILTER, INFO, SAMPLE fields.
}

impl VariantRepository {
    pub fn from_vcf(path: &str, contig_header: &ContigHeader) -> Result<Self, TGVError> {
        let mut vcf = vcf::io::reader::Builder::default().build_from_path(path)?;
        vcf.read_header()?;

        let variants: Vec<Variant> = vcf
            .records()
            .enumerate()
            .map(|(index, record)| Variant::new(record?, index, contig_header))
            .collect::<Result<Vec<Variant>, _>>()?;

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

    pub fn overlapping(&self, region: &Region) -> Result<Vec<&Variant>, TGVError> {
        self.variants.overlapping(region)
    }
}

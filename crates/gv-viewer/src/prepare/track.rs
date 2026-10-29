//! The genes a gene track shows in a region, with their exon and intron segments.

use gv_core::{
    gene::{GeneSchema, GeneSegmentSchema, query_segments},
    prelude::*,
    strand::Strand,
};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentKind {
    CodingExon,
    NoncodingExon,
    Intron,
}

/// An exon or an intron of a gene, 1-based and inclusive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneSegment {
    pub start: u64,
    pub end: u64,
    pub kind: SegmentKind,
    /// The exon or intron number, counted along the gene.
    pub number: u64,
}

/// A gene and its segments within the region.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gene {
    pub start: u64,
    pub end: u64,
    pub name: String,
    pub strand: Strand,
    pub has_exons: bool,
    pub segments: Vec<GeneSegment>,
}

impl Gene {
    /// The genes overlapping a region, in table order, with their segments within the region.
    pub fn in_region(state: &State, region: &Region) -> Result<Vec<Gene>, TGVError> {
        let rows = state
            .track
            .query(region.contig_index(), region.start(), region.end())?;
        let segments = query_segments(rows.clone(), region.start(), region.end())?;
        let segment_gene_ids = segments.column(GeneSegmentSchema::GENE_ROW_ID)?.u64()?;
        let segment_starts = segments.column(GeneSegmentSchema::START)?.u64()?;
        let segment_ends = segments.column(GeneSegmentSchema::END)?.u64()?;
        let segment_kinds = segments.column(GeneSegmentSchema::KIND)?.str()?;
        let segment_numbers = segments.column(GeneSegmentSchema::FEATURE_INDEX)?.u64()?;
        let mut gene_segments: HashMap<u64, Vec<GeneSegment>> = HashMap::new();
        for row in 0..segments.height() {
            let kind = match segment_kinds.get(row).expect("segment kinds are non-null") {
                "coding_exon" => SegmentKind::CodingExon,
                "noncoding_exon" => SegmentKind::NoncodingExon,
                "intron" => SegmentKind::Intron,
                kind => {
                    return Err(TGVError::ValueError(format!(
                        "Unknown gene segment kind: {kind}."
                    )));
                }
            };
            gene_segments
                .entry(
                    segment_gene_ids
                        .get(row)
                        .expect("segment gene IDs are non-null"),
                )
                .or_default()
                .push(GeneSegment {
                    start: segment_starts
                        .get(row)
                        .expect("segment starts are non-null"),
                    end: segment_ends.get(row).expect("segment ends are non-null"),
                    kind,
                    number: segment_numbers
                        .get(row)
                        .expect("segment indexes are non-null"),
                });
        }

        let ids = rows.column(GeneSchema::ROW_ID)?.u64()?;
        let starts = rows.column(GeneSchema::START)?.u64()?;
        let ends = rows.column(GeneSchema::END)?.u64()?;
        let names = rows.column(GeneSchema::NAME)?.str()?;
        let strands = rows.column(GeneSchema::STRAND)?.str()?;
        let has_exons = rows.column(GeneSchema::HAS_EXONS)?.bool()?;
        (0..rows.height())
            .map(|row| {
                let id = ids.get(row).expect("gene row IDs are non-null");
                Ok(Gene {
                    start: starts.get(row).expect("gene starts are non-null"),
                    end: ends.get(row).expect("gene ends are non-null"),
                    name: names.get(row).expect("gene names are non-null").to_owned(),
                    strand: Strand::from_str(
                        strands
                            .get(row)
                            .expect("gene strands are non-null")
                            .to_owned(),
                    )?,
                    has_exons: has_exons.get(row).expect("exon availability is non-null"),
                    segments: gene_segments.remove(&id).unwrap_or_default(),
                })
            })
            .collect()
    }
}

//! What the status line says about the data under the pointer.
//!
//! Each function describes the bases from `left` to `right`, 1-based and inclusive, that one
//! screen unit under the pointer covers.

use gv_core::prelude::*;
use gv_core::{
    alignment::{CoverageSchema, ReadBase},
    bed::BedSchema,
    gene::{GeneSchema, GeneSegmentSchema, query_segments},
    variant::VariantSchema,
};
use itertools::Itertools;
use polars::prelude::ChunkAgg;

/// The read drawn at row `y` of an alignment track over `left..=right`, by read ID.
pub fn read_at(
    state: &State,
    index: usize,
    left: u64,
    right: u64,
    y: usize,
) -> Result<Option<usize>, TGVError> {
    let alignment = &state.alignments[index];
    match &state.paired_alignments[index] {
        Some(paired) => paired.read_overlapping(alignment, left, right, y),
        None => alignment.read_overlapping(left, right, y),
    }
}

/// A read's name, the base under the pointer, its CIGAR, and its mapping quality.
pub fn describe_read(
    state: &State,
    index: usize,
    read_id: usize,
    contig_index: usize,
    left: u64,
    right: u64,
) -> Result<String, TGVError> {
    let alignment = &state.alignments[index];
    let record = &alignment.records[read_id];
    let contig = &state.contig_header.contigs[contig_index].name;
    let name = record
        .name()
        .map(|name| name.to_string())
        .unwrap_or_else(|| "<missing>".into());
    let description = if left == right {
        let base = match alignment.read_base_at(read_id, left)? {
            // Show the quality as the file stores it.
            Some(ReadBase::Base { base, quality }) => format!(
                "Base: {}  Qual: {}",
                base as char,
                quality.map_or_else(
                    || ".".into(),
                    |q| alignment.quality_encoding.stored(q).to_string()
                )
            ),
            Some(ReadBase::Deletion) => "Base: -".to_string(),
            None => "Base: .".to_string(),
        };
        format!("{contig}:{left}  {name}  {base}")
    } else {
        format!("{contig}:{left}-{right}  {name}")
    };
    let mut cigar = Vec::new();
    noodles::sam::io::writer::record::write_cigar(&mut cigar, record.cigar())?;
    let mapq = record
        .mapping_quality()
        .map(|quality| quality.get().to_string())
        .unwrap_or_else(|| ".".into());
    Ok(format!(
        "{description}  Cigar: {}  MAPQ: {mapq}",
        String::from_utf8(cigar)?
    ))
}

/// Each gene under the pointer, with the exon or intron the pointer is in.
pub fn describe_genes(
    state: &State,
    contig_index: usize,
    left: u64,
    right: u64,
) -> Result<Vec<String>, TGVError> {
    let genes = state.track.query(contig_index, left, right)?;
    let segments = query_segments(genes.clone(), left, right)?;
    let contig = &state.contig_header.contigs[contig_index].name;
    let locus = if left == right {
        format!("{contig}:{left}")
    } else {
        format!("{contig}:{left}-{right}")
    };
    let segment_genes = segments.column(GeneSegmentSchema::GENE_ROW_ID)?.u64()?;
    let segment_kinds = segments.column(GeneSegmentSchema::KIND)?.str()?;
    let segment_numbers = segments.column(GeneSegmentSchema::FEATURE_INDEX)?.u64()?;
    let row_ids = genes.column(GeneSchema::ROW_ID)?.u64()?;
    let names = genes.column(GeneSchema::NAME)?.str()?;
    let ids = genes.column(GeneSchema::ID)?.str()?;
    let strands = genes.column(GeneSchema::STRAND)?.str()?;
    let exon_starts = genes.column(GeneSchema::EXON_STARTS)?.list()?;
    let mut descriptions = Vec::new();
    for row in 0..genes.height() {
        let row_id = row_ids.get(row).expect("gene row IDs are non-null");
        let label = [names.get(row), ids.get(row)]
            .into_iter()
            .flatten()
            .dedup()
            .join(" ");
        let strand = strands.get(row).unwrap_or(".");
        let exons = exon_starts
            .get_as_series(row)
            .map_or(0, |starts| starts.len());
        let part = (0..segments.height())
            .find(|&segment| segment_genes.get(segment) == Some(row_id))
            .map(|segment| {
                let number = segment_numbers
                    .get(segment)
                    .expect("segment numbers are non-null");
                match segment_kinds.get(segment) {
                    Some("intron") => {
                        format!(": intron {number} of {}", exons.saturating_sub(1))
                    }
                    Some("noncoding_exon") => {
                        format!(": exon {number} of {exons}, untranslated")
                    }
                    _ => format!(": exon {number} of {exons}"),
                }
            })
            .unwrap_or_default();
        descriptions.push(format!("{locus}  {label} ({strand}){part}"));
    }
    Ok(descriptions)
}

/// The reference bases under the pointer.
pub fn describe_sequence(state: &State, left: u64, right: u64) -> String {
    (left..=right)
        .filter_map(|coordinate| {
            state
                .sequence
                .base_at(coordinate)
                .map(|base| format!("{}: {}", coordinate, base as char))
        })
        .join(", ")
}

/// The base counts of an alignment track's coverage under the pointer.
pub fn describe_coverage(
    state: &State,
    index: usize,
    left: u64,
    right: u64,
) -> Result<String, TGVError> {
    let coverage = state.alignments[index].coverage.query(left, right)?;
    let sum = |column: &str| -> Result<u64, TGVError> {
        Ok(coverage.column(column)?.u64()?.sum().unwrap_or(0))
    };
    let counts = format!(
        "A:{}, T:{}, C:{}, G:{}, N:{}, total:{}",
        sum(CoverageSchema::A)?,
        sum(CoverageSchema::T)?,
        sum(CoverageSchema::C)?,
        sum(CoverageSchema::G)?,
        sum(CoverageSchema::N)?,
        sum(CoverageSchema::TOTAL)?,
    );
    Ok(if left == right {
        format!("{}: {}", left, counts)
    } else {
        format!("{} - {}: {}", left, right, counts)
    })
}

/// Each variant under the pointer.
pub fn describe_variants(
    state: &State,
    index: usize,
    contig_index: usize,
    left: u64,
    right: u64,
) -> Result<Vec<String>, TGVError> {
    let rows = state.variants[index].query(contig_index, left, right)?;
    let starts = rows.column(VariantSchema::START)?.u64()?;
    let references = rows.column(VariantSchema::REFERENCE)?.str()?;
    let alternates = rows.column(VariantSchema::ALTERNATE)?.list()?;
    let qualities = rows.column(VariantSchema::QUALITY_SCORE)?.f32()?;
    let contig = &state.contig_header.contigs[contig_index].name;
    let mut descriptions = Vec::new();
    for row in 0..rows.height() {
        let alternate = match alternates.get_as_series(row) {
            Some(alleles) => alleles
                .str()?
                .iter()
                .map(|allele| allele.expect("alternate alleles are non-null"))
                .join(","),
            None => String::new(),
        };
        let quality = qualities
            .get(row)
            .map(|q| q.to_string())
            .unwrap_or_else(|| "?".into());
        descriptions.push(format!(
            "Variant: {}:{} {}>{} QUAL={}",
            contig,
            starts.get(row).expect("variant starts are non-null"),
            references.get(row).expect("reference bases are non-null"),
            alternate,
            quality
        ));
    }
    Ok(descriptions)
}

/// Each BED feature under the pointer.
pub fn describe_bed(
    state: &State,
    index: usize,
    contig_index: usize,
    left: u64,
    right: u64,
) -> Result<Vec<String>, TGVError> {
    let rows = state.bed_intervals[index].query(contig_index, left, right)?;
    let starts = rows.column(BedSchema::START)?.u64()?;
    let ends = rows.column(BedSchema::END)?.u64()?;
    let names = rows.column(BedSchema::NAME)?.str()?;
    let contig = &state.contig_header.contigs[contig_index].name;
    Ok((0..rows.height())
        .map(|row| {
            let name = names
                .get(row)
                .map_or_else(String::new, |name| format!(" {name}"));
            format!(
                "BED interval: {}:{}-{}{}",
                contig,
                starts.get(row).expect("BED starts are non-null"),
                ends.get(row).expect("BED ends are non-null"),
                name
            )
        })
        .collect())
}

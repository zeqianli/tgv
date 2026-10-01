//! HTTP request and response types, independent of session serialization.

use crate::track_registry::TrackId;
use gv_core::{
    alignment::{Alignment, BaseCoverage},
    bed::{BedInterval, BedTrack},
    feature::Gene,
    prelude::*,
    variant::{Variant, VariantTrack},
};
use noodles::vcf::variant::record::AlternateBases;
use serde::{Deserialize, Serialize};

const MAX_SUMMARY_ITEMS: usize = 1000;

/// Identifies an inclusive, 1-based interval for inspection.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::server) struct InspectInterval {
    pub contig: String,
    pub start: u64,
    pub end: u64,
}

/// Requests structured results for an explicit interval and optional tracks.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::server) struct InspectRequest {
    pub region: InspectInterval,
    pub tracks: Option<Vec<TrackId>>,
}

impl InspectInterval {
    pub const MAX_QUERY_WIDTH: u64 = 100_000;
}

/// Returns statistics for the effective interval after contig-end clamping.
#[derive(Serialize)]
pub(in crate::server) struct InspectResponse {
    pub region: InspectInterval,
    pub summary: InspectSummary,
    pub warnings: Vec<InspectWarning>,
}

/// Groups per-track and gene summaries for an inspected interval.
#[derive(Serialize)]
pub(in crate::server) struct InspectSummary {
    pub tracks: Vec<TrackSummary>,
    pub genes: GeneSummary,
}

/// Summarizes records overlapping the interval in one selected track.
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(in crate::server) enum TrackSummary {
    Alignment {
        track_id: TrackId,
        overlapping_records: usize,
        coverage: CoverageSummary,
    },
    Variant {
        track_id: TrackId,
        overlapping_records: usize,
        truncated: bool,
        items: Vec<VariantRecord>,
    },
    Bed {
        track_id: TrackId,
        overlapping_records: usize,
        truncated: bool,
        items: Vec<BedRecord>,
    },
}

impl TrackSummary {
    /// Summarizes overlapping reads and their per-position coverage.
    pub fn from_alignment(
        track_id: TrackId,
        alignment: &Alignment,
        contig_index: usize,
        region: &InspectInterval,
    ) -> Self {
        let overlapping_records = alignment
            .overlapping_reads(contig_index, region.start, region.end)
            .count();
        let positions = (region.start..=region.end)
            .map(|position| PositionCoverage::from((position, alignment.coverage_at(position))))
            .collect();
        Self::Alignment {
            track_id,
            overlapping_records,
            coverage: CoverageSummary {
                method: CoverageMethod::ViewerCurrent,
                positions,
            },
        }
    }

    /// Summarizes overlapping variants and includes up to the response item limit.
    pub fn from_variants(
        track_id: TrackId,
        variants: &VariantTrack,
        contig_index: usize,
        region: &InspectInterval,
    ) -> Result<Self, TGVError> {
        let mut records = variants.overlapping(contig_index, region.start, region.end)?;
        records.sort_by_key(|record| (record.start(), record.end(), record.index));
        let items = records
            .iter()
            .take(MAX_SUMMARY_ITEMS)
            .copied()
            .map(VariantRecord::try_from)
            .collect::<Result<Vec<_>, TGVError>>()?;
        Ok(Self::Variant {
            track_id,
            overlapping_records: records.len(),
            truncated: records.len() > items.len(),
            items,
        })
    }

    /// Summarizes overlapping BED intervals and includes up to the response item limit.
    pub fn from_bed(
        track_id: TrackId,
        intervals: &BedTrack,
        contig_index: usize,
        region: &InspectInterval,
    ) -> Result<Self, TGVError> {
        let mut records = intervals.overlapping(contig_index, region.start, region.end)?;
        records.sort_by_key(|record| (record.start(), record.end(), record.index));
        let items: Vec<_> = records
            .iter()
            .take(MAX_SUMMARY_ITEMS)
            .copied()
            .map(BedRecord::from)
            .collect();
        Ok(Self::Bed {
            track_id,
            overlapping_records: records.len(),
            truncated: records.len() > items.len(),
            items,
        })
    }
}

/// Describes one variant returned in an inspection summary.
#[derive(Serialize)]
pub(in crate::server) struct VariantRecord {
    pub start: u64,
    pub end: u64,
    pub reference: String,
    pub alternate: Vec<String>,
}

impl TryFrom<&Variant> for VariantRecord {
    type Error = TGVError;

    /// Extracts variant coordinates and alleles from a VCF record.
    fn try_from(variant: &Variant) -> Result<Self, Self::Error> {
        let alternate = variant
            .record
            .alternate_bases()
            .iter()
            .map(|allele| allele.map(str::to_owned))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            start: variant.start(),
            end: variant.end(),
            reference: variant.record.reference_bases().to_owned(),
            alternate,
        })
    }
}

/// Describes one BED interval returned in an inspection summary.
#[derive(Serialize)]
pub(in crate::server) struct BedRecord {
    pub start: u64,
    pub end: u64,
}

impl From<&BedInterval> for BedRecord {
    /// Copies the genomic coordinates of a BED interval.
    fn from(interval: &BedInterval) -> Self {
        Self {
            start: interval.start(),
            end: interval.end(),
        }
    }
}

/// Summarizes overlapping genes and whether annotations are available.
#[derive(Serialize)]
pub(in crate::server) struct GeneSummary {
    pub available: bool,
    pub overlapping_records: usize,
    pub truncated: bool,
    pub items: Vec<GeneRecord>,
}

impl GeneSummary {
    /// Summarizes overlapping genes while preserving annotation availability.
    pub fn from_genes(
        genes: &[Gene],
        available: bool,
        contig_index: usize,
        region: &InspectInterval,
    ) -> Self {
        let mut records: Vec<_> = genes
            .iter()
            .filter(|gene| gene.overlaps(contig_index, region.start, region.end))
            .collect();
        records.sort_by(|a, b| (a.start(), a.end(), &a.id).cmp(&(b.start(), b.end(), &b.id)));
        let items: Vec<_> = records
            .iter()
            .take(MAX_SUMMARY_ITEMS)
            .copied()
            .map(GeneRecord::from)
            .collect();
        Self {
            available,
            overlapping_records: records.len(),
            truncated: records.len() > items.len(),
            items,
        }
    }
}

/// Describes one gene returned in an inspection summary.
#[derive(Serialize)]
pub(in crate::server) struct GeneRecord {
    pub id: String,
    pub name: String,
    pub start: u64,
    pub end: u64,
    pub strand: String,
}

impl From<&Gene> for GeneRecord {
    /// Copies the fields exposed by the inspection response.
    fn from(gene: &Gene) -> Self {
        Self {
            id: gene.id.clone(),
            name: gene.name.clone(),
            start: gene.start(),
            end: gene.end(),
            strand: gene.strand.to_string(),
        }
    }
}

/// Reports per-position coverage for one alignment track.
#[derive(Serialize)]
pub(in crate::server) struct CoverageSummary {
    pub method: CoverageMethod,
    pub positions: Vec<PositionCoverage>,
}

/// Identifies the calculation used for reported coverage.
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::server) enum CoverageMethod {
    ViewerCurrent,
}

/// Reports base counts and soft clips at one genomic position.
#[derive(Serialize)]
pub(in crate::server) struct PositionCoverage {
    pub position: u64,
    #[serde(rename = "A")]
    pub a: usize,
    #[serde(rename = "C")]
    pub c: usize,
    #[serde(rename = "G")]
    pub g: usize,
    #[serde(rename = "T")]
    pub t: usize,
    #[serde(rename = "N")]
    pub n: usize,
    pub total: usize,
    pub softclip: usize,
}

impl From<(u64, &BaseCoverage)> for PositionCoverage {
    /// Copies the current viewer coverage counts at a position.
    fn from((position, coverage): (u64, &BaseCoverage)) -> Self {
        Self {
            position,
            a: coverage.A,
            c: coverage.C,
            g: coverage.G,
            t: coverage.T,
            n: coverage.N,
            total: coverage.total,
            softclip: coverage.softclip,
        }
    }
}

/// Reports unavailable data in an inspection response.
#[derive(Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub(in crate::server) enum InspectWarning {
    ReferenceUnavailable { message: String },
    GenesUnavailable { message: String },
}

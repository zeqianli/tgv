//! HTTP request and response types, independent of session serialization.

use crate::track_registry::TrackId;
use gv_core::{
    alignment::BaseCoverage, bed::BedInterval, feature::Gene, prelude::*, variant::Variant,
};
use noodles::vcf::variant::record::AlternateBases;
use serde::{Deserialize, Serialize};

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
    const MAX_QUERY_WIDTH: u64 = 100_000;
    /// Validate and convert a InspectInterval (with explict contig names, start, and end) to a tgv Region query.
    pub(crate) fn try_to_region(&self, contig_header: &ContigHeader) -> Result<Region, TGVError> {
        if self.start == 0 || self.end < self.start {
            return Err(TGVError::StateError(
                "Use a positive 1-based inclusive interval with an end at or after the start."
                    .to_string(),
            ));
        }
        let contig_index = contig_header.try_get_index_by_str(self.contig.as_ref())?;

        let header = &contig_header.contigs[contig_index];
        if header.length.is_some_and(|length| self.start > length) {
            return Err(TGVError::StateError(
                "The interval self.starts beyond the contig.".to_string(),
            ));
        }
        let end = header
            .length
            .map_or(self.end, |length| self.end.min(length));
        if self.end - self.start >= Self::MAX_QUERY_WIDTH {
            return Err(TGVError::StateError(
                format!(
                    "Use an interval of at most {} bases within the platform coordinate range.",
                    Self::MAX_QUERY_WIDTH
                )
                .to_string(),
            ));
        }

        Ok(Region {
            focus: Focus {
                contig_index,
                position: start + (end - start) / 2,
            },
            half_width: (end - start).div_ceil(2),
        })
    }
}

/// Returns statistics for the effective interval after contig-end clamping.
#[derive(Serialize)]
pub(in crate::server) struct InspectResponse {
    pub region: InspectInterval,
    pub summary: InspectSummary,
    pub coverage: CoverageSummary,
    pub warnings: Vec<ResponseWarning>,
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

/// Describes one variant returned in an inspection summary.
#[derive(Serialize)]
pub(in crate::server) struct VariantRecord {
    pub start: u64,
    pub end: u64,
    pub reference: String,
    pub alternate: Vec<String>,
}

impl TryFrom<&Variant> for VariantRecord {
    type Error = std::io::Error;

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

/// Groups per-position coverage for the selected alignment tracks.
#[derive(Serialize)]
pub(in crate::server) struct CoverageSummary {
    pub method: CoverageMethod,
    pub tracks: Vec<TrackCoverage>,
}

/// Identifies the calculation used for reported coverage.
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::server) enum CoverageMethod {
    ViewerCurrent,
}

/// Reports coverage positions for one alignment track.
#[derive(Serialize)]
pub(in crate::server) struct TrackCoverage {
    pub track_id: TrackId,
    pub positions: Vec<PositionCoverage>,
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

/// Reports unavailable data or limitations of a rendered view.
#[derive(Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub(in crate::server) enum ResponseWarning {
    ReferenceUnavailable { message: String },
    GenesUnavailable { message: String },
    RenderLimited { track_id: TrackId, message: String },
    RenderBinned { message: String },
}

//! HTTP request and response types, independent of session serialization.

use crate::track_registry::TrackId;
use gv_core::{
    alignment::BaseCoverage, bed::BedInterval, feature::Gene, intervals::GenomeInterval,
    variant::Variant,
};
use noodles::vcf::variant::record::AlternateBases;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::server) struct Interval {
    pub contig: String,
    pub start: u64,
    pub end: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::server) struct InspectRequest {
    pub region: Interval,
    pub tracks: Option<Vec<TrackId>>,
}

#[derive(Serialize)]
pub(in crate::server) struct InspectResponse {
    pub region: Interval,
    pub summary: InspectSummary,
    pub coverage: CoverageSummary,
    pub warnings: Vec<ResponseWarning>,
}

#[derive(Serialize)]
pub(in crate::server) struct InspectSummary {
    pub tracks: Vec<TrackSummary>,
    pub genes: GeneSummary,
}

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

#[derive(Serialize)]
pub(in crate::server) struct VariantRecord {
    pub start: u64,
    pub end: u64,
    pub reference: String,
    pub alternate: Vec<String>,
}

impl TryFrom<&Variant> for VariantRecord {
    type Error = std::io::Error;

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

#[derive(Serialize)]
pub(in crate::server) struct BedRecord {
    pub start: u64,
    pub end: u64,
}

impl From<&BedInterval> for BedRecord {
    fn from(interval: &BedInterval) -> Self {
        Self {
            start: interval.start(),
            end: interval.end(),
        }
    }
}

#[derive(Serialize)]
pub(in crate::server) struct GeneSummary {
    pub available: bool,
    pub overlapping_records: usize,
    pub truncated: bool,
    pub items: Vec<GeneRecord>,
}

#[derive(Serialize)]
pub(in crate::server) struct GeneRecord {
    pub id: String,
    pub name: String,
    pub start: u64,
    pub end: u64,
    pub strand: String,
}

impl From<&Gene> for GeneRecord {
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

#[derive(Serialize)]
pub(in crate::server) struct CoverageSummary {
    pub method: CoverageMethod,
    pub tracks: Vec<TrackCoverage>,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::server) enum CoverageMethod {
    ViewerCurrent,
}

#[derive(Serialize)]
pub(in crate::server) struct TrackCoverage {
    pub track_id: TrackId,
    pub positions: Vec<PositionCoverage>,
}

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

#[derive(Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub(in crate::server) enum ResponseWarning {
    ReferenceUnavailable { message: String },
    GenesUnavailable { message: String },
    RenderLimited { track_id: TrackId, message: String },
    RenderBinned { message: String },
}

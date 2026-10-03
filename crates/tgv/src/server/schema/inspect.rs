//! MCP inspection request and response types, independent of session serialization.

use crate::track_registry::TrackId;
use gv_core::{
    alignment::Alignment, bed::BedTable, gene::GeneTable, prelude::*, variant::VariantTable,
};
use polars::prelude::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const MAX_SUMMARY_ITEMS: usize = 1000;

/// Identifies an inclusive, 1-based interval for inspection.
#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(in crate::server) struct InspectInterval {
    pub contig: String,
    pub start: u64,
    pub end: u64,
}

/// Requests structured results for an explicit interval and optional tracks.
#[derive(Deserialize, JsonSchema)]
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
    ) -> Result<Self, TGVError> {
        let overlapping_records =
            if alignment.contig_index != contig_index || region.start > region.end {
                0
            } else {
                alignment
                    .tables
                    .reads
                    .clone()
                    .lazy()
                    .filter(
                        col("stacking_start")
                            .lt_eq(lit(region.end))
                            .and(col("stacking_end").gt_eq(lit(region.start))),
                    )
                    .select([col("read_id")])
                    .collect()?
                    .height()
            };
        let coverage = alignment.coverage.query(region.start, region.end)?;
        let mut rows = coverage
            .column("pos")?
            .u64()?
            .into_no_null_iter()
            .enumerate()
            .peekable();
        let a = coverage.column("A")?.u64()?;
        let c = coverage.column("C")?.u64()?;
        let g = coverage.column("G")?.u64()?;
        let t = coverage.column("T")?.u64()?;
        let n = coverage.column("N")?.u64()?;
        let total = coverage.column("total")?.u64()?;
        let softclip = coverage.column("softclip")?.u64()?;
        let positions = (region.start..=region.end)
            .map(|position| {
                let row = if rows.peek().is_some_and(|(_, pos)| *pos == position) {
                    rows.next().map(|(row, _)| row)
                } else {
                    None
                };
                let count = |column: &polars::prelude::UInt64Chunked| {
                    row.map_or(0, |row| {
                        column.get(row).expect("coverage counts are non-null") as usize
                    })
                };
                PositionCoverage {
                    position,
                    a: count(a),
                    c: count(c),
                    g: count(g),
                    t: count(t),
                    n: count(n),
                    total: count(total),
                    softclip: count(softclip),
                }
            })
            .collect();
        Ok(Self::Alignment {
            track_id,
            overlapping_records,
            coverage: CoverageSummary {
                method: CoverageMethod::ViewerCurrent,
                positions,
            },
        })
    }

    /// Summarizes overlapping variants and includes up to the response item limit.
    pub fn from_variants(
        track_id: TrackId,
        variants: &VariantTable,
        contig_index: usize,
        region: &InspectInterval,
    ) -> Result<Self, TGVError> {
        let rows = variants.query(contig_index, region.start, region.end)?;
        let starts = rows.column("start")?.u64()?;
        let ends = rows.column("end")?.u64()?;
        let reference = rows.column("reference")?.str()?;
        let alternate = rows.column("alternate")?.list()?;
        let items = (0..rows.height().min(MAX_SUMMARY_ITEMS))
            .map(|row| {
                let alleles = alternate.get_as_series(row);
                Ok(VariantRecord {
                    start: starts.get(row).expect("variant starts are non-null"),
                    end: ends.get(row).expect("variant ends are non-null"),
                    reference: reference
                        .get(row)
                        .expect("reference bases are non-null")
                        .to_owned(),
                    alternate: match alleles {
                        Some(alleles) => alleles
                            .str()?
                            .iter()
                            .map(|allele| allele.expect("alternate alleles are non-null"))
                            .map(str::to_owned)
                            .collect(),
                        None => Vec::new(),
                    },
                })
            })
            .collect::<Result<Vec<_>, TGVError>>()?;
        Ok(Self::Variant {
            track_id,
            overlapping_records: rows.height(),
            truncated: rows.height() > items.len(),
            items,
        })
    }

    /// Summarizes overlapping BED intervals and includes up to the response item limit.
    pub fn from_bed(
        track_id: TrackId,
        intervals: &BedTable,
        contig_index: usize,
        region: &InspectInterval,
    ) -> Result<Self, TGVError> {
        let rows = intervals.query(contig_index, region.start, region.end)?;
        let items = rows
            .column("start")?
            .u64()?
            .into_no_null_iter()
            .zip(rows.column("end")?.u64()?.into_no_null_iter())
            .take(MAX_SUMMARY_ITEMS)
            .map(|(start, end)| BedRecord { start, end })
            .collect::<Vec<_>>();
        Ok(Self::Bed {
            track_id,
            overlapping_records: rows.height(),
            truncated: rows.height() > items.len(),
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

/// Describes one BED interval returned in an inspection summary.
#[derive(Serialize)]
pub(in crate::server) struct BedRecord {
    pub start: u64,
    pub end: u64,
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
        genes: &GeneTable,
        available: bool,
        contig_index: usize,
        region: &InspectInterval,
    ) -> Result<Self, TGVError> {
        let rows = genes
            .query(contig_index, region.start, region.end)?
            .lazy()
            .sort(
                ["start", "end", "id", "row_id"],
                SortMultipleOptions::default(),
            )
            .collect()?;
        let ids = rows.column("id")?.str()?;
        let names = rows.column("name")?.str()?;
        let starts = rows.column("start")?.u64()?;
        let ends = rows.column("end")?.u64()?;
        let strands = rows.column("strand")?.str()?;
        let items = (0..rows.height().min(MAX_SUMMARY_ITEMS))
            .map(|row| GeneRecord {
                id: ids.get(row).expect("gene IDs are non-null").to_owned(),
                name: names.get(row).expect("gene names are non-null").to_owned(),
                start: starts.get(row).expect("gene starts are non-null"),
                end: ends.get(row).expect("gene ends are non-null"),
                strand: strands
                    .get(row)
                    .expect("gene strands are non-null")
                    .to_owned(),
            })
            .collect::<Vec<_>>();
        Ok(Self {
            available,
            overlapping_records: rows.height(),
            truncated: rows.height() > items.len(),
            items,
        })
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

/// Reports unavailable data in an inspection response.
#[derive(Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub(in crate::server) enum InspectWarning {
    ReferenceUnavailable { message: String },
    GenesUnavailable { message: String },
}

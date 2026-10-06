//! Inspection request and response types, independent of session file serialization.

use crate::error::SessionError;
use gv_core::{
    alignment::{Alignment, CoverageSchema, tables::ReadSchema},
    bed::{BedSchema, BedTable},
    gene::{GeneSchema, GeneTable},
    prelude::*,
    variant::{VariantSchema, VariantTable},
};
use polars::prelude::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const MAX_SUMMARY_ITEMS: usize = 1000;

/// Identifies an inclusive, 1-based interval for inspection.
#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InspectInterval {
    pub contig: String,
    pub start: u64,
    pub end: u64,
}

/// Requests structured results for an explicit interval and optional tracks.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InspectRequest {
    pub region: InspectInterval,
    pub tracks: Option<Vec<TrackId>>,
}

impl InspectInterval {
    pub const MAX_QUERY_WIDTH: u64 = 100_000;

    /// Validates the interval against the dataset contigs.
    ///
    /// Returns the core query region and the effective interval, with its end clamped to a
    /// known contig length.
    pub fn resolve(
        &self,
        contigs: &ContigHeader,
    ) -> Result<(Region, InspectInterval), SessionError> {
        let query = Region::try_from_contig_names_and_bounds(
            &self.contig,
            self.start,
            self.end,
            contigs,
            Some(Self::MAX_QUERY_WIDTH),
        )
        .map_err(|error| SessionError::InvalidInput {
            field: "region",
            message: error.to_string(),
        })?;
        let header = &contigs.contigs[query.contig_index()];
        let effective = InspectInterval {
            contig: header.name.clone(),
            start: self.start,
            end: header
                .length
                .map_or(self.end, |length| self.end.min(length)),
        };
        Ok((query, effective))
    }
}

/// Returns statistics for the effective interval after contig-end clamping.
#[derive(Serialize)]
pub struct InspectResponse {
    pub region: InspectInterval,
    pub summary: InspectSummary,
    pub warnings: Vec<InspectWarning>,
}

/// Groups per-track and gene summaries for an inspected interval.
#[derive(Serialize)]
pub struct InspectSummary {
    pub tracks: Vec<TrackSummary>,
    pub genes: GeneSummary,
}

/// Summarizes records overlapping the interval in one selected track.
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TrackSummary {
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
    /// Summarizes overlapping reads and their depth over the interval.
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
                        col(ReadSchema::STACKING_START)
                            .lt_eq(lit(region.end))
                            .and(col(ReadSchema::STACKING_END).gt_eq(lit(region.start))),
                    )
                    .select([col(ReadSchema::READ_ID)])
                    .collect()?
                    .height()
            };
        let width = region.end - region.start + 1;
        let coverage = if alignment.contig_index == contig_index {
            alignment.coverage.query(region.start, region.end)?
        } else {
            CoverageSchema::empty()
        };
        let totals = coverage.column(CoverageSchema::TOTAL)?.u64()?;
        let covered = totals
            .into_no_null_iter()
            .filter(|&total| total > 0)
            .count() as u64;
        let total = totals.sum().unwrap_or(0);
        Ok(Self::Alignment {
            track_id,
            overlapping_records,
            coverage: CoverageSummary {
                method: CoverageMethod::ViewerCurrent,
                positions: width,
                zero_depth_positions: width - covered,
                mean_depth: total as f64 / width as f64,
                min_depth: if covered < width {
                    0
                } else {
                    totals.min().unwrap_or(0)
                },
                max_depth: totals.max().unwrap_or(0),
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
        let starts = rows.column(VariantSchema::START)?.u64()?;
        let ends = rows.column(VariantSchema::END)?.u64()?;
        let reference = rows.column(VariantSchema::REFERENCE)?.str()?;
        let alternate = rows.column(VariantSchema::ALTERNATE)?.list()?;
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
            .column(BedSchema::START)?
            .u64()?
            .into_no_null_iter()
            .zip(rows.column(BedSchema::END)?.u64()?.into_no_null_iter())
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
pub struct VariantRecord {
    pub start: u64,
    pub end: u64,
    pub reference: String,
    pub alternate: Vec<String>,
}

/// Describes one BED interval returned in an inspection summary.
#[derive(Serialize)]
pub struct BedRecord {
    pub start: u64,
    pub end: u64,
}

/// Summarizes overlapping genes and whether annotations are available.
#[derive(Serialize)]
pub struct GeneSummary {
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
                [
                    GeneSchema::START,
                    GeneSchema::END,
                    GeneSchema::ID,
                    GeneSchema::ROW_ID,
                ],
                SortMultipleOptions::default(),
            )
            .collect()?;
        let ids = rows.column(GeneSchema::ID)?.str()?;
        let names = rows.column(GeneSchema::NAME)?.str()?;
        let starts = rows.column(GeneSchema::START)?.u64()?;
        let ends = rows.column(GeneSchema::END)?.u64()?;
        let strands = rows.column(GeneSchema::STRAND)?.str()?;
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
pub struct GeneRecord {
    pub id: String,
    pub name: String,
    pub start: u64,
    pub end: u64,
    pub strand: String,
}

/// Summarizes the depth of one alignment track over the interval.
///
/// Per-position counts are available through the `coverage` table of the `query` tool.
#[derive(Serialize)]
pub struct CoverageSummary {
    pub method: CoverageMethod,
    pub positions: u64,
    pub zero_depth_positions: u64,
    pub mean_depth: f64,
    pub min_depth: u64,
    pub max_depth: u64,
}

/// Identifies the calculation used for reported coverage.
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageMethod {
    ViewerCurrent,
}

/// Reports unavailable data in an inspection response.
#[derive(Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum InspectWarning {
    ReferenceUnavailable { message: String },
    GenesUnavailable { message: String },
}

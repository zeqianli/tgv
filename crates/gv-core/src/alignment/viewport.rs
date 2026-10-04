//! Batched viewport projections of CIGAR runs and sparse annotations.

use super::AlignmentTables;
use super::tables::{BaseModificationSchema, CigarSchema, ReadSchema, ReferenceMismatchSchema};
use crate::error::TGVError;
use polars::lazy::dsl::{max_horizontal, min_horizontal};
use polars::prelude::*;

/// Query results contain selected reads, clipped CIGAR runs, and sparse annotations.
///
/// Run rows include read display state (`y` and `reverse`). `run_offset` indexes
/// the original run SEQ string at the clipped, one-based `display_start`.
#[derive(Debug)]
pub struct AlignmentViewport {
    pub reads: DataFrame,
    pub runs: DataFrame,
    pub reference_mismatches: DataFrame,
    pub base_modifications: DataFrame,
}

impl AlignmentTables {
    pub(super) fn viewport(
        &self,
        start: u64,
        end: u64,
        read_ids: &[usize],
    ) -> Result<AlignmentViewport, TGVError> {
        let selected = Series::new(
            ReadSchema::READ_ID.into(),
            read_ids.iter().map(|id| *id as u64).collect::<Vec<_>>(),
        );
        let membership = col(ReadSchema::READ_ID).is_in(lit(selected).implode(true), false);
        let reads = self.reads.clone().lazy().filter(membership.clone());
        let left = max_horizontal([col(CigarSchema::DISPLAY_START), lit(start.max(1))])?;
        let right = min_horizontal([col(CigarSchema::DISPLAY_END), lit(end)])?;
        let runs = self
            .cigar_runs
            .clone()
            .lazy()
            .filter(
                col(CigarSchema::DISPLAY_START)
                    .lt_eq(lit(end))
                    .and(col(CigarSchema::DISPLAY_END).gt_eq(lit(start.max(1))))
                    .and(lit(start.max(1) <= end)),
            )
            .inner_join(
                reads.clone().select([
                    col(ReadSchema::READ_ID),
                    col(ReadSchema::Y),
                    col(ReadSchema::REVERSE),
                ]),
                col(CigarSchema::READ_ID),
                col(ReadSchema::READ_ID),
            )
            .with_columns([
                (left.clone() - col(CigarSchema::DISPLAY_START)
                    + col(CigarSchema::RUN_OFFSET).cast(DataType::UInt64))
                .cast(DataType::UInt32)
                .alias(CigarSchema::RUN_OFFSET),
                left.alias(CigarSchema::DISPLAY_START),
                right.alias(CigarSchema::DISPLAY_END),
            ])
            .sort(
                [CigarSchema::READ_ID, CigarSchema::OP_INDEX],
                SortMultipleOptions::default(),
            )
            .collect()?;
        let annotations = |table: &DataFrame, position: &str| {
            table
                .clone()
                .lazy()
                .filter(
                    membership
                        .clone()
                        .and(col(position).gt_eq(lit(start)))
                        .and(col(position).lt_eq(lit(end))),
                )
                .collect()
        };
        Ok(AlignmentViewport {
            reads: reads.collect()?,
            runs,
            reference_mismatches: annotations(
                &self.reference_mismatches,
                ReferenceMismatchSchema::REF_POS,
            )?,
            base_modifications: annotations(
                &self.base_modifications,
                BaseModificationSchema::DISPLAY_POS,
            )?,
        })
    }
}

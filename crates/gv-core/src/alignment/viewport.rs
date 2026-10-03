//! Batched viewport projections of CIGAR runs and sparse annotations.

use super::AlignmentTables;
use super::tables::{BaseModificationSchema, CigarRunSchema, ReadSchema, ReferenceMismatchSchema};
use crate::error::TGVError;
use noodles::sam::alignment::record::cigar::op::Kind;
use polars::lazy::dsl::{max_horizontal, min_horizontal};
use polars::prelude::*;

/// Query results retain run payloads and add clipped, one-based, inclusive display bounds.
///
/// `run_offset` is the offset into the original run SEQ string at
/// `display_start`. Insertions have equal display bounds at their reference cursor.
#[derive(Debug)]
pub struct AlignmentViewport {
    pub reads: DataFrame,
    pub runs: Vec<(Kind, DataFrame)>,
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
        let membership = col(CigarRunSchema::READ_ID).is_in(lit(selected).implode(true), false);
        let reads = self
            .reads
            .clone()
            .lazy()
            .filter(membership.clone())
            .collect()?;
        let first_ops = concat(
            [
                &self.r#match,
                &self.sequence_match,
                &self.mismatch,
                &self.insertion,
                &self.deletion,
                &self.reference_skip,
                &self.soft_clip,
            ]
            .map(|table| {
                table
                    .clone()
                    .lazy()
                    .filter(membership.clone())
                    .select([col(CigarRunSchema::READ_ID), col(CigarRunSchema::OP_INDEX)])
            }),
            UnionArgs::default(),
        )?
        .group_by([col(CigarRunSchema::READ_ID)])
        .agg([col(CigarRunSchema::OP_INDEX)
            .min()
            .alias(CigarRunSchema::FIRST_OP_INDEX)]);
        let mut runs = Vec::with_capacity(9);
        for (kind, table) in [
            (Kind::Match, &self.r#match),
            (Kind::SequenceMatch, &self.sequence_match),
            (Kind::SequenceMismatch, &self.mismatch),
            (Kind::Insertion, &self.insertion),
            (Kind::Deletion, &self.deletion),
            (Kind::Skip, &self.reference_skip),
            (Kind::SoftClip, &self.soft_clip),
            (Kind::HardClip, &self.hard_clip),
            (Kind::Pad, &self.padding),
        ] {
            let mut query = table.clone().lazy().filter(membership.clone());
            let cursor = col(CigarRunSchema::REF_START).cast(DataType::Int128);
            let length = col(CigarRunSchema::OP_LEN).cast(DataType::Int128);
            let origin = if kind == Kind::SoftClip {
                query = query.left_join(
                    first_ops.clone(),
                    col(CigarRunSchema::READ_ID),
                    col(CigarRunSchema::READ_ID),
                );
                when(col(CigarRunSchema::OP_INDEX).eq(col(CigarRunSchema::FIRST_OP_INDEX)))
                    .then(cursor.clone() - length.clone())
                    .otherwise(cursor.clone())
            } else {
                cursor.clone()
            };
            if matches!(kind, Kind::HardClip | Kind::Pad) {
                runs.push((
                    kind,
                    query
                        .with_columns([
                            lit(NULL)
                                .cast(DataType::UInt64)
                                .alias(CigarRunSchema::DISPLAY_START),
                            lit(NULL)
                                .cast(DataType::UInt64)
                                .alias(CigarRunSchema::DISPLAY_END),
                            lit(NULL)
                                .cast(DataType::UInt32)
                                .alias(CigarRunSchema::RUN_OFFSET),
                        ])
                        .filter(lit(false))
                        .collect()?,
                ));
                continue;
            }
            let limit = if kind == Kind::Insertion {
                origin.clone()
            } else {
                origin.clone() + length.clone() - lit(1i128)
            };
            let left = max_horizontal([origin.clone(), lit(start.max(1) as i128)])?;
            let right = min_horizontal([limit, lit(end as i128)])?;
            let valid = col(CigarRunSchema::REF_START)
                .is_not_null()
                .and(left.clone().lt_eq(right.clone()));
            let valid = if kind == Kind::Insertion {
                valid
            } else {
                valid.and(col(CigarRunSchema::OP_LEN).gt(lit(0u32)))
            };
            let frame = query
                .filter(valid)
                .with_columns([
                    left.clone()
                        .cast(DataType::UInt64)
                        .alias(CigarRunSchema::DISPLAY_START),
                    right
                        .cast(DataType::UInt64)
                        .alias(CigarRunSchema::DISPLAY_END),
                    (left - origin)
                        .cast(DataType::UInt32)
                        .alias(CigarRunSchema::RUN_OFFSET),
                ])
                .sort(
                    [CigarRunSchema::READ_ID, CigarRunSchema::OP_INDEX],
                    SortMultipleOptions::default(),
                )
                .collect()?;
            runs.push((kind, frame));
        }
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
            reads,
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

//! Batched viewport projections of CIGAR runs and sparse annotations.

use super::AlignmentTables;
use crate::error::TGVError;
use noodles::sam::alignment::record::cigar::op::Kind;
use polars::prelude::*;
use std::collections::HashSet;

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

fn select_reads(frame: &DataFrame, selected: &HashSet<u64>) -> PolarsResult<DataFrame> {
    let ids = frame.column("read_id")?.u64()?;
    let mask: BooleanChunked = ids
        .iter()
        .map(|id| id.map(|id| selected.contains(&id)))
        .collect();
    frame.filter(&mask)
}

impl AlignmentTables {
    pub(super) fn viewport(
        &self,
        start: u64,
        end: u64,
        read_ids: &[usize],
    ) -> Result<AlignmentViewport, TGVError> {
        let selected = read_ids.iter().map(|id| *id as u64).collect::<HashSet<_>>();
        let reads = select_reads(&self.reads, &selected)?;
        let mut ignored_keys = HashSet::new();
        for kind in [Kind::HardClip, Kind::Pad] {
            let ignored = select_reads(self.run(kind), &selected)?;
            ignored_keys.extend(
                ignored
                    .column("read_id")?
                    .u64()?
                    .into_no_null_iter()
                    .zip(ignored.column("op_index")?.u32()?.into_no_null_iter()),
            );
        }
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
            let mut frame = select_reads(table, &selected)?;
            if kind.consumes_reference() {
                let cursors = frame.column("ref_start")?.u64()?;
                let lengths = frame.column("op_len")?.cast(&DataType::UInt64)?;
                let limits = cursors + lengths.u64()?;
                frame = frame.filter(&(cursors.lt_eq(end) & limits.gt(start)))?;
            } else if kind == Kind::Insertion {
                let cursors = frame.column("ref_start")?.u64()?;
                frame = frame.filter(&(cursors.gt_eq(start) & cursors.lt_eq(end)))?;
            }
            let ids = frame.column("read_id")?.u64()?;
            let indexes = frame.column("op_index")?.u32()?;
            let cursors = frame.column("ref_start")?.u64()?;
            let lengths = frame.column("op_len")?.u32()?;
            let mut display_start = Vec::with_capacity(frame.height());
            let mut display_end = Vec::with_capacity(frame.height());
            let mut run_offset = Vec::with_capacity(frame.height());
            for row in 0..frame.height() {
                let bounds = cursors.get(row).and_then(|cursor| {
                    let index = indexes.get(row).expect("run indexes are non-null");
                    let id = ids.get(row).expect("run IDs are non-null");
                    let length = u64::from(lengths.get(row).expect("run lengths are non-null"));
                    let leading = kind == Kind::SoftClip
                        && (0..index).all(|previous| ignored_keys.contains(&(id, previous)));
                    let origin = i128::from(cursor) - if leading { i128::from(length) } else { 0 };
                    let limit = match kind {
                        Kind::HardClip | Kind::Pad => return None,
                        Kind::Insertion => origin,
                        _ if length == 0 => return None,
                        _ => origin + i128::from(length) - 1,
                    };
                    if kind == Kind::Insertion {
                        return (cursor >= start && cursor <= end).then_some((cursor, cursor, 0));
                    }
                    let left = origin.max(i128::from(start.max(1)));
                    let right = limit.min(i128::from(end));
                    (left <= right).then_some((left as u64, right as u64, (left - origin) as u32))
                });
                display_start.push(bounds.map(|b| b.0));
                display_end.push(bounds.map(|b| b.1));
                run_offset.push(bounds.map(|b| b.2));
            }
            frame.with_column(Column::new("display_start".into(), display_start))?;
            frame.with_column(Column::new("display_end".into(), display_end))?;
            frame.with_column(Column::new("run_offset".into(), run_offset))?;
            let mask = frame.column("display_start")?.is_not_null();
            runs.push((kind, frame.filter(&mask)?));
        }
        let select_annotations = |frame: &DataFrame, position: &str| -> PolarsResult<DataFrame> {
            let positions = frame.column(position)?.u64()?;
            let ids = frame.column("read_id")?.u64()?;
            let selected_mask: BooleanChunked = ids
                .iter()
                .map(|id| id.map(|id| selected.contains(&id)))
                .collect();
            frame.filter(&(selected_mask & positions.gt_eq(start) & positions.lt_eq(end)))
        };
        Ok(AlignmentViewport {
            reads,
            runs,
            reference_mismatches: select_annotations(&self.reference_mismatches, "ref_pos")?,
            base_modifications: select_annotations(&self.base_modifications, "display_pos")?,
        })
    }
}

//! Draw visible runs and annotations directly into the buffer in layer order.

use crate::{
    layout::{AlignmentView, OnScreenCoordinate},
    rendering::colors::Palette,
};
use gv_core::{
    alignment::{
        Alignment, AlignmentTables, PairSchema, PairedAlignment,
        tables::{BaseModificationSchema, CigarSchema, ReadSchema, ReferenceMismatchSchema},
    },
    prelude::*,
};
use itertools::izip;
use noodles::sam::record::data::field::value::base_modifications::group::Modification;
use polars::prelude::*;
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::Color,
};

fn pixel(position: u64, view: &AlignmentView, area: &Rect) -> Option<u16> {
    match view.onscreen_x_coordinate(position, area) {
        OnScreenCoordinate::OnScreen(x) if x < usize::from(area.width) => Some(x as u16),
        _ => None,
    }
}

/// Render visible reads from the zero-based `top` row, followed by annotations.
pub fn render_alignment(
    top: usize,
    area: &Rect,
    buf: &mut Buffer,
    alignment: &Alignment,
    view: &AlignmentView,
    palette: &Palette,
) -> Result<(), TGVError> {
    let region = view.region(area);
    if area.is_empty() || region.contig_index() != alignment.contig_index {
        return Ok(());
    }
    let reads = alignment.tables.reads.clone().lazy().filter(
        col(ReadSchema::SHOW)
            .and(col(ReadSchema::STACKING_START).lt_eq(lit(region.end())))
            .and(col(ReadSchema::STACKING_END).gt_eq(lit(region.start())))
            .and(col(ReadSchema::Y).gt_eq(lit(top as u64)))
            .and(col(ReadSchema::Y).lt(lit((top + usize::from(area.height)) as u64))),
    );
    draw_reads(top, area, buf, &alignment.tables, reads, view, palette)?;
    Ok(())
}

/// Render pair gaps, mates, and singletons from the zero-based `top` row.
pub fn render_paired_alignment(
    top: usize,
    area: &Rect,
    buf: &mut Buffer,
    alignment: &Alignment,
    view: &AlignmentView,
    paired: &PairedAlignment,
    palette: &Palette,
) -> Result<(), TGVError> {
    let region = view.region(area);
    if area.is_empty() || region.contig_index() != alignment.contig_index {
        return Ok(());
    }
    let visible = col(PairSchema::SHOW)
        .and(col(PairSchema::STACKING_START).lt_eq(lit(region.end())))
        .and(col(PairSchema::STACKING_END).gt_eq(lit(region.start())))
        .and(col(PairSchema::Y).gt_eq(lit(top as u64)))
        .and(col(PairSchema::Y).lt(lit((top + usize::from(area.height)) as u64)));
    let pairs = paired
        .pairs
        .clone()
        .lazy()
        .filter(visible.clone())
        .collect()?;
    let first = pairs.column(PairSchema::READ_1_ID)?.u64()?;
    let second = pairs.column(PairSchema::READ_2_ID)?.u64()?;
    let ys = pairs.column(PairSchema::Y)?.u64()?;
    let reads = &alignment.tables.reads;
    let starts = reads.column(ReadSchema::STACKING_START)?.u64()?;
    let ends = reads.column(ReadSchema::STACKING_END)?.u64()?;
    let shown = reads.column(ReadSchema::SHOW)?.bool()?;
    for ((first, second), y) in first
        .into_no_null_iter()
        .zip(second.iter())
        .zip(ys.into_no_null_iter())
    {
        let Some(second) = second else { continue };
        let (first, second) = (first as usize, second as usize);
        if !shown.get(first).expect("read visibility is non-null")
            || !shown.get(second).expect("read visibility is non-null")
        {
            continue;
        }
        let (Some(a), Some(b), Some(c), Some(d)) = (
            starts.get(first),
            ends.get(first),
            starts.get(second),
            ends.get(second),
        ) else {
            continue;
        };
        let (start, end) = if b < c {
            (b + 1, c - 1)
        } else if d < a {
            (d + 1, a - 1)
        } else {
            continue;
        };
        let (start, end) = (start.max(region.start()), end.min(region.end()));
        if start > end {
            continue;
        }
        let Some(left) = pixel(start, view, area) else {
            continue;
        };
        let right = pixel(end, view, area).unwrap_or(area.width - 1);
        let y = area.y + (y as usize - top) as u16;
        for x in left..=right {
            if let Some(cell) = buf.cell_mut(Position::new(area.x + x, y)) {
                cell.set_symbol("-")
                    .set_bg(palette.background)
                    .set_fg(palette.PAIRGAP_COLOR);
            }
        }
    }
    let pairs = pairs.lazy();
    let members = concat(
        [
            pairs.clone().select([
                col(PairSchema::READ_1_ID).alias(ReadSchema::READ_ID),
                col(PairSchema::Y),
            ]),
            pairs
                .filter(col(PairSchema::READ_2_ID).is_not_null())
                .select([
                    col(PairSchema::READ_2_ID).alias(ReadSchema::READ_ID),
                    col(PairSchema::Y),
                ]),
            paired
                .singles
                .clone()
                .lazy()
                .filter(visible)
                .select([col(ReadSchema::READ_ID), col(ReadSchema::Y)]),
        ],
        UnionArgs::default(),
    )?;
    let reads = alignment
        .tables
        .reads
        .clone()
        .lazy()
        .filter(col(ReadSchema::SHOW))
        .drop(cols([ReadSchema::Y]))
        .inner_join(members, col(ReadSchema::READ_ID), col(ReadSchema::READ_ID));
    draw_reads(top, area, buf, &alignment.tables, reads, view, palette)?;
    Ok(())
}

fn draw_reads(
    top: usize,
    area: &Rect,
    buf: &mut Buffer,
    tables: &AlignmentTables,
    reads: LazyFrame,
    view: &AlignmentView,
    palette: &Palette,
) -> PolarsResult<()> {
    let region = view.region(area);
    let reads = reads.select([
        col(ReadSchema::READ_ID),
        col(ReadSchema::Y),
        col(ReadSchema::REVERSE),
        col(ReadSchema::STACKING_START),
        col(ReadSchema::STACKING_END),
    ]);
    // The stored left-table order defines which event wins within each layer.
    let query = |table: &DataFrame, start: &'static str, end: &'static str| {
        table
            .clone()
            .lazy()
            .filter(
                col(start)
                    .lt_eq(lit(region.end()))
                    .and(col(end).gt_eq(lit(region.start()))),
            )
            .join(
                reads.clone(),
                [col(ReadSchema::READ_ID)],
                [col(ReadSchema::READ_ID)],
                JoinArgs {
                    maintain_order: MaintainOrderJoin::Left,
                    ..JoinArgs::new(JoinType::Inner)
                },
            )
            .collect()
    };
    let runs = query(
        &tables.cigar_runs,
        CigarSchema::DISPLAY_START,
        CigarSchema::DISPLAY_END,
    )?;
    let ys = runs.column(ReadSchema::Y)?.u64()?;
    let kinds = runs.column(CigarSchema::KIND)?.u8()?;
    let starts = runs.column(CigarSchema::DISPLAY_START)?.u64()?;
    let ends = runs.column(CigarSchema::DISPLAY_END)?.u64()?;
    let offsets = runs.column(CigarSchema::RUN_OFFSET)?.u32()?;
    let sequences = runs.column(CigarSchema::SEQ)?.str()?;
    let reverse = runs.column(ReadSchema::REVERSE)?.bool()?;
    let read_starts = runs.column(ReadSchema::STACKING_START)?.u64()?;
    let read_ends = runs.column(ReadSchema::STACKING_END)?.u64()?;
    let run_rows = || {
        izip!(
            kinds.into_no_null_iter(),
            starts.into_no_null_iter(),
            ends.into_no_null_iter(),
            offsets.into_no_null_iter(),
            sequences.iter(),
            ys.into_no_null_iter(),
            reverse
                .iter()
                .map(|reverse| reverse.expect("read flags are non-null")),
            read_starts.into_no_null_iter(),
            read_ends.into_no_null_iter(),
        )
    };
    let screen_y = |y: u64| area.y + (y as usize - top) as u16;
    let screen_position =
        |position, y| pixel(position, view, area).map(|x| Position::new(area.x + x, screen_y(y)));
    #[derive(Clone, Copy)]
    enum Layer {
        Body,
        Arrow,
        Insertion,
        Mismatch,
    }
    for layer in [Layer::Body, Layer::Arrow, Layer::Insertion, Layer::Mismatch] {
        for (kind, run_start, run_end, offset, sequence, y, reverse, read_start, read_end) in
            run_rows()
        {
            if kind > CigarSchema::SEQUENCE_MISMATCH {
                return Err(PolarsError::ComputeError(
                    format!("Invalid CIGAR kind code: {kind}.").into(),
                ));
            }
            if matches!(kind, CigarSchema::HARD_CLIP | CigarSchema::PADDING) {
                continue;
            }
            match layer {
                Layer::Body | Layer::Arrow if kind == CigarSchema::INSERTION => continue,
                Layer::Insertion if kind != CigarSchema::INSERTION => continue,
                Layer::Mismatch if kind != CigarSchema::SEQUENCE_MISMATCH => continue,
                _ => {}
            }
            let sequence = sequence.map(str::as_bytes);
            if matches!(
                kind,
                CigarSchema::MATCH
                    | CigarSchema::SEQUENCE_MATCH
                    | CigarSchema::SEQUENCE_MISMATCH
                    | CigarSchema::SOFT_CLIP
            ) && sequence.is_none()
            {
                continue;
            }
            let (start, end) = (run_start.max(region.start()), run_end.min(region.end()));
            let Some(left) = pixel(start, view, area) else {
                continue;
            };
            let right = pixel(end, view, area).unwrap_or(area.width - 1);
            let (left, right) = if matches!(layer, Layer::Arrow) {
                let endpoint = if reverse { read_start } else { read_end };
                if endpoint != (if reverse { run_start } else { run_end }) {
                    continue;
                }
                let Some(x) = pixel(endpoint, view, area) else {
                    continue;
                };
                (x, x)
            } else {
                (left, right)
            };
            let y = screen_y(y);
            for x in left..=right {
                let Some(cell) = buf.cell_mut(Position::new(area.x + x, y)) else {
                    continue;
                };
                match (layer, kind) {
                    (Layer::Body, CigarSchema::SOFT_CLIP) | (Layer::Mismatch, _) => {
                        let position =
                            (region.start() + u64::from(x) * view.zoom + view.zoom - 1).min(end);
                        let offset = offset as usize + (position - run_start) as usize;
                        let base = sequence.expect("base-bearing runs have SEQ")[offset];
                        cell.set_char(base as char);
                        if kind == CigarSchema::SOFT_CLIP {
                            cell.set_bg(palette.softclip_color(base))
                                .set_fg(Color::Reset);
                        } else {
                            cell.set_fg(palette.mismatch_color(base));
                        }
                    }
                    (Layer::Body, _) => {
                        let (bg, fg) = if matches!(
                            kind,
                            CigarSchema::DELETION | CigarSchema::REFERENCE_SKIP
                        ) {
                            (palette.background, palette.DELETION_COLOR)
                        } else {
                            (palette.MATCH_COLOR, palette.MATCH_FG_COLOR)
                        };
                        cell.set_symbol("-").set_bg(bg).set_fg(fg);
                    }
                    (Layer::Arrow, _) => {
                        cell.set_symbol(if reverse { "◄" } else { "►" });
                    }
                    (Layer::Insertion, _) => {
                        cell.set_symbol("▌").set_fg(palette.INSERTION_COLOR);
                    }
                }
            }
        }
    }
    let mismatches = query(
        &tables.reference_mismatches,
        ReferenceMismatchSchema::REF_POS,
        ReferenceMismatchSchema::REF_POS,
    )?;
    let positions = mismatches.column(ReferenceMismatchSchema::REF_POS)?.u64()?;
    let bases = mismatches.column(ReferenceMismatchSchema::BASE)?.u8()?;
    let ys = mismatches.column(ReadSchema::Y)?.u64()?;
    for ((position, base), y) in positions
        .into_no_null_iter()
        .zip(bases.into_no_null_iter())
        .zip(ys.into_no_null_iter())
    {
        if let Some(position) = screen_position(position, y)
            && let Some(cell) = buf.cell_mut(position)
        {
            cell.set_char(base as char)
                .set_fg(palette.mismatch_color(base));
        }
    }
    let modifications = query(
        &tables.base_modifications,
        BaseModificationSchema::DISPLAY_POS,
        BaseModificationSchema::DISPLAY_POS,
    )?;
    let positions = modifications
        .column(BaseModificationSchema::DISPLAY_POS)?
        .u64()?;
    let codes = modifications.column(BaseModificationSchema::CODE)?.u8()?;
    let chebi = modifications
        .column(BaseModificationSchema::CHEBI_ID)?
        .u32()?;
    let probabilities = modifications
        .column(BaseModificationSchema::PROBABILITY)?
        .u8()?;
    let ys = modifications.column(ReadSchema::Y)?.u64()?;
    for (position, code, chebi, probability, y) in izip!(
        positions.into_no_null_iter(),
        codes.iter(),
        chebi.iter(),
        probabilities.iter(),
        ys.into_no_null_iter(),
    ) {
        let modification = match (code, chebi) {
            (Some(code), None) => Modification::Code(code),
            (None, Some(id)) => Modification::ChebiId(id),
            _ => unreachable!("modifications contain exactly one noodles variant"),
        };
        if let Some(position) = screen_position(position, y)
            && let Some(cell) = buf.cell_mut(position)
        {
            cell.set_bg(palette.modification_color(&modification, probability.unwrap_or(255)));
        }
    }
    Ok(())
}

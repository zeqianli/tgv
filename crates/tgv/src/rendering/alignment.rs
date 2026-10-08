//! Draw visible runs and annotations directly into the buffer in layer order.
//! Each stage preserves source-table order, so later records overwrite earlier ones.

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
///
/// A pair is shown when either mate passes the display options, and then both mates are drawn
/// so a pair always stays together.
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
    for ((first, second), y) in first
        .into_no_null_iter()
        .zip(second.iter())
        .zip(ys.into_no_null_iter())
    {
        let Some(second) = second else { continue };
        let (first, second) = (first as usize, second as usize);
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
    // Members are already limited to shown pairs and singletons.
    let reads = alignment
        .tables
        .reads
        .clone()
        .lazy()
        .drop(cols([ReadSchema::Y]))
        .join(
            members,
            [col(ReadSchema::READ_ID)],
            [col(ReadSchema::READ_ID)],
            JoinArgs {
                maintain_order: MaintainOrderJoin::Left,
                ..JoinArgs::new(JoinType::Inner)
            },
        );
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

    let reads = reads
        .select([
            col(ReadSchema::READ_ID),
            col(ReadSchema::Y),
            col(ReadSchema::REVERSE),
            col(ReadSchema::STACKING_START),
            col(ReadSchema::STACKING_END),
            col(ReadSchema::MAPQ),
        ])
        .collect()?;
    if reads.height() == 0 {
        return Ok(());
    }
    // Reads with MAPQ 0 map equally well elsewhere, so they are drawn faded, as in IGV.
    let mut zero_mapq = vec![false; tables.reads.height()];
    for (id, mapq) in izip!(
        reads
            .column(ReadSchema::READ_ID)?
            .u64()?
            .into_no_null_iter(),
        reads.column(ReadSchema::MAPQ)?.u8()?.iter(),
    ) {
        zero_mapq[id as usize] = mapq == Some(0);
    }
    // Read IDs are row indexes into the reads table, so drawn rows are looked up directly
    // instead of joining every annotation table with the drawn reads on each frame.
    let mut read_rows = vec![None; tables.reads.height()];
    for (id, y) in izip!(
        reads
            .column(ReadSchema::READ_ID)?
            .u64()?
            .into_no_null_iter(),
        reads.column(ReadSchema::Y)?.u64()?.into_no_null_iter(),
    ) {
        read_rows[id as usize] = Some(y);
    }

    // 1. Draw the main bodies and soft clips.

    let runs = with_read_rows(
        tables
            .cigar_runs
            .clone()
            .lazy()
            .filter(
                col(CigarSchema::DISPLAY_START)
                    .lt_eq(lit(region.end()))
                    .and(col(CigarSchema::DISPLAY_END).gt_eq(lit(region.start()))),
            )
            .select([
                col(CigarSchema::READ_ID),
                col(CigarSchema::KIND),
                col(CigarSchema::DISPLAY_START),
                col(CigarSchema::DISPLAY_END),
                col(CigarSchema::RUN_OFFSET),
                col(CigarSchema::SEQ),
                col(CigarSchema::QUAL),
            ])
            .collect()?,
        &read_rows,
    )?;

    if runs.height() == 0 {
        return Ok(());
    }

    let run_ys = runs.column(ReadSchema::Y)?.u64()?;
    let run_kinds = runs.column(CigarSchema::KIND)?.u8()?;
    let run_starts = runs.column(CigarSchema::DISPLAY_START)?.u64()?;
    let run_ends = runs.column(CigarSchema::DISPLAY_END)?.u64()?;
    let run_offsets = runs.column(CigarSchema::RUN_OFFSET)?.u32()?;
    let run_sequences = runs.column(CigarSchema::SEQ)?.str()?;
    let run_qualities = runs.column(CigarSchema::QUAL)?.binary()?;
    let run_read_ids = runs.column(CigarSchema::READ_ID)?.u64()?;

    // 1.1 Draw the main bodies.
    for (id, kind, y, start, end) in izip!(
        run_read_ids.into_no_null_iter(),
        run_kinds.into_no_null_iter(),
        run_ys.into_no_null_iter(),
        run_starts.into_no_null_iter(),
        run_ends.into_no_null_iter(),
    ) {
        if kind == CigarSchema::SOFT_CLIP {
            continue;
        }
        let (start, end) = (start.max(region.start()), end.min(region.end()));
        let Some(left) = pixel(start, view, area) else {
            continue;
        };
        let right = pixel(end, view, area).unwrap_or(area.width - 1);

        let y = area.y + (y as usize - top) as u16;
        for x in left..=right {
            if let Some(cell) = buf.cell_mut(Position::new(area.x + x, y)) {
                cell.set_symbol("-")
                    .set_bg(if zero_mapq[id as usize] {
                        palette.ZERO_MAPQ_COLOR
                    } else {
                        palette.MATCH_COLOR
                    })
                    .set_fg(palette.MATCH_FG_COLOR);
            }
        }
    }

    // 1.2 Draw the soft clips.

    for (kind, y, run_start, end, run_offset, seq) in izip!(
        run_kinds.into_no_null_iter(),
        run_ys.into_no_null_iter(),
        run_starts.into_no_null_iter(),
        run_ends.into_no_null_iter(),
        run_offsets.into_no_null_iter(),
        run_sequences.iter(),
    ) {
        if kind != CigarSchema::SOFT_CLIP {
            continue;
        }
        let Some(seq) = seq.map(str::as_bytes) else {
            continue;
        };
        let (start, end) = (run_start.max(region.start()), end.min(region.end()));
        let Some(left) = pixel(start, view, area) else {
            continue;
        };
        let right = pixel(end, view, area).unwrap_or(area.width - 1);
        let y = area.y + (y as usize - top) as u16;
        for x in left..=right {
            if let Some(cell) = buf.cell_mut(Position::new(area.x + x, y)) {
                let position = (region.start() + u64::from(x) * view.zoom + view.zoom - 1).min(end);
                let offset = run_offset as usize + (position - run_start) as usize;
                let base = seq[offset];
                cell.set_char(base as char);
                cell.set_bg(palette.softclip_color(base))
                    .set_fg(Color::Reset);
            }
        }
    }

    // 2. Draw the deletions and reference skips.

    for (kind, y, start, end) in izip!(
        run_kinds.into_no_null_iter(),
        run_ys.into_no_null_iter(),
        run_starts.into_no_null_iter(),
        run_ends.into_no_null_iter(),
    ) {
        if !matches!(kind, CigarSchema::DELETION | CigarSchema::REFERENCE_SKIP) {
            continue;
        }
        let (start, end) = (start.max(region.start()), end.min(region.end()));
        let Some(left) = pixel(start, view, area) else {
            continue;
        };
        let right = pixel(end, view, area).unwrap_or(area.width - 1);

        let y = area.y + (y as usize - top) as u16;
        for x in left..=right {
            if let Some(cell) = buf.cell_mut(Position::new(area.x + x, y)) {
                cell.set_symbol("-")
                    .set_bg(palette.background)
                    .set_fg(palette.DELETION_COLOR);
            }
        }
    }

    // Draw arrows after deletion bodies so terminal deletions retain their strand marker.
    for (y, reverse, start, end) in izip!(
        reads.column(ReadSchema::Y)?.u64()?.into_no_null_iter(),
        reads
            .column(ReadSchema::REVERSE)?
            .bool()?
            .iter()
            .map(|reverse| reverse.expect("read flags are non-null")),
        reads.column(ReadSchema::STACKING_START)?.u64()?.iter(),
        reads.column(ReadSchema::STACKING_END)?.u64()?.iter(),
    ) {
        let (Some(start), Some(end)) = (start, end) else {
            continue;
        };
        let position = if reverse { start } else { end };
        let Some(x) = pixel(position, view, area) else {
            continue;
        };
        let y = area.y + (y as usize - top) as u16;
        if let Some(cell) = buf.cell_mut(Position::new(area.x + x, y)) {
            cell.set_symbol(if reverse { "◄" } else { "►" });
        }
    }

    // 3. Draw the insertions.

    for (kind, y, start) in izip!(
        run_kinds.into_no_null_iter(),
        run_ys.into_no_null_iter(),
        run_starts.into_no_null_iter(),
    ) {
        if kind != CigarSchema::INSERTION {
            continue;
        }
        let Some(x) = pixel(start, view, area) else {
            continue;
        };

        let y = area.y + (y as usize - top) as u16;

        if let Some(cell) = buf.cell_mut(Position::new(area.x + x, y)) {
            cell.set_symbol("▌").set_fg(palette.INSERTION_COLOR);
        }
    }

    // 4. Get sequence mismatch positions and draw the bases.
    for (kind, y, run_start, end, run_offset, seq, qual) in izip!(
        run_kinds.into_no_null_iter(),
        run_ys.into_no_null_iter(),
        run_starts.into_no_null_iter(),
        run_ends.into_no_null_iter(),
        run_offsets.into_no_null_iter(),
        run_sequences.iter(),
        run_qualities.iter(),
    ) {
        if kind != CigarSchema::SEQUENCE_MISMATCH {
            continue;
        }
        let Some(seq) = seq.map(str::as_bytes) else {
            continue;
        };
        let (start, end) = (run_start.max(region.start()), end.min(region.end()));
        let Some(left) = pixel(start, view, area) else {
            continue;
        };
        let right = pixel(end, view, area).unwrap_or(area.width - 1);
        let y = area.y + (y as usize - top) as u16;
        for x in left..=right {
            if let Some(cell) = buf.cell_mut(Position::new(area.x + x, y)) {
                let position = (region.start() + u64::from(x) * view.zoom + view.zoom - 1).min(end);
                let offset = run_offset as usize + (position - run_start) as usize;
                let base = seq[offset];
                cell.set_char(base as char).set_style(
                    palette.mismatch_style(base, qual.and_then(|qual| qual.get(offset).copied())),
                );
            }
        }
    }

    let mismatches = with_read_rows(
        tables
            .reference_mismatches
            .clone()
            .lazy()
            .filter(
                col(ReferenceMismatchSchema::REF_POS)
                    .lt_eq(lit(region.end()))
                    .and(col(ReferenceMismatchSchema::REF_POS).gt_eq(lit(region.start()))),
            )
            .select([
                col(ReferenceMismatchSchema::READ_ID),
                col(ReferenceMismatchSchema::REF_POS),
                col(ReferenceMismatchSchema::BASE),
                col(ReferenceMismatchSchema::QUAL),
            ])
            .collect()?,
        &read_rows,
    )?;

    for (position, base, y, qual) in izip!(
        mismatches
            .column(ReferenceMismatchSchema::REF_POS)?
            .u64()?
            .into_no_null_iter(),
        mismatches
            .column(ReferenceMismatchSchema::BASE)?
            .u8()?
            .into_no_null_iter(),
        mismatches.column(ReadSchema::Y)?.u64()?.into_no_null_iter(),
        mismatches
            .column(ReferenceMismatchSchema::QUAL)?
            .u8()?
            .iter(),
    ) {
        if let Some(position) = pixel(position, view, area)
            .map(|x| Position::new(area.x + x, area.y + (y as usize - top) as u16))
            && let Some(cell) = buf.cell_mut(position)
        {
            cell.set_char(base as char)
                .set_style(palette.mismatch_style(base, qual));
        }
    }

    // 5. Draw the base modifications.
    let modifications = with_read_rows(
        tables
            .base_modifications
            .clone()
            .lazy()
            .filter(
                col(BaseModificationSchema::DISPLAY_POS)
                    .lt_eq(lit(region.end()))
                    .and(col(BaseModificationSchema::DISPLAY_POS).gt_eq(lit(region.start()))),
            )
            .select([
                col(BaseModificationSchema::READ_ID),
                col(BaseModificationSchema::DISPLAY_POS),
                col(BaseModificationSchema::CODE),
                col(BaseModificationSchema::CHEBI_ID),
                col(BaseModificationSchema::PROBABILITY),
            ])
            .collect()?,
        &read_rows,
    )?;

    for (position, code, chebi, probability, y) in izip!(
        modifications
            .column(BaseModificationSchema::DISPLAY_POS)?
            .u64()?
            .into_no_null_iter(),
        modifications
            .column(BaseModificationSchema::CODE)?
            .u8()?
            .iter(),
        modifications
            .column(BaseModificationSchema::CHEBI_ID)?
            .u32()?
            .iter(),
        modifications
            .column(BaseModificationSchema::PROBABILITY)?
            .u8()?
            .iter(),
        modifications
            .column(ReadSchema::Y)?
            .u64()?
            .into_no_null_iter(),
    ) {
        let modification = match (code, chebi) {
            (Some(code), None) => Modification::Code(code),
            (None, Some(id)) => Modification::ChebiId(id),
            _ => unreachable!("modifications contain exactly one noodles variant"),
        };
        if let Some(position) = pixel(position, view, area)
            .map(|x| Position::new(area.x + x, area.y + (y as usize - top) as u16))
            && let Some(cell) = buf.cell_mut(position)
        {
            cell.set_bg(palette.modification_color(&modification, probability.unwrap_or(255)));
        }
    }

    Ok(())
}

/// Keep annotation rows of drawn reads and attach each read's stacking row as `Y`.
/// `read_rows` is indexed by read ID and holds the row of every drawn read.
fn with_read_rows(frame: DataFrame, read_rows: &[Option<u64>]) -> PolarsResult<DataFrame> {
    let mask: BooleanChunked = frame
        .column(ReadSchema::READ_ID)?
        .u64()?
        .into_no_null_iter()
        .map(|id| read_rows[id as usize].is_some())
        .collect();
    let mut frame = frame.filter(&mask)?;
    let ys: Vec<u64> = frame
        .column(ReadSchema::READ_ID)?
        .u64()?
        .into_no_null_iter()
        .map(|id| read_rows[id as usize].expect("rows are filtered to drawn reads"))
        .collect();
    frame.with_column(Column::new(ReadSchema::Y.into(), ys))?;
    Ok(frame)
}

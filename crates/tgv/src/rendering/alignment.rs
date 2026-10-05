//! Draw visible runs and annotations directly into the buffer in layer order.

use std::io::Read;

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
use noodles::sam::record::{Cigar, data::field::value::base_modifications::group::Modification};
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

    // steps:
    // 1. Get match / mismatch / sequence_match / softclip runs.
    //     1.1 draw the main body
    //     1.2 draw the softclips
    //     1.3 Start / end arrow head: group by read id. display start or display end.

    let runs = tables
        .cigar_runs
        .clone()
        .lazy()
        .filter(
            col(CigarSchema::DISPLAY_START)
                .lt_eq(lit(region.end()))
                .and(col(CigarSchema::DISPLAY_END).gt_eq(lit(region.start()))),
        )
        .join(
            reads.clone(),
            [col(ReadSchema::READ_ID)],
            [col(ReadSchema::READ_ID)],
            JoinArgs::new(JoinType::Inner),
        )
        .select([
            col(ReadSchema::READ_ID),
            col(ReadSchema::Y),
            col(ReadSchema::REVERSE),
            col(CigarSchema::KIND),
            col(CigarSchema::DISPLAY_START),
            col(CigarSchema::DISPLAY_END),
            col(CigarSchema::SEQ),
        ])
        .collect()?;

    // 1.1 Draw the main body, match bases
    let match_runs = runs
        .clone()
        .lazy()
        .filter(col(CigarSchema::KIND).neq(lit(CigarSchema::SOFT_CLIP)))
        .collect()?;

    for (y, start, end) in izip!(
        match_runs.column(ReadSchema::Y)?.u64()?.into_no_null_iter(),
        match_runs
            .column(CigarSchema::DISPLAY_START)?
            .u64()?
            .into_no_null_iter(),
        match_runs
            .column(CigarSchema::DISPLAY_END)?
            .u64()?
            .into_no_null_iter(),
    ) {
        let (start, end) = (start.max(region.start()), end.min(region.end()));
        let Some(left) = pixel(start, view, area) else {
            continue; // tODO: I don't think this is necessary?
        };
        let right = pixel(end, view, area).unwrap_or(area.width - 1);

        let y = area.y + (y as usize - top) as u16;
        for x in left..=right {
            if let Some(cell) = buf.cell_mut(Position::new(area.x + x, y)) {
                cell.set_symbol("-")
                    .set_bg(palette.MATCH_COLOR)
                    .set_fg(palette.MATCH_FG_COLOR);
            } else {
                continue;
            }
        }
    }

    // 1.2 Draw softclips
    let softclip_runs = runs
        .clone()
        .lazy()
        .filter(col(CigarSchema::KIND).eq(lit(CigarSchema::SOFT_CLIP)))
        .collect()?;

    for (y, kind, start, end, seq) in izip!(
        softclip_runs
            .column(ReadSchema::Y)?
            .u64()?
            .into_no_null_iter(),
        softclip_runs
            .column(CigarSchema::KIND)?
            .u8()?
            .into_no_null_iter(),
        softclip_runs
            .column(CigarSchema::DISPLAY_START)?
            .u64()?
            .into_no_null_iter(),
        softclip_runs
            .column(CigarSchema::DISPLAY_END)?
            .u64()?
            .into_no_null_iter(),
        softclip_runs.column(CigarSchema::SEQ)?.str()?.iter()
    ) {
        let (start, end) = (start.max(region.start()), end.min(region.end()));
        let Some(left) = pixel(start, view, area) else {
            continue; // tODO: I don't think this is necessary?
        };
        let right = pixel(end, view, area).unwrap_or(area.width - 1);
        // TODO: this is a mess

        let y = area.y + (y as usize - top) as u16;
        for x in left..=right {
            if let Some(cell) = buf.cell_mut(Position::new(area.x + x, y))
                && let Some(seq) = seq.map(|s| s.as_bytes())
            {
                let position = (region.start() + u64::from(x) * view.zoom + view.zoom - 1).min(end);
                let offset = (position - start) as usize;
                let base = seq[offset];
                cell.set_char(base as char);
                cell.set_bg(palette.softclip_color(base))
                    .set_fg(Color::Reset);
            } else {
                continue;
            }
        }
    }

    // 1.3: Draw arrows
    // TODO: the coordinate is not right yet
    let arrow_positions = runs
        .clone()
        .lazy()
        .group_by([col(ReadSchema::READ_ID)])
        .agg([
            when(col(ReadSchema::REVERSE))
                .then(col(CigarSchema::DISPLAY_START).min())
                .otherwise(col(CigarSchema::DISPLAY_END).max())
                .first()
                .alias("arrow_position"),
            col(ReadSchema::Y).first(),
            col(ReadSchema::REVERSE).first(),
        ])
        .collect()?;

    for (y, reverse, arrow_position) in izip!(
        arrow_positions
            .column(ReadSchema::Y)?
            .u64()?
            .into_no_null_iter(),
        arrow_positions
            .column(ReadSchema::REVERSE)?
            .bool()?
            .iter()
            .map(|reverse| reverse.expect("read flags are non-null")),
        arrow_positions
            .column("arrow_position")?
            .u64()?
            .into_no_null_iter(),
    ) {
        let Some(left) = pixel(arrow_position, view, area) else {
            continue; // TODO: I don't think this is necessary?
        };

        let y = area.y + (y as usize - top) as u16;

        if let Some(cell) = buf.cell_mut(Position::new(area.x + left, y)) {
            cell.set_symbol(if reverse { "◄" } else { "►" });
        }
    }

    // 2. Get deletion runs / ref skip. Paint.

    let deletion_runs = runs
        .clone()
        .lazy()
        .filter(
            col(CigarSchema::KIND)
                .eq(lit(CigarSchema::DELETION))
                .or(col(CigarSchema::KIND).eq(lit(CigarSchema::REFERENCE_SKIP))),
        )
        .select([
            //col(ReadSchema::READ_ID),
            col(ReadSchema::Y),
            col(CigarSchema::DISPLAY_START),
            col(CigarSchema::DISPLAY_END),
        ])
        .collect()?;

    for (y, start, end) in izip!(
        deletion_runs
            .column(ReadSchema::Y)?
            .u64()?
            .into_no_null_iter(),
        deletion_runs
            .column(CigarSchema::DISPLAY_START)?
            .u64()?
            .into_no_null_iter(),
        deletion_runs
            .column(CigarSchema::DISPLAY_END)?
            .u64()?
            .into_no_null_iter(),
    ) {
        let (start, end) = (start.max(region.start()), end.min(region.end()));
        let Some(left) = pixel(start, view, area) else {
            continue; // tODO: I don't think this is necessary?
        };
        let right = pixel(end, view, area).unwrap_or(area.width - 1);

        let y = area.y + (y as usize - top) as u16;
        for x in left..=right {
            if let Some(cell) = buf.cell_mut(Position::new(area.x + x, y)) {
                cell.set_symbol("-")
                    .set_bg(palette.background)
                    .set_fg(palette.DELETION_COLOR);
            } else {
                continue;
            }
        }
    }

    // 3. Get Insertion runs. Paint.
    let insertion_runs = runs
        .clone()
        .lazy()
        .filter(col(CigarSchema::KIND).eq(lit(CigarSchema::INSERTION)))
        .select([
            //col(ReadSchema::READ_ID),
            col(ReadSchema::Y),
            col(CigarSchema::DISPLAY_START),
            //col(CigarSchema::DISPLAY_END),
        ])
        .collect()?;

    for (y, start) in izip!(
        insertion_runs
            .column(ReadSchema::Y)?
            .u64()?
            .into_no_null_iter(),
        insertion_runs
            .column(CigarSchema::DISPLAY_START)?
            .u64()?
            .into_no_null_iter(),
    ) {
        let Some(x) = pixel(start, view, area) else {
            continue; // TODO: I don't think this is necessary?
        };

        let y = area.y + (y as usize - top) as u16;

        if let Some(cell) = buf.cell_mut(Position::new(area.x + x, y)) {
            cell.set_symbol("▌").set_fg(palette.INSERTION_COLOR);
        }
    }

    // 4. Ged sequence mismatch positons. paint.
    let mismatches = tables
        .reference_mismatches
        .clone()
        .lazy()
        .filter(
            col(ReferenceMismatchSchema::REF_POS)
                .lt_eq(lit(region.end()))
                .and(col(ReferenceMismatchSchema::REF_POS).gt_eq(lit(region.start()))),
        )
        .join(
            tables.reads.clone().lazy(),
            [col(ReadSchema::READ_ID)],
            [col(ReadSchema::READ_ID)],
            JoinArgs::new(JoinType::Inner),
        )
        .select([
            //col(ReadSchema::READ_ID),
            col(ReadSchema::Y),
            col(ReferenceMismatchSchema::REF_POS),
            col(ReferenceMismatchSchema::BASE),
        ])
        .collect()?;

    // Mismatches

    for (position, base, y) in izip!(
        mismatches
            .column(ReferenceMismatchSchema::REF_POS)?
            .u64()?
            .into_no_null_iter(),
        mismatches
            .column(ReferenceMismatchSchema::BASE)?
            .u8()?
            .into_no_null_iter(),
        mismatches.column(ReadSchema::Y)?.u64()?.into_no_null_iter()
    ) {
        if let Some(position) = pixel(position, view, area)
            .map(|x| Position::new(area.x + x, area.y + (y as usize - top) as u16))
            && let Some(cell) = buf.cell_mut(position)
        {
            cell.set_char(base as char)
                .set_fg(palette.mismatch_color(base));
        }
    }

    // 5. Get BaseModification positions. paint.
    //
    let modifications = tables
        .base_modifications
        .clone()
        .lazy()
        .filter(
            col(BaseModificationSchema::DISPLAY_POS)
                .lt_eq(lit(region.end()))
                .and(col(BaseModificationSchema::DISPLAY_POS).gt_eq(lit(region.start()))),
        )
        .join(
            tables.reads.clone().lazy(),
            [col(ReadSchema::READ_ID)],
            [col(ReadSchema::READ_ID)],
            JoinArgs::new(JoinType::Inner),
        )
        .select([
            //col(ReadSchema::READ_ID),
            col(ReadSchema::Y),
            col(BaseModificationSchema::DISPLAY_POS),
            col(BaseModificationSchema::CODE),
            col(BaseModificationSchema::CHEBI_ID),
            col(BaseModificationSchema::PROBABILITY),
        ])
        .collect()?;

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

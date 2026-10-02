//! Draw viewport-filtered CIGAR runs and sparse annotations.

use crate::{
    layout::{AlignmentView, OnScreenCoordinate},
    rendering::colors::Palette,
};
use gv_core::{
    alignment::{Alignment, AlignmentViewport, PairedAlignment},
    prelude::*,
};
use noodles::sam::{
    alignment::record::cigar::op::Kind,
    record::data::field::value::base_modifications::group::Modification,
};
use polars::prelude::*;
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::Style,
};
use std::collections::HashMap;

/// A scratch terminal cell for the current frame, never retained by an alignment.
#[derive(Clone, Copy)]
struct Paint {
    kind: Kind,
    op_index: u32,
    start: u64,
    end: u64,
    softclip: Option<u8>,
    mismatch: Option<(u64, u8)>,
    insertion: Option<(u64, u32)>,
    reverse_arrow: Option<bool>,
    modification: Option<(u64, Modification, u8, u64)>,
    conflict: bool,
}

impl Paint {
    fn new(kind: Kind, op_index: u32, start: u64, end: u64) -> Self {
        Self {
            kind,
            op_index,
            start,
            end,
            softclip: None,
            mismatch: None,
            insertion: None,
            reverse_arrow: None,
            modification: None,
            conflict: false,
        }
    }
}

fn pixel(position: u64, view: &AlignmentView, area: &Rect) -> Option<usize> {
    match view.onscreen_x_coordinate(position, area) {
        OnScreenCoordinate::OnScreen(x) if x < usize::from(area.width) => Some(x),
        _ => None,
    }
}

/// Render an alignment from a single batch of visible CIGAR rows.
pub fn render_alignment(
    index: usize,
    area: &Rect,
    buf: &mut Buffer,
    alignment: &Alignment,
    view: &AlignmentView,
    palette: &Palette,
) -> Result<(), TGVError> {
    if area.height == 0 || area.width == 0 {
        return Ok(());
    }
    let region = view.region(area);
    let visible = alignment
        .overlapping_reads(region.contig_index(), region.start(), region.end())?
        .into_iter()
        .filter(|id| alignment.show_read[*id])
        .filter_map(
            |id| match view.onscreen_y_coordinate(index, alignment.ys[id], area) {
                OnScreenCoordinate::OnScreen(y) if y < usize::from(area.height) => Some((id, y)),
                _ => None,
            },
        )
        .collect::<Vec<_>>();
    let ids = visible.iter().map(|(id, _)| *id).collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(());
    }
    let viewport = alignment.query_viewport(&region, &ids)?;
    let cells = paint_runs(&viewport, view, area)?;
    for (id, y) in visible {
        for (x, paint) in cells[&id].iter().enumerate() {
            if let Some(paint) = paint {
                draw_cell(*paint, x, y, buf, area, palette)
            }
        }
    }
    Ok(())
}

/// Render paired reads by combining only their current viewport cells.
pub fn render_paired_alignment(
    index: usize,
    area: &Rect,
    buf: &mut Buffer,
    alignment: &Alignment,
    view: &AlignmentView,
    paired: &PairedAlignment,
    palette: &Palette,
) -> Result<(), TGVError> {
    if area.height == 0 || area.width == 0 {
        return Ok(());
    }
    let region = view.region(area);
    let visible = paired
        .overlapping_pairs(
            alignment,
            region.contig_index(),
            region.start(),
            region.end(),
        )?
        .into_iter()
        .filter(|id| paired.show_pair[*id])
        .filter_map(
            |id| match view.onscreen_y_coordinate(index, paired.ys[id], area) {
                OnScreenCoordinate::OnScreen(y) if y < usize::from(area.height) => Some((id, y)),
                _ => None,
            },
        )
        .collect::<Vec<_>>();
    let ids = visible
        .iter()
        .flat_map(|(id, _)| {
            let (first, second) = paired.members(*id);
            std::iter::once(first).chain(second)
        })
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(());
    }
    let viewport = alignment.query_viewport(&region, &ids)?;
    let cells = paint_runs(&viewport, view, area)?;
    for (id, y) in visible {
        let (first, second) = paired.members(id);
        let first_cells = &cells[&first];
        let second_cells = second.map(|id| &cells[&id]);
        let gap = alignment
            .stacking_bounds(first)
            .zip(second.and_then(|id| alignment.stacking_bounds(id)))
            .and_then(|(a, b)| {
                if a.1 < b.0 {
                    Some((a.1 + 1, b.0 - 1))
                } else if b.1 < a.0 {
                    Some((b.1 + 1, a.0 - 1))
                } else {
                    None
                }
            });
        if let Some((start, end)) = gap {
            let start = start.max(region.start());
            let end = end.min(region.end());
            if start <= end {
                let left = pixel(start, view, area).unwrap_or(0);
                let right = pixel(end, view, area).unwrap_or(usize::from(area.width) - 1);
                for x in left..=right {
                    if let Some(cell) =
                        buf.cell_mut(Position::new(area.x + x as u16, area.y + y as u16))
                    {
                        cell.set_symbol("-").set_style(
                            Style::default()
                                .bg(palette.background)
                                .fg(palette.PAIRGAP_COLOR),
                        );
                    }
                }
            }
        }
        for (x, first) in first_cells.iter().enumerate() {
            let second = second_cells.and_then(|cells| cells[x]);
            let paint = match (*first, second) {
                (Some(first), Some(second)) => Some(merge_pair_cell(first, second)),
                (Some(paint), None) | (None, Some(paint)) => Some(paint),
                (None, None) => None,
            };
            if let Some(paint) = paint {
                draw_cell(paint, x, y, buf, area, palette)
            }
        }
    }
    Ok(())
}

fn paint_runs(
    viewport: &AlignmentViewport,
    view: &AlignmentView,
    area: &Rect,
) -> PolarsResult<HashMap<usize, Vec<Option<Paint>>>> {
    let mut cells = viewport
        .reads
        .column("read_id")?
        .u64()?
        .into_no_null_iter()
        .map(|id| (id as usize, vec![None; usize::from(area.width)]))
        .collect::<HashMap<_, _>>();
    let columns = viewport
        .runs
        .iter()
        .filter(|(kind, _)| !matches!(kind, Kind::Insertion | Kind::HardClip | Kind::Pad))
        .map(|(kind, frame)| {
            Ok((
                *kind,
                frame.column("read_id")?.u64()?,
                frame.column("op_index")?.u32()?,
                frame.column("display_start")?.u64()?,
                frame.column("display_end")?.u64()?,
                frame.column("run_offset")?.u32()?,
                if kind.consumes_read() {
                    Some(frame.column("seq")?.str()?)
                } else {
                    None
                },
            ))
        })
        .collect::<PolarsResult<Vec<_>>>()?;
    let mut order = Vec::new();
    for (table, (_, ids, indexes, _, _, _, _)) in columns.iter().enumerate() {
        order.extend((0..ids.len()).map(|row| {
            (
                ids.get(row).expect("run IDs are non-null") as usize,
                indexes.get(row).expect("run indexes are non-null"),
                table,
                row,
            )
        }));
    }
    // CIGAR order determines which operation wins when zoom projects several runs onto one cell.
    order.sort_unstable_by_key(|(id, index, _, _)| (*id, *index));
    for (id, index, table, row) in order {
        let (kind, _, _, starts, ends, offsets, sequences) = columns[table];
        let start = starts.get(row).expect("queried runs have display bounds");
        let end = ends.get(row).expect("queried runs have display bounds");
        let offset = offsets.get(row).expect("queried runs have offsets") as usize;
        let sequence = sequences.and_then(|sequences| sequences.get(row).map(str::as_bytes));
        if sequences.is_some() && sequence.is_none() {
            continue;
        }
        let target = cells
            .get_mut(&id)
            .expect("queried runs belong to selected reads");
        if matches!(kind, Kind::SoftClip | Kind::SequenceMismatch) {
            for position in start..=end {
                let Some(x) = pixel(position, view, area) else {
                    continue;
                };
                let base = sequence.expect("base-bearing CIGAR kinds have SEQ strings")
                    [offset + (position - start) as usize];
                let mut paint = Paint::new(kind, index, position, position);
                if kind == Kind::SoftClip {
                    paint.softclip = Some(base)
                } else {
                    paint.mismatch = Some((position, base))
                }
                target[x] = Some(paint);
            }
        } else {
            let left = pixel(start, view, area).unwrap_or(0);
            let right = pixel(end, view, area).unwrap_or(usize::from(area.width) - 1);
            for (x, cell) in target.iter_mut().enumerate().take(right + 1).skip(left) {
                let pixel_start = view.left(area) + x as u64 * view.zoom;
                *cell = Some(Paint::new(
                    kind,
                    index,
                    start.max(pixel_start),
                    end.min(pixel_start + view.zoom - 1),
                ));
            }
        }
    }
    let reads = &viewport.reads;
    let ids = reads.column("read_id")?.u64()?;
    let starts = reads.column("stacking_start")?.u64()?;
    let ends = reads.column("stacking_end")?.u64()?;
    let reverse = reads.column("reverse")?.bool()?;
    for row in 0..reads.height() {
        let is_reverse = reverse.get(row).expect("read flags are non-null");
        let position = if is_reverse {
            starts.get(row)
        } else {
            ends.get(row)
        };
        if let Some(x) = position.and_then(|position| pixel(position, view, area)) {
            let id = ids.get(row).expect("read IDs are non-null") as usize;
            if let Some(paint) = &mut cells
                .get_mut(&id)
                .expect("selected reads have scratch cells")[x]
            {
                paint.reverse_arrow = Some(is_reverse);
            }
        }
    }
    for (kind, frame) in &viewport.runs {
        if *kind != Kind::Insertion {
            continue;
        }
        let ids = frame.column("read_id")?.u64()?;
        let indexes = frame.column("op_index")?.u32()?;
        let starts = frame.column("display_start")?.u64()?;
        let lengths = frame.column("op_len")?.u32()?;
        for row in 0..frame.height() {
            let Some(x) = pixel(
                starts.get(row).expect("insertions have anchors"),
                view,
                area,
            ) else {
                continue;
            };
            let id = ids.get(row).expect("run IDs are non-null") as usize;
            let target = &mut cells
                .get_mut(&id)
                .expect("queried runs belong to selected reads")[x];
            target
                .get_or_insert_with(|| {
                    Paint::new(
                        Kind::Insertion,
                        indexes.get(row).expect("run indexes are non-null"),
                        starts.get(row).expect("insertions have anchors"),
                        starts.get(row).expect("insertions have anchors"),
                    )
                })
                .insertion = Some((
                starts.get(row).expect("insertions have anchors"),
                lengths.get(row).expect("run lengths are non-null"),
            ));
        }
    }
    let frame = &viewport.reference_mismatches;
    let ids = frame.column("read_id")?.u64()?;
    let indexes = frame.column("op_index")?.u32()?;
    let positions = frame.column("ref_pos")?.u64()?;
    let bases = frame.column("base")?.u8()?;
    for row in 0..frame.height() {
        let Some(x) = pixel(
            positions.get(row).expect("mismatch positions are non-null"),
            view,
            area,
        ) else {
            continue;
        };
        let id = ids.get(row).expect("annotation IDs are non-null") as usize;
        if let Some(paint) = &mut cells
            .get_mut(&id)
            .expect("annotations belong to selected reads")[x]
            && Some(paint.op_index) == indexes.get(row)
        {
            paint.mismatch = Some((
                positions.get(row).expect("mismatch positions are non-null"),
                bases.get(row).expect("mismatch bases are non-null"),
            ));
        }
    }
    let frame = &viewport.base_modifications;
    let ids = frame.column("read_id")?.u64()?;
    let indexes = frame.column("op_index")?.u32()?;
    let positions = frame.column("display_pos")?.u64()?;
    let codes = frame.column("code")?.u8()?;
    let chebi = frame.column("chebi_id")?.u32()?;
    let probabilities = frame.column("probability")?.u8()?;
    let orders = frame.column("source_order")?.u64()?;
    for row in 0..frame.height() {
        let position = positions
            .get(row)
            .expect("modification positions are non-null");
        let Some(x) = pixel(position, view, area) else {
            continue;
        };
        let id = ids.get(row).expect("annotation IDs are non-null") as usize;
        if let Some(paint) = &mut cells
            .get_mut(&id)
            .expect("annotations belong to selected reads")[x]
            && Some(paint.op_index) == indexes.get(row)
        {
            let modification = match (codes.get(row), chebi.get(row)) {
                (Some(code), None) => Modification::Code(code),
                (None, Some(id)) => Modification::ChebiId(id),
                _ => unreachable!("modifications contain exactly one noodles variant"),
            };
            // Preserve the existing display default without storing a fabricated probability.
            let candidate = (
                position,
                modification,
                probabilities.get(row).unwrap_or(255),
                orders.get(row).expect("source orders are non-null"),
            );
            paint.modification = best_modification(paint.modification, Some(candidate));
        }
    }
    Ok(cells)
}

fn best_modification(
    first: Option<(u64, Modification, u8, u64)>,
    second: Option<(u64, Modification, u8, u64)>,
) -> Option<(u64, Modification, u8, u64)> {
    match (first, second) {
        (Some(a), Some(b)) if b.0 > a.0 || (b.0 == a.0 && b.2 > a.2) => Some(b),
        (Some(a), _) => Some(a),
        (None, b) => b,
    }
}

fn merge_pair_cell(first: Paint, second: Paint) -> Paint {
    if first.end < second.start {
        return second;
    }
    if second.end < first.start {
        return first;
    }
    let class = |kind| match kind {
        Kind::Match | Kind::SequenceMatch | Kind::SequenceMismatch => Kind::Match,
        Kind::Deletion | Kind::Skip => Kind::Deletion,
        kind => kind,
    };
    let mut result = first;
    result.conflict = class(first.kind) != class(second.kind)
        || first.softclip != second.softclip
        || first
            .mismatch
            .zip(second.mismatch)
            .is_some_and(|(a, b)| a.0 == b.0 && a.1 != b.1)
        || first
            .insertion
            .zip(second.insertion)
            .is_some_and(|(a, b)| a.0 == b.0 && a.1 != b.1)
        || first
            .insertion
            .zip(second.mismatch)
            .is_some_and(|(a, b)| a.0 == b.0)
        || second
            .insertion
            .zip(first.mismatch)
            .is_some_and(|(a, b)| a.0 == b.0);
    if result.conflict {
        result.mismatch = None;
        result.insertion = None;
        result.reverse_arrow = None;
        result.modification = None;
    } else {
        result.mismatch = first.mismatch.or(second.mismatch);
        result.insertion = first.insertion.or(second.insertion);
        result.reverse_arrow = second.reverse_arrow.or(first.reverse_arrow);
        result.modification = best_modification(first.modification, second.modification);
    }
    result
}

fn draw_cell(paint: Paint, x: usize, y: usize, buf: &mut Buffer, area: &Rect, palette: &Palette) {
    let Some(cell) = buf.cell_mut(Position::new(area.x + x as u16, area.y + y as u16)) else {
        return;
    };
    let style = match paint.kind {
        Kind::Match | Kind::SequenceMatch | Kind::SequenceMismatch => Style::default()
            .bg(palette.MATCH_COLOR)
            .fg(palette.MATCH_FG_COLOR),
        Kind::Deletion | Kind::Skip => Style::default()
            .bg(palette.background)
            .fg(palette.DELETION_COLOR),
        Kind::SoftClip => Style::default()
            .bg(palette.softclip_color(paint.softclip.expect("soft-clip cells have a base"))),
        _ => Style::default().bg(palette.background),
    };
    cell.set_symbol("-").set_style(style);
    if let Some(base) = paint.softclip {
        cell.set_char(base as char);
    }
    if let Some(reverse) = paint.reverse_arrow {
        cell.set_symbol(if reverse { "◄" } else { "►" });
    }
    if paint.insertion.is_some() {
        cell.set_symbol("▌")
            .set_style(Style::default().fg(palette.INSERTION_COLOR));
    }
    if let Some((_, base)) = paint.mismatch {
        cell.set_char(base as char)
            .set_style(Style::default().fg(palette.mismatch_color(base)));
    }
    if paint.conflict {
        cell.set_symbol("?").set_style(
            Style::default()
                .bg(palette.background)
                .fg(palette.PAIR_OVERLAP_COLOR),
        );
    }
    if let Some((_, modification, probability, _)) = paint.modification {
        cell.set_style(Style::default().bg(palette.modification_color(&modification, probability)));
    }
}

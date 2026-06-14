use crate::{
    layout::{AlignmentView, OnScreenCoordinate},
    rendering::colors::Palette,
};
use gv_core::{
    alignment::{
        Alignment, PairedAlignment, RenderingContext, RenderingContextKind,
        RenderingContextModifier,
    },
    error::TGVError,
    sequence::Sequence,
};
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::Style,
};

/// Render an alignment on the alignment area.
pub fn render_alignment(
    area: &Rect,
    buf: &mut Buffer,
    alignment: &mut Alignment,
    alignment_view: &AlignmentView,
    reference_sequence: &Sequence,
    pallete: &Palette,
) -> Result<(), TGVError> {
    if area.height < 1 {
        return Ok(());
    }

    let visible_reads = alignment
        .ys_index
        .iter()
        .enumerate()
        .flat_map(|(y, read_indexes)| read_indexes.iter().map(move |read_index| (y, *read_index)))
        .filter(|(_y, read_index)| alignment.show_read[*read_index])
        .collect::<Vec<_>>();

    for (y, read_index) in visible_reads {
        let context_index =
            if let Some(context_index) = alignment.get_rendering_context_index(read_index) {
                context_index
            } else {
                alignment.calculate_read_rendering_context(read_index, reference_sequence)?
            };
        for context in alignment.rendering_contexts[context_index as usize].iter() {
            render_contexts(context, y, buf, alignment_view, area, pallete)?;
        }
    }

    Ok(())
}

pub fn render_paired_alignment(
    area: &Rect,
    buf: &mut Buffer,
    alignment: &mut Alignment,
    alignment_view: &AlignmentView,
    paired_alignment: &mut PairedAlignment,
    reference_sequence: &Sequence,
    pallete: &Palette,
) -> Result<(), TGVError> {
    if area.height < 1 {
        return Ok(());
    }

    let visible_pairs = paired_alignment
        .ys_index
        .iter()
        .enumerate()
        .flat_map(|(y, read_indexes)| read_indexes.iter().map(move |read_index| (y, *read_index)))
        .filter(|(_y, read_index)| paired_alignment.show_pair[*read_index])
        .collect::<Vec<_>>();

    for (y, pair_index) in visible_pairs {
        let context_index = if let Some(context_index) =
            paired_alignment.get_pair_rendering_context_index(pair_index)
        {
            context_index
        } else {
            paired_alignment.calculate_pair_rendering_context(
                alignment,
                pair_index,
                reference_sequence,
            )?
        };
        for context in paired_alignment.rendering_contexts[context_index as usize].iter() {
            render_contexts(context, y, buf, alignment_view, area, pallete)?;
        }
    }

    Ok(())
}

fn render_contexts(
    context: &RenderingContext,
    y: usize,
    buf: &mut Buffer,
    alignment_view: &AlignmentView,
    area: &Rect,
    pallete: &Palette,
) -> Result<(), TGVError> {
    let onscreen_y = match alignment_view.onscreen_y_coordinate(y, area) {
        OnScreenCoordinate::OnScreen(y_start) => y_start as u16,
        _ => return Ok(()),
    };

    let start_onscreen_coordinate = alignment_view.onscreen_x_coordinate(context.start, area);
    let end_onscreen_coordinate = alignment_view.onscreen_x_coordinate(context.end, area);

    let (onscreen_x, length) = match OnScreenCoordinate::onscreen_start_and_length(
        &start_onscreen_coordinate,
        &end_onscreen_coordinate,
        area,
    ) {
        Some((onscreen_x, length)) => (onscreen_x, length),
        None => return Ok(()),
    };

    // ── Base context rendering ─────────────────────────────────────────────
    match context.kind {
        RenderingContextKind::Match => {
            buf.set_string(
                area.x + onscreen_x,
                area.y + onscreen_y,
                "-".repeat(length as usize),
                Style::default()
                    .bg(pallete.MATCH_COLOR)
                    .fg(pallete.MATCH_FG_COLOR),
            );
        }

        RenderingContextKind::Deletion => buf.set_string(
            area.x + onscreen_x,
            area.y + onscreen_y,
            "-".repeat(length as usize),
            Style::new()
                .bg(pallete.background)
                .fg(pallete.DELETION_COLOR),
        ),

        RenderingContextKind::SoftClip(base) => buf.set_string(
            area.x + onscreen_x,
            area.y + onscreen_y,
            String::from_utf8(vec![base])?, // FIXME
            Style::default().bg(pallete.softclip_color(base)),
        ),

        RenderingContextKind::PairGap => buf.set_string(
            area.x + onscreen_x,
            area.y + onscreen_y,
            "-".repeat(length as usize),
            Style::new()
                .bg(pallete.background)
                .fg(pallete.PAIRGAP_COLOR),
        ),

        RenderingContextKind::PairOverlap => buf.set_string(
            area.x + onscreen_x,
            area.y + onscreen_y,
            "-".repeat(length as usize),
            Style::new()
                .bg(pallete.background)
                .fg(pallete.PAIR_OVERLAP_COLOR),
        ),
    }

    // ── Modifiers ─────────────────────────────────────────────────────────
    for modifier in context.modifiers.iter() {
        match modifier {
            RenderingContextModifier::Forward => {
                if let OnScreenCoordinate::OnScreen(x) = end_onscreen_coordinate
                    && let Some(cell) =
                        buf.cell_mut(Position::new(area.x + x as u16, area.y + onscreen_y))
                {
                    cell.set_symbol("►");
                }
            }

            RenderingContextModifier::Reverse => {
                if let OnScreenCoordinate::OnScreen(x) = start_onscreen_coordinate
                    && let Some(cell) =
                        buf.cell_mut(Position::new(area.x + x as u16, area.y + onscreen_y))
                {
                    cell.set_symbol("◄");
                }
            }

            RenderingContextModifier::Insertion(_l) => {
                if let OnScreenCoordinate::OnScreen(x) = start_onscreen_coordinate
                    && let Some(cell) =
                        buf.cell_mut(Position::new(area.x + x as u16, area.y + onscreen_y))
                {
                    cell.set_symbol("▌")
                        .set_style(Style::default().fg(pallete.INSERTION_COLOR));
                }
            }

            RenderingContextModifier::Mismatch(coordinate, base) => {
                if let OnScreenCoordinate::OnScreen(x) =
                    alignment_view.onscreen_x_coordinate(*coordinate, area)
                    && let Some(cell) =
                        buf.cell_mut(Position::new(area.x + x as u16, area.y + onscreen_y))
                {
                    cell.set_char(*base as char)
                        .set_style(Style::default().fg(pallete.mismatch_color(*base)));
                }
            }

            RenderingContextModifier::PairConflict(coordinate) => {
                if let OnScreenCoordinate::OnScreen(x) =
                    alignment_view.onscreen_x_coordinate(*coordinate, area)
                    && let Some(cell) =
                        buf.cell_mut(Position::new(area.x + x as u16, area.y + onscreen_y))
                {
                    cell.set_symbol("?");
                }
            }

            RenderingContextModifier::BaseModification(coordinate, modification, probability) => {
                if let OnScreenCoordinate::OnScreen(x) =
                    alignment_view.onscreen_x_coordinate(*coordinate, area)
                    && let Some(cell) =
                        buf.cell_mut(Position::new(area.x + x as u16, area.y + onscreen_y))
                {
                    cell.set_style(
                        Style::default().bg(pallete.modification_color(modification, *probability)),
                    );
                }
            }
        }
    }

    Ok(())
}

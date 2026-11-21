//! Draws an alignment track's scene in layer order.
//! Each layer preserves source-table order, so later records overwrite earlier ones.

use super::prelude::*;
use crate::{
    layout::ALIGNMENT_ROW_HEIGHT,
    rendering::{colors::READ_BASE_LETTER, sequence::MIN_PIXELS_PER_LETTER},
};
use gv_core::alignment::{Alignment, PairedAlignment};
use gv_viewer::{
    colors,
    prepare::alignment::{AlignmentScene, BaseRun, RowSpan},
};

/// The gap between two stacking rows.
const ROW_GAP: f32 = 2.0;
/// The width of a read's strand arrow.
const ARROW_WIDTH: f32 = 4.0;

/// Render visible reads from the zero-based `top` row, followed by annotations. With `paired`,
/// pairs are drawn together with the gap between their mates.
pub fn render_alignment(
    area: &Rect,
    alignment: &Alignment,
    paired: Option<&PairedAlignment>,
    view: &PixelView,
    top: usize,
    palette: &Palette,
    window: &mut Window,
    cx: &mut App,
) -> Result<(), crate::Error> {
    let region = view.region(area);
    let rows = (area.height / ALIGNMENT_ROW_HEIGHT).ceil() as usize;
    let scene = AlignmentScene::new(alignment, paired, &region, top, rows)?;
    let row_top = |y: u64| area.y + (y as usize - top) as f32 * ALIGNMENT_ROW_HEIGHT;
    let height = ALIGNMENT_ROW_HEIGHT - ROW_GAP;
    let span = |span: &RowSpan| {
        view.span(span.start, span.end, area)
            .map(|(x, width)| Rect::new(x, row_top(span.y), width, height))
    };

    for gap in &scene.pair_gaps {
        if let Some(rect) = span(gap) {
            fill_rect(
                Rect::new(rect.x, rect.y + height / 2.0 - 0.5, rect.width, 1.0),
                to_hsla(palette.PAIRGAP_COLOR),
                window,
            );
        }
    }

    // 1. Draw the main bodies and soft clips.
    for body in &scene.bodies {
        if let Some(rect) = span(&body.span) {
            let color = if body.zero_mapq {
                palette.ZERO_MAPQ_COLOR
            } else {
                palette.MATCH_COLOR
            };
            fill_rect(rect, to_hsla(color), window);
        }
    }
    for run in &scene.soft_clips {
        draw_bases(
            area,
            view,
            run,
            row_top(run.span.y),
            height,
            |base, _| (palette.softclip_color(base), 1.0),
            window,
            cx,
        )?;
    }

    // 2. Draw the deletions and reference skips as a line through the gap.
    for deletion in &scene.deletions {
        if let Some(rect) = span(deletion) {
            fill_rect(rect, to_hsla(palette.background), window);
            fill_rect(
                Rect::new(rect.x, rect.y + height / 2.0 - 0.5, rect.width, 1.0),
                to_hsla(palette.DELETION_COLOR),
                window,
            );
        }
    }

    // Arrows come after deletions, so a read that ends in a deletion keeps its strand marker.
    let arrow_width = ARROW_WIDTH.min(view.pixels_per_base().max(2.0));
    for arrow in &scene.arrows {
        let y = row_top(arrow.y);
        let color = to_hsla(palette.MATCH_COLOR);
        if arrow.reverse {
            let x = view.x(arrow.position, area);
            fill_polygon(
                &[(x, y), (x - arrow_width, y + height / 2.0), (x, y + height)],
                color,
                window,
            );
        } else {
            let x = view.x(arrow.position + 1, area);
            fill_polygon(
                &[(x, y), (x + arrow_width, y + height / 2.0), (x, y + height)],
                color,
                window,
            );
        }
    }

    // 3. Draw the insertions at the boundary they're inserted at.
    for insertion in &scene.insertions {
        let x = view.x(insertion.start, area);
        if x >= area.x && x < area.right() {
            fill_rect(
                Rect::new(x - 1.0, row_top(insertion.y), 2.0, height),
                to_hsla(palette.INSERTION_COLOR),
                window,
            );
        }
    }

    // 4. Draw the mismatched bases.
    for run in &scene.mismatch_runs {
        draw_bases(
            area,
            view,
            run,
            row_top(run.span.y),
            height,
            |base, quality| mismatch_color(palette, base, quality),
            window,
            cx,
        )?;
    }
    for mismatch in &scene.mismatches {
        draw_base(
            area,
            view,
            mismatch.position,
            mismatch.base,
            row_top(mismatch.y),
            height,
            mismatch_color(palette, mismatch.base, mismatch.quality),
            window,
            cx,
        )?;
    }

    // 5. Draw the base modifications.
    for modification in &scene.modifications {
        if let Some((x, width)) = view.span(modification.position, modification.position, area) {
            fill_rect(
                Rect::new(x, row_top(modification.y), width, height),
                to_hsla_with_alpha(
                    palette
                        .modification_color(&modification.modification, modification.probability),
                    0.85,
                ),
                window,
            );
        }
    }
    Ok(())
}

/// A mismatched base's color and opacity. Low-quality bases are faded, so likely sequencing
/// errors stand out less.
fn mismatch_color(palette: &Palette, base: u8, quality: Option<u8>) -> (Rgb, f32) {
    let alpha = if colors::Palette::is_mismatch_shaded(quality) {
        0.45
    } else {
        1.0
    };
    (palette.mismatch_color(base), alpha)
}

/// Draws a run's bases, one box per base, or one per pixel when bases are narrower.
fn draw_bases(
    area: &Rect,
    view: &PixelView,
    run: &BaseRun,
    y: f32,
    height: f32,
    color: impl Fn(u8, Option<u8>) -> (Rgb, f32),
    window: &mut Window,
    cx: &mut App,
) -> Result<(), crate::Error> {
    let (left, right) = view.visible(area);
    let (start, end) = (run.span.start.max(left), run.span.end.min(right));
    if start > end {
        return Ok(());
    }
    if view.pixels_per_base() >= 1.0 {
        for position in start..=end {
            let base = run.base_at(position);
            draw_base(
                area,
                view,
                position,
                base,
                y,
                height,
                color(base, run.quality_at(position)),
                window,
                cx,
            )?;
        }
    } else {
        let mut x = view.x(start, area).max(area.x).floor();
        let last = view.x(end + 1, area).min(area.right());
        while x < last {
            let position = view.position_at(x, area).clamp(start, end);
            let (color, alpha) = color(run.base_at(position), run.quality_at(position));
            fill_rect(
                Rect::new(x, y, 1.0, height),
                to_hsla_with_alpha(color, alpha),
                window,
            );
            x += 1.0;
        }
    }
    Ok(())
}

/// Draws one base: a colored box, with its letter when the base is wide enough.
#[allow(clippy::too_many_arguments)]
fn draw_base(
    area: &Rect,
    view: &PixelView,
    position: u64,
    base: u8,
    y: f32,
    height: f32,
    (color, alpha): (Rgb, f32),
    window: &mut Window,
    cx: &mut App,
) -> Result<(), crate::Error> {
    let Some((x, width)) = view.span(position, position, area) else {
        return Ok(());
    };
    fill_rect(
        Rect::new(x, y, width, height),
        to_hsla_with_alpha(color, alpha),
        window,
    );
    if view.pixels_per_base() >= MIN_PIXELS_PER_LETTER {
        paint_letter(
            Rect::new(x, y, width, height),
            base as char,
            READ_BASE_LETTER,
            window,
            cx,
        )?;
    }
    Ok(())
}

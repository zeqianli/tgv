//! Draws an alignment track's scene in layer order.
//! Each layer preserves source-table order, so later records overwrite earlier ones.

use crate::{
    layout::{ALIGNMENT_ROW_HEIGHT, PixelView, Rect},
    rendering::{
        Painter,
        colors::{Palette, Rgb, ToHsla},
        sequence::MIN_PIXELS_PER_LETTER,
    },
};
use gv_core::{
    alignment::{Alignment, PairedAlignment},
    prelude::*,
};
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
    painter: &mut Painter,
    area: &Rect,
    alignment: &Alignment,
    paired: Option<&PairedAlignment>,
    view: &PixelView,
    top: usize,
    palette: &Palette,
) -> Result<(), TGVError> {
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
            painter.fill(
                Rect::new(rect.x, rect.y + height / 2.0 - 0.5, rect.width, 1.0),
                palette.PAIRGAP_COLOR.hsla(),
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
            painter.fill(rect, color.hsla());
        }
    }
    for run in &scene.soft_clips {
        draw_bases(
            painter,
            area,
            view,
            run,
            row_top(run.span.y),
            height,
            |base, _| (palette.softclip_color(base), 1.0),
        );
    }

    // 2. Draw the deletions and reference skips as a line through the gap.
    for deletion in &scene.deletions {
        if let Some(rect) = span(deletion) {
            painter.fill(rect, palette.background.hsla());
            painter.fill(
                Rect::new(rect.x, rect.y + height / 2.0 - 0.5, rect.width, 1.0),
                palette.DELETION_COLOR.hsla(),
            );
        }
    }

    // Arrows come after deletions, so a read that ends in a deletion keeps its strand marker.
    let arrow_width = ARROW_WIDTH.min(view.pixels_per_base().max(2.0));
    for arrow in &scene.arrows {
        let y = row_top(arrow.y);
        let color = palette.MATCH_COLOR.hsla();
        if arrow.reverse {
            let x = view.x(arrow.position, area);
            painter.polygon(
                &[(x, y), (x - arrow_width, y + height / 2.0), (x, y + height)],
                color,
            );
        } else {
            let x = view.x(arrow.position + 1, area);
            painter.polygon(
                &[(x, y), (x + arrow_width, y + height / 2.0), (x, y + height)],
                color,
            );
        }
    }

    // 3. Draw the insertions at the boundary they're inserted at.
    for insertion in &scene.insertions {
        let x = view.x(insertion.start, area);
        if x >= area.x && x < area.right() {
            painter.fill(
                Rect::new(x - 1.0, row_top(insertion.y), 2.0, height),
                palette.INSERTION_COLOR.hsla(),
            );
        }
    }

    // 4. Draw the mismatched bases.
    for run in &scene.mismatch_runs {
        draw_bases(
            painter,
            area,
            view,
            run,
            row_top(run.span.y),
            height,
            |base, quality| mismatch_color(palette, base, quality),
        );
    }
    for mismatch in &scene.mismatches {
        draw_base(
            painter,
            area,
            view,
            mismatch.position,
            mismatch.base,
            row_top(mismatch.y),
            height,
            mismatch_color(palette, mismatch.base, mismatch.quality),
        );
    }

    // 5. Draw the base modifications.
    for modification in &scene.modifications {
        if let Some((x, width)) = view.span(modification.position, modification.position, area) {
            painter.fill(
                Rect::new(x, row_top(modification.y), width, height),
                palette
                    .modification_color(&modification.modification, modification.probability)
                    .alpha(0.85),
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
    painter: &mut Painter,
    area: &Rect,
    view: &PixelView,
    run: &BaseRun,
    y: f32,
    height: f32,
    color: impl Fn(u8, Option<u8>) -> (Rgb, f32),
) {
    let (left, right) = view.visible(area);
    let (start, end) = (run.span.start.max(left), run.span.end.min(right));
    if start > end {
        return;
    }
    if view.pixels_per_base() >= 1.0 {
        for position in start..=end {
            let base = run.base_at(position);
            draw_base(
                painter,
                area,
                view,
                position,
                base,
                y,
                height,
                color(base, run.quality_at(position)),
            );
        }
    } else {
        let mut x = view.x(start, area).max(area.x).floor();
        let last = view.x(end + 1, area).min(area.right());
        while x < last {
            let position = view.position_at(x, area).clamp(start, end);
            let (color, alpha) = color(run.base_at(position), run.quality_at(position));
            painter.fill(Rect::new(x, y, 1.0, height), color.alpha(alpha));
            x += 1.0;
        }
    }
}

/// Draws one base: a colored box, with its letter when the base is wide enough.
#[allow(clippy::too_many_arguments)]
fn draw_base(
    painter: &mut Painter,
    area: &Rect,
    view: &PixelView,
    position: u64,
    base: u8,
    y: f32,
    height: f32,
    (color, alpha): (Rgb, f32),
) {
    let Some((x, width)) = view.span(position, position, area) else {
        return;
    };
    painter.fill(Rect::new(x, y, width, height), color.alpha(alpha));
    if view.pixels_per_base() >= MIN_PIXELS_PER_LETTER {
        painter.letter(
            Rect::new(x, y, width, height),
            base as char,
            Rgb::from_u32(0x111111),
        );
    }
}

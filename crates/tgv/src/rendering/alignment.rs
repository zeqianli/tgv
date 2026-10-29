//! Draws an alignment track's scene into the buffer in layer order.
//! Each layer preserves source-table order, so later records overwrite earlier ones.

use crate::layout::CellView;
use crate::rendering::colors::{ToColor, mismatch_style};
use crate::{
    layout::{AlignmentView, OnScreenCoordinate},
    rendering::colors::Palette,
};
use gv_core::{
    alignment::{Alignment, PairedAlignment},
    prelude::*,
};
use gv_viewer::prepare::alignment::{AlignmentScene, BaseRun, RowSpan};
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
    if area.is_empty() {
        return Ok(());
    }
    let region = view.region(f64::from(area.width));
    let scene = AlignmentScene::new(alignment, None, &region, top, usize::from(area.height))?;
    draw_scene(top, area, buf, &scene, &region, view, palette);
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
    if area.is_empty() {
        return Ok(());
    }
    let region = view.region(f64::from(area.width));
    let scene = AlignmentScene::new(
        alignment,
        Some(paired),
        &region,
        top,
        usize::from(area.height),
    )?;
    draw_scene(top, area, buf, &scene, &region, view, palette);
    Ok(())
}

/// The columns a span covers on screen, and its terminal row.
fn columns(
    span: &RowSpan,
    top: usize,
    region: &Region,
    view: &AlignmentView,
    area: &Rect,
) -> Option<(u16, u16, u16)> {
    let (start, end) = (span.start.max(region.start()), span.end.min(region.end()));
    let left = pixel(start, view, area)?;
    let right = pixel(end, view, area).unwrap_or(area.width - 1);
    Some((left, right, area.y + (span.y as usize - top) as u16))
}

/// The run's base shown in a column: the column's last base within the run.
fn column_base(run: &BaseRun, x: u16, region: &Region, view: &AlignmentView) -> (u8, Option<u8>) {
    let zoom = view.zoom.0;
    let end = run.span.end.min(region.end());
    let position = (region.start() + u64::from(x) * zoom + zoom - 1).min(end);
    (run.base_at(position), run.quality_at(position))
}

fn draw_scene(
    top: usize,
    area: &Rect,
    buf: &mut Buffer,
    scene: &AlignmentScene,
    region: &Region,
    view: &AlignmentView,
    palette: &Palette,
) {
    for gap in &scene.pair_gaps {
        let Some((left, right, y)) = columns(gap, top, region, view, area) else {
            continue;
        };
        for x in left..=right {
            if let Some(cell) = buf.cell_mut(Position::new(area.x + x, y)) {
                cell.set_symbol("-")
                    .set_bg(palette.background.color())
                    .set_fg(palette.PAIRGAP_COLOR.color());
            }
        }
    }

    // 1. Draw the main bodies and soft clips.
    for body in &scene.bodies {
        let Some((left, right, y)) = columns(&body.span, top, region, view, area) else {
            continue;
        };
        for x in left..=right {
            if let Some(cell) = buf.cell_mut(Position::new(area.x + x, y)) {
                cell.set_symbol("-")
                    .set_bg(if body.zero_mapq {
                        palette.ZERO_MAPQ_COLOR.color()
                    } else {
                        palette.MATCH_COLOR.color()
                    })
                    .set_fg(palette.MATCH_FG_COLOR.color());
            }
        }
    }

    for run in &scene.soft_clips {
        let Some((left, right, y)) = columns(&run.span, top, region, view, area) else {
            continue;
        };
        for x in left..=right {
            if let Some(cell) = buf.cell_mut(Position::new(area.x + x, y)) {
                let (base, _) = column_base(run, x, region, view);
                cell.set_char(base as char);
                cell.set_bg(palette.softclip_color(base).color())
                    .set_fg(Color::Reset);
            }
        }
    }

    // 2. Draw the deletions and reference skips.
    for deletion in &scene.deletions {
        let Some((left, right, y)) = columns(deletion, top, region, view, area) else {
            continue;
        };
        for x in left..=right {
            if let Some(cell) = buf.cell_mut(Position::new(area.x + x, y)) {
                cell.set_symbol("-")
                    .set_bg(palette.background.color())
                    .set_fg(palette.DELETION_COLOR.color());
            }
        }
    }

    for arrow in &scene.arrows {
        let Some(x) = pixel(arrow.position, view, area) else {
            continue;
        };
        let y = area.y + (arrow.y as usize - top) as u16;
        if let Some(cell) = buf.cell_mut(Position::new(area.x + x, y)) {
            cell.set_symbol(if arrow.reverse { "◄" } else { "►" });
        }
    }

    // 3. Draw the insertions.
    for insertion in &scene.insertions {
        let Some(x) = pixel(insertion.start, view, area) else {
            continue;
        };
        let y = area.y + (insertion.y as usize - top) as u16;
        if let Some(cell) = buf.cell_mut(Position::new(area.x + x, y)) {
            cell.set_symbol("▌").set_fg(palette.INSERTION_COLOR.color());
        }
    }

    // 4. Draw the mismatched bases.
    for run in &scene.mismatch_runs {
        let Some((left, right, y)) = columns(&run.span, top, region, view, area) else {
            continue;
        };
        for x in left..=right {
            if let Some(cell) = buf.cell_mut(Position::new(area.x + x, y)) {
                let (base, quality) = column_base(run, x, region, view);
                cell.set_char(base as char)
                    .set_style(mismatch_style(palette, base, quality));
            }
        }
    }

    for mismatch in &scene.mismatches {
        if let Some(position) = pixel(mismatch.position, view, area)
            .map(|x| Position::new(area.x + x, area.y + (mismatch.y as usize - top) as u16))
            && let Some(cell) = buf.cell_mut(position)
        {
            cell.set_char(mismatch.base as char)
                .set_style(mismatch_style(palette, mismatch.base, mismatch.quality));
        }
    }

    // 5. Draw the base modifications.
    for modification in &scene.modifications {
        if let Some(position) = pixel(modification.position, view, area)
            .map(|x| Position::new(area.x + x, area.y + (modification.y as usize - top) as u16))
            && let Some(cell) = buf.cell_mut(position)
        {
            cell.set_bg(
                palette
                    .modification_color(&modification.modification, modification.probability)
                    .color(),
            );
        }
    }
}

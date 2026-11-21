use super::prelude::*;
use gv_core::alignment::CoverageTable;
use gv_viewer::prepare::coverage::{mean_depths, round_up_max_coverage};

/// A base whose other alleles reach this fraction of its depth shows each allele's share, as in
/// IGV.
const ALLELE_FREQUENCY_THRESHOLD: f64 = 0.2;

/// Draws the coverage: one bar per base when bases are at least a pixel wide, or the mean
/// depth of each pixel's bases otherwise. The y-axis fits the visible maximum.
pub fn render_coverage(
    area: &Rect,
    coverage: &CoverageTable,
    view: &PixelView,
    palette: &Palette,
    window: &mut Window,
    cx: &mut App,
) -> Result<(), crate::Error> {
    let plot = Rect::new(
        area.x,
        area.y + LINE_HEIGHT,
        area.width,
        area.height - LINE_HEIGHT - 2.0,
    );
    if plot.is_empty() {
        return Ok(());
    }
    let (left, right) = view.visible(area);
    if view.pixels_per_base() >= 1.0 {
        let bases = coverage.bases(left, right)?;
        let y_max =
            round_up_max_coverage(bases.iter().map(|base| base.total).max().unwrap_or(0) as usize)
                as f32;
        for base in &bases {
            let total = base.total;
            let Some((x, width)) = view.span(base.position, base.position, area) else {
                continue;
            };
            // Leave a gap between wide bars, so neighboring bases stay apart.
            let width = if width > 4.0 { width - 1.0 } else { width };
            let reference = base.reference.to_ascii_uppercase();
            let counts = [
                (b'A', base.a, palette.COVERAGE_A),
                (b'C', base.c, palette.COVERAGE_C),
                (b'G', base.g, palette.COVERAGE_G),
                (b'T', base.t, palette.COVERAGE_T),
            ];
            let alternate = counts
                .iter()
                .filter(|(letter, _, _)| *letter != reference)
                .map(|(_, count, _)| count)
                .sum::<u64>();
            if total > 0 && alternate as f64 / total as f64 >= ALLELE_FREQUENCY_THRESHOLD {
                let mut bottom = plot.bottom();
                for (_, count, color) in &counts {
                    let height = *count as f32 / y_max * plot.height;
                    fill_rect(
                        Rect::new(x, bottom - height, width, height),
                        to_hsla(*color),
                        window,
                    );
                    bottom -= height;
                }
            } else {
                let height = total as f32 / y_max * plot.height;
                fill_rect(
                    Rect::new(x, plot.bottom() - height, width, height),
                    to_hsla_with_alpha(palette.COVERAGE_TOTAL, 0.7),
                    window,
                );
            }
        }
        TextContext::default()
            .text(&format!("[0-{y_max}]"))
            .color(MUTED_TEXT)
            .paint(area.x + 2.0, area.y, LINE_HEIGHT, window, cx)?;
    } else {
        let bins = mean_depths(
            coverage,
            left,
            right,
            (plot.width as usize).clamp(1, (right - left + 1) as usize),
        )?;
        let y_max = round_up_max_coverage(
            bins.iter().map(|bin| bin.depth).fold(0.0, f32::max).ceil() as usize,
        ) as f32;
        for bin in &bins {
            let Some((x, width)) = view.span(bin.start, bin.end, area) else {
                continue;
            };
            let height = bin.depth / y_max * plot.height;
            fill_rect(
                Rect::new(x, plot.bottom() - height, width, height),
                to_hsla_with_alpha(palette.COVERAGE_TOTAL, 0.7),
                window,
            );
        }
        TextContext::default()
            .text(&format!("[0-{y_max}]"))
            .color(MUTED_TEXT)
            .paint(area.x + 2.0, area.y, LINE_HEIGHT, window, cx)?;
    }
    Ok(())
}

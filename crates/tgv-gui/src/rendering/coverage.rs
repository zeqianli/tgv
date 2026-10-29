use crate::{
    layout::{PixelView, Rect},
    rendering::{
        LINE_HEIGHT, Painter,
        colors::{MUTED_TEXT, Palette, ToHsla},
    },
};
use gv_core::{
    alignment::{CoverageSchema, CoverageTable},
    prelude::*,
};
use gv_viewer::prepare::coverage::{get_linear_space, round_up_max_coverage};
use polars::prelude::ChunkAgg;

/// A base whose other alleles reach this fraction of its depth shows each allele's share, as in
/// IGV.
const ALLELE_FREQUENCY_THRESHOLD: f64 = 0.2;

/// Draws the coverage: one bar per base when bases are at least a pixel wide, or the mean
/// depth of each pixel's bases otherwise. The y-axis fits the visible maximum.
pub fn render_coverage(
    painter: &mut Painter,
    area: &Rect,
    coverage: &CoverageTable,
    view: &PixelView,
    palette: &Palette,
) -> Result<(), TGVError> {
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
        let table = coverage.query(left, right)?;
        let positions = table.column(CoverageSchema::POS)?.u64()?;
        let totals = table.column(CoverageSchema::TOTAL)?.u64()?;
        let y_max = round_up_max_coverage(totals.max().unwrap_or(0) as usize) as f32;
        let counts = [
            (CoverageSchema::A, b'A', palette.COVERAGE_A),
            (CoverageSchema::C, b'C', palette.COVERAGE_C),
            (CoverageSchema::G, b'G', palette.COVERAGE_G),
            (CoverageSchema::T, b'T', palette.COVERAGE_T),
        ]
        .into_iter()
        .map(|(column, base, color)| Ok((table.column(column)?.u64()?.clone(), base, color)))
        .collect::<Result<Vec<_>, TGVError>>()?;
        let references = table.column(CoverageSchema::REFERENCE_BASE)?.u8()?;
        for (row, position) in positions.into_no_null_iter().enumerate() {
            let total = totals.get(row).expect("coverage totals are non-null");
            let Some((x, width)) = view.span(position, position, area) else {
                continue;
            };
            // Leave a gap between wide bars, so neighboring bases stay apart.
            let width = if width > 4.0 { width - 1.0 } else { width };
            let reference = references
                .get(row)
                .expect("reference bases are non-null")
                .to_ascii_uppercase();
            let alternate = counts
                .iter()
                .filter(|(_, base, _)| *base != reference)
                .map(|(counts, _, _)| counts.get(row).expect("coverage counts are non-null"))
                .sum::<u64>();
            if total > 0 && alternate as f64 / total as f64 >= ALLELE_FREQUENCY_THRESHOLD {
                let mut bottom = plot.bottom();
                for (counts, _, color) in &counts {
                    let count = counts.get(row).expect("coverage counts are non-null");
                    let height = count as f32 / y_max * plot.height;
                    painter.fill(Rect::new(x, bottom - height, width, height), color.hsla());
                    bottom -= height;
                }
            } else {
                let height = total as f32 / y_max * plot.height;
                painter.fill(
                    Rect::new(x, plot.bottom() - height, width, height),
                    palette.COVERAGE_TOTAL.alpha(0.7),
                );
            }
        }
        painter.text(area.x + 2.0, area.y, &format!("[0-{y_max}]"), MUTED_TEXT);
    } else {
        let bins = get_linear_space(
            left,
            right,
            (plot.width as usize).clamp(1, (right - left + 1) as usize),
        )?;
        let table = coverage.query(left, right)?;
        let positions = table.column(CoverageSchema::POS)?.u64()?;
        let totals = table.column(CoverageSchema::TOTAL)?.u64()?;
        let mut sums = vec![0u64; bins.len()];
        let mut bin = 0;
        for (row, position) in positions.into_no_null_iter().enumerate() {
            while position > bins[bin].1 {
                bin += 1;
            }
            sums[bin] += totals.get(row).expect("coverage totals are non-null");
        }
        let depths = sums
            .iter()
            .zip(&bins)
            .map(|(sum, (start, end))| *sum as f32 / (end - start + 1) as f32)
            .collect::<Vec<_>>();
        let y_max = round_up_max_coverage(depths.iter().copied().fold(0.0, f32::max).ceil() as usize)
            as f32;
        for (depth, (start, end)) in depths.iter().zip(&bins) {
            let Some((x, width)) = view.span(*start, *end, area) else {
                continue;
            };
            let height = depth / y_max * plot.height;
            painter.fill(
                Rect::new(x, plot.bottom() - height, width, height),
                palette.COVERAGE_TOTAL.alpha(0.7),
            );
        }
        painter.text(area.x + 2.0, area.y, &format!("[0-{y_max}]"), MUTED_TEXT);
    }
    Ok(())
}

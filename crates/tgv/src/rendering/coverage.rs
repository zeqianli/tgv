use std::usize;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::Widget,
};

use ratatui::symbols::bar::{NINE_LEVELS, Set};

use gv_core::{
    alignment::{Alignment, BaseCoverage},
    error::TGVError,
    state::State,
};

use crate::{layout::AlignmentView, rendering::Palette};
const MIN_AREA_WIDTH: u16 = 2;
const MIN_AREA_HEIGHT: u16 = 1;

/// Render the coverage barplot.
pub fn render_coverage(
    area: &Rect,
    buf: &mut Buffer,
    state: &State,
    alignment_view: &AlignmentView,
    palette: &Palette,
) -> Result<(), TGVError> {
    if area.width < MIN_AREA_WIDTH || area.height < MIN_AREA_HEIGHT {
        return Ok(());
    }

    let mut binned_coverage = calculate_binned_coverage(
        &state.alignment,
        alignment_view.left(area),
        alignment_view.right(area),
        area.width as usize,
    )?;

    println!("{}", binned_coverage[0].len());

    let y_max: usize = round_up_max_coverage(
        (0..binned_coverage[0].len())
            .map(|i| binned_coverage[0][i] + binned_coverage[1][i])
            .max()
            .unwrap_or(0),
    );
    StackedSparkline::default()
        .add_data(binned_coverage.remove(0), palette.COVERAGE_ALT)
        .add_data(binned_coverage.remove(0), palette.COVERAGE_TOTAL)
        .max(y_max)
        .render(*area, buf);

    buf.set_string(area.x, area.y, format!("[0-{}]", y_max,), Style::default());

    Ok(())
}

/// Round up the maximum coverage to two significant digits.
fn round_up_max_coverage(x: usize) -> usize {
    if x < 10 {
        return 10;
    }
    let mut x = x;
    let mut multiplier = 1;

    let mut round_up = false;
    while x >= 100 {
        if x % 10 > 0 {
            round_up = true;
        }
        x /= 10;
        multiplier *= 10;
    }

    if round_up {
        (x + 1) * multiplier
    } else {
        x * multiplier
    }
}

/// Get a linear space of n_bins between left and right.
/// 1-based, inclusive.
/// Returns a vector of n_bins + 1 elements.
fn get_linear_space(left: u64, right: u64, n_bins: usize) -> Result<Vec<(u64, u64)>, TGVError> {
    if n_bins == 0 {
        return Err(TGVError::ValueError("n_bins is 0".to_string()));
    }

    if right <= left {
        return Err(TGVError::ValueError("Right is less than left".to_string()));
    }

    if n_bins as u64 > right - left {
        // FIXME: make this permissive? Don't wanna crash the app because of a rendering problem.
        // Could happen if the region is weird.
        return Err(TGVError::ValueError(format!(
            "n_bins = {} is greater than the number of bases in the region [{}, {}]",
            n_bins, left, right
        )));
    }

    let mut bins: Vec<(u64, u64)> = Vec::new();
    let mut pivot = left as f64; // f32 here actually causes problem for genome coordinates

    let bin_width: f64 = (right - left) as f64 / n_bins as f64;

    for i in 0..n_bins {
        let bin_left = if i == 0 { left } else { pivot as u64 + 1 };

        pivot += bin_width;

        let bin_right = if i == n_bins - 1 { right } else { pivot as u64 };

        bins.push((bin_left, bin_right));
    }

    Ok(bins)
}

/// Calculate the binned coverage in [left_bound, right_bound].
/// 1-based, inclusive.
fn calculate_binned_coverage(
    alignment: &Alignment,
    left: u64,
    right: u64,
    n_bins: usize,
) -> Result<Vec<Vec<usize>>, TGVError> {
    if right < left {
        return Err(TGVError::ValueError("Right is less than left".to_string()));
    }

    if n_bins == 0 {
        return Err(TGVError::ValueError("n_bins is 0".to_string()));
    }

    if right - left + 1 == n_bins as u64 {
        // 1x zoom. Not need to calulate binned coverage.

        // Stack 0: alt allele if above a threshold
        // Stack 1: non-alt alleles
        let mut output = vec![vec![0; n_bins]; 2];
        (left..right + 1).enumerate().for_each(|(i, x)| {
            let coverage = alignment.coverage_at(x);
            let max_alt_depth = coverage.max_alt_depth().unwrap_or(0);

            if max_alt_depth * BaseCoverage::MAX_DISPLAY_ALLELE_FREQUENCY_RECIPROCOL
                > coverage.total
            {
                output[0][i] = max_alt_depth;
                output[1][i] = coverage.total - max_alt_depth;
            } else {
                output[0][i] = 0;
                output[1][i] = coverage.total;
            }
        });
        return Ok(output);
    }

    let linear_space = get_linear_space(left, right, n_bins)?;

    let mut output = vec![vec![0; linear_space.len()]; 2];
    linear_space
        .into_iter()
        .enumerate()
        .for_each(|(i, (bin_left, bin_right))| {
            (bin_left..bin_right + 1).for_each(|x| output[1][i] += alignment.coverage_at(x).total);
        });

    Ok(output)
}

/// Stacked sparkline with multiple colors.
/// TODO: move this to a separate crate.
struct StackedSparkline {
    max: Option<usize>,

    data: Vec<(Vec<usize>, Color)>, // bottom, top

    bar_set: Set,
}

impl Default for StackedSparkline {
    fn default() -> Self {
        Self {
            max: Some(0),
            data: Vec::new(),
            bar_set: NINE_LEVELS,
        }
    }
}

impl StackedSparkline {
    pub fn add_data(mut self, data: Vec<usize>, color: Color) -> Self {
        self.data.push((data, color));
        self
    }

    pub fn max(mut self, max: usize) -> Self {
        self.max = Some(max);
        self
    }

    const fn symbol_for_height(&self, height: usize) -> &str {
        match height {
            0 => self.bar_set.empty,
            1 => self.bar_set.one_eighth,
            2 => self.bar_set.one_quarter,
            3 => self.bar_set.three_eighths,
            4 => self.bar_set.half,
            5 => self.bar_set.five_eighths,
            6 => self.bar_set.three_quarters,
            7 => self.bar_set.seven_eighths,
            _ => self.bar_set.full,
        }
    }

    fn get_color(&self, i_stack: usize) -> &Color {
        &self.data[i_stack].1
    }

    fn get_data(&self, i_data: usize, i_stack: usize) -> usize {
        *self.data[i_stack].0.get(i_data).unwrap_or(&0)
    }
}

impl Widget for StackedSparkline {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Inspired by Ratatui's sparkline implementation

        if area.is_empty() {
            return;
        }

        let max_index = usize::min(
            area.width as usize,
            self.data
                .iter()
                .map(|stack_data| stack_data.0.len())
                .max()
                .unwrap_or(0),
        );
        let max = self.max.unwrap_or(
            self.data
                .iter()
                .map(|stack_data| *stack_data.0.iter().max().unwrap_or(&0))
                .max()
                .unwrap_or(0),
        );

        if max == 0 {
            return;
        }

        // Ratatui's sparkline converts the height to # of 1/8 cells.
        // But this doesn't work for the stacked plot because it causes numerical errors.
        let cell_height = usize::max(1, max / area.height as usize);

        // render each item in the data
        for i in 0..max_index {
            let x = area.left() + i as u16;

            let mut pivot = 0;
            let mut accumulator = self.get_data(i, pivot); // accumate un-plotted heights

            for j in (0..area.height).rev() {
                // render from screen bottom to top (loop is top to bottom)
                if accumulator >= cell_height {
                    // render a whole cell
                    buf[(x, area.top() + j)]
                        .set_symbol(self.bar_set.full)
                        .set_style(Style::default().fg(*self.get_color(pivot)));

                    accumulator -= cell_height
                } else {
                    // Multiple color in the same cell
                    // Each cell fits max two colors.
                    // Accumate next stacks until the cell is filled. Only render the top two colors.

                    let mut rendered = false;

                    let mut top_two_indexes: (usize, usize) = (pivot, 0); // largest, second largest
                    let mut top_two_accumulators: (usize, usize) = (accumulator, 0);

                    for k in pivot + 1..self.data.len() {
                        let item = self.get_data(i, k);
                        accumulator += item;

                        if item > top_two_indexes.0 {
                            top_two_indexes = (k, top_two_indexes.0);
                            top_two_accumulators = (accumulator, top_two_accumulators.0);
                        } else if item > top_two_indexes.1 {
                            top_two_indexes = (top_two_indexes.0, k);
                            top_two_accumulators = (top_two_accumulators.0, accumulator);
                        };

                        if accumulator >= cell_height {
                            // render

                            // Note to maintain the order of these two colors
                            let (fg_height, fg_color, bg_color) =
                                if top_two_indexes.0 > top_two_indexes.1 {
                                    // 1 is the bottom stack
                                    // 1's accumulator is smaller than 0, so the foreground height is 1's accumulator
                                    (
                                        top_two_accumulators.1 * 8 / cell_height,
                                        self.get_color(top_two_indexes.1),
                                        self.get_color(top_two_indexes.0),
                                    )
                                } else {
                                    // 0 is the bottom stack
                                    // 0's accumulator is larger than 1, so the foreground height is the difference
                                    (
                                        (top_two_accumulators.1 - top_two_accumulators.0) * 8
                                            / cell_height,
                                        self.get_color(top_two_indexes.0),
                                        self.get_color(top_two_indexes.1),
                                    )
                                };

                            buf[(x, area.top() + j)]
                                .set_symbol(self.symbol_for_height(fg_height))
                                .set_style(Style::default().fg(*fg_color).bg(*bg_color));

                            accumulator -= cell_height;
                            pivot = k;
                            rendered = true;
                            break;
                        }
                    }

                    if !rendered {
                        // Reached the end of data and the whole cell is not filled.
                        // Only render the top accumulator
                        buf[(x, area.top() + j)]
                            .set_symbol(
                                self.symbol_for_height(top_two_accumulators.0 * 8 / cell_height),
                            )
                            .set_style(Style::default().fg(*self.get_color(top_two_indexes.0)));
                        break;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case(0, 10)]
    #[case(10, 10)]
    #[case(11, 11)]
    #[case(100, 100)]
    #[case(101, 110)]
    #[case(150, 150)]
    #[case(999, 1000)]
    #[case(1000, 1000)]
    #[case(1001, 1100)]
    #[case(10001, 11000)]
    fn test_round_up_max_coverage(#[case] input: usize, #[case] expected: usize) {
        assert_eq!(round_up_max_coverage(input), expected);
    }

    #[rstest]
    #[case(1, 5, 0, Err(TGVError::ValueError("n_bins is 0".to_string())))]
    #[case(1, 5, 5, Err(TGVError::ValueError("n_bins is greater than the number of bases in the region".to_string())))]
    #[case(5,5, 1, Err(TGVError::ValueError("n_bins is greater than the number of bases in the region".to_string())))]
    #[case(5, 4, 1, Err(TGVError::ValueError("Right is less than left".to_string())))]
    #[case(25398019, 25398025, 3, Ok(vec![(25398019, 25398021), (25398022, 25398023), (25398024, 25398025)]))] // large interger matters here. Using f32 in the function causes problem for large integers.
    #[case(5,10, 1, Ok(vec![(5,10)]))]
    #[case(5, 10, 2, Ok(vec![(5,7), (8,10)]))]
    fn test_get_linear_space_specific_cases(
        #[case] left: u64,
        #[case] right: u64,
        #[case] n_bins: usize,
        #[case] expected: Result<Vec<(u64, u64)>, TGVError>,
    ) {
        let result = get_linear_space(left, right, n_bins);
        match (result, expected) {
            (Ok(result), Ok(expected)) => assert_eq!(result, expected),
            (Err(e), Err(expected)) => {} // OK
            _ => panic!("Unexpected test result"),
        }
    }
}

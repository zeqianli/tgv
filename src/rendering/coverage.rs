use std::usize;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::{Sparkline, Widget},
};

use ratatui::symbols::bar::{Set, NINE_LEVELS};

use crate::{
    alignment::{Alignment, BaseCoverage},
    error::TGVError,
    rendering::Palette,
    states::State,
};

const MIN_AREA_WIDTH: u16 = 2;
const MIN_AREA_HEIGHT: u16 = 1;

/// Render the coverage barplot.
pub fn render_coverage(
    area: &Rect,
    buf: &mut Buffer,
    state: &State,
    palette: &Palette,
) -> Result<(), TGVError> {
    if area.width < MIN_AREA_WIDTH || area.height < MIN_AREA_HEIGHT {
        return Ok(());
    }

    if state.alignment.is_none() {
        return Ok(());
    }

    let alignment = state.alignment.as_ref().unwrap();

    let binned_coverage = calculate_binned_coverage(
        alignment,
        state.window.left(),
        state.window.right(area),
        area.width as usize,
    )?;

    let y_max: usize = round_up_max_coverage(
        binned_coverage
            .iter()
            .max_by_key(|x| x.iter().sum::<usize>())
            .map(|coverage| coverage.iter().sum())
            .unwrap_or(0),
    );

    StackedSparkline::new(
        binned_coverage,
        y_max,
        vec![
            palette.COVERAGE_A,
            palette.COVERAGE_T,
            palette.COVERAGE_C,
            palette.COVERAGE_G,
            palette.COVERAGE_N,
            palette.COVERAGE_SOFTCLIP,
        ],
    )
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
fn get_linear_space(
    left: usize,
    right: usize,
    n_bins: usize,
) -> Result<Vec<(usize, usize)>, TGVError> {
    if n_bins == 0 {
        return Err(TGVError::ValueError("n_bins is 0".to_string()));
    }

    if right <= left {
        return Err(TGVError::ValueError("Right is less than left".to_string()));
    }

    if n_bins > right - left {
        return Err(TGVError::ValueError(
            "n_bins is greater than the number of bases in the region".to_string(),
        ));
    }

    let mut bins: Vec<(usize, usize)> = Vec::new();
    let mut pivot = left as f64; // f32 here actually causes problem for genome coordinates

    let bin_width: f64 = (right - left) as f64 / n_bins as f64;

    for i in 0..n_bins {
        let bin_left = if i == 0 { left } else { pivot as usize + 1 };

        pivot += bin_width;

        let bin_right = if i == n_bins - 1 {
            right
        } else {
            pivot as usize
        };

        bins.push((bin_left, bin_right));
    }

    Ok(bins)
}

/// Calculate the binned coverage in [left_bound, right_bound].
/// 1-based, inclusive.
fn calculate_binned_coverage(
    alignment: &Alignment,
    left: usize,
    right: usize,
    n_bins: usize,
) -> Result<Vec<Vec<usize>>, TGVError> {
    if right < left {
        return Err(TGVError::ValueError("Right is less than left".to_string()));
    }

    if n_bins == 0 {
        return Err(TGVError::ValueError("n_bins is 0".to_string()));
    }

    if (right - left + 1 == n_bins) {
        // 1x zoom. Not need to calulate binned coverage.
        return Ok((left..right + 1)
            .map(|x| {
                let coverage = alignment.coverage_at(x);
                vec![
                    coverage.A,
                    coverage.T,
                    coverage.C,
                    coverage.G,
                    coverage.N,
                    coverage.softclip,
                ]
            })
            .collect::<Vec<Vec<usize>>>());
    }

    let linear_space: Vec<(usize, usize)> = get_linear_space(left, right, n_bins)?;

    let binned_coverage: Vec<Vec<usize>> = linear_space
        .into_iter()
        .map(|(bin_left, bin_right)| {
            let mut data = vec![0, 0, 0, 0, 0, 0];
            (bin_left..bin_right + 1)
                .map(|x| {
                    let coverage = alignment.coverage_at(x);
                    data[0] += coverage.A;
                    data[1] += coverage.T;
                    data[2] += coverage.C;
                    data[3] += coverage.G;
                    data[4] += coverage.N;
                    data[5] += coverage.softclip;
                })
                .collect::<()>();
            data
        })
        .collect();

    Ok(binned_coverage)
}

struct StackedSparkline {
    max: usize,

    data: Vec<Vec<usize>>, // bottom, top
    color: Vec<Color>,     // bottom color, top color

    bar_set: Set,
}

impl StackedSparkline {
    pub fn new(data: Vec<Vec<usize>>, max: usize, color: Vec<Color>) -> Self {
        Self {
            max: max,
            data: data,
            color: color,

            bar_set: NINE_LEVELS,
        }
    }
}

impl StackedSparkline {
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
}

impl Widget for StackedSparkline {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Shout out Ratatui's Sparkline implementation - very elegent!
        // This is very similar.

        if area.is_empty() {
            return;
        }

        // determine the maximum index to render
        let max_index = usize::min(area.width as usize, self.data.len());

        let pixel_height = self.max / area.height as usize;

        // render each item in the data
        for (i, items) in self.data.iter().take(max_index).enumerate() {
            let x = area.left() + i as u16;
            // render from bottom to top (loop is top to bottom)

            if items.is_empty() {
                continue;
            }

            let mut stack_pivot = 0;
            let mut accumulator = items[0]; // accumate un-plotted heights

            for j in (0..area.height).rev() {
                if accumulator >= pixel_height {
                    // render a whole pixel
                    buf[(x, area.top() + j)]
                        .set_symbol(self.bar_set.full)
                        .set_style(Style::default().fg(self.color[stack_pivot]));

                    accumulator -= pixel_height
                } else {
                    // add accumulator until a whole character is filled

                    let fg_height = accumulator;
                    let mut rendered = false;

                    let mut top_two_indexes: (usize, usize) = (stack_pivot, 0);
                    let mut top_two_indexes_accumulators: (usize, usize) = (0, 0);

                    for k in stack_pivot + 1..items.len() {
                        accumulator += items[k];

                        let item = items[k];

                        if item > top_two_indexes.0 {
                            top_two_indexes = (item, top_two_indexes.0);
                            top_two_indexes_accumulators =
                                (accumulator, top_two_indexes_accumulators.0);
                        } else if item > top_two_indexes.1 {
                            top_two_indexes = (top_two_indexes.0, item);
                            top_two_indexes_accumulators = (top_two_indexes_accumulators.0, item);
                        };

                        if accumulator >= pixel_height {
                            // render

                            let (fg_height, fg_color, bg_color) =
                                if (top_two_indexes.0 > top_two_indexes.1) {
                                    // use 1
                                    (
                                        top_two_indexes_accumulators.1 * 8 / pixel_height,
                                        self.color[top_two_indexes.1],
                                        self.color[top_two_indexes.0],
                                    )
                                } else {
                                    (
                                        (top_two_indexes_accumulators.1
                                            - top_two_indexes_accumulators.0)
                                            * 8
                                            / pixel_height,
                                        self.color[top_two_indexes.0],
                                        self.color[top_two_indexes.1],
                                    )
                                };

                            buf[(x, area.top() + j)]
                                .set_symbol(self.symbol_for_height(fg_height))
                                .set_style(Style::default().fg(fg_color).bg(bg_color));

                            accumulator -= pixel_height;
                            stack_pivot = k;
                            rendered = true;
                            break;
                        }
                    }

                    if !rendered {
                        // end of data. render the top accumulator
                        buf[(x, area.top() + j)]
                            .set_symbol(self.symbol_for_height(
                                top_two_indexes_accumulators.0 * 8 / pixel_height,
                            ))
                            .set_style(Style::default().fg(self.color[top_two_indexes.0]));
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
        #[case] left: usize,
        #[case] right: usize,
        #[case] n_bins: usize,
        #[case] expected: Result<Vec<(usize, usize)>, TGVError>,
    ) {
        let result = get_linear_space(left, right, n_bins);
        match (result, expected) {
            (Ok(result), Ok(expected)) => assert_eq!(result, expected),
            (Err(e), Err(expected)) => {} // OK
            _ => panic!("Unexpected test result"),
        }
    }
}

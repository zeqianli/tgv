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
            .max_by_key(|x| x.ATCGN_softclip)
            .map(|coverage| coverage.ATCGN_softclip)
            .unwrap_or(0),
    );

    // stacked bar plot by rendering multiple sparklines. Start with the bottom.
    // TODO
    // For now, use just two colors like IGV.
    // Add more functions in the future?

    // Softclip
    // let data = binned_coverage
    //     .iter()
    //     .map(|coverage| coverage.ATCGN_softclip as u64)
    //     .collect::<Vec<u64>>();
    // Sparkline::default()
    //     .data(&data)
    //     .max(y_max as u64)
    //     .style(Style::default().fg(palette.COVERAGE_SOFTCLIP))
    //     .render(*area, buf);

    // N
    let data = binned_coverage
        .iter()
        .map(|coverage| coverage.ATCGN as u64)
        .collect::<Vec<u64>>();
    Sparkline::default()
        .data(&data)
        .max(y_max as u64)
        .style(Style::default().fg(palette.COVERAGE_TOTAL))
        .render(*area, buf);

    // G
    // let data = binned_coverage
    //     .iter()
    //     .map(|coverage| coverage.ATCG as u64)
    //     .collect::<Vec<u64>>();
    // Sparkline::default()
    //     .data(&data)
    //     .max(y_max as u64)
    //     .style(Style::default().fg(palette.COVERAGE_G))
    //     .render(*area, buf);

    // C
    // let data = binned_coverage
    //     .iter()
    //     .map(|coverage| coverage.ATC as u64)
    //     .collect::<Vec<u64>>();
    // Sparkline::default()
    //     .data(&data)
    //     .max(y_max as u64)
    //     .style(Style::default().fg(palette.COVERAGE_C))
    //     .render(*area, buf);

    // // T
    // let data = binned_coverage
    //     .iter()
    //     .map(|coverage| coverage.AT as u64)
    //     .collect::<Vec<u64>>();
    // Sparkline::default()
    //     .data(&data)
    //     .max(y_max as u64)
    //     .style(Style::default().fg(palette.COVERAGE_T))
    //     .render(*area, buf);

    // // A
    // let data = binned_coverage
    //     .iter()
    //     .map(|coverage| coverage.A as u64)
    //     .collect::<Vec<u64>>();
    // Sparkline::default()
    //     .data(&data)
    //     .max(y_max as u64)
    //     .style(Style::default().fg(palette.COVERAGE_A))
    //     .render(*area, buf);

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
) -> Result<Vec<(usize, usize)>, TGVError> {
    if right < left {
        return Err(TGVError::ValueError("Right is less than left".to_string()));
    }

    if n_bins == 0 {
        return Err(TGVError::ValueError("n_bins is 0".to_string()));
    }

    if (right - left + 1 == n_bins) {
        // 1x zoom. Not need to calulate binned coverage.
        return Ok((left..right + 1)
            .map(|x| CoverageHistogramData::from(alignment.coverage_at(x)))
            .collect());
    }

    let linear_space: Vec<(usize, usize)> = get_linear_space(left, right, n_bins)?;

    let binned_coverage: Vec<CoverageHistogramData> = linear_space
        .into_iter()
        .map(|(bin_left, bin_right)| {
            let mut data = CoverageHistogramData::default();
            (bin_left..bin_right + 1)
                .map(|x| data.add(&CoverageHistogramData::from(alignment.coverage_at(x))))
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

        // render each item in the data
        for (i, items) in self.data.iter().take(max_index).enumerate() {
            let mut heights = items
                .iter()
                .map(|value| *value * usize::from(area.height) * 8 / self.max)
                .collect::<Vec<usize>>();

            let x = area.left() + i as u16;
            // render from bottom to top (loop is top to bottom)

            let stack_pivot = 0;

            for j in (0..area.height).rev() {
                let height = heights[stack_pivot];
                let color = self.color[stack_pivot]; // TODO: ensure that color and data have the  same number of entries

                if height > 0 {
                    // render the current stack

                    let symbol: &str = self.symbol_for_height(height);
                    let style = if height > 8 {
                        height -= 8;
                        Style::default().fg(color)
                    } else {
                        // move stack_pivot until the blank is filled
                        let mut blank_height = 8 - height;
                        heights[stack_pivot] = 0;


                        for new_pivot in stack_pivot+1 .. heights.len() {
                            if blank_height < 
                            
                            
                        }

                        if height_2 >= blank_height {
                            // background: height_2

                            height_2 -= blank_height;
                            Style::default().fg(self.color.0).bg(self.color.1)
                        } else {
                            // if height_2 is not enough to fill the background, don't render.
                            height_2 = 0;
                            Style::default().fg(self.color.0)
                        }
                    };

                    buf[(x, area.top() + j)].set_symbol(symbol).set_style(style);
                } else if height_2 > 0 {
                    let symbol = self.symbol_for_height(height_2);
                    if height_2 > 8 {
                        height_2 -= 8;
                    } else {
                        height_2 = 0;
                    }

                    buf[(x, area.top() + j)]
                        .set_symbol(symbol)
                        .set_style(Style::default().fg(self.color.1));
                } else {
                    break;
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

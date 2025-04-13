use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    widgets::{Sparkline, Widget},
};

use crate::error::TGVError;
use crate::models::alignment::Alignment;
use crate::models::window::ViewingWindow;

const MIN_AREA_WIDTH: u16 = 2;
const MIN_AREA_HEIGHT: u16 = 1;

/// Render the coverage barplot.
pub fn render_coverage(
    area: &Rect,
    buf: &mut Buffer,
    window: &ViewingWindow,
    alignment: &Alignment,
) -> Result<(), TGVError> {
    if area.width < MIN_AREA_WIDTH || area.height < MIN_AREA_HEIGHT {
        return Ok(());
    }

    let binned_coverage = calculate_binned_coverage(
        alignment,
        window.left(),
        window.right(area),
        area.width as usize,
    )?;

    let y_max = round_up_max_coverage(*binned_coverage.iter().max().unwrap_or(&0));

    let sparkline = Sparkline::default().data(&binned_coverage).max(y_max);

    sparkline.render(*area, buf);

    buf.set_string(area.x, area.y, format!("[0-{}]", y_max,), Style::default());

    Ok(())
}

/// Round up the maximum coverage to two significant digits.
fn round_up_max_coverage(x: u64) -> u64 {
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
) -> Result<Vec<u64>, TGVError> {
    if right < left {
        return Err(TGVError::ValueError("Right is less than left".to_string()));
    }

    if n_bins == 0 {
        return Err(TGVError::ValueError("n_bins is 0".to_string()));
    }

    match (right - left + 1).cmp(&n_bins) {
        std::cmp::Ordering::Less => {
            return Err(TGVError::ValueError(
                "n_bins is greater than the number of bases in the region".to_string(),
            ))
        }
        std::cmp::Ordering::Equal => {
            return Ok((left..right + 1)
                .map(|x| alignment.coverage_at(x) as u64)
                .collect());
        }
        std::cmp::Ordering::Greater => {}
    }

    let linear_space: Vec<(usize, usize)> = get_linear_space(left, right, n_bins)?;

    let binned_coverage: Result<Vec<u64>, TGVError> = linear_space
        .iter()
        .map(|(bin_left, bin_right)| {
            if bin_left > bin_right {
                return Err(TGVError::ValueError(
                    "bin_left is greater than bin_right".to_string(),
                ));
            }
            Ok(alignment
                .mean_basewise_coverage_in(*bin_left, *bin_right)
                .unwrap() as u64)
        })
        .collect();

    binned_coverage
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
    fn test_round_up_max_coverage(#[case] input: u64, #[case] expected: u64) {
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
            (Err(e), Err(expected)) => assert!(matches!(e, expected)),
            _ => panic!("Unexpected test result"),
        }
    }
}

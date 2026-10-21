//! Coverage binned to screen units, and the scale of its y-axis.

use gv_core::{
    alignment::{CoverageSchema, CoverageTable},
    prelude::*,
};

const MAX_DISPLAY_ALLELE_FREQUENCY_RECIPROCOL: u64 = 100;

/// Round up the maximum coverage to two significant digits.
pub fn round_up_max_coverage(x: usize) -> usize {
    if x < 10 {
        return 10;
    }
    let mut x = x;
    let mut multiplier = 1;

    let mut round_up = false;
    while x >= 100 {
        if !x.is_multiple_of(10) {
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
/// Returns a vector of n_bins elements.
pub fn get_linear_space(left: u64, right: u64, n_bins: usize) -> Result<Vec<(u64, u64)>, TGVError> {
    if n_bins == 0 {
        return Err(TGVError::ValueError("n_bins is 0".to_string()));
    }

    if right < left {
        return Err(TGVError::ValueError("Right is less than left".to_string()));
    }

    let length = right - left + 1;
    if n_bins as u64 > length {
        // FIXME: make this permissive? Don't wanna crash the app because of a rendering problem.
        // Could happen if the region is weird.
        return Err(TGVError::ValueError(format!(
            "n_bins = {} is greater than the number of bases in the region [{}, {}]",
            n_bins, left, right
        )));
    }

    Ok((0..n_bins)
        .map(|i| {
            let bin_left = left + i as u64 * length / n_bins as u64;
            let bin_right = left + (i as u64 + 1) * length / n_bins as u64 - 1;

            (bin_left, bin_right)
        })
        .collect())
}

/// Calculate the binned coverage in [left_bound, right_bound].
/// 1-based, inclusive.
pub fn calculate_binned_coverage(
    coverage: &CoverageTable,
    left: u64,
    right: u64,
    n_bins: usize,
) -> Result<Vec<Vec<usize>>, TGVError> {
    let linear_space = get_linear_space(left, right, n_bins)?;
    let table = coverage.query(left, right)?;
    let positions = table.column(CoverageSchema::POS)?.u64()?;
    let totals = table.column(CoverageSchema::TOTAL)?.u64()?;
    let a = table.column(CoverageSchema::A)?.u64()?;
    let t = table.column(CoverageSchema::T)?.u64()?;
    let c = table.column(CoverageSchema::C)?.u64()?;
    let reference_bases = table.column(CoverageSchema::REFERENCE_BASE)?.u8()?;
    let mut output = vec![vec![0; n_bins]; 2];
    let single_base_bins = right - left + 1 == n_bins as u64;
    let mut bin = 0;
    for (row, position) in positions.into_no_null_iter().enumerate() {
        while position > linear_space[bin].1 {
            bin += 1;
        }
        let total = totals.get(row).expect("coverage totals are non-null");
        let alt = if single_base_bins && total > 0 {
            let reference = reference_bases
                .get(row)
                .expect("reference bases are non-null");
            let a = a.get(row).expect("coverage counts are non-null");
            let t = t.get(row).expect("coverage counts are non-null");
            let c = c.get(row).expect("coverage counts are non-null");
            let depth = match reference {
                b'A' | b'a' | b'G' | b'g' => c.max(t),
                b'T' | b't' => a.max(c),
                b'C' | b'c' => a.max(t),
                _ => 0,
            };
            if depth * MAX_DISPLAY_ALLELE_FREQUENCY_RECIPROCOL > total {
                depth
            } else {
                0
            }
        } else {
            0
        };
        output[0][bin] += alt as usize;
        output[1][bin] += (total - alt) as usize;
    }

    Ok(output)
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
    #[case(1, 5, 5, Ok(vec![(1,1), (2,2), (3,3), (4,4), (5,5)]))]
    #[case(5,5, 1, Ok(vec![(5,5)]))]
    #[case(5, 4, 1, Err(TGVError::ValueError("Right is less than left".to_string())))]
    #[case(25398019, 25398025, 3, Ok(vec![(25398019, 25398020), (25398021, 25398022), (25398023, 25398025)]))]
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
            (Err(_e), Err(_expected)) => {}
            _ => panic!("Unexpected test result"),
        }
    }
}

use crate::{error::Error, intervals::IntervalSchema};
use polars::prelude::*;

/// Bounds when stacking intervals vertically.
#[derive(Debug, Default)]
pub struct TrackBounds {
    left: Vec<u64>,
    right: Vec<u64>,

    /// The largest left bound across rows.
    max_left: u64,
}

impl TrackBounds {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            left: Vec::with_capacity(capacity),
            right: Vec::with_capacity(capacity),
            max_left: 0,
        }
    }

    /// Open a new row holding [start, end], and return its index.
    pub fn push(&mut self, start: u64, end: u64) -> usize {
        self.left.push(start);
        self.right.push(end);
        self.max_left = self.max_left.max(start);
        self.left.len() - 1
    }

    /// Place [start, end] in the first row it fits before, then in the first row it fits
    /// after, or else in a new row. Returns the row index.
    pub fn place(&mut self, start: u64, end: u64, min_gap: u64) -> usize {
        if end + min_gap < self.max_left
            && let Some(y) = self.left.iter().position(|&left| end + min_gap < left)
        {
            let previous = std::mem::replace(&mut self.left[y], start);
            if previous == self.max_left {
                self.max_left = self.left.iter().copied().max().unwrap_or(0);
            }
            return y;
        }

        if let Some(y) = self.right.iter().position(|&right| start > right + min_gap) {
            self.right[y] = end;
            return y;
        }

        self.push(start, end)
    }
}

pub fn stack_intervals(frame: &DataFrame, min_gap: u64) -> Result<Vec<u64>, Error> {
    let contigs = frame.column(IntervalSchema::CONTIG_INDEX)?.u64()?;
    let starts = frame.column(IntervalSchema::START)?.u64()?;
    let ends = frame.column(IntervalSchema::END)?.u64()?;
    let mut tracks = TrackBounds::default();
    let mut current_contig = None;
    (0..frame.height())
        .map(|row| {
            let start = starts.get(row).ok_or(Error::NullValue {
                column: IntervalSchema::START,
            })?;
            let end = ends.get(row).ok_or(Error::NullValue {
                column: IntervalSchema::END,
            })?;
            let contig = contigs.get(row);
            if current_contig != Some(contig) {
                tracks = TrackBounds::default();
                current_contig = Some(contig);
            }
            Ok(tracks.place(start, end, min_gap) as u64)
        })
        .collect()
}

pub fn with_stacked_rows(frame: DataFrame, min_gap: u64) -> Result<DataFrame, Error> {
    let rows = stack_intervals(&frame, min_gap)?;
    let mut frame = frame;
    frame.with_column(Column::new(IntervalSchema::Y.into(), rows))?;
    Ok(frame)
}

pub fn interval_depth(frame: &DataFrame) -> Result<usize, Error> {
    Ok(frame
        .column(IntervalSchema::Y)?
        .u64()?
        .max()
        .map_or(0, |y| y as usize + 1))
}

pub fn filter_rows(frame: DataFrame, rows: std::ops::Range<u64>) -> Result<DataFrame, Error> {
    Ok(frame
        .lazy()
        .filter(
            col(IntervalSchema::Y)
                .gt_eq(lit(rows.start))
                .and(col(IntervalSchema::Y).lt(lit(rows.end))),
        )
        .collect()?)
}

pub const STACKING_GAP: u64 = 1;

pub fn empty_with_rows(mut frame: DataFrame) -> DataFrame {
    frame
        .with_column(Column::new(IntervalSchema::Y.into(), Vec::<u64>::new()))
        .expect("an empty row column fits an empty frame");
    frame
}

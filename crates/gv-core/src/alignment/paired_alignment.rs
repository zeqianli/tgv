use crate::{
    alignment::{
        alignment::{Alignment, RENDERING_CONTEXT_NOT_CALCULATED},
        read::{AlignedRead, ReadPair, RenderingContext, calculate_paired_context},
    },
    error::TGVError,
    intervals::{GenomeInterval, Region},
    sequence::Sequence,
};
use std::collections::HashMap;

/// State and utilities for paired alignment display.
#[derive(Debug)]
pub struct PairedAlignment {
    /// Read index to mate read index, if present.
    mate_map: Vec<usize>,

    /// Read pairs used when viewing as pairs.
    read_pairs: Vec<ReadPair>,

    /// Pair index to y locations.
    ys: Vec<usize>,

    /// y to pair indexes at y location.
    ys_index: Vec<Vec<usize>>,

    /// Whether to display the pair.
    show_read: Vec<bool>,

    /// Paired rendering contexts.
    rendering_contexts: Vec<Vec<RenderingContext>>,

    pub contig_index: usize,

    /// The left bound of region with complete data.
    /// 1-based, inclusive.
    data_complete_left_bound: u64,

    /// The right bound of region with complete data.
    /// 1-based, inclusive.
    data_complete_right_bound: u64,
}

impl PairedAlignment {
    pub fn new(alignment: &mut Alignment, reference_sequence: &Sequence) -> Result<Self, TGVError> {
        let mate_map = calculate_mate_map(&alignment.reads)?;
        let (data_complete_left_bound, data_complete_right_bound) =
            alignment.data_complete_bounds();
        let mut paired_alignment = Self {
            mate_map,
            read_pairs: Vec::new(),
            ys: Vec::new(),
            ys_index: Vec::new(),
            show_read: Vec::new(),
            rendering_contexts: Vec::new(),
            contig_index: alignment.contig_index,
            data_complete_left_bound,
            data_complete_right_bound,
        };

        paired_alignment.read_pairs = paired_alignment.build_read_pairs(alignment)?;
        paired_alignment.show_read = paired_alignment.build_show_read(alignment)?;
        paired_alignment.ys = paired_alignment.stack_tracks();
        paired_alignment.build_y_index()?;
        paired_alignment.build_rendering_contexts(alignment, reference_sequence)?;

        Ok(paired_alignment)
    }

    /// Check if data in [left, right] is all loaded.
    /// 1-based, inclusive.
    pub fn has_complete_data(&self, region: &Region) -> bool {
        (region.contig_index() == self.contig_index)
            && (region.start() >= self.data_complete_left_bound)
            && (region.end() <= self.data_complete_right_bound)
    }

    /// Return the number of paired alignment tracks.
    pub fn depth(&self) -> usize {
        self.ys_index.len()
    }

    pub fn visible_pairs(&self) -> Result<Vec<(usize, usize)>, TGVError> {
        self.show_read
            .iter()
            .zip(self.ys.iter())
            .enumerate()
            .filter_map(|(pair_index, (show_read, y))| show_read.then_some((pair_index, *y)))
            .map(|(pair_index, y)| {
                self.read_pair(pair_index)?;
                Ok((pair_index, y))
            })
            .collect()
    }

    pub fn pair_rendering_contexts(
        &self,
        pair_index: usize,
    ) -> Result<&[RenderingContext], TGVError> {
        let read_pair = self.read_pair(pair_index)?;
        let context_index =
            self.valid_rendering_context_index(read_pair.rendering_context_index)?;

        Ok(&self.rendering_contexts[context_index])
    }

    pub fn read_at<'a>(
        &self,
        reads: &'a [AlignedRead],
        x: u64,
        y: usize,
    ) -> Option<&'a AlignedRead> {
        self.ys_index
            .get(y)?
            .iter()
            .find_map(|pair_index| self.pair_read_at(reads, *pair_index, x))
    }

    pub fn read_overlapping<'a>(
        &self,
        reads: &'a [AlignedRead],
        left: u64,
        right: u64,
        y: usize,
    ) -> Option<&'a AlignedRead> {
        self.ys_index
            .get(y)?
            .iter()
            .find_map(|pair_index| self.pair_read_overlapping(reads, *pair_index, left, right))
    }

    fn build_show_read(&self, alignment: &Alignment) -> Result<Vec<bool>, TGVError> {
        self.read_pairs
            .iter()
            .map(|read_pair| self.pair_is_visible(alignment, read_pair))
            .collect()
    }

    fn build_y_index(&mut self) -> Result<(), TGVError> {
        let mut ys_index = vec![Vec::new(); *self.ys.iter().max().unwrap_or(&0) + 1];
        for (pair_index, (y, show_read)) in self.ys.iter().zip(self.show_read.iter()).enumerate() {
            if *show_read {
                ys_index[*y].push(pair_index);
            }
        }
        self.ys_index = ys_index;

        Ok(())
    }

    fn build_rendering_contexts(
        &mut self,
        alignment: &mut Alignment,
        reference_sequence: &Sequence,
    ) -> Result<(), TGVError> {
        for pair_index in 0..self.read_pairs.len() {
            let context_index =
                self.build_pair_rendering_context(alignment, pair_index, reference_sequence)?;
            let context_index_u64 = u64::try_from(context_index).map_err(|_| {
                TGVError::StateError(
                    "Rendering context cache index does not fit in u64.".to_string(),
                )
            })?;
            self.read_pair_mut(pair_index)?.rendering_context_index = context_index_u64;
        }

        Ok(())
    }

    fn build_pair_rendering_context(
        &mut self,
        alignment: &mut Alignment,
        pair_index: usize,
        reference_sequence: &Sequence,
    ) -> Result<usize, TGVError> {
        let (read_1_index, read_2_index) = {
            let read_pair = self.read_pair(pair_index)?;
            (read_pair.read_1_index, read_pair.read_2_index)
        };

        let read_1_context_index =
            alignment.ensure_read_rendering_context(read_1_index, reference_sequence)?;
        let read_1_contexts = alignment.rendering_contexts[read_1_context_index].clone();
        let contexts = match read_2_index {
            Some(read_2_index) => {
                let read_2_context_index =
                    alignment.ensure_read_rendering_context(read_2_index, reference_sequence)?;
                calculate_paired_context(
                    read_1_contexts,
                    alignment.rendering_contexts[read_2_context_index].clone(),
                )
            }
            None => read_1_contexts,
        };

        self.push_rendering_contexts(contexts)
    }

    fn build_read_pairs(&self, alignment: &Alignment) -> Result<Vec<ReadPair>, TGVError> {
        let mate_not_found_flag = self.mate_map.len();
        let mut read_pairs = Vec::new();
        let mut read_index_is_built = vec![false; alignment.reads.len()];

        // FIXME: all these scenarios display a read alone with the same color:
        // - Not paired.
        // - Paired, but the mate is not loaded.
        // - Supplementary alignment.
        // - Secondary alignment.
        // Introduce some option, for example, coloring, to separate these scenarios.

        for (i, read) in alignment.reads.iter().enumerate() {
            if read_index_is_built[i] {
                continue;
            }
            if read.show_as_pair() {
                let mate_index = *self.mate_map.get(i).ok_or_else(|| {
                    TGVError::StateError(format!(
                        "Mate index out of bounds while building read pairs: {i}"
                    ))
                })?;
                if mate_index == mate_not_found_flag {
                    read_pairs.push(Self::make_read_pair(alignment, i, None)?);
                    read_index_is_built[i] = true;
                } else {
                    if mate_index >= alignment.reads.len() {
                        return Err(TGVError::StateError(format!(
                            "Mate index out of bounds while building read pairs: {mate_index}"
                        )));
                    }
                    read_pairs.push(Self::make_read_pair(alignment, i, Some(mate_index))?);
                    read_index_is_built[i] = true;
                    read_index_is_built[mate_index] = true;
                }
            } else {
                read_pairs.push(Self::make_read_pair(alignment, i, None)?);
                read_index_is_built[i] = true;
            };
        }

        Ok(read_pairs)
    }

    fn make_read_pair(
        alignment: &Alignment,
        read_index_1: usize,
        read_index_2: Option<usize>,
    ) -> Result<ReadPair, TGVError> {
        match read_index_2 {
            Some(read_index_2) => {
                let read_1 = alignment.reads.get(read_index_1).ok_or_else(|| {
                    TGVError::StateError(format!(
                        "Read index out of bounds while building read pairs: {read_index_1}"
                    ))
                })?;
                let read_2 = alignment.reads.get(read_index_2).ok_or_else(|| {
                    TGVError::StateError(format!(
                        "Read index out of bounds while building read pairs: {read_index_2}"
                    ))
                })?;

                let stacking_start = u64::min(read_1.stacking_start(), read_2.stacking_start());
                let stacking_end = u64::max(read_1.stacking_end(), read_2.stacking_end());

                Ok(ReadPair {
                    read_1_index: read_index_1,
                    read_2_index: Some(read_index_2),
                    stacking_start,
                    stacking_end,
                    rendering_context_index: RENDERING_CONTEXT_NOT_CALCULATED,
                })
            }
            None => {
                let read = alignment.reads.get(read_index_1).ok_or_else(|| {
                    TGVError::StateError(format!(
                        "Read index out of bounds while building read pairs: {read_index_1}"
                    ))
                })?;
                Ok(ReadPair {
                    read_1_index: read_index_1,
                    read_2_index: None,
                    stacking_start: read.stacking_start(),
                    stacking_end: read.stacking_end(),
                    rendering_context_index: RENDERING_CONTEXT_NOT_CALCULATED,
                })
            }
        }
    }

    fn stack_tracks(&self) -> Vec<usize> {
        let mut track_left_bounds: Vec<u64> = Vec::new();
        let mut track_right_bounds: Vec<u64> = Vec::new();

        self.read_pairs
            .iter()
            .zip(self.show_read.iter())
            .map(|(read_pair, show_read)| {
                if *show_read {
                    find_track(
                        read_pair.stacking_start,
                        read_pair.stacking_end,
                        &mut track_left_bounds,
                        &mut track_right_bounds,
                        10,
                    )
                } else {
                    0
                }
            })
            .collect()
    }

    fn pair_is_visible(
        &self,
        alignment: &Alignment,
        read_pair: &ReadPair,
    ) -> Result<bool, TGVError> {
        let read_1_is_visible = alignment.read_is_visible(read_pair.read_1_index)?;
        let read_2_is_visible = read_pair
            .read_2_index
            .map(|read_index| alignment.read_is_visible(read_index))
            .transpose()?
            .unwrap_or(false);

        Ok(read_1_is_visible || read_2_is_visible)
    }

    fn pair_read_at<'a>(
        &self,
        reads: &'a [AlignedRead],
        pair_index: usize,
        x: u64,
    ) -> Option<&'a AlignedRead> {
        let read_pair = self.read_pairs.get(pair_index)?;
        let read_1 = reads.get(read_pair.read_1_index)?;
        if read_1.full_read_covers(x) {
            return Some(read_1);
        }

        read_pair
            .read_2_index
            .and_then(|read_2_index| reads.get(read_2_index))
            .filter(|read_2| read_2.full_read_covers(x))
    }

    fn pair_read_overlapping<'a>(
        &self,
        reads: &'a [AlignedRead],
        pair_index: usize,
        left: u64,
        right: u64,
    ) -> Option<&'a AlignedRead> {
        let read_pair = self.read_pairs.get(pair_index)?;
        let read_1 = reads.get(read_pair.read_1_index)?;
        if read_1.full_read_overlaps(left, right) {
            return Some(read_1);
        }

        read_pair
            .read_2_index
            .and_then(|read_2_index| reads.get(read_2_index))
            .filter(|read_2| read_2.full_read_overlaps(left, right))
    }

    fn read_pair(&self, pair_index: usize) -> Result<&ReadPair, TGVError> {
        self.read_pairs.get(pair_index).ok_or_else(|| {
            TGVError::StateError(format!(
                "Read pair index out of bounds while accessing read pair: {pair_index}"
            ))
        })
    }

    fn read_pair_mut(&mut self, pair_index: usize) -> Result<&mut ReadPair, TGVError> {
        self.read_pairs.get_mut(pair_index).ok_or_else(|| {
            TGVError::StateError(format!(
                "Read pair index out of bounds while accessing read pair: {pair_index}"
            ))
        })
    }

    fn push_rendering_contexts(
        &mut self,
        contexts: Vec<RenderingContext>,
    ) -> Result<usize, TGVError> {
        let context_index = self.rendering_contexts.len();
        u64::try_from(context_index).map_err(|_| {
            TGVError::StateError("Rendering context cache index does not fit in u64.".to_string())
        })?;
        self.rendering_contexts.push(contexts);
        Ok(context_index)
    }

    fn valid_rendering_context_index(&self, context_index: u64) -> Result<usize, TGVError> {
        if context_index == RENDERING_CONTEXT_NOT_CALCULATED {
            return Err(TGVError::StateError(
                "Read pair rendering context is not calculated.".to_string(),
            ));
        }

        let context_index = usize::try_from(context_index).map_err(|_| {
            TGVError::StateError("Rendering context cache index does not fit in usize.".to_string())
        })?;
        if context_index >= self.rendering_contexts.len() {
            return Err(TGVError::StateError(format!(
                "Rendering context cache index out of bounds: {context_index}"
            )));
        }
        Ok(context_index)
    }
}

pub fn calculate_mate_map(reads: &Vec<AlignedRead>) -> Result<Vec<usize>, TGVError> {
    let mut read_id_map = HashMap::<Vec<u8>, usize>::new();

    let mut output = vec![reads.len(); reads.len()];

    for (i, read) in reads.iter().enumerate() {
        if read.show_as_pair() {
            if let Some(read_name) = read.record.name() {
                let read_name = read_name.to_vec();
                match read_id_map.remove(&read_name) {
                    Some(mate_index) => {
                        output[i] = mate_index;
                        output[mate_index] = i;
                    }
                    _ => {
                        read_id_map.insert(read_name, i);
                    }
                }
            };
        }
    }

    Ok(output)
}

fn find_track(
    start: u64,
    end: u64,
    track_left_bounds: &mut Vec<u64>,
    track_right_bounds: &mut Vec<u64>,
    min_gap: u64,
) -> usize {
    for (y, left_bound) in track_left_bounds.iter_mut().enumerate() {
        if end + min_gap < *left_bound {
            *left_bound = start;

            return y;
        }
    }

    for (y, right_bound) in track_right_bounds.iter_mut().enumerate() {
        if start > *right_bound + min_gap {
            *right_bound = end;
            return y;
        }
    }

    track_left_bounds.push(start);
    track_right_bounds.push(end);
    track_left_bounds.len()
}

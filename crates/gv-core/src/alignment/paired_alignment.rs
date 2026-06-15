use crate::{
    alignment::{
        alignment::{Alignment, RENDERING_CONTEXT_NOT_CALCULATED, find_track},
        read::{AlignedRead, ReadPair, RenderingContext, calculate_paired_context},
    },
    error::TGVError,
    sequence::Sequence,
};
use std::collections::HashMap;

/// State and utilities for paired alignment display.
#[derive(Debug)]
pub struct PairedAlignment {
    /// Read index to mate read index, if present.
    mate_map: Vec<usize>,

    /// Read pairs used when viewing as pairs.
    pub read_pairs: Vec<ReadPair>,

    /// Paired rendering contexts.
    pub rendering_contexts: Vec<Vec<RenderingContext>>,

    /// Same length as read_pairs. read_index -> index in rendering_context.
    /// If not yet calculated, use u64::MAX
    pair_rendering_context_index: Vec<u64>,

    /// Pair index to y locations.
    pub ys: Vec<usize>,

    /// y to pair indexes at y location.
    pub ys_index: Vec<Vec<usize>>,

    /// Whether to display the pair.
    pub show_pair: Vec<bool>,
}

impl PairedAlignment {
    pub fn new(alignment: &Alignment) -> Result<Self, TGVError> {
        let mate_map = calculate_mate_map(&alignment.reads)?;
        let read_pairs = build_read_pairs(alignment, &mate_map)?;
        let n_pair = read_pairs.len();
        let show_pair = vec![true; n_pair];
        let ys = stack_tracks_for_pairs(&alignment.reads, &read_pairs, &show_pair);

        let mut paired_alignment = Self {
            mate_map,
            read_pairs,
            show_pair,
            rendering_contexts: Vec::new(),
            pair_rendering_context_index: vec![RENDERING_CONTEXT_NOT_CALCULATED; n_pair],
            ys,
            ys_index: Vec::new(),
        };

        paired_alignment.ys = stack_tracks_for_pairs(
            &alignment.reads,
            &paired_alignment.read_pairs,
            &paired_alignment.show_pair,
        );
        paired_alignment.build_y_index()?;

        Ok(paired_alignment)
    }

    /// Return the number of paired alignment tracks.
    pub fn depth(&self) -> usize {
        self.ys_index.len()
    }

    pub fn pair_overlapping(
        &self,
        reads: &[AlignedRead],
        left: u64,
        right: u64,
        y: usize,
    ) -> Option<&ReadPair> {
        if y >= self.depth() {
            return None;
        }

        self.ys_index[y]
            .iter()
            .find(|i_pair| self.read_pairs[**i_pair].full_pair_overlaps(reads, left, right))
            .map(|index| &self.read_pairs[*index])
    }

    /// If rendering context is calculated for read_index, return the rendering context index in self.rendering_contexts
    /// Return None if not yet calculated.
    pub fn get_pair_rendering_context_index(&self, pair_index: usize) -> Option<u64> {
        match self.pair_rendering_context_index[pair_index] {
            RENDERING_CONTEXT_NOT_CALCULATED => None,
            i => Some(i),
        }
    }

    /// Calculate and write rendering context for read_index.
    /// The new context is added to the end of the context vector.
    /// Returns the index of the new contexts.
    pub fn calculate_pair_rendering_context(
        &mut self,
        alignment: &mut Alignment,
        pair_index: usize,
        reference_sequence: &Sequence,
    ) -> Result<u64, TGVError> {
        let pair = &self.read_pairs[pair_index];

        let read_1_context_index =
            if let Some(context_index) = alignment.get_rendering_context_index(pair.read_1_index) {
                context_index
            } else {
                alignment.calculate_read_rendering_context(pair.read_1_index, reference_sequence)?
            };
        let read_2_context_index = if let Some(read_2_index) = pair.read_2_index {
            if let Some(context_index) = alignment.get_rendering_context_index(read_2_index) {
                Some(context_index)
            } else {
                Some(alignment.calculate_read_rendering_context(read_2_index, reference_sequence)?)
            }
        } else {
            None
        };

        self.rendering_contexts.push(calculate_paired_context(
            &alignment.rendering_contexts[read_1_context_index as usize],
            read_2_context_index.map(|i| &alignment.rendering_contexts[i as usize]),
        ));
        let rendering_context_index = (self.rendering_contexts.len() - 1) as u64;
        self.pair_rendering_context_index[pair_index] = rendering_context_index;

        Ok(rendering_context_index)
    }

    fn build_y_index(&mut self) -> Result<(), TGVError> {
        let mut ys_index = vec![Vec::new(); *self.ys.iter().max().unwrap_or(&0) + 1];
        for (pair_index, (y, show_pair)) in self.ys.iter().zip(self.show_pair.iter()).enumerate() {
            if *show_pair {
                ys_index[*y].push(pair_index);
            }
        }
        self.ys_index = ys_index;

        Ok(())
    }
}

pub fn calculate_mate_map(reads: &Vec<AlignedRead>) -> Result<Vec<usize>, TGVError> {
    let mut read_id_map = HashMap::<Vec<u8>, usize>::new();

    let mut output = vec![reads.len(); reads.len()];

    for (i, read) in reads.iter().enumerate() {
        if read.show_as_pair()
            && let Some(read_name) = read.record.name()
        {
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

    Ok(output)
}

fn build_read_pairs(
    alignment: &Alignment,
    mate_map: &Vec<usize>,
) -> Result<Vec<ReadPair>, TGVError> {
    let mate_not_found_flag = mate_map.len();
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
            let mate_index = *mate_map.get(i).ok_or_else(|| {
                TGVError::StateError(format!(
                    "Mate index out of bounds while building read pairs: {i}"
                ))
            })?;
            if mate_index == mate_not_found_flag {
                read_pairs.push(ReadPair {
                    read_1_index: i,
                    read_2_index: None,
                });
                read_index_is_built[i] = true;
            } else {
                if mate_index >= alignment.reads.len() {
                    return Err(TGVError::StateError(format!(
                        "Mate index out of bounds while building read pairs: {mate_index}"
                    )));
                }
                read_pairs.push(ReadPair {
                    read_1_index: i,
                    read_2_index: Some(mate_index),
                });
                read_index_is_built[i] = true;
                read_index_is_built[mate_index] = true;
            }
        } else {
            read_pairs.push(ReadPair {
                read_1_index: i,
                read_2_index: None,
            });
            read_index_is_built[i] = true;
        };
    }

    Ok(read_pairs)
}

fn stack_tracks_for_pairs(
    read: &[AlignedRead],
    read_pairs: &[ReadPair],
    show_pairs: &[bool],
) -> Vec<usize> {
    let mut track_left_bounds: Vec<u64> = Vec::new();
    let mut track_right_bounds: Vec<u64> = Vec::new();

    read_pairs
        .iter()
        .zip(show_pairs.iter())
        .map(|(read_pair, show_pair)| {
            if *show_pair {
                find_track(
                    read_pair.stacking_start(read),
                    read_pair.stacking_end(read),
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

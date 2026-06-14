use crate::{
    alignment::{
        alignment::{Alignment, RENDERING_CONTEXT_NOT_CALCULATED, find_track},
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

    /// Paired rendering contexts.
    rendering_contexts: Vec<Vec<RenderingContext>>,

    /// Same length as read_pairs. read_index -> index in rendering_context.
    /// If not yet calculated, use u64::MAX
    pair_rendering_context_index: Vec<u64>,

    /// Pair index to y locations.
    ys: Vec<usize>,

    /// y to pair indexes at y location.
    ys_index: Vec<Vec<usize>>,

    /// Whether to display the pair.
    show_pair: Vec<bool>,
}

impl PairedAlignment {
    pub fn new(alignment: &mut Alignment, reference_sequence: &Sequence) -> Result<Self, TGVError> {
        let mate_map = calculate_mate_map(&alignment.reads)?;
        let read_pairs = paired_alignment.build_read_pairs(alignment)?;
        let n_pair = read_pairs.len();
        let show_pair =  vec![true; n_pair]
        let ys = stack_tracks_for_pairs(&alignment.reads, &read_pairs, &show_pair);

        let mut paired_alignment = Self {
            mate_map,
            read_pairs: read_pairs,
            show_pair: show_pair,
            rendering_contexts: Vec::new(),
            pair_rendering_context_index: vec![RENDERING_CONTEXT_NOT_CALCULATED; n_pair],
            ys: Vec::new(),
            ys_index: Vec::new(),
        };

        paired_alignment.ys = paired_alignment.stack_tracks();
        paired_alignment.build_y_index()?;
        paired_alignment.build_rendering_contexts(alignment, reference_sequence)?;

        // fn build_rendering_contexts(
        //     &mut self,
        //     alignment: &mut Alignment,
        //     reference_sequence: &Sequence,
        // ) -> Result<(), TGVError> {
        //     for pair_index in 0..self.read_pairs.len() {
        //         let context_index = {
        //             let (read_1_index, read_2_index) = {
        //                 let read_pair = self.read_pair(pair_index)?;
        //                 (read_pair.read_1_index, read_pair.read_2_index)
        //             };

        //             let read_1_context_index =
        //                 alignment.ensure_read_rendering_context(read_1_index, reference_sequence)?;
        //             let read_1_contexts = alignment.rendering_contexts[read_1_context_index].clone();
        //             let contexts = match read_2_index {
        //                 Some(read_2_index) => {
        //                     let read_2_context_index =
        //                         alignment.ensure_read_rendering_context(read_2_index, reference_sequence)?;
        //                     calculate_paired_context(
        //                         read_1_contexts,
        //                         alignment.rendering_contexts[read_2_context_index].clone(),
        //                     )
        //                 }
        //                 None => read_1_contexts,
        //             };

        //             self.push_rendering_contexts(contexts)}
        //         let context_index_u64 = u64::try_from(context_index).map_err(|_| {
        //             TGVError::StateError(
        //                 "Rendering context cache index does not fit in u64.".to_string(),
        //             )
        //         })?;
        //         self.read_pair[pair_index].rendering_context_index = context_index_u64;
        //     }

        //     Ok(())
        // }

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
            .find(|i_pair| self.read_pairs[**i_pair].full_pair_overlaps(&reads, left, right))
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

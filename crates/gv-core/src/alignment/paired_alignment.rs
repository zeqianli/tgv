use crate::error::TGVError;
use crate::intervals::{GenomeInterval, Region};
use crate::message::{AlignmentFilter, AlignmentSort};
use crate::sequence::Sequence;
use crate::{
    alignment::{
        alignment::Alignment,
        coverage::{BaseCoverage, DEFAULT_COVERAGE, calculate_basewise_coverage},
        read::{
            AlignedRead, ReadPair, RenderingContext, calculate_paired_context,
            calculate_rendering_contexts,
        },
    },
    message::AlignmentDisplayOption,
};
use std::collections::{BTreeMap, HashMap, hash_map::Entry};
/// State and utilities for paired alignment display.
#[derive(Debug)]
pub struct PairedAlignment {
    /// Read index to mate read index, if present.
    mate_map: Vec<usize>,

    /// Read pairs used when viewing as pairs.
    read_pairs: Vec<ReadPair>,
}

impl PairedAlignment {
    pub fn new(alignment: &Alignment) -> Result<Self, TGVError> {
        let mate_map = calculate_mate_map(&alignment.reads)?;
        let mut paired_alignment = Self {
            mate_map,
            read_pairs: Vec::new(),
        };
        paired_alignment.read_pairs = paired_alignment.build_read_pairs(alignment)?;

        Ok(paired_alignment)
    }

    pub fn visible_pairs(&self, alignment: &Alignment) -> Result<Vec<(usize, usize)>, TGVError> {
        let mut visible_pairs = Vec::new();

        for (pair_index, read_pair) in self.read_pairs.iter().enumerate() {
            if !self.pair_is_visible(alignment, read_pair)? {
                continue;
            }

            let y = alignment
                .ys
                .get(read_pair.read_1_index)
                .copied()
                .ok_or_else(|| {
                    TGVError::StateError(format!(
                        "Read index out of bounds while rendering read pairs: {}",
                        read_pair.read_1_index
                    ))
                })?;
            visible_pairs.push((pair_index, y));
        }

        Ok(visible_pairs)
    }

    fn y_coordinates(&self, alignment: &Alignment) -> Result<Vec<usize>, TGVError> {
        let paired_ys = self.stack_tracks(alignment)?;
        let mut ys = vec![0; alignment.reads.len()];

        for (pair, y) in self.read_pairs.iter().zip(paired_ys) {
            Self::validate_read_index(pair.read_1_index, alignment)?;
            ys[pair.read_1_index] = y;
            if let Some(read_2_index) = pair.read_2_index {
                Self::validate_read_index(read_2_index, alignment)?;
                ys[read_2_index] = y;
            }
        }

        Ok(ys)
    }

    fn ensure_pair_rendering_context(
        &mut self,
        alignment: &mut Alignment,
        pair_index: usize,
        reference_sequence: &Sequence,
    ) -> Result<usize, TGVError> {
        let (context_index, read_1_index, read_2_index) = {
            let read_pair = self.read_pair(pair_index)?;
            (
                read_pair.rendering_context_index,
                read_pair.read_1_index,
                read_pair.read_2_index,
            )
        };

        if context_index != RENDERING_CONTEXT_NOT_CALCULATED {
            return alignment.valid_rendering_context_index(context_index);
        }

        let read_1_context_index =
            alignment.ensure_read_rendering_context(read_1_index, reference_sequence)?;
        let context_index = match read_2_index {
            Some(read_2_index) => {
                let read_2_context_index =
                    alignment.ensure_read_rendering_context(read_2_index, reference_sequence)?;
                let contexts = calculate_paired_context(
                    alignment.rendering_contexts[read_1_context_index].clone(),
                    alignment.rendering_contexts[read_2_context_index].clone(),
                );
                alignment.push_rendering_contexts(contexts)?
            }
            None => read_1_context_index,
        };

        let context_index_u64 = u64::try_from(context_index).map_err(|_| {
            TGVError::StateError("Rendering context cache index does not fit in u64.".to_string())
        })?;
        self.read_pair_mut(pair_index)?.rendering_context_index = context_index_u64;

        Ok(context_index)
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
                    read_pairs.push(Self::make_read_pair(alignment, read_pairs.len(), i, None)?);
                    read_index_is_built[i] = true;
                } else {
                    if mate_index >= alignment.reads.len() {
                        return Err(TGVError::StateError(format!(
                            "Mate index out of bounds while building read pairs: {mate_index}"
                        )));
                    }
                    read_pairs.push(Self::make_read_pair(
                        alignment,
                        read_pairs.len(),
                        i,
                        Some(mate_index),
                    )?);
                    read_index_is_built[i] = true;
                    read_index_is_built[mate_index] = true;
                }
            } else {
                read_pairs.push(Self::make_read_pair(alignment, read_pairs.len(), i, None)?);
                read_index_is_built[i] = true;
            };
        }

        Ok(read_pairs)
    }

    fn make_read_pair(
        alignment: &Alignment,
        pair_index: usize,
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
                    index: pair_index,
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
                    index: pair_index,
                    rendering_context_index: RENDERING_CONTEXT_NOT_CALCULATED,
                })
            }
        }
    }

    fn stack_tracks(&self, alignment: &Alignment) -> Result<Vec<usize>, TGVError> {
        let mut track_left_bounds: Vec<u64> = Vec::new();
        let mut track_right_bounds: Vec<u64> = Vec::new();
        let mut ys = Vec::with_capacity(self.read_pairs.len());

        for read_pair in &self.read_pairs {
            let y = if self.pair_is_visible(alignment, read_pair)? {
                find_track(
                    read_pair.stacking_start,
                    read_pair.stacking_end,
                    &mut track_left_bounds,
                    &mut track_right_bounds,
                    10,
                )
            } else {
                0
            };
            ys.push(y);
        }

        Ok(ys)
    }

    fn pair_is_visible(
        &self,
        alignment: &Alignment,
        read_pair: &ReadPair,
    ) -> Result<bool, TGVError> {
        let read_1_is_visible = Self::read_is_visible(alignment, read_pair.read_1_index)?;
        let read_2_is_visible = read_pair
            .read_2_index
            .map(|read_index| Self::read_is_visible(alignment, read_index))
            .transpose()?
            .unwrap_or(false);

        Ok(read_1_is_visible || read_2_is_visible)
    }

    fn read_pair(&self, pair_index: usize) -> Result<&ReadPair, TGVError> {
        self.read_pairs.get(pair_index).ok_or_else(|| {
            TGVError::StateError(format!(
                "Read pair index out of bounds while building rendering context: {pair_index}"
            ))
        })
    }

    fn read_pair_mut(&mut self, pair_index: usize) -> Result<&mut ReadPair, TGVError> {
        self.read_pairs.get_mut(pair_index).ok_or_else(|| {
            TGVError::StateError(format!(
                "Read pair index out of bounds while building rendering context: {pair_index}"
            ))
        })
    }

    fn read_is_visible(alignment: &Alignment, read_index: usize) -> Result<bool, TGVError> {
        alignment.show_read.get(read_index).copied().ok_or_else(|| {
            TGVError::StateError(format!(
                "Read index out of bounds while checking pair visibility: {read_index}"
            ))
        })
    }

    fn validate_read_index(read_index: usize, alignment: &Alignment) -> Result<(), TGVError> {
        if read_index >= alignment.reads.len() {
            return Err(TGVError::StateError(format!(
                "Read index out of bounds while stacking read pairs: {read_index}"
            )));
        }

        Ok(())
    }

    pub fn pair_rendering_contexts(
        &mut self,
        pair_index: usize,
        reference_sequence: &Sequence,
    ) -> Result<&[RenderingContext], TGVError> {
        let mut paired_alignment = self.paired_alignment.take().ok_or_else(|| {
            TGVError::StateError("Read pairs are not calculated before rendering.".to_string())
        })?;
        let context_index =
            paired_alignment.ensure_pair_rendering_context(self, pair_index, reference_sequence);
        self.paired_alignment = Some(paired_alignment);
        let context_index = context_index?;

        Ok(&self.rendering_contexts[context_index])
    }

    pub fn visible_read_pairs(&self) -> Result<Vec<(usize, usize)>, TGVError> {
        let paired_alignment = self.paired_alignment.as_ref().ok_or_else(|| {
            TGVError::StateError("Read pairs are not calculated before rendering.".to_string())
        })?;

        paired_alignment.visible_pairs(self)
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

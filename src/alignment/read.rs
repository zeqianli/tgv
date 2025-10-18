use crate::error::TGVError;
use crate::message::AlignmentFilter;
use crate::sequence::Sequence;
// use rust_htslib::bam::{record::Seq, Read, Record};
//
use itertools::Itertools;
use noodles::bam::record::{self, Cigar, Record};
use noodles::sam::alignment::record::{
    cigar::{op::Kind, Op},
    data::field::tag::Tag,
    Cigar as CigarTrait, Flags,
};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenderingContextModifier {
    /// Annoate the forward arrow at the end.
    Forward,

    /// Annotate the reverse arrow at the beginning.
    Reverse,

    /// The previous cigar is an insertion. Annotate this at the beginning of this segment.
    Insertion(usize),

    /// Mismatch at location with base
    Mismatch(usize, u8),

    /// Pair overlaps and have differnet RenderingContextKind
    /// (except Softclip + Match: softclip is displayed in this case (same as IGV))
    PairConflict(usize),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenderingContextKind {
    SoftClip(u8),

    Match, // Mismatches are annotated with modifiers

    Deletion,

    /// Gap between a read pair
    PairGap,
    /// Overlaps of a read pair
    PairOverlap,
}
/// Information on how to display the read on screen. Each context represent a segment on screen.
/// Parsed from the cigar string.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderingContext {
    /// Start coordinate of a displayed segment, 1-based
    pub start: usize,

    /// End coordinates of a displayed segment, 1-based, inclusive
    pub end: usize,

    /// The renderer will decide style based on the cigar segment kind.
    pub kind: RenderingContextKind,

    /// Mismatches, insertions, arrows, etc
    pub modifiers: Vec<RenderingContextModifier>,
}

impl RenderingContext {
    fn add_modifier(&mut self, modifier: RenderingContextModifier) {
        self.modifiers.push(modifier)
    }

    fn len(&self) -> usize {
        self.end - self.start + 1
    }
}

#[derive(Clone, Debug)]
/// An aligned read with viewing coordinates.
pub struct AlignedRead {
    /// Alignment record data
    pub read: Record,

    /// Non-clipped start genome coordinate on the alignment view
    /// 1-based, inclusive
    pub start: usize,
    /// Non-clipped end genome coordinate on the alignment view
    /// Note that this includes the soft-clipped reads and differ from the built-in methods. TODO
    /// 1-based, inclusive
    pub end: usize,

    /// Leading softclips. Used for track stacking calculation.
    pub leading_softclips: usize,

    /// Trailing softclips. Used for track stacking calculation.
    pub trailing_softclips: usize,

    pub cigar: Vec<Op>,

    pub flags: Flags,

    /// index in the alignment read array.
    pub index: usize,

    /// Base mismatches with the reference
    pub rendering_contexts: Vec<RenderingContext>,
}

#[derive(Clone, Debug)]
pub struct ReadPair {
    /// Read 1 index in the alignment
    pub read_1_index: usize,

    /// If some: Read 2 index in the alignment
    /// if none: Read not shown as paired
    pub read_2_index: Option<usize>,

    /// 1-based start (including soft-clips)
    pub stacking_start: usize,

    /// 1-based end (including soft-clips)
    pub stacking_end: usize,

    /// Index in the alignment
    pub index: usize,

    /// The paired rendering contexts
    pub rendering_contexts: Vec<RenderingContext>,
}

impl AlignedRead {
    pub fn stacking_start(&self) -> usize {
        usize::max(self.start.saturating_sub(self.leading_softclips), 1)
    }

    pub fn stacking_end(&self) -> usize {
        self.end.saturating_add(self.trailing_softclips)
    }

    /// Read details
    pub fn describe(&self) -> Result<String, TGVError> {
        // FIXME: improve display information
        // Example IGV display:
        // Read name = HISEQ1:29:HA2WPADXX:1:1216:5183:9385
        // Read length = 148bp
        // Flags = 147
        // ----------------------
        // Mapping = Primary @ MAPQ 70
        // Reference span = chr20:78,203-78,350 (-) = 148bp
        // Cigar = 148M
        // Clipping = None
        // ----------------------
        // Mate is mapped = yes
        // Mate start = chr20:77619 (+)
        // Insert size = -731
        // Second in pair
        // Pair orientation = F1R2
        // ----------------------
        // PG = novoalign
        // AM = 70
        // NM = 0
        // SM = 70
        // PQ = 5
        // UQ = 0
        // AS = 0
        // Hidden tags: MDLocation = chr20:78,249
        // Base = C @ QV 30
        Ok(format!(
            "{}  Flags={:?}  Start={}  MAPQ={}  Cigar={:?}", // TODO
            self.read.name().unwrap().to_string(),
            self.flags,
            self.start,
            self.read.mapping_quality().unwrap().get(),
            self.cigar
        ))
    }

    /// Whether the alignment segment (excluding softclips) covers a x_coordinate (1-based).
    pub fn covers(&self, x_coordinate: usize) -> bool {
        self.start <= x_coordinate && self.end >= x_coordinate
    }
    /// Whether the alignment segment (including softclips) covers a x_coordinate (1-based).
    pub fn full_read_covers(&self, x_coordinate: usize) -> bool {
        self.stacking_start() <= x_coordinate && self.stacking_end() >= x_coordinate
    }

    /// Whether the alignment segment (excluding softclips) covers a x_coordinate (1-based).
    pub fn overlaps(&self, x_left_coordinate: usize, x_right_coordinate: usize) -> bool {
        self.start <= x_right_coordinate && self.end >= x_left_coordinate
    }
    /// Whether the alignment segment (including softclips) covers a x_coordinate (1-based).
    pub fn full_read_overlaps(&self, x_left_coordinate: usize, x_right_coordinate: usize) -> bool {
        self.stacking_start() <= x_right_coordinate && self.stacking_end() >= x_left_coordinate
    }

    /// Whether show together with the mate in paired view
    pub fn show_as_pair(&self) -> bool {
        self.flags.is_segmented() && !self.flags.is_supplementary() && !self.flags.is_secondary()
    }

    /// Return the base at coordinate.
    /// None: Not covered, deletion, softclip.
    /// Insertion: the inserted sequences are not returned.
    ///
    /// coordinate: 1-based
    pub fn base_at(&self, coordinate: usize) -> Option<u8> {
        if coordinate < self.start || coordinate > self.end {
            return None;
        }

        let cigars = self.read.cigar();
        let mut reference_pivot: usize = self.start;
        let mut query_pivot: usize = 1; // 1-based. # bases on the sequence. Note that need to substract leading softclips to get aligned base coordinate.

        for op in cigars.iter() {
            if reference_pivot > coordinate {
                break;
            }

            let op = op.unwrap();

            let kind = op.kind();
            let len = op.len();

            let next_reference_pivot = if kind.consumes_reference() {
                reference_pivot + len as usize
            } else {
                reference_pivot
            };

            let next_query_pivot = if kind.consumes_read() {
                query_pivot + len as usize
            } else {
                query_pivot
            };

            if next_reference_pivot <= coordinate {
                reference_pivot = next_reference_pivot;
                query_pivot = next_query_pivot;
                continue;
            }

            match kind {
                Kind::SoftClip | Kind::Insertion | Kind::HardClip | Kind::Pad => {
                    // This should never reach
                    reference_pivot = next_reference_pivot;
                    query_pivot = next_query_pivot;
                }

                Kind::Deletion | Kind::Skip => {
                    return None;
                }

                Kind::SequenceMismatch | Kind::SequenceMatch | Kind::Match => {
                    return Some(
                        self.read
                            .sequence()
                            .get(query_pivot + coordinate - reference_pivot - 1)
                            .unwrap(),
                    );
                }
            }
        }
        None
    }

    pub fn is_softclip_at(&self, coordinate: usize) -> bool {
        if coordinate < self.start && coordinate + self.leading_softclips >= self.start {
            return true;
        }
        if coordinate > self.end && coordinate <= self.end + self.trailing_softclips {
            return true;
        }
        false
    }

    pub fn is_deletion_at(&self, coordinate: usize) -> bool {
        if coordinate < self.start || coordinate > self.end {
            return false;
        }

        let mut reference_pivot: usize = self.start;
        let mut query_pivot: usize = 1; // 1-based. # bases on the sequence. Note that need to substract leading softclips to get aligned base coordinate.

        for op in self.cigar.iter() {
            if reference_pivot > coordinate {
                break;
            }
            let kind = op.kind();

            let next_reference_pivot = if kind.consumes_reference() {
                reference_pivot + op.len() as usize
            } else {
                reference_pivot
            };

            let next_query_pivot = if kind.consumes_read() {
                query_pivot + op.len() as usize
            } else {
                query_pivot
            };

            if next_reference_pivot <= coordinate {
                reference_pivot = next_reference_pivot;
                query_pivot = next_query_pivot;
                continue;
            }

            match kind {
                Kind::SoftClip | Kind::Insertion | Kind::HardClip | Kind::Pad => {
                    // This should never reach
                    reference_pivot = next_reference_pivot;
                    query_pivot = next_query_pivot;
                }

                Kind::Deletion | Kind::Skip => {
                    return true;
                }

                Kind::SequenceMismatch | Kind::SequenceMatch | Kind::Match => {
                    return false;
                }
            }
        }
        false
    }

    pub fn passes_filter(&self, filter: &AlignmentFilter) -> bool {
        match filter {
            AlignmentFilter::Default => true,
            AlignmentFilter::Base(position, base) => {
                if let Some(base_u8) = self.base_at(*position) {
                    *base as u8 == base_u8
                } else {
                    false
                }
            }

            AlignmentFilter::BaseSoftclip(position) => self.is_softclip_at(*position),

            // They should be not be passed here.
            // They should be translated upstream.
            AlignmentFilter::BaseAtCurrentPosition(_)
            | AlignmentFilter::BaseAtCurrentPositionSoftClip => true,

            _ => true, // TODO
        }
    }

    pub fn from_bam_record(
        read_index: usize,
        read: Record,
        reference_sequence: &Sequence,
    ) -> Result<Self, TGVError> {
        let read_start = read.alignment_start().unwrap().unwrap().get();
        let cigars: Cigar = read.cigar();
        let read_end = read_start + cigars.alignment_span().unwrap() - 1;

        let cigars = cigars.iter().collect::<Result<Vec<Op>, _>>().unwrap();
        let leading_softclips = cigars
            .first()
            .map(|op| match op.kind() {
                Kind::SoftClip => op.len(),
                _ => 0,
            })
            .unwrap_or(0);
        let trailing_softclips = if cigars.len() > 1 {
            cigars
                .last()
                .map(|op| match op.kind() {
                    Kind::SoftClip => op.len(),
                    _ => 0,
                })
                .unwrap_or(0)
        } else {
            0
        };
        let flags = read.flags();
        // read.pos() in htslib: 0-based, inclusive, excluding leading hardclips and softclips
        // read.reference_end() in htslib: 0-based, exclusive, excluding trailing hardclips and softclips

        let rendering_contexts = calculate_rendering_contexts(
            read_start,
            &cigars,
            &read.sequence(),
            flags.is_reverse_complemented(),
            reference_sequence,
        )?;

        Ok(Self {
            read,
            start: read_start,
            end: read_end,
            cigar: cigars,
            flags: flags,
            leading_softclips,
            trailing_softclips,
            index: read_index,

            rendering_contexts,
        })
    }
}

/// See: https://samtools.github.io/hts-specs/SAMv1.pdf
pub fn calculate_rendering_contexts(
    reference_start: usize, // 1-based. Alignment start, not softclip start
    cigars: &Vec<Op>,
    seq: &record::Sequence,
    is_reverse: bool,
    reference_sequence: &Sequence,
) -> Result<Vec<RenderingContext>, TGVError> {
    let mut output: Vec<RenderingContext> = Vec::new();
    if cigars.is_empty() {
        return Ok(output);
    }

    let mut reference_pivot: usize = reference_start;
    let mut query_pivot: usize = 1; // 1-based. # bases on the sequence. Note that need to substract leading softclips to get aligned base coordinate.

    let mut annotate_insertion_in_next_cigar = None;

    let mut cigar_index_with_arrow_annotation = None;

    for (i_op, op) in cigars.iter().enumerate() {
        let kind = op.kind();
        let next_reference_pivot = if kind.consumes_reference() {
            reference_pivot + op.len() as usize
        } else {
            reference_pivot
        };

        let next_query_pivot = if kind.consumes_read() {
            query_pivot + op.len() as usize
        } else {
            query_pivot
        };

        let mut new_contexts = Vec::new();

        // let mut new_contexts = Vec::new();
        let add_insertion: bool = annotate_insertion_in_next_cigar.is_some();
        let l = op.len();
        match kind {
            Kind::SoftClip => {
                // S

                if i_op == 0 {
                    // leading softclips. base rendered at the left of reference pivot.
                    for i_soft_clip_base in 0..l as usize {
                        if reference_pivot + i_soft_clip_base <= l + 1 {
                            //base_coordinate <= 1 (on the edge of screen)
                            // Prevent cases when a soft clip is at the very starting of the reference genome:
                            //    ----------- (ref)
                            //  ssss======>   (read)
                            //    ^           edge of screen
                            //  ^^            these softcliped bases are not displayed

                            continue;
                        }

                        let base_coordinate: usize = reference_pivot - l + i_soft_clip_base;

                        let base = seq.get(i_soft_clip_base).unwrap();
                        new_contexts.push(RenderingContext {
                            start: base_coordinate,
                            end: base_coordinate,
                            kind: RenderingContextKind::SoftClip(base),
                            modifiers: Vec::new(),
                        });
                    }
                } else {
                    // right softclips. base rendered at the right of reference pivot.
                    for i_soft_clip_base in 0..l as usize {
                        let base_coordinate: usize = reference_pivot + i_soft_clip_base;
                        let base = seq.get(query_pivot + i_soft_clip_base - 1).unwrap();
                        new_contexts.push(RenderingContext {
                            start: base_coordinate,
                            end: base_coordinate,
                            kind: RenderingContextKind::SoftClip(base),
                            modifiers: Vec::new(),
                        });
                    }
                }
            }

            Kind::Insertion => {
                // The next loop catches on this flag and add an insertion modifier.
                // Insertion is displayed at the next cigar segment.
                annotate_insertion_in_next_cigar = Some(l as usize);
            }

            Kind::Deletion | Kind::Skip => {
                // D / N
                // ---------------- ref
                // ===----===       read (lines with no bckground colors)
                new_contexts.push(RenderingContext {
                    start: reference_pivot,
                    end: next_reference_pivot - 1,
                    kind: RenderingContextKind::Deletion,
                    modifiers: Vec::new(),
                });
            }

            Kind::SequenceMismatch => {
                // X
                new_contexts.push(RenderingContext {
                    start: reference_pivot,
                    end: next_reference_pivot - 1,
                    kind: RenderingContextKind::Match,
                    modifiers: (query_pivot..next_query_pivot as usize)
                        .map(|coordinate| {
                            let reference_coordinate = coordinate - query_pivot + reference_pivot;

                            RenderingContextModifier::Mismatch(
                                reference_coordinate,
                                seq.get(coordinate - 1).unwrap(),
                            )
                        })
                        .collect::<Vec<_>>(),
                })
            }

            Kind::SequenceMatch => new_contexts.push(RenderingContext {
                // =
                start: reference_pivot,
                end: next_reference_pivot - 1,
                kind: RenderingContextKind::Match,
                modifiers: Vec::new(),
            }),

            Kind::Match => {
                // M
                // check reference sequence for mismatches
                // FEAT:
                // Parse base mismatches from the MD field: https://samtools.github.io/hts-specs/SAMtags.pdf#page=3

                let modifiers: Vec<RenderingContextModifier> = (0..l)
                    .filter_map(|i| {
                        let reference_position = reference_pivot + i as usize;
                        if let Some(reference_base) = reference_sequence.base_at(reference_position)
                        // convert to 1-based
                        {
                            let query_position = reference_pivot + i as usize;
                            let query_base = seq.get(query_pivot + i as usize - 1).unwrap();
                            if !matches_base(query_base, reference_base) {
                                Some(RenderingContextModifier::Mismatch(
                                    query_position,
                                    query_base,
                                ))
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    })
                    .collect_vec();
                new_contexts.push(RenderingContext {
                    start: reference_pivot,
                    end: next_reference_pivot - 1,
                    kind: RenderingContextKind::Match,
                    modifiers,
                });
            }

            Kind::HardClip | Kind::Pad => {
                // P / H
                // Don't need to do anything
            }
        }

        if new_contexts.is_empty() {
            reference_pivot = next_reference_pivot;
            query_pivot = next_query_pivot;
            continue;
        }

        if add_insertion {
            // Insertion (detected in the previous loop) notated at the beginning of the first segment.
            if let Some(context) = new_contexts.first_mut() {
                context.add_modifier(RenderingContextModifier::Insertion(
                    annotate_insertion_in_next_cigar.unwrap(),
                ))
            }
        };
        annotate_insertion_in_next_cigar = None;

        if is_reverse {
            // reverse: first one
            if cigar_index_with_arrow_annotation.is_none() {
                cigar_index_with_arrow_annotation = Some(output.len());
            }
        } else {
            // forward: last one
            if can_be_annotated_with_arrows(&kind) {
                cigar_index_with_arrow_annotation = Some(output.len() + new_contexts.len() - 1)
                // first context
            }
        }
        output.extend(new_contexts);

        reference_pivot = next_reference_pivot;
        query_pivot = next_query_pivot;
    }

    if let Some(index) = cigar_index_with_arrow_annotation {
        output[index].add_modifier(if is_reverse {
            RenderingContextModifier::Reverse
        } else {
            RenderingContextModifier::Forward
        })
    }

    Ok(output)
}

/// Read 1 is the forward read, read 2 is the reverse read
pub fn calculate_paired_context(
    rendering_contexts_1: Vec<RenderingContext>,
    rendering_contexts_2: Vec<RenderingContext>,
) -> Vec<RenderingContext> {
    match (
        rendering_contexts_1.is_empty(),
        rendering_contexts_2.is_empty(),
    ) {
        (true, _) => {
            return rendering_contexts_2;
        }
        (false, true) => {
            return rendering_contexts_1;
        }
        _ => {}
    };

    let (rendering_start_1, rendering_end_1) = (
        rendering_contexts_1.first().unwrap().start,
        rendering_contexts_1.last().unwrap().end,
    );
    let (rendering_start_2, rendering_end_2) = (
        rendering_contexts_2.first().unwrap().start,
        rendering_contexts_2.last().unwrap().end,
    );

    // Gaps
    if rendering_end_1 + 1 < rendering_start_2 {
        let gap_context = RenderingContext {
            start: rendering_end_1 + 1,
            end: rendering_start_2 - 1,
            kind: RenderingContextKind::PairGap,
            modifiers: vec![],
        };
        return rendering_contexts_1
            .into_iter()
            .chain(vec![gap_context])
            .chain(rendering_contexts_2)
            .collect::<Vec<_>>();
    }

    if rendering_end_1 + 1 == rendering_start_2 {
        return rendering_contexts_1
            .into_iter()
            .chain(rendering_contexts_2)
            .collect::<Vec<_>>();
    }
    if rendering_end_2 + 1 < rendering_start_1 {
        let gap_context = RenderingContext {
            start: rendering_end_2 + 1,
            end: rendering_start_1 - 1,
            kind: RenderingContextKind::PairGap,
            modifiers: vec![],
        };
        return rendering_contexts_2
            .into_iter()
            .chain(vec![gap_context])
            .chain(rendering_contexts_1)
            .collect::<Vec<_>>();
    }
    if rendering_end_2 + 1 == rendering_start_1 {
        return rendering_contexts_2
            .into_iter()
            .chain(rendering_contexts_1)
            .collect::<Vec<_>>();
    }

    // Overlaps

    let mut iter1 = rendering_contexts_1.into_iter();
    let mut iter2 = rendering_contexts_2.into_iter();

    let mut context_1 = iter1.next();
    let mut context_2 = iter2.next();
    let mut contexts = Vec::new();

    // Whether or not the next iteration should resolve the left overhang
    let mut left_overhang_resolved = false;

    loop {
        match (context_1.is_some(), context_2.is_some()) {
            (true, true) => {
                let c1 = context_1.as_ref().unwrap();
                let c2 = context_2.as_ref().unwrap();

                // No overlaps
                if c1.end < c2.start {
                    contexts.push(context_1.unwrap());
                    context_1 = iter1.next();
                    continue;
                }

                if c2.end < c1.start {
                    contexts.push(context_2.unwrap());
                    context_2 = iter2.next();
                    continue;
                }

                // Overlaps: chop up the contexts

                // left overhang
                let (start, next_start, kind, modifiers) = if c1.start < c2.start {
                    (c1.start, c2.start, &c1.kind, &c1.modifiers)
                } else {
                    (c2.start, c1.start, &c2.kind, &c2.modifiers)
                };

                if start < next_start && !left_overhang_resolved {
                    // Should only happens at the first iteration
                    contexts.push(RenderingContext {
                        start,
                        end: next_start - 1,
                        kind: kind.clone(),
                        modifiers: modifiers
                            .iter()
                            .filter_map(|modifier| match modifier {
                                RenderingContextModifier::Mismatch(pos, _)
                                | RenderingContextModifier::Insertion(pos) => {
                                    if *pos < next_start {
                                        Some(modifier.clone())
                                    } else {
                                        None
                                    }
                                }
                                _ => Some(modifier.clone()),
                            })
                            .collect::<Vec<_>>(),
                    });
                }

                // overlapping region
                let start = next_start;
                let end = if c1.end < c2.end { c1.end } else { c2.end };

                contexts.push(get_overlapped_pair_rendering_text(
                    start,
                    end,
                    &c1.kind,
                    &c2.kind,
                    &c1.modifiers,
                    &c2.modifiers,
                ));

                // right overhang
                let previous_end = end;
                let (end, kind, modifiers) = if c1.end < c2.end {
                    (c2.end, &c2.kind, &c2.modifiers)
                } else {
                    (c1.end, &c1.kind, &c1.modifiers)
                };

                if previous_end != end {
                    contexts.push(RenderingContext {
                        start: previous_end + 1,
                        end,
                        kind: kind.clone(),
                        modifiers: modifiers
                            .iter()
                            .filter_map(|modifier| match modifier {
                                RenderingContextModifier::Mismatch(pos, _)
                                | RenderingContextModifier::Insertion(pos) => {
                                    if *pos > previous_end {
                                        Some(modifier.clone())
                                    } else {
                                        None
                                    }
                                }
                                _ => Some(modifier.clone()),
                            })
                            .collect::<Vec<_>>(),
                    })
                }

                left_overhang_resolved = true;

                if c1.end < c2.end {
                    context_1 = iter1.next();
                } else if c1.end > c2.end {
                    context_2 = iter2.next();
                } else {
                    context_1 = iter1.next();
                    context_2 = iter2.next();
                }
            }
            (true, false) => {
                contexts.push(context_1.unwrap());
                context_1 = iter1.next();
            }
            (false, true) => {
                contexts.push(context_2.unwrap());
                context_2 = iter2.next();
            }
            (false, false) => {
                break;
            }
        }
    }

    contexts
}

pub fn get_overlapped_pair_rendering_text(
    start: usize,
    end: usize,
    kind_1: &RenderingContextKind,
    kind_2: &RenderingContextKind,
    modifiers_1: &Vec<RenderingContextModifier>,
    modifiers_2: &Vec<RenderingContextModifier>,
) -> RenderingContext {
    if *kind_1 != *kind_2 {
        return RenderingContext {
            start,
            end,
            kind: RenderingContextKind::PairOverlap,
            modifiers: (start..=end)
                .map(RenderingContextModifier::PairConflict)
                .collect::<Vec<_>>(),
        };
    }

    let mut base_modifier_lookup = HashMap::<usize, RenderingContextModifier>::new();
    let mut modifiers = vec![];

    modifiers_1.iter().for_each(|modifier| match modifier {
        RenderingContextModifier::Mismatch(pos, _) | RenderingContextModifier::Insertion(pos) => {
            base_modifier_lookup.insert(*pos, modifier.clone());
        }
        _ => modifiers.push(modifier.clone()),
    });

    modifiers_2.iter().for_each(|modifier| match modifier {
        RenderingContextModifier::Mismatch(pos, _) | RenderingContextModifier::Insertion(pos) => {
            match base_modifier_lookup.remove(pos) {
                Some(other_modifier) => {
                    if *modifier == other_modifier {
                        modifiers.push(other_modifier)
                    } else {
                        modifiers.push(RenderingContextModifier::PairConflict(*pos))
                    }
                }
                None => {
                    base_modifier_lookup.insert(*pos, modifier.clone());
                }
            }
        }
        _ => modifiers.push(modifier.clone()),
    });

    base_modifier_lookup
        .into_values()
        .for_each(|modifier| modifiers.push(modifier));

    RenderingContext {
        start,
        end,
        kind: kind_1.clone(),
        modifiers,
    }
}

pub fn matches_base(base1: u8, base2: u8) -> bool {
    if base1 == base2 {
        return true;
    }

    match (base1, base2) {
        (b'A', b'a')
        | (b'a', b'A')
        | (b'C', b'c')
        | (b'c', b'C')
        | (b'G', b'g')
        | (b'g', b'G')
        | (b'T', b't')
        | (b't', b'T') => true,
        _ => false,
    }
}

/// Whether the cigar operation can be annotated with the < / > signs.
/// Yes: M/I/S/=/X
/// No: D/N/H/P
fn can_be_annotated_with_arrows(kind: &Kind) -> bool {
    match kind {
        Kind::Match
        | Kind::SoftClip
        | Kind::SequenceMatch
        | Kind::SequenceMismatch
        | Kind::Deletion
        | Kind::Skip => true,

        Kind::HardClip | Kind::Pad | Kind::Insertion => false,
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    // use noodles::bam::record::{Cigar, CigarString};
    use noodles::bam;
    use noodles::sam::{
        self,
        alignment::{
            io::Write,
            record::cigar::{op::Kind, Op},
        },
    };
    use std::io;

    use rstest::rstest;

    #[rstest]
    #[case(10, vec![(Kind::Match, 3)],  b"ATT", false,Sequence::default(), vec![RenderingContext{
        start:10,
        end:12,
        kind: RenderingContextKind::Match,
        modifiers:vec![RenderingContextModifier::Forward]
    }])]
    // Test reverse strand
    #[case(10, vec![(Kind::Match, 3)],  b"ATT", true, Sequence::default(), vec![RenderingContext{
        start:10,
        end:12,
        kind: RenderingContextKind::Match,
        modifiers:vec![RenderingContextModifier::Reverse]
    }])]
    // Test deletion
    #[case(10, vec![(Kind::Match, 3),(Kind::Deletion, 2), (Kind::Match, 3)], b"AAATTT", true, Sequence::default(), vec![RenderingContext{
        start:10,
        end:12,
        kind: RenderingContextKind::Match,
        modifiers:vec![RenderingContextModifier::Reverse]
    }, RenderingContext{
        start:13,
        end:14,
        kind: RenderingContextKind::Deletion,
        modifiers:vec![]
    }, RenderingContext{
        start:15,
        end:17,
        kind: RenderingContextKind::Match,
        modifiers:vec![]
    }])]
    // Test RefSkip
    #[case(10, vec![(Kind::Match, 3),(Kind::Skip, 2)], b"AAA", false, Sequence::default(), vec![RenderingContext{
        start:10,
        end:12,
        kind: RenderingContextKind::Match,
        modifiers:vec![]
    }, RenderingContext{
        start:13,
        end:14,
        kind: RenderingContextKind::Deletion,
        modifiers:vec![RenderingContextModifier::Forward]
    }])]
    // Test insertion
    #[case(10, vec![(Kind::Match, 3), (Kind::Insertion, 2), (Kind::Match, 3)], b"AAATTCCC", false, Sequence::default(), vec![RenderingContext{
        start:10,
        end:12,
        kind: RenderingContextKind::Match,
        modifiers:vec![]
    }, RenderingContext{
        start:13,
        end:15,
        kind: RenderingContextKind::Match,
        modifiers:vec![RenderingContextModifier::Insertion(2), RenderingContextModifier::Forward]
    }])]
    // Test soft clips
    #[case(10, vec![(Kind::SoftClip, 2), (Kind::Match, 3), (Kind::SoftClip, 1)], b"GGATTC", true, Sequence::default(), vec![
        RenderingContext{
            start:8,
            end:8,
            kind: RenderingContextKind::SoftClip(b'G'),
            modifiers:vec![RenderingContextModifier::Reverse]
        },
        RenderingContext{
            start:9,
            end:9,
            kind: RenderingContextKind::SoftClip(b'G'),
            modifiers:vec![]
        },
        RenderingContext{
            start:10,
            end:12,
            kind: RenderingContextKind::Match,
            modifiers:vec![]
        },
        RenderingContext{
            start:13,
            end:13,
            kind: RenderingContextKind::SoftClip(b'C'),
            modifiers:vec![]
        }
    ])]
    // Test Equal cigar (matches current implementation with query pivot)
    #[case(10, vec![(Kind::SequenceMatch, 3)], b"ATT", false, Sequence::default(), vec![RenderingContext{
        start:10,
        end:12, // This matches the current implementation which uses next_query_pivot - 1
        kind: RenderingContextKind::Match,
        modifiers:vec![RenderingContextModifier::Forward]
    }])]
    // Test Diff cigar (explicit mismatch)
    #[case(10, vec![(Kind::SequenceMismatch, 3)], b"ATT", false, Sequence::default(), vec![RenderingContext{
        start:10,
        end:12,
        kind: RenderingContextKind::Match,
        modifiers:vec![
            RenderingContextModifier::Mismatch(10, b'A'),
            RenderingContextModifier::Mismatch(11, b'T'),
            RenderingContextModifier::Mismatch(12, b'T'),
            RenderingContextModifier::Forward
        ]
    }])]
    // Test complex cigar: soft clip + match + insertion + match + deletion + match
    #[case(10, vec![(Kind::SoftClip, 1), (Kind::Match, 2), (Kind::Insertion, 1), (Kind::Match, 2), (Kind::Deletion, 3), (Kind::Match, 2)],
           b"GATCGAA", false, Sequence::default(), vec![
        RenderingContext{
            start:9,
            end:9,
            kind: RenderingContextKind::SoftClip(b'G'),
            modifiers:vec![]
        },
        RenderingContext{
            start:10,
            end:11,
            kind: RenderingContextKind::Match,
            modifiers:vec![]
        },
        RenderingContext{
            start:12,
            end:13,
            kind: RenderingContextKind::Match,
            modifiers:vec![RenderingContextModifier::Insertion(1)]
        },
        RenderingContext{
            start:14,
            end:16,
            kind: RenderingContextKind::Deletion,
            modifiers:vec![]
        },
        RenderingContext{
            start:17,
            end:18,
            kind: RenderingContextKind::Match,
            modifiers:vec![RenderingContextModifier::Forward]
        }
    ])]
    // Test soft clips
    #[case(10, vec![(Kind::SoftClip, 2), (Kind::Match, 3), (Kind::SoftClip, 1)], b"GGATTC", true, Sequence::new(10, b"AATG".to_vec(), 0), vec![
        RenderingContext{
            start:8,
            end:8,
            kind: RenderingContextKind::SoftClip(b'G'),
            modifiers:vec![RenderingContextModifier::Reverse]
        },
        RenderingContext{
            start:9,
            end:9,
            kind: RenderingContextKind::SoftClip(b'G'),
            modifiers:vec![]
        },
        RenderingContext{
            start:10,
            end:12,
            kind: RenderingContextKind::Match,
            modifiers:vec![RenderingContextModifier::Mismatch(11, b'T')]
        },
        RenderingContext{
            start:13,
            end:13,
            kind: RenderingContextKind::SoftClip(b'C'),
            modifiers:vec![]
        }
    ])]
    fn test_calculate_rendering_contexts(
        #[case] reference_start: usize, // 1-based
        #[case] cigars: Vec<(Kind, usize)>,
        #[case] seq: &[u8],
        #[case] is_reverse: bool,
        #[case] reference_sequence: Sequence,
        #[case] expected_rendering_contexts: Vec<RenderingContext>,
    ) {
        let cigars = cigars
            .into_iter()
            .map(|(kind, length)| Op::new(kind, length))
            .collect::<Vec<Op>>();

        let header = sam::Header::default();

        let record_buf = sam::alignment::RecordBuf::builder()
            .set_sequence(sam::alignment::record_buf::Sequence::from(seq))
            .build();
        let record = serialize_as_bam_record(&header, &record_buf).unwrap();

        let contexts = calculate_rendering_contexts(
            reference_start,
            &cigars,
            &record.sequence(),
            is_reverse,
            &reference_sequence,
        )
        .unwrap();

        assert_eq!(contexts, expected_rendering_contexts)
    }

    /// Helper function to create bam::Record test cases
    fn serialize_as_bam_record(
        header: &sam::Header,
        record_buf: &sam::alignment::RecordBuf,
    ) -> io::Result<bam::Record> {
        let mut writer = bam::io::Writer::from(Vec::new());
        writer.write_alignment_record(header, record_buf)?;

        let src = writer.into_inner();
        let mut reader = bam::io::Reader::from(&src[..]);
        let mut record = bam::Record::default();
        reader.read_record(&mut record)?;

        Ok(record)
    }
}

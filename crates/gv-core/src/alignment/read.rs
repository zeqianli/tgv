use crate::error::TGVError;
use crate::intervals::{GenomeInterval, Region};
use crate::message::AlignmentFilter;
use crate::sequence::Sequence;
// use rust_htslib::bam::{record::Seq, Read, Record};
//
use itertools::Itertools;
use noodles::bam::record::{self, Cigar, Record};
pub use noodles::sam::record::data::field::value::base_modifications::group::Modification as BaseModification;
use noodles::sam::{
    self, Header,
    alignment::{
        RecordBuf,
        record::{
            Cigar as CigarTrait, Flags,
            cigar::{Op, op::Kind},
        },
    },
};
use std::collections::HashMap;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BaseModificationProbability {
    /// Probability from the ML tag, where 0 = unmodified and 255 = fully modified.
    pub probability: u8,
    pub modification: BaseModification,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenderingContextModifier {
    /// Annoate the forward arrow at the end.
    Forward,

    /// Annotate the reverse arrow at the beginning.
    Reverse,

    /// The previous cigar is an insertion. Annotate this at the beginning of this segment.
    Insertion(u64),

    /// Mismatch at location with base
    Mismatch(u64, u8),

    /// Pair overlaps and have differnet RenderingContextKind
    /// (except Softclip + Match: softclip is displayed in this case (same as IGV))
    PairConflict(u64),

    /// Base modifications observed at a reference coordinate on this read.
    BaseModifications(u64, Vec<BaseModificationProbability>),
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
    pub start: u64,

    /// End coordinates of a displayed segment, 1-based, inclusive
    pub end: u64,

    /// The renderer will decide style based on the cigar segment kind.
    pub kind: RenderingContextKind,

    /// Mismatches, insertions, arrows, etc
    pub modifiers: Vec<RenderingContextModifier>,
}

impl RenderingContext {
    fn add_modifier(&mut self, modifier: RenderingContextModifier) {
        self.modifiers.push(modifier)
    }

    fn len(&self) -> u64 {
        self.end - self.start + 1
    }
}

#[derive(Clone, Debug)]
/// An aligned read with viewing coordinates.
pub struct AlignedRead {
    /// Alignment record data
    pub read: RecordBuf,

    /// Non-clipped start genome coordinate on the alignment view
    /// 1-based, inclusive
    pub start: u64,
    /// Non-clipped end genome coordinate on the alignment view
    /// Note that this includes the soft-clipped reads and differ from the built-in methods. TODO
    /// 1-based, inclusive
    pub end: u64,

    /// Leading softclips. Used for track stacking calculation.
    pub leading_softclips: u64,

    /// Trailing softclips. Used for track stacking calculation.
    pub trailing_softclips: u64,

    pub cigars: Vec<Op>,

    pub flags: Flags,

    /// index in the alignment read array.
    pub index: usize,

    /// Base mismatches with the reference
    pub rendering_contexts: Vec<RenderingContext>,

    /// Per-position base modification data parsed from MM/ML auxiliary tags.
    /// Key: 1-based reference position. Value: list of modifications at that position.
    /// Empty when the BAM record has no MM/ML tags.
    pub base_modifications: HashMap<u64, Vec<BaseModificationProbability>>,
}

impl AlignedRead {
    pub fn stacking_start(&self) -> u64 {
        u64::max(self.start.saturating_sub(self.leading_softclips), 1)
    }

    pub fn stacking_end(&self) -> u64 {
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
            self.read.name().unwrap(),
            self.flags,
            self.start,
            self.read.mapping_quality().unwrap().get(),
            self.cigars
        ))
    }

    /// Whether the alignment segment (excluding softclips) covers a x_coordinate (1-based).
    pub fn covers(&self, posiion: u64) -> bool {
        self.start <= posiion && self.end >= posiion
    }
    /// Whether the alignment segment (including softclips) covers a posiion (1-based).
    pub fn full_read_covers(&self, posiion: u64) -> bool {
        self.stacking_start() <= posiion && self.stacking_end() >= posiion
    }

    /// Whether the alignment segment (excluding softclips) covers a x_coordinate (1-based).
    pub fn overlaps(&self, left: u64, right: u64) -> bool {
        self.start <= right && self.end >= left
    }
    /// Whether the alignment segment (including softclips) covers a x_coordinate (1-based).
    pub fn full_read_overlaps(&self, left: u64, right: u64) -> bool {
        self.stacking_start() <= right && self.stacking_end() >= left
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
    pub fn base_at(&self, coordinate: u64) -> Option<u8> {
        if coordinate < self.start || coordinate > self.end {
            return None;
        }

        let coordinate = coordinate as usize;

        let cigars = self.read.cigar();
        let mut reference_pivot = self.start as usize;
        let mut query_pivot: usize = 1; // 1-based. # bases on the sequence. Note that need to substract leading softclips to get aligned base coordinate.

        for op in cigars.iter() {
            if reference_pivot > coordinate {
                break;
            }

            let op = op.unwrap();

            let kind = op.kind();
            let len = op.len();

            let next_reference_pivot = if kind.consumes_reference() {
                reference_pivot + len
            } else {
                reference_pivot
            };

            let next_query_pivot = if kind.consumes_read() {
                query_pivot + len
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

    pub fn is_softclip_at(&self, coordinate: u64) -> bool {
        if coordinate < self.start && coordinate + self.leading_softclips >= self.start {
            return true;
        }
        if coordinate > self.end && coordinate <= self.end + self.trailing_softclips {
            return true;
        }
        false
    }

    pub fn is_deletion_at(&self, coordinate: u64) -> bool {
        if coordinate < self.start || coordinate > self.end {
            return false;
        }

        let coordinate = coordinate as usize;
        let mut reference_pivot: usize = self.start as usize;
        let mut query_pivot: usize = 1; // 1-based. # bases on the sequence. Note that need to substract leading softclips to get aligned base coordinate.

        for op in self.cigars.iter() {
            if reference_pivot > coordinate {
                break;
            }
            let kind = op.kind();

            let next_reference_pivot = if kind.consumes_reference() {
                reference_pivot + op.len()
            } else {
                reference_pivot
            };

            let next_query_pivot = if kind.consumes_read() {
                query_pivot + op.len()
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

    // /// Construct an `AlignedRead` from a CRAM `RecordBuf` by round-tripping through an in-memory
    // /// BAM encoding. CRAM queries yield `RecordBuf` records, which need to be bridged to the
    // /// `bam::Record`-based representation used internally.
    // pub fn from_cram_record(
    //     read_index: usize,
    //     header: &Header,
    //     record_buf: &RecordBuf,
    //     reference_sequence: &Sequence,
    // ) -> Result<Self, TGVError> {
    //     use noodles::sam::alignment::io::Write as AlignmentWrite;

    //     let mut buf = Vec::new();
    //     let mut writer = noodles::bam::io::Writer::from(&mut buf);
    //     writer.write_alignment_record(header, record_buf)?;
    //     drop(writer);

    //     let mut reader = noodles::bam::io::Reader::from(&buf[..]);
    //     let mut record = Record::default();
    //     reader.read_record(&mut record)?;

    //     Self::from_bam_record(read_index, record, reference_sequence)
    // }
    //

    pub fn build_rendering_context(
        &mut self,
        reference_sequence: &Sequence,
    ) -> Result<(), TGVError> {
        calculate_rendering_contexts(
            &mut self.rendering_contexts,
            self.start,
            &self.cigars,
            self.read.sequence(),
            self.flags.is_reverse_complemented(),
            reference_sequence,
            &self.base_modifications,
        )?;

        Ok(())
    }

    pub fn from_record(
        read_index: usize,
        read: RecordBuf,
        reference_sequence: &Sequence,
    ) -> Result<Self, TGVError> {
        let start = read.alignment_start().unwrap().get() as u64;
        let cigars = read.cigar();
        let end = start + cigars.alignment_span() as u64 - 1;

        let cigars = cigars.iter().collect::<Result<Vec<Op>, _>>().unwrap();
        let leading_softclips = cigars
            .first()
            .map(|op| match op.kind() {
                Kind::SoftClip => op.len() as u64,
                _ => 0,
            })
            .unwrap_or(0);
        let trailing_softclips = if cigars.len() > 1 {
            cigars
                .last()
                .map(|op| match op.kind() {
                    Kind::SoftClip => op.len() as u64,
                    _ => 0,
                })
                .unwrap_or(0)
        } else {
            0
        };
        let flags = read.flags();
        // read.pos() in htslib: 0-based, inclusive, excluding leading hardclips and softclips
        // read.reference_end() in htslib: 0-based, exclusive, excluding trailing hardclips and softclips

        let base_modifications = extract_base_modifications(&read, &cigars, start);

        Ok(Self {
            read: read,

            start,
            end,
            cigars,
            flags,
            leading_softclips,
            trailing_softclips,
            index: read_index,

            rendering_contexts: Vec::new(),
            base_modifications,
        })
    }
}

/// Parse base modification data from the MM and ML auxiliary tags of a SAM record.
/// Returns an empty map if the record has no MM/ML tags or if parsing fails.
fn extract_base_modifications(
    record: &RecordBuf,
    cigars: &[Op],
    alignment_start: u64,
) -> HashMap<u64, Vec<BaseModificationProbability>> {
    use noodles::sam::alignment::record_buf::data::field::Value;
    use noodles::sam::alignment::record_buf::data::field::value::Array;
    use noodles::sam::record::data::field::value::{
        BaseModifications as NoodlesBaseModifications,
    };

    let data = record.data();

    // Fetch MM tag (string, type Z).
    let mm_str: String = match data.get(b"MM") {
        Some(Value::String(s)) => String::from_utf8_lossy(s.as_ref()).into_owned(),
        _ => return HashMap::new(),
    };

    // Fetch ML tag (uint8 array, type B:C).
    let ml_bytes: Vec<u8> = match data.get(b"ML") {
        Some(Value::Array(Array::UInt8(values))) => values.clone(),
        _ => Vec::new(),
    };

    let base_modifications = match NoodlesBaseModifications::parse(
        mm_str.as_bytes(),
        record.flags().is_reverse_complemented(),
        record.sequence(),
    ) {
        Ok(base_modifications) => base_modifications,
        Err(_) => return HashMap::new(),
    };

    let query_to_reference_position =
        build_query_to_reference_position_map(cigars, alignment_start);

    let mut result: HashMap<u64, Vec<BaseModificationProbability>> = HashMap::new();
    let mut ml_index = 0;

    for group in base_modifications.as_ref() {
        for &query_position in group.positions() {
            let reference_position = match query_to_reference_position.get(&query_position) {
                Some(reference_position) => *reference_position,
                None => {
                    ml_index += group.modifications().len();
                    continue;
                }
            };

            for modification in group.modifications() {
                let probability = ml_bytes.get(ml_index).copied().unwrap_or(255);
                ml_index += 1;

                result
                    .entry(reference_position)
                    .or_default()
                    .push(BaseModificationProbability {
                        probability,
                        modification: *modification,
                    });
            }
        }
    }

    result
}

fn build_query_to_reference_position_map(
    cigars: &[Op],
    alignment_start: u64,
) -> HashMap<usize, u64> {
    let mut query_to_reference_position = HashMap::new();
    let mut query_cursor = 0usize;
    let mut reference_cursor = alignment_start;

    for op in cigars {
        match op.kind() {
            Kind::SoftClip | Kind::Insertion => {
                query_cursor += op.len();
            }
            Kind::HardClip | Kind::Pad => {}
            Kind::Deletion | Kind::Skip => {
                reference_cursor += op.len() as u64;
            }
            Kind::Match | Kind::SequenceMatch | Kind::SequenceMismatch => {
                for i in 0..op.len() {
                    query_to_reference_position
                        .insert(query_cursor + i, reference_cursor + i as u64);
                }

                query_cursor += op.len();
                reference_cursor += op.len() as u64;
            }
        }
    }

    query_to_reference_position
}

fn add_base_modification_modifiers(
    rendering_contexts: &mut [RenderingContext],
    base_modifications: &HashMap<u64, Vec<BaseModificationProbability>>,
) {
    let mut positions = base_modifications.iter().collect::<Vec<_>>();
    positions.sort_by_key(|(position, _)| *position);

    for (&position, modifications) in positions {
        if let Some(context) = rendering_contexts.iter_mut().find(|context| {
            matches!(context.kind, RenderingContextKind::Match)
                && context.start <= position
                && position <= context.end
        }) {
            context.add_modifier(RenderingContextModifier::BaseModifications(
                position,
                modifications.clone(),
            ));
        }
    }
}

/// See: https://samtools.github.io/hts-specs/SAMv1.pdf
pub fn calculate_rendering_contexts(
    rendering_context: &mut Vec<RenderingContext>,
    reference_start: u64, // 1-based. Alignment start, not softclip start
    cigars: &Vec<Op>,
    seq: &sam::alignment::record_buf::Sequence,
    is_reverse: bool,
    reference_sequence: &Sequence,
    base_modifications: &HashMap<u64, Vec<BaseModificationProbability>>,
) -> Result<(), TGVError> {
    rendering_context.clear();
    if cigars.is_empty() {
        return Ok(());
    }

    let mut reference_pivot: usize = reference_start as usize;
    let mut query_pivot: usize = 1; // 1-based. # bases on the sequence. Note that need to substract leading softclips to get aligned base coordinate.

    let mut annotate_insertion_in_next_cigar = None;

    let mut cigar_index_with_arrow_annotation = None;

    for (i_op, op) in cigars.iter().enumerate() {
        let kind = op.kind();
        let next_reference_pivot = if kind.consumes_reference() {
            reference_pivot + op.len()
        } else {
            reference_pivot
        };

        let next_query_pivot = if kind.consumes_read() {
            query_pivot + op.len()
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
                    for i_soft_clip_base in 0..l {
                        if reference_pivot + i_soft_clip_base <= l + 1 {
                            //base_coordinate <= 1 (on the edge of screen)
                            // Prevent cases when a soft clip is at the very starting of the reference genome:
                            //    ----------- (ref)
                            //  ssss======>   (read)
                            //    ^           edge of screen
                            //  ^^            these softcliped bases are not displayed

                            continue;
                        }

                        let base_coordinate = (reference_pivot - l + i_soft_clip_base) as u64;

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
                    for i_soft_clip_base in 0..l {
                        let base_coordinate = (reference_pivot + i_soft_clip_base) as u64;
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
                annotate_insertion_in_next_cigar = Some(l);
            }

            Kind::Deletion | Kind::Skip => {
                // D / N
                // ---------------- ref
                // ===----===       read (lines with no bckground colors)
                new_contexts.push(RenderingContext {
                    start: reference_pivot as u64,
                    end: next_reference_pivot as u64 - 1,
                    kind: RenderingContextKind::Deletion,
                    modifiers: Vec::new(),
                });
            }

            Kind::SequenceMismatch => {
                // X
                new_contexts.push(RenderingContext {
                    start: reference_pivot as u64,
                    end: next_reference_pivot as u64 - 1,
                    kind: RenderingContextKind::Match,
                    modifiers: (query_pivot..next_query_pivot)
                        .map(|coordinate| {
                            let reference_coordinate = coordinate - query_pivot + reference_pivot;

                            RenderingContextModifier::Mismatch(
                                reference_coordinate as u64,
                                seq.get(coordinate - 1).unwrap(),
                            )
                        })
                        .collect::<Vec<_>>(),
                })
            }

            Kind::SequenceMatch => new_contexts.push(RenderingContext {
                // =
                start: reference_pivot as u64,
                end: next_reference_pivot as u64 - 1,
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
                        let reference_position = reference_pivot + i;
                        if let Some(reference_base) =
                            reference_sequence.base_at(reference_position as u64)
                        // convert to 1-based
                        {
                            let query_position = reference_pivot + i;
                            let query_base = seq.get(query_pivot + i - 1).unwrap();
                            if !matches_base(query_base, reference_base) {
                                Some(RenderingContextModifier::Mismatch(
                                    query_position as u64,
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
                    start: reference_pivot as u64,
                    end: next_reference_pivot as u64 - 1,
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
                    annotate_insertion_in_next_cigar.unwrap() as u64,
                ))
            }
        };
        annotate_insertion_in_next_cigar = None;

        if is_reverse {
            // reverse: first one
            if cigar_index_with_arrow_annotation.is_none() {
                cigar_index_with_arrow_annotation = Some(rendering_context.len());
            }
        } else {
            // forward: last one
            if can_be_annotated_with_arrows(&kind) {
                cigar_index_with_arrow_annotation =
                    Some(rendering_context.len() + new_contexts.len() - 1)
                // first context
            }
        }
        rendering_context.extend(new_contexts);

        reference_pivot = next_reference_pivot;
        query_pivot = next_query_pivot;
    }

    if let Some(index) = cigar_index_with_arrow_annotation {
        rendering_context[index].add_modifier(if is_reverse {
            RenderingContextModifier::Reverse
        } else {
            RenderingContextModifier::Forward
        })
    }

    add_base_modification_modifiers(rendering_context, base_modifications);

    Ok(())
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
    start: u64,
    end: u64,
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

    let mut base_modifier_lookup = HashMap::<u64, RenderingContextModifier>::new();
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

#[derive(Clone, Debug)]
pub struct ReadPair {
    /// Read 1 index in the alignment
    pub read_1_index: usize,

    /// If some: Read 2 index in the alignment
    /// if none: Read not shown as paired
    pub read_2_index: Option<usize>,

    /// 1-based start (including soft-clips)
    pub stacking_start: u64,

    /// 1-based end (including soft-clips)
    pub stacking_end: u64,

    /// Index in the alignment
    pub index: usize,

    /// The paired rendering contexts
    pub rendering_contexts: Vec<RenderingContext>,
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
            record::cigar::{Op, op::Kind},
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
    #[case(10, vec![(Kind::SoftClip, 2), (Kind::Match, 3), (Kind::SoftClip, 1)], b"GGATTC", true, Sequence{start: 10, sequence: b"AATG".to_vec(), contig_index: 0}, vec![
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
        #[case] reference_start: u64, // 1-based
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

        let mut contexts = Vec::new();
        calculate_rendering_contexts(
            &mut contexts,
            reference_start,
            &cigars,
            &record_buf.sequence(),
            is_reverse,
            &reference_sequence,
            &HashMap::new(),
        )
        .unwrap();

        assert_eq!(contexts, expected_rendering_contexts)
    }
}

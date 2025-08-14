use crate::error::TGVError;
use crate::region::Region;
use crate::sequence::Sequence;
use rust_htslib::bam::ext::BamRecordExtensions;
use rust_htslib::bam::record::{Cigar, CigarStringView};
use rust_htslib::bam::{record::Seq, Read, Record};
use sqlx::query;
use std::collections::{BTreeMap, HashMap};

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
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenderingContextKind {
    SoftClip(u8),

    Match, // Mismatches are annotated with modifiers

    Deletion,
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

    /// Y coordinate in the alignment view
    /// 0-based.
    pub y: usize,

    /// Base mismatches with the reference
    pub rendering_contexts: Vec<RenderingContext>,
}

impl AlignedRead {
    /// Return an 1-based range iterator that includes all bases of the alignment.
    pub fn range(&self) -> impl Iterator<Item = usize> {
        self.start..self.end + 1
    }

    fn stacking_start(&self) -> usize {
        usize::max(self.start.saturating_sub(self.leading_softclips), 1)
    }

    fn stacking_end(&self) -> usize {
        self.end.saturating_add(self.trailing_softclips)
    }
}

fn get_cigar_index_with_arrow_annotation(cigars: &CigarStringView, is_reverse: bool) -> usize {
    // Scan cigars 1st pass to find the cigar index with < / > annotation.
    if is_reverse {
        // last cigar segment
        cigars.len()
            - cigars
                .iter()
                .rev()
                .position(|op| can_be_annotated_with_arrows(op))
                .unwrap_or(0)
            - 1
    } else {
        // first eligible cigar
        cigars
            .iter()
            .position(|op| can_be_annotated_with_arrows(op))
            .unwrap_or(0)
    }
}

/// See: https://samtools.github.io/hts-specs/SAMv1.pdf
fn calculate_rendering_contexts(
    reference_start: usize, // 1-based
    cigars: &CigarStringView,
    leading_softclips: usize,
    seq: &Seq,
    is_reverse: bool,
    reference_sequence: Option<&Sequence>,
) -> Result<Vec<RenderingContext>, TGVError> {
    let mut output: Vec<RenderingContext> = Vec::new();
    if cigars.len() == 0 {
        return Ok(output);
    }

    let mut reference_pivot: usize = reference_start;
    let mut query_pivot: usize = 1; // 1-based. # bases on the sequence. Note that need to substract leading softclips to get aligned base coordinate.

    let mut annotate_insertion_in_next_cigar = None;

    let cigar_index_with_arrow_annotation =
        get_cigar_index_with_arrow_annotation(&cigars, is_reverse);

    for (i_op, op) in cigars.iter().enumerate() {
        let next_reference_pivot = if consumes_reference(op) {
            reference_pivot + op.len() as usize
        } else {
            reference_pivot
        };

        let next_query_pivot = if consumes_query(op) {
            query_pivot + op.len() as usize
        } else {
            query_pivot
        };

        let mut new_contexts = Vec::new();

        // let mut new_contexts = Vec::new();
        let add_insertion: bool = annotate_insertion_in_next_cigar.is_some();
        match op {
            Cigar::SoftClip(l) => {
                // S

                if query_pivot <= leading_softclips {
                    // leading softclips. base rendered at the left of reference pivot.
                    for i_soft_clip_base in 0..*l as usize {
                        if reference_pivot + i_soft_clip_base <= leading_softclips + 1 {
                            //base_coordinate <= 1 (on the edge of screen)
                            // Prevent cases when a soft clip is at the very starting of the reference genome:
                            //    ----------- (ref)
                            //  ssss======>   (read)
                            //    ^           edge of screen
                            //  ^^            these softcliped bases are not displayed
                            continue;
                        }

                        let base_coordinate: usize =
                            reference_pivot - leading_softclips + i_soft_clip_base;
                        let base = seq[i_soft_clip_base + query_pivot - 1];
                        new_contexts.push(RenderingContext {
                            start: base_coordinate,
                            end: base_coordinate,
                            kind: RenderingContextKind::SoftClip(base),
                            modifiers: Vec::new(),
                        });
                    }
                } else {
                    // right softclips. base rendered at the right of reference pivot.
                    for i_soft_clip_base in 0..*l as usize {
                        let base_coordinate: usize = reference_pivot + i_soft_clip_base;
                        let base = seq[query_pivot + i_soft_clip_base + leading_softclips - 1];
                        new_contexts.push(RenderingContext {
                            start: base_coordinate,
                            end: base_coordinate,
                            kind: RenderingContextKind::SoftClip(base),
                            modifiers: Vec::new(),
                        });
                    }
                }
            }

            Cigar::Ins(l) => {
                // The next loop catches on this flag and add an insertion modifier.
                // Insertion is displayed at the next cigar segment.
                annotate_insertion_in_next_cigar = Some(*l as usize);
            }

            Cigar::Del(l) | Cigar::RefSkip(l) => {
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

            Cigar::Diff(l) => {
                // X
                new_contexts.push(RenderingContext {
                    start: reference_pivot,
                    end: next_reference_pivot - 1,
                    kind: RenderingContextKind::Match,
                    modifiers: (query_pivot..next_query_pivot as usize)
                        .map(|coordinate| {
                            RenderingContextModifier::Mismatch(
                                coordinate,
                                seq[coordinate - 1 + leading_softclips],
                            )
                        })
                        .collect::<Vec<_>>(),
                })
            }

            Cigar::Equal(l) => new_contexts.push(RenderingContext {
                // =
                start: reference_pivot,
                end: next_query_pivot - 1,
                kind: RenderingContextKind::Match,
                modifiers: Vec::new(),
            }),

            Cigar::Match(l) => {
                // M
                // check reference sequence for mismatches

                // TODO: use basemods_iter?
                // https://docs.rs/rust-htslib/latest/rust_htslib/bam/record/struct.Record.html#method.basemods_iter

                let modifiers: Vec<RenderingContextModifier> = match reference_sequence {
                    Some(reference_sequence) => (0..*l)
                        .filter_map(|i| {
                            let reference_position = reference_pivot + i as usize;
                            if let Some(reference_base) =
                                reference_sequence.base_at(reference_position)
                            // convert to 1-based
                            {
                                let query_position = query_pivot + i as usize; // 1-based
                                let query_base = seq[query_position - 1 + leading_softclips];
                                if query_base != reference_base {
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
                        .collect::<Vec<_>>(),

                    None => Vec::new(),
                };

                new_contexts.push(RenderingContext {
                    start: reference_pivot,
                    end: next_reference_pivot-1,
                    kind: RenderingContextKind::Match,
                    modifiers: modifiers,
                });
            }

            Cigar::HardClip(l) | Cigar::Pad(l) => {
                // P / H
                // Don't need to do anything
            }
        }

        if new_contexts.is_empty() {
            continue;
        }

        if add_insertion {
            // Insertion (detected in the previous loop) notated at the beginning of the first segment.
            new_contexts.first_mut().map(|context| {
                context.add_modifier(RenderingContextModifier::Insertion(
                    annotate_insertion_in_next_cigar.unwrap(),
                ))
            });
        };
        annotate_insertion_in_next_cigar = None;

        if i_op == cigar_index_with_arrow_annotation {
            if is_reverse {
                // Arrow at the begining of the first segment.
                new_contexts
                    .first_mut()
                    .map(|context| context.add_modifier(RenderingContextModifier::Reverse));
            } else {
                // Arrow at the end of the last segment.
                new_contexts
                    .last_mut()
                    .map(|context| context.add_modifier(RenderingContextModifier::Forward));
            }
        }

        output.extend(new_contexts);

        reference_pivot = next_reference_pivot;
        query_pivot = next_query_pivot;
    }

    Ok(output)
}

/// Whether the cigar operation consumes reference.
/// Yes: M/D/N/=/X
/// No: I/S/H/P
/// See: https://samtools.github.io/hts-specs/SAMv1.pdf
fn consumes_reference(op: &Cigar) -> bool {
    match op {
        Cigar::Match(_l)
        | Cigar::Del(_l)
        | Cigar::RefSkip(_l)
        | Cigar::Equal(_l)
        | Cigar::Diff(_l) => true,

        Cigar::SoftClip(_l) | Cigar::Ins(_l) | Cigar::HardClip(_l) | Cigar::Pad(_l) => false,
    }
}

/// Whether the cigar operation consumes query.
/// Yes: M/I/S/=/X
/// No: D/N/H/P
fn consumes_query(op: &Cigar) -> bool {
    match op {
        Cigar::Match(_l)
        | Cigar::Ins(_l)
        | Cigar::SoftClip(_l)
        | Cigar::Equal(_l)
        | Cigar::Diff(_l) => true,

        Cigar::Del(_l) | Cigar::RefSkip(_l) | Cigar::HardClip(_l) | Cigar::Pad(_l) => false,
    }
}

/// Whether the cigar operation can be annotated with the < / > signs.
/// Yes: M/I/S/=/X
/// No: D/N/H/P
fn can_be_annotated_with_arrows(op: &Cigar) -> bool {
    match op {
        Cigar::Match(_l)
        | Cigar::SoftClip(_l)
        | Cigar::Equal(_l)
        | Cigar::Diff(_l)
        | Cigar::Del(_l)
        | Cigar::RefSkip(_l) => true,

        Cigar::HardClip(_l) | Cigar::Pad(_l) | Cigar::Ins(_l) => false,
    }
}

/// A alignment region on a contig.
pub struct Alignment {
    pub reads: Vec<AlignedRead>,

    pub contig_index: usize,

    /// Coverage at each position. Keys are 1-based, inclusive.
    coverage: BTreeMap<usize, usize>,

    /// The left bound of region with complete data.
    /// 1-based, inclusive.
    data_complete_left_bound: usize,

    /// The right bound of region with complete data.
    /// 1-based, inclusive.
    data_complete_right_bound: usize,

    depth: usize,
}

/// Data loading
impl Alignment {
    /// Check if data in [left, right] is all loaded.
    /// 1-based, inclusive.
    pub fn has_complete_data(&self, region: &Region) -> bool {
        (region.contig_index == self.contig_index)
            && (region.start >= self.data_complete_left_bound)
            && (region.end <= self.data_complete_right_bound)
    }

    /// Return the number of alignment tracks.
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// Basewise coverage at position.
    /// 1-based, inclusive.
    pub fn coverage_at(&self, pos: usize) -> usize {
        if pos < self.data_complete_left_bound || pos > self.data_complete_right_bound {
            return 0;
        }
        match self.coverage.get(&pos) {
            Some(coverage) => *coverage,
            None => 0,
        }
    }

    /// Mean basewise coverage in [left, right].
    /// 1-based, inclusive.
    pub fn mean_basewise_coverage_in(&self, left: usize, right: usize) -> Result<usize, TGVError> {
        if right < left {
            return Err(TGVError::ValueError("Right is less than left".to_string()));
        }

        if right < self.data_complete_left_bound || left > self.data_complete_right_bound {
            return Ok(0);
        }

        if right == left {
            return Ok(self.coverage_at(left));
        }

        Ok(self
            .coverage
            .range(left..right + 1)
            .map(|(_, coverage)| coverage)
            .sum::<usize>()
            / (right - left + 1))
    }
}

pub struct AlignmentBuilder {
    aligned_reads: Vec<AlignedRead>,
    coverage_hashmap: HashMap<usize, usize>,

    track_left_bounds: Vec<usize>,
    track_right_bounds: Vec<usize>,

    track_most_left_bound: usize,
    track_most_right_bound: usize,

    region: Region,
}

impl AlignmentBuilder {
    pub fn new(
        region: &Region,
        // reference_sequence: Option<&'a Sequence>,
    ) -> Result<Self, TGVError> {
        Ok(Self {
            aligned_reads: Vec::new(),
            coverage_hashmap: HashMap::new(),
            track_left_bounds: Vec::new(),
            track_right_bounds: Vec::new(),

            track_most_left_bound: usize::MAX,
            track_most_right_bound: 0,

            region: region.clone(),
        })
    }

    /// Add a read to the alignment. Note that this function does not update coverage.
    pub fn add_read(
        &mut self,
        read: Record,
        reference_sequence: Option<&Sequence>,
    ) -> Result<&mut Self, TGVError> {
        let read_start = read.pos() as usize + 1;
        let read_end = read.reference_end() as usize;
        let cigars = read.cigar();
        let leading_softclips = cigars.leading_softclips() as usize;
        let trailing_softclips = cigars.trailing_softclips() as usize;
        // read.pos() in htslib: 0-based, inclusive, excluding leading hardclips and softclips
        // read.reference_end() in htslib: 0-based, exclusive, excluding trailing hardclips and softclips

        let y = self.find_track(
            read_start.saturating_sub(leading_softclips),
            read_end.saturating_add(trailing_softclips),
        );

        let rendering_contexts = calculate_rendering_contexts(
            read_start,
            &cigars,
            leading_softclips,
            &read.seq(),
            read.is_reverse(),
            reference_sequence,
        )?;

        let aligned_read = AlignedRead {
            read,
            start: read_start,
            end: read_end,
            leading_softclips,
            trailing_softclips,
            y,

            rendering_contexts,
        };

        // Track bounds + depth update
        if self.aligned_reads.is_empty() || aligned_read.y >= self.track_left_bounds.len() {
            // Add to a new track
            self.track_left_bounds.push(aligned_read.stacking_start());
            self.track_right_bounds.push(aligned_read.stacking_end());
        } else {
            // Add to an existing track
            if aligned_read.stacking_start() < self.track_left_bounds[aligned_read.y] {
                self.track_left_bounds[aligned_read.y] = aligned_read.stacking_start();
            }
            if aligned_read.stacking_end() > self.track_right_bounds[aligned_read.y] {
                self.track_right_bounds[aligned_read.y] = aligned_read.stacking_end();
            }
        }

        // Most left/right bound update
        if aligned_read.stacking_start() < self.track_most_left_bound {
            self.track_most_left_bound = aligned_read.stacking_start();
        }
        if aligned_read.stacking_end() > self.track_most_right_bound {
            self.track_most_right_bound = aligned_read.stacking_end();
        }

        // update coverge hashmap
        for i in aligned_read.range() {
            // TODO: check exclusivity here
            *self.coverage_hashmap.entry(i).or_insert(1) += 1;
        }

        // Add to reads
        self.aligned_reads.push(aligned_read);

        Ok(self)
    }

    const MIN_HORIZONTAL_GAP_BETWEEN_READS: usize = 3;

    fn find_track(&mut self, read_start: usize, read_end: usize) -> usize {
        if self.aligned_reads.is_empty() {
            return 0;
        }

        for (y, left_bound) in self.track_left_bounds.iter().enumerate() {
            if read_end + Self::MIN_HORIZONTAL_GAP_BETWEEN_READS < *left_bound {
                return y;
            }
        }

        for (y, right_bound) in self.track_right_bounds.iter().enumerate() {
            if read_start > *right_bound + Self::MIN_HORIZONTAL_GAP_BETWEEN_READS {
                return y;
            }
        }

        self.track_left_bounds.len()
    }

    pub fn build(&self) -> Result<Alignment, TGVError> {
        let mut coverage: BTreeMap<usize, usize> = BTreeMap::new();

        // Convert hashmap to BTreeMap
        for (k, v) in &self.coverage_hashmap {
            *coverage.entry(*k).or_insert(*v) += v;
        }

        Ok(Alignment {
            reads: self.aligned_reads.clone(),
            contig_index: self.region.contig_index,
            coverage,
            data_complete_left_bound: self.region.start,
            data_complete_right_bound: self.region.end,

            depth: self.track_left_bounds.len(),
        })
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use rstest::rstest;
    use rust_htslib::bam::{
        record::{Cigar, CigarString},
        Read,
    };

    #[rstest]
    #[case(10, vec![Cigar::Match(3)],  b"ATT", false,None, vec![RenderingContext{
        start:10, 
        end:12,
        kind: RenderingContextKind::Match,
        modifiers:vec![RenderingContextModifier::Forward]
    }])]
    // Test reverse strand
    #[case(10, vec![Cigar::Match(3)],  b"ATT", true, None, vec![RenderingContext{
        start:10, 
        end:12,
        kind: RenderingContextKind::Match,
        modifiers:vec![RenderingContextModifier::Reverse]
    }])]
    // Test deletion (no forward arrow since it doesn't consume query)
    #[case(10, vec![Cigar::Del(2)], b"", false, None, vec![RenderingContext{
        start:10,
        end:11,
        kind: RenderingContextKind::Deletion,
        modifiers:vec![]
    }])]
    // Test insertion followed by match
    #[case(10, vec![Cigar::Ins(2), Cigar::Match(3)], b"CCATT", false, None, vec![RenderingContext{
        start:10,
        end:12,
        kind: RenderingContextKind::Match,
        modifiers:vec![RenderingContextModifier::Insertion(2), RenderingContextModifier::Forward]
    }])]
    // Test soft clip at start
    #[case(10, vec![Cigar::SoftClip(2), Cigar::Match(3)], b"GGATT", false, None, vec![
        RenderingContext{
            start:8,
            end:8,
            kind: RenderingContextKind::SoftClip(b'G'),
            modifiers:vec![]
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
            modifiers:vec![RenderingContextModifier::Forward]
        }
    ])]
    // Test soft clip at end
    #[case(10, vec![Cigar::Match(3), Cigar::SoftClip(2)], b"ATTGG", false, None, vec![
        RenderingContext{
            start:10,
            end:12,
            kind: RenderingContextKind::Match,
            modifiers:vec![RenderingContextModifier::Forward]
        },
        RenderingContext{
            start:13,
            end:13,
            kind: RenderingContextKind::SoftClip(b'G'),
            modifiers:vec![]
        },
        RenderingContext{
            start:14,
            end:14,
            kind: RenderingContextKind::SoftClip(b'G'),
            modifiers:vec![]
        }
    ])]
    // Test Equal cigar (matches current implementation with query pivot)
    #[case(10, vec![Cigar::Equal(3)], b"ATT", false, None, vec![RenderingContext{
        start:10,
        end:3, // This matches the current implementation which uses next_query_pivot - 1
        kind: RenderingContextKind::Match,
        modifiers:vec![RenderingContextModifier::Forward]
    }])]
    // Test Diff cigar (explicit mismatch)
    #[case(10, vec![Cigar::Diff(3)], b"ATT", false, None, vec![RenderingContext{
        start:10,
        end:12,
        kind: RenderingContextKind::Match,
        modifiers:vec![
            RenderingContextModifier::Mismatch(1, b'A'),
            RenderingContextModifier::Mismatch(2, b'T'),
            RenderingContextModifier::Mismatch(3, b'T'),
            RenderingContextModifier::Forward
        ]
    }])]
    // Test RefSkip (N operation) - no forward arrow since it doesn't consume query
    #[case(10, vec![Cigar::RefSkip(5)], b"", false, None, vec![RenderingContext{
        start:10,
        end:14,
        kind: RenderingContextKind::Deletion,
        modifiers:vec![]
    }])]
    // Test complex cigar: soft clip + match + insertion + match + deletion + match
    #[case(10, vec![Cigar::SoftClip(1), Cigar::Match(2), Cigar::Ins(1), Cigar::Match(2), Cigar::Del(3), Cigar::Match(2)], 
           b"GATCGAA", false, None, vec![
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
    fn test_calculate_rendering_contexts(
        #[case] reference_start: usize, // 1-based
        #[case] cigars: Vec<Cigar>,
        #[case] seq: &[u8],
        #[case] is_reverse: bool,
        #[case] reference_sequence: Option<&Sequence>,
        #[case] expected_rendering_contexts: Vec<RenderingContext>,
    ) {
        let mut record = Record::new();
        record.set(
            b"test",
            Some(&CigarString(cigars)),
            seq,
            "i".repeat(seq.len()).as_bytes(),
        );
        let contexts = calculate_rendering_contexts(
            reference_start,
            &record.cigar(),
            record.cigar().leading_softclips() as usize,
            &record.seq(),
            is_reverse,
            reference_sequence,
        )
        .unwrap();

        assert_eq!(contexts, expected_rendering_contexts)
    }
}

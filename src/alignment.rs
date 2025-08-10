use crate::error::TGVError;
use crate::sequence::Sequence;
use crate::{contig::Contig, region::Region};
use rust_htslib::bam::ext::BamRecordExtensions;
use rust_htslib::bam::record::{Cigar, CigarStringView};
use rust_htslib::bam::{Read, Record};
use sqlx::query;
use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Debug)]
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
#[derive(Clone, Debug)]
pub enum RenderingContextKind {
    SoftClip(u8),

    Match, // Mismatches are annotated with modifiers

    Deltion,
}
/// Information on how to display the read on screen. Each context represent a segment on screen.
/// Parsed from the cigar string.
#[derive(Clone, Debug)]
pub struct RenderingContext {
    /// Start coordinate of a displayed segment
    pub start: usize,

    /// End coordinates of a displayed segmenbt
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
    read: &Record,
    start: usize,
    end: usize,
    leading_softclips: usize,
    trailing_softclips: usize,
    reference_sequence: Option<&Sequence>,
) -> Result<Vec<RenderingContext>, TGVError> {
    let mut output: Vec<RenderingContext> = Vec::new();

    let mut reference_pivot: usize = start; // used in the output
    let mut query_pivot: usize = 0; // # bases relative to the softclip start.

    let is_reverse = read.is_reverse();

    let cigars = read.cigar();
    let seq = read.seq();

    if cigars.len() == 0 {
        return Ok(output);
    }

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
                // TODO:
                // 1x zoom: display color
                // 2x zoom: half-block rendering
                // higher zoom: whole block color? half-block to the best ability? Think about this.
                for i_base in query_pivot..query_pivot + *l as usize {
                    // Prevent cases when a soft clip is at the very starting of the reference genome:
                    //    ----------- (ref)
                    //  ssss======>   (read)
                    //    ^           edge of screen
                    //  ^^            these softcliped bases are not displayed
                    if reference_pivot + i_base < 1 + leading_softclips {
                        continue;
                    }

                    let base_coordinate: usize = reference_pivot + i_base - leading_softclips;
                    let base = seq[i_base];
                    new_contexts.push(RenderingContext {
                        start: base_coordinate,
                        end: base_coordinate,
                        kind: RenderingContextKind::SoftClip(base),
                        modifiers: Vec::new(),
                    });
                }
            }

            Cigar::Ins(l) => {
                annotate_insertion_in_next_cigar = Some(*l as usize);
                // TODO: draw the insertion.
            }

            Cigar::Del(l) | Cigar::RefSkip(l) => {
                // D / N
                // ---------------- ref
                // ===----===       read (lines with no bckground colors)
                new_contexts.push(RenderingContext {
                    start: reference_pivot,
                    end: next_reference_pivot,
                    kind: RenderingContextKind::Deltion,
                    modifiers: Vec::new(),
                });
            }

            Cigar::Diff(l) => {
                // X
                // TODO:
                // 1x zoom: Display base letter + color
                // 2x zoom: (?) Half-base rendering with mismatch color
                //

                new_contexts.push(RenderingContext {
                    start: reference_pivot,
                    end: next_reference_pivot,
                    kind: RenderingContextKind::Match,
                    modifiers: (query_pivot..next_query_pivot as usize)
                        .map(|coordinate| {
                            RenderingContextModifier::Mismatch(coordinate, seq[coordinate])
                        })
                        .collect::<Vec<_>>(),
                })

                // if let Some((x, length)) = OnScreenCoordinate::onscreen_start_and_length(
                //     &viewing_window.onscreen_x_coordinate(reference_pivot, area),
                //     &viewing_window.onscreen_x_coordinate(next_reference_pivot, area),
                //     area,
                // ) {
                //     new_contexts.push(RenderingContext {
                //         x: x,
                //         y: onscreen_y,
                //         string: " ".repeat(length as usize),
                //         style: Style::new().bg(pallete.MISMATCH_COLOR),
                //     })
                // }
            }

            Cigar::Equal(l) => new_contexts.push(RenderingContext {
                // =
                start: reference_pivot,
                end: next_query_pivot,
                kind: RenderingContextKind::Match,
                modifiers: Vec::new(),
            }),

            Cigar::Match(l) => {
                // M
                // check reference sequence for mismatches

                let modifiers: Vec<RenderingContextModifier> = match reference_sequence {
                    Some(reference_sequence) => (0..*l)
                        .filter_map(|i| {
                            if let Some(reference_base) =
                                reference_sequence.base_at(reference_pivot + i as usize)
                            {
                                let query_position = query_pivot + i as usize;
                                let query_base = seq[query_position];
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
                    end: next_reference_pivot,
                    kind: RenderingContextKind::Match,
                    modifiers: modifiers,
                });
            }

            Cigar::HardClip(l) | Cigar::Pad(l) => {
                // P / H
                // Don't need to do anything
                //continue;
            }
        }

        if new_contexts.is_empty() {
            continue;
        }

        if add_insertion {
            new_contexts.first_mut().map(|context| {
                context.add_modifier(RenderingContextModifier::Insertion(
                    annotate_insertion_in_next_cigar.unwrap(),
                ))
            });
        };
        annotate_insertion_in_next_cigar = None;

        if i_op == cigar_index_with_arrow_annotation {
            if is_reverse {
                new_contexts
                    .first_mut()
                    .map(|context| context.add_modifier(RenderingContextModifier::Reverse));
            } else {
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

    pub contig: usize, // contig name

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
        (region.contig == self.contig)
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

    /// TODO: read contig headers

    /// Add a read to the alignment. Note that this function does not update coverage.
    pub fn add_read(
        &mut self,
        read: Record,
        reference_sequence: Option<&Sequence>,
    ) -> Result<&mut Self, TGVError> {
        let read_start = read.pos() as usize + 1;
        let read_end = read.reference_end() as usize;
        let leading_softclips = read.cigar().leading_softclips() as usize;
        let trailing_softclips = read.cigar().trailing_softclips() as usize;
        // read.pos() in htslib: 0-based, inclusive, excluding leading hardclips and softclips
        // read.reference_end() in htslib: 0-based, exclusive, excluding trailing hardclips and softclips

        let y = self.find_track(
            read_start.saturating_sub(leading_softclips),
            read_end.saturating_add(trailing_softclips),
        );

        let rendering_contexts = calculate_rendering_contexts(
            &read,
            read_start,
            read_end,
            leading_softclips,
            trailing_softclips,
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
            reads: self.aligned_reads.clone(), // TODO: lookup on how to move this
            contig: self.region.contig.clone(),
            coverage,
            data_complete_left_bound: self.region.start,
            data_complete_right_bound: self.region.end,

            depth: self.track_left_bounds.len(),
        })
    }
}

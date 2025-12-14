use crate::error::TGVError;
use crate::sequence::Sequence;
use noodles::bam::record::{self};
use noodles::sam::alignment::record::cigar::{Op, op::Kind};
use std::collections::HashMap;
use std::default::Default;

/// See: https://samtools.github.io/hts-specs/SAMv1.pdf
pub fn calculate_basewise_coverage(
    reference_start: u64, // 1-based. Alignment start, not softclip start
    cigars: &Vec<Op>,
    sequence: &record::Sequence,
    reference_sequence: &Sequence,
) -> Result<HashMap<usize, BaseCoverage>, TGVError> {
    let mut output: HashMap<usize, BaseCoverage> = HashMap::new();
    if cigars.is_empty() {
        return Ok(output);
    }

    let mut reference_pivot: usize = reference_start as usize;
    let mut query_pivot: usize = 1; // 1-based. # bases on the sequence. Note that need to substract leading softclips to get aligned base coordinate.

    // FIXME:
    // Mismatches are re-calculated by comparing with the reference genome, but BAM has MM/ML tags for this.
    for (i_op, op) in cigars.iter().enumerate() {
        let kind = op.kind();
        let len = op.len();
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

        match kind {
            Kind::SoftClip => {
                // S
                if i_op == 0 {
                    // leading softclips. base rendered at the left of reference pivot.
                    for i_soft_clip_base in 0..len {
                        if reference_pivot + i_soft_clip_base <= len + 1 {
                            //base_coordinate <= 1 (on the edge of screen)
                            // Prevent cases when a soft clip is at the very starting of the reference genome:
                            //    ----------- (ref)
                            //  ssss======>   (read)
                            //    ^           edge of screen
                            //  ^^            these softcliped bases are not displayed
                            continue;
                        }

                        let base_coordinate: usize = reference_pivot - len + i_soft_clip_base;
                        let base = sequence.get(i_soft_clip_base).unwrap();

                        output
                            .entry(base_coordinate)
                            .or_insert(BaseCoverage::new(
                                // FIXME: This can cause problems when sequence cache didn't catch up with alignment.
                                reference_sequence
                                    .base_at(base_coordinate as u64)
                                    .unwrap_or(b'N'),
                            ))
                            .update_softclip(base)
                    }
                } else {
                    // right softclips. base rendered at the right of reference pivot.
                    for i_soft_clip_base in 0..len {
                        let base_coordinate: usize = reference_pivot + i_soft_clip_base;
                        let base = sequence.get(query_pivot + i_soft_clip_base - 1).unwrap();
                        output
                            .entry(base_coordinate)
                            .or_insert(BaseCoverage::new(
                                // FIXME: This can cause problems when sequence cache didn't catch up with alignment.
                                reference_sequence
                                    .base_at(base_coordinate as u64)
                                    .unwrap_or(b'N'),
                            ))
                            .update_softclip(base);
                    }
                }
            }

            Kind::Insertion => {}

            Kind::Deletion | Kind::Skip => {}

            Kind::SequenceMismatch | Kind::SequenceMatch | Kind::Match => {
                for i in 0..len {
                    let base_coordinate = reference_pivot + i;
                    output
                        .entry(base_coordinate)
                        .or_insert(BaseCoverage::new(
                            // FIXME: This can cause problems when sequence cache didn't catch up with alignment.
                            reference_sequence
                                .base_at(base_coordinate as u64)
                                .unwrap_or(b'N'),
                        ))
                        .update(sequence.get(query_pivot + i - 1).unwrap())
                }
            }
            Kind::HardClip | Kind::Pad => {}
        }

        query_pivot = next_query_pivot;
        reference_pivot = next_reference_pivot;
    }

    Ok(output)
}

#[derive(Clone, Debug)]
#[allow(non_snake_case)]
pub struct BaseCoverage {
    pub A: usize,
    pub T: usize,
    pub C: usize,
    pub G: usize,

    pub N: usize,

    // total coverage, exluding softclips
    pub total: usize,

    // Softclip count
    pub softclip: usize,

    // reference_base
    pub reference_base: u8,
}

impl BaseCoverage {
    pub const MAX_DISPLAY_ALLELE_FREQUENCY_RECIPROCOL: usize = 100;
    pub fn new(reference_base: u8) -> Self {
        Self {
            A: 0,
            T: 0,
            C: 0,
            G: 0,
            N: 0,
            total: 0,
            softclip: 0,
            reference_base,
        }
    }

    pub fn update(&mut self, base: u8) {
        match base {
            b'A' | b'a' => self.A += 1,
            b'T' | b't' => self.T += 1,
            b'C' | b'c' => self.C += 1,
            b'G' | b'g' => self.G += 1,

            _ => self.N += 1,
        }

        self.total += 1;
    }

    pub fn update_softclip(&mut self, base: u8) {
        self.softclip += 1
    }

    pub fn add(&mut self, other: &BaseCoverage) {
        self.A += other.A;
        self.T += other.T;
        self.C += other.C;
        self.G += other.G;
        self.total += other.total;
        self.softclip += other.softclip;
    }

    pub fn max_alt_depth(&self) -> Option<usize> {
        match self.reference_base {
            b'A' | b'a' => Some(usize::max(self.C, self.T)),
            b'T' | b't' => Some(usize::max(self.A, self.C)),
            b'C' | b'c' => Some(usize::max(self.A, self.T)),
            b'G' | b'g' => Some(usize::max(self.C, self.T)),
            _ => None,
        }
    }

    pub fn describe(&self) -> String {
        format!(
            "A:{}, T:{}, C:{}, G:{}, N:{}, total:{}",
            self.A, self.T, self.C, self.G, self.N, self.total
        )
    }
}

impl Default for BaseCoverage {
    fn default() -> Self {
        DEFAULT_COVERAGE.clone()
    }
}

pub static DEFAULT_COVERAGE: BaseCoverage = BaseCoverage {
    A: 0,
    T: 0,
    C: 0,
    G: 0,
    N: 0,
    total: 0,
    softclip: 0,
    reference_base: b'N',
};

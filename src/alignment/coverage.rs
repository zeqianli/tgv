use crate::alignment::read::{consumes_query, consumes_reference, AlignedRead};
use crate::error::TGVError;
use crate::message::{AlignmentFilter, AlignmentSort};
use crate::reference;
use crate::region::Region;
use crate::sequence::Sequence;
use crate::window::ViewingWindow;
use ratatui::layout::Rect;
use rust_htslib::bam::ext::BamRecordExtensions;
use rust_htslib::bam::record::{Cigar, CigarStringView};
use rust_htslib::bam::{record::Seq, Read, Record};
use std::collections::{hash_map::Entry, BTreeMap, HashMap};
use std::default::Default;

/// See: https://samtools.github.io/hts-specs/SAMv1.pdf
pub fn calculate_basewise_coverage(
    reference_start: usize, // 1-based. Alignment start, not softclip start
    cigars: &CigarStringView,
    leading_softclips: usize,
    seq: &Seq,
    reference_sequence: Option<&Sequence>,
) -> Result<HashMap<usize, BaseCoverage>, TGVError> {
    let mut output: HashMap<usize, BaseCoverage> = HashMap::new();
    if cigars.len() == 0 {
        return Ok(output);
    }

    let mut reference_pivot: usize = reference_start;
    let mut query_pivot: usize = 1; // 1-based. # bases on the sequence. Note that need to substract leading softclips to get aligned base coordinate.

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
                        let base = seq[i_soft_clip_base];

                        output
                            .entry(base_coordinate)
                            .or_insert(BaseCoverage::new(match reference_sequence {
                                Some(sequence) => sequence.base_at(base_coordinate).ok_or(
                                    TGVError::ValueError(format!(
                                        "Sequence not loaded for {}",
                                        base_coordinate
                                    )),
                                )?,
                                None => b'N',
                            }))
                            .update_softclip(base)
                    }
                } else {
                    // right softclips. base rendered at the right of reference pivot.
                    for i_soft_clip_base in 0..*l as usize {
                        let base_coordinate: usize = reference_pivot + i_soft_clip_base;
                        let base = seq[query_pivot + i_soft_clip_base - 1];
                        output
                            .entry(base_coordinate)
                            .or_insert(BaseCoverage::new(match reference_sequence {
                                Some(sequence) => sequence.base_at(base_coordinate).ok_or(
                                    TGVError::ValueError(format!(
                                        "Sequence not loaded for {}",
                                        base_coordinate
                                    )),
                                )?,
                                None => b'N',
                            }))
                            .update_softclip(base);
                    }
                }
            }

            Cigar::Ins(l) => {}

            Cigar::Del(l) | Cigar::RefSkip(l) => {}

            Cigar::Diff(l) | Cigar::Equal(l) | Cigar::Match(l) => {
                for i in 0..*l as usize {
                    let base_coordinate = reference_pivot + i;
                    output
                        .entry(base_coordinate)
                        .or_insert(BaseCoverage::new(match reference_sequence {
                            Some(sequence) => {
                                sequence
                                    .base_at(base_coordinate)
                                    .ok_or(TGVError::ValueError(format!(
                                        "Sequence not loaded for {}",
                                        base_coordinate
                                    )))?
                            }
                            None => b'N',
                        }))
                        .update(seq[query_pivot + i - 1])
                }
            }
            Cigar::HardClip(l) | Cigar::Pad(l) => {}
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

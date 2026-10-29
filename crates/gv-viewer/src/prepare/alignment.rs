//! What an alignment track shows in a region: the visible reads and their annotations, in bases
//! and stacking rows, in the order they are drawn.
//!
//! Each list preserves source-table order, so a front end that draws them in order lets later
//! records overwrite earlier ones.

use gv_core::{
    alignment::{
        Alignment, AlignmentTables, PairSchema, PairedAlignment,
        tables::{BaseModificationSchema, CigarSchema, ReadSchema, ReferenceMismatchSchema},
    },
    prelude::*,
};
use itertools::izip;
use noodles::sam::record::data::field::value::base_modifications::group::Modification;
use polars::prelude::*;

/// A span of bases on one stacking row, 1-based and inclusive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowSpan {
    pub y: u64,
    pub start: u64,
    pub end: u64,
}

/// A read's aligned body, from one CIGAR run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Body {
    pub span: RowSpan,
    /// Reads with MAPQ 0 map equally well elsewhere, so they are drawn faded, as in IGV.
    pub zero_mapq: bool,
}

/// A CIGAR run with read bases: a soft clip or a sequence mismatch (`X`) run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaseRun {
    pub span: RowSpan,
    /// The offset into `sequence` of the base at `span.start`.
    pub offset: usize,
    pub sequence: Vec<u8>,
    /// Phred qualities by offset into `sequence`, when the read has them.
    pub qualities: Option<Vec<Option<u8>>>,
}

impl BaseRun {
    /// The read base at a position of the run.
    pub fn base_at(&self, position: u64) -> u8 {
        self.sequence[self.offset + (position - self.span.start) as usize]
    }

    /// The quality of the base at a position of the run.
    pub fn quality_at(&self, position: u64) -> Option<u8> {
        let offset = self.offset + (position - self.span.start) as usize;
        self.qualities
            .as_ref()
            .and_then(|qualities| qualities.get(offset).copied().flatten())
    }
}

/// The strand arrow at a read's end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arrow {
    pub y: u64,
    pub position: u64,
    pub reverse: bool,
}

/// One base on one row, such as an insertion or a mismatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaseMark {
    pub y: u64,
    pub position: u64,
    pub base: u8,
    pub quality: Option<u8>,
}

/// A base modification call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModificationMark {
    pub y: u64,
    pub position: u64,
    pub modification: Modification,
    /// 0 to 255, from the ML tag.
    pub probability: u8,
}

/// Everything an alignment track draws in a region, in drawing order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AlignmentScene {
    /// The gaps between paired mates, drawn first. Clipped to the region.
    pub pair_gaps: Vec<RowSpan>,
    pub bodies: Vec<Body>,
    pub soft_clips: Vec<BaseRun>,
    /// Deletions and reference skips.
    pub deletions: Vec<RowSpan>,
    /// Arrows come after deletions, so a read that ends in a deletion keeps its strand marker.
    pub arrows: Vec<Arrow>,
    pub insertions: Vec<RowSpan>,
    pub mismatch_runs: Vec<BaseRun>,
    pub mismatches: Vec<BaseMark>,
    pub modifications: Vec<ModificationMark>,
}

impl AlignmentScene {
    /// The reads of `alignment` over `region`, on the stacking rows `top..top + rows`. With
    /// `paired`, a pair is shown when either mate passes the display options, and then both
    /// mates are drawn, so a pair always stays together.
    pub fn new(
        alignment: &Alignment,
        paired: Option<&PairedAlignment>,
        region: &Region,
        top: usize,
        rows: usize,
    ) -> Result<Self, TGVError> {
        let mut scene = Self::default();
        if rows == 0 || region.contig_index() != alignment.contig_index {
            return Ok(scene);
        }
        let bottom = (top + rows) as u64;
        let reads = match paired {
            None => alignment.tables.reads.clone().lazy().filter(
                col(ReadSchema::SHOW)
                    .and(col(ReadSchema::STACKING_START).lt_eq(lit(region.end())))
                    .and(col(ReadSchema::STACKING_END).gt_eq(lit(region.start())))
                    .and(col(ReadSchema::Y).gt_eq(lit(top as u64)))
                    .and(col(ReadSchema::Y).lt(lit(bottom))),
            ),
            Some(paired) => {
                let visible = col(PairSchema::SHOW)
                    .and(col(PairSchema::STACKING_START).lt_eq(lit(region.end())))
                    .and(col(PairSchema::STACKING_END).gt_eq(lit(region.start())))
                    .and(col(PairSchema::Y).gt_eq(lit(top as u64)))
                    .and(col(PairSchema::Y).lt(lit(bottom)));
                let pairs = paired
                    .pairs
                    .clone()
                    .lazy()
                    .filter(visible.clone())
                    .collect()?;
                scene.pair_gaps = pair_gaps(&alignment.tables, &pairs, region)?;
                let pairs = pairs.lazy();
                let members = concat(
                    [
                        pairs.clone().select([
                            col(PairSchema::READ_1_ID).alias(ReadSchema::READ_ID),
                            col(PairSchema::Y),
                        ]),
                        pairs
                            .filter(col(PairSchema::READ_2_ID).is_not_null())
                            .select([
                                col(PairSchema::READ_2_ID).alias(ReadSchema::READ_ID),
                                col(PairSchema::Y),
                            ]),
                        paired
                            .singles
                            .clone()
                            .lazy()
                            .filter(visible)
                            .select([col(ReadSchema::READ_ID), col(ReadSchema::Y)]),
                    ],
                    UnionArgs::default(),
                )?;
                // Members are already limited to shown pairs and singletons.
                alignment
                    .tables
                    .reads
                    .clone()
                    .lazy()
                    .drop(cols([ReadSchema::Y]))
                    .join(
                        members,
                        [col(ReadSchema::READ_ID)],
                        [col(ReadSchema::READ_ID)],
                        JoinArgs {
                            maintain_order: MaintainOrderJoin::Left,
                            ..JoinArgs::new(JoinType::Inner)
                        },
                    )
            }
        };
        scene.add_reads(&alignment.tables, reads, region)?;
        Ok(scene)
    }

    fn add_reads(
        &mut self,
        tables: &AlignmentTables,
        reads: LazyFrame,
        region: &Region,
    ) -> Result<(), TGVError> {
        let reads = reads
            .select([
                col(ReadSchema::READ_ID),
                col(ReadSchema::Y),
                col(ReadSchema::REVERSE),
                col(ReadSchema::STACKING_START),
                col(ReadSchema::STACKING_END),
                col(ReadSchema::MAPQ),
            ])
            .collect()?;
        if reads.height() == 0 {
            return Ok(());
        }
        let mut zero_mapq = vec![false; tables.reads.height()];
        for (id, mapq) in izip!(
            reads
                .column(ReadSchema::READ_ID)?
                .u64()?
                .into_no_null_iter(),
            reads.column(ReadSchema::MAPQ)?.u8()?.iter(),
        ) {
            zero_mapq[id as usize] = mapq == Some(0);
        }
        // Read IDs are row indexes into the reads table, so drawn rows are looked up directly
        // instead of joining every annotation table with the drawn reads on each frame.
        let mut read_rows = vec![None; tables.reads.height()];
        for (id, y) in izip!(
            reads
                .column(ReadSchema::READ_ID)?
                .u64()?
                .into_no_null_iter(),
            reads.column(ReadSchema::Y)?.u64()?.into_no_null_iter(),
        ) {
            read_rows[id as usize] = Some(y);
        }

        let runs = with_read_rows(
            tables
                .cigar_runs
                .clone()
                .lazy()
                .filter(
                    col(CigarSchema::DISPLAY_START)
                        .lt_eq(lit(region.end()))
                        .and(col(CigarSchema::DISPLAY_END).gt_eq(lit(region.start()))),
                )
                .select([
                    col(CigarSchema::READ_ID),
                    col(CigarSchema::KIND),
                    col(CigarSchema::DISPLAY_START),
                    col(CigarSchema::DISPLAY_END),
                    col(CigarSchema::RUN_OFFSET),
                    col(CigarSchema::SEQ),
                    col(CigarSchema::QUAL),
                ])
                .collect()?,
            &read_rows,
        )?;

        let run_ys = runs.column(ReadSchema::Y)?.u64()?;
        let run_kinds = runs.column(CigarSchema::KIND)?.u8()?;
        let run_starts = runs.column(CigarSchema::DISPLAY_START)?.u64()?;
        let run_ends = runs.column(CigarSchema::DISPLAY_END)?.u64()?;
        let run_offsets = runs.column(CigarSchema::RUN_OFFSET)?.u32()?;
        let run_sequences = runs.column(CigarSchema::SEQ)?.str()?;
        let run_qualities = runs.column(CigarSchema::QUAL)?.list()?;
        let run_read_ids = runs.column(CigarSchema::READ_ID)?.u64()?;

        for (row, (id, kind, y, start, end, offset, sequence)) in izip!(
            run_read_ids.into_no_null_iter(),
            run_kinds.into_no_null_iter(),
            run_ys.into_no_null_iter(),
            run_starts.into_no_null_iter(),
            run_ends.into_no_null_iter(),
            run_offsets.into_no_null_iter(),
            run_sequences.iter(),
        )
        .enumerate()
        {
            let span = RowSpan { y, start, end };
            match kind {
                CigarSchema::SOFT_CLIP => {
                    if let Some(sequence) = sequence {
                        self.soft_clips.push(BaseRun {
                            span,
                            offset: offset as usize,
                            sequence: sequence.as_bytes().to_vec(),
                            qualities: None,
                        });
                    }
                }
                CigarSchema::DELETION | CigarSchema::REFERENCE_SKIP => {
                    self.bodies.push(Body {
                        span: span.clone(),
                        zero_mapq: zero_mapq[id as usize],
                    });
                    self.deletions.push(span);
                }
                CigarSchema::INSERTION => {
                    self.bodies.push(Body {
                        span: span.clone(),
                        zero_mapq: zero_mapq[id as usize],
                    });
                    self.insertions.push(span);
                }
                CigarSchema::SEQUENCE_MISMATCH => {
                    self.bodies.push(Body {
                        span: span.clone(),
                        zero_mapq: zero_mapq[id as usize],
                    });
                    if let Some(sequence) = sequence {
                        // `X` runs are rare, so their qualities are read only here.
                        let qualities = run_qualities
                            .get_as_series(row)
                            .map(|qualities| -> Result<Vec<Option<u8>>, TGVError> {
                                Ok(qualities.u8()?.iter().collect())
                            })
                            .transpose()?;
                        self.mismatch_runs.push(BaseRun {
                            span,
                            offset: offset as usize,
                            sequence: sequence.as_bytes().to_vec(),
                            qualities,
                        });
                    }
                }
                _ => self.bodies.push(Body {
                    span,
                    zero_mapq: zero_mapq[id as usize],
                }),
            }
        }

        for (y, reverse, start, end) in izip!(
            reads.column(ReadSchema::Y)?.u64()?.into_no_null_iter(),
            reads
                .column(ReadSchema::REVERSE)?
                .bool()?
                .iter()
                .map(|reverse| reverse.expect("read flags are non-null")),
            reads.column(ReadSchema::STACKING_START)?.u64()?.iter(),
            reads.column(ReadSchema::STACKING_END)?.u64()?.iter(),
        ) {
            let (Some(start), Some(end)) = (start, end) else {
                continue;
            };
            self.arrows.push(Arrow {
                y,
                position: if reverse { start } else { end },
                reverse,
            });
        }

        let mismatches = with_read_rows(
            tables
                .reference_mismatches
                .clone()
                .lazy()
                .filter(
                    col(ReferenceMismatchSchema::REF_POS)
                        .lt_eq(lit(region.end()))
                        .and(col(ReferenceMismatchSchema::REF_POS).gt_eq(lit(region.start()))),
                )
                .select([
                    col(ReferenceMismatchSchema::READ_ID),
                    col(ReferenceMismatchSchema::REF_POS),
                    col(ReferenceMismatchSchema::BASE),
                    col(ReferenceMismatchSchema::QUAL),
                ])
                .collect()?,
            &read_rows,
        )?;
        for (position, base, y, quality) in izip!(
            mismatches
                .column(ReferenceMismatchSchema::REF_POS)?
                .u64()?
                .into_no_null_iter(),
            mismatches
                .column(ReferenceMismatchSchema::BASE)?
                .u8()?
                .into_no_null_iter(),
            mismatches.column(ReadSchema::Y)?.u64()?.into_no_null_iter(),
            mismatches
                .column(ReferenceMismatchSchema::QUAL)?
                .u8()?
                .iter(),
        ) {
            self.mismatches.push(BaseMark {
                y,
                position,
                base,
                quality,
            });
        }

        let modifications = with_read_rows(
            tables
                .base_modifications
                .clone()
                .lazy()
                .filter(
                    col(BaseModificationSchema::DISPLAY_POS)
                        .lt_eq(lit(region.end()))
                        .and(col(BaseModificationSchema::DISPLAY_POS).gt_eq(lit(region.start()))),
                )
                .select([
                    col(BaseModificationSchema::READ_ID),
                    col(BaseModificationSchema::DISPLAY_POS),
                    col(BaseModificationSchema::CODE),
                    col(BaseModificationSchema::CHEBI_ID),
                    col(BaseModificationSchema::PROBABILITY),
                ])
                .collect()?,
            &read_rows,
        )?;
        for (position, code, chebi, probability, y) in izip!(
            modifications
                .column(BaseModificationSchema::DISPLAY_POS)?
                .u64()?
                .into_no_null_iter(),
            modifications
                .column(BaseModificationSchema::CODE)?
                .u8()?
                .iter(),
            modifications
                .column(BaseModificationSchema::CHEBI_ID)?
                .u32()?
                .iter(),
            modifications
                .column(BaseModificationSchema::PROBABILITY)?
                .u8()?
                .iter(),
            modifications
                .column(ReadSchema::Y)?
                .u64()?
                .into_no_null_iter(),
        ) {
            let modification = match (code, chebi) {
                (Some(code), None) => Modification::Code(code),
                (None, Some(id)) => Modification::ChebiId(id),
                _ => unreachable!("modifications contain exactly one noodles variant"),
            };
            self.modifications.push(ModificationMark {
                y,
                position,
                modification,
                probability: probability.unwrap_or(255),
            });
        }
        Ok(())
    }
}

/// The gaps between the mates of each shown pair, clipped to the region.
fn pair_gaps(
    tables: &AlignmentTables,
    pairs: &DataFrame,
    region: &Region,
) -> Result<Vec<RowSpan>, TGVError> {
    let first = pairs.column(PairSchema::READ_1_ID)?.u64()?;
    let second = pairs.column(PairSchema::READ_2_ID)?.u64()?;
    let ys = pairs.column(PairSchema::Y)?.u64()?;
    let starts = tables.reads.column(ReadSchema::STACKING_START)?.u64()?;
    let ends = tables.reads.column(ReadSchema::STACKING_END)?.u64()?;
    let mut gaps = Vec::new();
    for ((first, second), y) in first
        .into_no_null_iter()
        .zip(second.iter())
        .zip(ys.into_no_null_iter())
    {
        let Some(second) = second else { continue };
        let (first, second) = (first as usize, second as usize);
        let (Some(a), Some(b), Some(c), Some(d)) = (
            starts.get(first),
            ends.get(first),
            starts.get(second),
            ends.get(second),
        ) else {
            continue;
        };
        let (start, end) = if b < c {
            (b + 1, c - 1)
        } else if d < a {
            (d + 1, a - 1)
        } else {
            continue;
        };
        let (start, end) = (start.max(region.start()), end.min(region.end()));
        if start <= end {
            gaps.push(RowSpan { y, start, end });
        }
    }
    Ok(gaps)
}

/// Keep annotation rows of drawn reads and attach each read's stacking row as `Y`.
/// `read_rows` is indexed by read ID and holds the row of every drawn read.
fn with_read_rows(frame: DataFrame, read_rows: &[Option<u64>]) -> PolarsResult<DataFrame> {
    let mask: BooleanChunked = frame
        .column(ReadSchema::READ_ID)?
        .u64()?
        .into_no_null_iter()
        .map(|id| read_rows[id as usize].is_some())
        .collect();
    let mut frame = frame.filter(&mask)?;
    let ys: Vec<u64> = frame
        .column(ReadSchema::READ_ID)?
        .u64()?
        .into_no_null_iter()
        .map(|id| read_rows[id as usize].expect("rows are filtered to drawn reads"))
        .collect();
    frame.with_column(Column::new(ReadSchema::Y.into(), ys))?;
    Ok(frame)
}

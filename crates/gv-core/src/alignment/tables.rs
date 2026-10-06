//! Columnar alignment data, schemas, construction, and sparse base annotations.
//!
//! The core schemas follow the LAMF alignment layout without its Lance compression
//! metadata. Polars schemas specify column names and types, but do not enforce
//! whether values may be null. Optional tags remain in the original records.

use crate::{
    error::TGVError,
    sequence::Sequence,
    table_schema::{ColumnDoc, TableSchema},
};
use noodles::sam::{
    self,
    alignment::{
        RecordBuf,
        record::{
            Cigar, Flags,
            cigar::{Op, op::Kind},
            data::field::Tag,
        },
        record_buf::data::field::{Value, value::Array},
    },
    record::data::field::value::{BaseModifications, base_modifications::group::Modification},
};
use polars::prelude::*;
use std::sync::Arc;

fn binary_column(name: &'static str, values: &[Option<Vec<u8>>]) -> Column {
    BinaryChunked::from_iter_options(name.into(), values.iter().map(Option::as_deref))
        .into_series()
        .into()
}

/// The read and CIGAR tables share stable `read_id` values with the record sidecar.
#[derive(Debug)]
pub struct AlignmentTables {
    pub reads: DataFrame,
    /// All CIGAR operations in read and operation order, with nullable SEQ and qualities.
    pub cigar_runs: DataFrame,

    /// Reads without CIGAR
    pub unmapped: DataFrame,

    pub reference_mismatches: DataFrame,
    pub base_modifications: DataFrame,
}

impl Default for AlignmentTables {
    fn default() -> Self {
        Self {
            reads: ReadSchema::empty(),
            cigar_runs: CigarSchema::empty(),
            unmapped: UnmappedReadSchema::empty(),
            reference_mismatches: ReferenceMismatchSchema::empty(),
            base_modifications: BaseModificationSchema::empty(),
        }
    }
}

impl AlignmentTables {
    /// Append a record batch, assigning IDs after the existing reads.
    ///
    /// Batches for a region use the same reference and contig index, with
    /// reference IDs from the same source header.
    /// The consuming builder returns only fully appended tables.
    pub fn add_records(
        mut self,
        records: &[RecordBuf],
        reference_sequence: &Sequence,
        contig_index: usize,
    ) -> Result<Self, TGVError> {
        if records.is_empty() {
            return Ok(self);
        }
        let offset = self.reads.height() as u64;
        let batch = build_batch(records, reference_sequence, contig_index, offset)?;
        self.reads = concat(
            [self.reads.lazy(), batch.reads.lazy()],
            UnionArgs::default(),
        )?
        .collect()?;
        self.cigar_runs = concat(
            [self.cigar_runs.lazy(), batch.cigar_runs.lazy()],
            UnionArgs::default(),
        )?
        .collect()?;
        self.unmapped = concat(
            [self.unmapped.lazy(), batch.unmapped.lazy()],
            UnionArgs::default(),
        )?
        .collect()?;
        self.reference_mismatches = concat(
            [
                self.reference_mismatches.lazy(),
                batch.reference_mismatches.lazy(),
            ],
            UnionArgs::default(),
        )?
        .collect()?;
        self.base_modifications = concat(
            [
                self.base_modifications.lazy(),
                batch.base_modifications.lazy(),
            ],
            UnionArgs::default(),
        )?
        .collect()?;
        Ok(self)
    }
}

fn build_batch(
    records: &[RecordBuf],
    reference_sequence: &Sequence,
    contig_index: usize,
    read_id_offset: u64,
) -> Result<AlignmentTables, TGVError> {
    let mut read_id: Vec<u64> = Vec::with_capacity(records.len());
    let mut qname: Vec<Option<String>> = Vec::with_capacity(records.len());
    let mut ref_id: Vec<Option<u32>> = Vec::with_capacity(records.len());
    let mut pos: Vec<Option<u64>> = Vec::with_capacity(records.len());
    let mut mapq: Vec<Option<u8>> = Vec::with_capacity(records.len());
    let mut next_ref_id: Vec<Option<u32>> = Vec::with_capacity(records.len());
    let mut next_pos: Vec<Option<u64>> = Vec::with_capacity(records.len());
    let mut tlen: Vec<i32> = Vec::with_capacity(records.len());
    let mut stacking_start: Vec<Option<u64>> = Vec::with_capacity(records.len());
    let mut stacking_end: Vec<Option<u64>> = Vec::with_capacity(records.len());
    let mut paired: Vec<bool> = Vec::with_capacity(records.len());
    let mut proper_pair: Vec<bool> = Vec::with_capacity(records.len());
    let mut unmapped: Vec<bool> = Vec::with_capacity(records.len());
    let mut mate_unmapped: Vec<bool> = Vec::with_capacity(records.len());
    let mut reverse: Vec<bool> = Vec::with_capacity(records.len());
    let mut mate_reverse: Vec<bool> = Vec::with_capacity(records.len());
    let mut first_segment: Vec<bool> = Vec::with_capacity(records.len());
    let mut last_segment: Vec<bool> = Vec::with_capacity(records.len());
    let mut secondary: Vec<bool> = Vec::with_capacity(records.len());
    let mut qc_failed: Vec<bool> = Vec::with_capacity(records.len());
    let mut duplicate: Vec<bool> = Vec::with_capacity(records.len());
    let mut supplementary: Vec<bool> = Vec::with_capacity(records.len());
    let mut run_read_id = Vec::new();
    let mut run_op_index = Vec::new();
    let mut run_kind = Vec::new();
    let mut run_ref_id = Vec::new();
    let mut run_ref_start = Vec::new();
    let mut run_op_len = Vec::new();
    let mut run_seq = Vec::new();
    let mut run_qual = Vec::new();
    let mut run_display_start = Vec::new();
    let mut run_display_end = Vec::new();
    let mut run_offset = Vec::new();
    let mut unmapped_read_id: Vec<u64> = Vec::new();
    let mut base_count: Vec<u32> = Vec::new();
    let mut unmapped_seq: Vec<Option<String>> = Vec::new();
    let mut unmapped_qual: Vec<Option<Vec<u8>>> = Vec::new();
    for (index, record) in records.iter().enumerate() {
        let id = read_id_offset + index as u64;
        let sequence = std::str::from_utf8(record.sequence().as_ref())?;
        if !sequence.is_ascii() {
            return Err(TGVError::AlignmentParseError(format!(
                "SEQ is not ASCII for read {id}"
            )));
        }
        append_read(
            record,
            id,
            &mut read_id,
            &mut qname,
            &mut ref_id,
            &mut pos,
            &mut mapq,
            &mut next_ref_id,
            &mut next_pos,
            &mut tlen,
            &mut stacking_start,
            &mut stacking_end,
            &mut paired,
            &mut proper_pair,
            &mut unmapped,
            &mut mate_unmapped,
            &mut reverse,
            &mut mate_reverse,
            &mut first_segment,
            &mut last_segment,
            &mut secondary,
            &mut qc_failed,
            &mut duplicate,
            &mut supplementary,
        )?;
        append_runs(
            record,
            id,
            sequence,
            &mut run_read_id,
            &mut run_op_index,
            &mut run_kind,
            &mut run_ref_id,
            &mut run_ref_start,
            &mut run_op_len,
            &mut run_seq,
            &mut run_qual,
            &mut run_display_start,
            &mut run_display_end,
            &mut run_offset,
        )?;
        if record.cigar().is_empty() {
            append_unmapped(
                record,
                id,
                sequence,
                &mut unmapped_read_id,
                &mut base_count,
                &mut unmapped_seq,
                &mut unmapped_qual,
            );
        }
    }
    let height = read_id.len();
    let read_columns = vec![
        Column::new(ReadSchema::READ_ID.into(), read_id),
        Column::new(ReadSchema::QNAME.into(), qname),
        Column::new(ReadSchema::REF_ID.into(), ref_id),
        Column::new(ReadSchema::POS.into(), pos),
        Column::new(ReadSchema::MAPQ.into(), mapq),
        Column::new(ReadSchema::NEXT_REF_ID.into(), next_ref_id),
        Column::new(ReadSchema::NEXT_POS.into(), next_pos),
        Column::new(ReadSchema::TLEN.into(), tlen),
        Column::new(ReadSchema::STACKING_START.into(), stacking_start),
        Column::new(ReadSchema::STACKING_END.into(), stacking_end),
        Column::new(ReadSchema::PAIRED.into(), paired),
        Column::new(ReadSchema::PROPER_PAIR.into(), proper_pair),
        Column::new(ReadSchema::UNMAPPED.into(), unmapped),
        Column::new(ReadSchema::MATE_UNMAPPED.into(), mate_unmapped),
        Column::new(ReadSchema::REVERSE.into(), reverse),
        Column::new(ReadSchema::MATE_REVERSE.into(), mate_reverse),
        Column::new(ReadSchema::FIRST_SEGMENT.into(), first_segment),
        Column::new(ReadSchema::LAST_SEGMENT.into(), last_segment),
        Column::new(ReadSchema::SECONDARY.into(), secondary),
        Column::new(ReadSchema::QC_FAILED.into(), qc_failed),
        Column::new(ReadSchema::DUPLICATE.into(), duplicate),
        Column::new(ReadSchema::SUPPLEMENTARY.into(), supplementary),
    ];
    let reads = DataFrame::new(height, read_columns)?
        .lazy()
        .with_columns([
            col(ReadSchema::STACKING_START)
                .is_not_null()
                .alias(ReadSchema::SHOW),
            lit(0u64).cast(DataType::UInt64).alias(ReadSchema::Y),
        ])
        .collect()?;
    let cigar_runs = DataFrame::new(
        run_read_id.len(),
        vec![
            Column::new(CigarSchema::READ_ID.into(), run_read_id),
            Column::new(CigarSchema::OP_INDEX.into(), run_op_index),
            Column::new(CigarSchema::KIND.into(), run_kind),
            Column::new(CigarSchema::REF_ID.into(), run_ref_id),
            Column::new(CigarSchema::REF_START.into(), run_ref_start),
            Column::new(CigarSchema::OP_LEN.into(), run_op_len),
            Column::new(CigarSchema::SEQ.into(), run_seq),
            binary_column(CigarSchema::QUAL, &run_qual),
            Column::new(CigarSchema::DISPLAY_START.into(), run_display_start),
            Column::new(CigarSchema::DISPLAY_END.into(), run_display_end),
            Column::new(CigarSchema::RUN_OFFSET.into(), run_offset),
        ],
    )?;
    let unmapped = DataFrame::new(
        unmapped_read_id.len(),
        vec![
            Column::new(UnmappedReadSchema::READ_ID.into(), unmapped_read_id),
            Column::new(UnmappedReadSchema::BASE_COUNT.into(), base_count),
            Column::new(UnmappedReadSchema::SEQ.into(), unmapped_seq),
            binary_column(UnmappedReadSchema::QUAL, &unmapped_qual),
        ],
    )?;
    let reference_mismatches = reference_mismatches(&cigar_runs, reference_sequence, contig_index)?;
    let base_modifications = base_modifications(records, read_id_offset)?;
    Ok(AlignmentTables {
        reads,
        cigar_runs,
        unmapped,
        reference_mismatches,
        base_modifications,
    })
}

fn append_read(
    record: &RecordBuf,
    id: u64,
    read_id: &mut Vec<u64>,
    qname: &mut Vec<Option<String>>,
    ref_id: &mut Vec<Option<u32>>,
    pos: &mut Vec<Option<u64>>,
    mapq: &mut Vec<Option<u8>>,
    next_ref_id: &mut Vec<Option<u32>>,
    next_pos: &mut Vec<Option<u64>>,
    tlen: &mut Vec<i32>,
    stacking_start: &mut Vec<Option<u64>>,
    stacking_end: &mut Vec<Option<u64>>,
    paired: &mut Vec<bool>,
    proper_pair: &mut Vec<bool>,
    unmapped: &mut Vec<bool>,
    mate_unmapped: &mut Vec<bool>,
    reverse: &mut Vec<bool>,
    mate_reverse: &mut Vec<bool>,
    first_segment: &mut Vec<bool>,
    last_segment: &mut Vec<bool>,
    secondary: &mut Vec<bool>,
    qc_failed: &mut Vec<bool>,
    duplicate: &mut Vec<bool>,
    supplementary: &mut Vec<bool>,
) -> Result<(), TGVError> {
    read_id.push(id);
    qname.push(
        record
            .name()
            .map(|name| String::from_utf8(name.to_vec()))
            .transpose()?,
    );
    ref_id.push(record.reference_sequence_id().map(|id| id as u32));
    pos.push(record.alignment_start().map(|p| p.get() as u64));
    mapq.push(record.mapping_quality().map(|q| q.get()));
    next_ref_id.push(record.mate_reference_sequence_id().map(|id| id as u32));
    next_pos.push(record.mate_alignment_start().map(|p| p.get() as u64));
    tlen.push(record.template_length());
    let flags = record.flags();
    paired.push(flags.contains(Flags::SEGMENTED));
    proper_pair.push(flags.contains(Flags::PROPERLY_SEGMENTED));
    unmapped.push(flags.contains(Flags::UNMAPPED));
    mate_unmapped.push(flags.contains(Flags::MATE_UNMAPPED));
    reverse.push(flags.contains(Flags::REVERSE_COMPLEMENTED));
    mate_reverse.push(flags.contains(Flags::MATE_REVERSE_COMPLEMENTED));
    first_segment.push(flags.contains(Flags::FIRST_SEGMENT));
    last_segment.push(flags.contains(Flags::LAST_SEGMENT));
    secondary.push(flags.contains(Flags::SECONDARY));
    qc_failed.push(flags.contains(Flags::QC_FAIL));
    duplicate.push(flags.contains(Flags::DUPLICATE));
    supplementary.push(flags.contains(Flags::SUPPLEMENTARY));
    let bounds = record.alignment_start().map(|position| {
        let start = position.get() as u64;
        let cigar = record.cigar().as_ref();
        let leading = cigar
            .iter()
            .find(|op| !matches!(op.kind(), Kind::HardClip | Kind::Pad))
            .filter(|op| op.kind() == Kind::SoftClip)
            .map_or(0, |op| op.len() as u64);
        let trailing = cigar
            .iter()
            .rev()
            .find(|op| !matches!(op.kind(), Kind::HardClip | Kind::Pad))
            .filter(|op| op.kind() == Kind::SoftClip)
            .map_or(0, |op| op.len() as u64);
        let span = record.cigar().alignment_span() as u64;
        let end = start + span.max(1) - 1 + trailing;
        (start.saturating_sub(leading).max(1), end)
    });
    stacking_start.push(bounds.map(|bounds| bounds.0));
    stacking_end.push(bounds.map(|bounds| bounds.1));
    Ok(())
}

fn append_runs(
    record: &RecordBuf,
    id: u64,
    sequence: &str,
    run_read_id: &mut Vec<u64>,
    run_op_index: &mut Vec<u32>,
    run_kind: &mut Vec<u8>,
    run_ref_id: &mut Vec<Option<u32>>,
    run_ref_start: &mut Vec<Option<u64>>,
    run_op_len: &mut Vec<u32>,
    run_seq: &mut Vec<Option<String>>,
    run_qual: &mut Vec<Option<Vec<u8>>>,
    run_display_start: &mut Vec<Option<u64>>,
    run_display_end: &mut Vec<Option<u64>>,
    run_offset: &mut Vec<u32>,
) -> Result<(), TGVError> {
    let quality = record.quality_scores().as_ref();
    let reference_id = record.reference_sequence_id().map(|id| id as u32);
    let mut query_cursor = 0usize;
    let mut reference_cursor = record.alignment_start().map(|p| p.get() as u64);
    let mut leading = true;
    for (op_idx, op) in record.cigar().as_ref().iter().enumerate() {
        let kind = op.kind();
        let len = op.len();
        run_read_id.push(id);
        run_op_index.push(op_idx as u32);
        run_kind.push(match kind {
            Kind::Match => CigarSchema::MATCH,
            Kind::Insertion => CigarSchema::INSERTION,
            Kind::Deletion => CigarSchema::DELETION,
            Kind::Skip => CigarSchema::REFERENCE_SKIP,
            Kind::SoftClip => CigarSchema::SOFT_CLIP,
            Kind::HardClip => CigarSchema::HARD_CLIP,
            Kind::Pad => CigarSchema::PADDING,
            Kind::SequenceMatch => CigarSchema::SEQUENCE_MATCH,
            Kind::SequenceMismatch => CigarSchema::SEQUENCE_MISMATCH,
        });
        run_ref_id.push(reference_id);
        run_ref_start.push(reference_cursor);
        run_op_len.push(len as u32);
        let display = reference_cursor.and_then(|cursor| {
            if matches!(kind, Kind::HardClip | Kind::Pad) || (kind != Kind::Insertion && len == 0) {
                return None;
            }
            let origin = if kind == Kind::SoftClip && leading {
                cursor as i128 - len as i128
            } else {
                cursor as i128
            };
            let end = if kind == Kind::Insertion {
                origin
            } else {
                origin + len as i128 - 1
            };
            if end < 1 {
                return None;
            }
            let start = origin.max(1);
            Some((start as u64, end as u64, (start - origin) as u32))
        });
        run_display_start.push(display.map(|(start, _, _)| start));
        run_display_end.push(display.map(|(_, end, _)| end));
        run_offset.push(display.map_or(0, |(_, _, offset)| offset));
        if !matches!(kind, Kind::HardClip | Kind::Pad) {
            leading = false;
        }
        if kind.consumes_read() {
            let end = query_cursor + len;
            if end > record.sequence().len() && !record.sequence().is_empty() {
                return Err(TGVError::AlignmentParseError(format!(
                    "CIGAR exceeds sequence length for read {id}"
                )));
            }
            run_seq.push(if sequence.is_empty() {
                None
            } else {
                Some(sequence[query_cursor..end].to_owned())
            });
            run_qual.push(if quality.is_empty() {
                None
            } else {
                Some(
                    quality
                        .get(query_cursor..end)
                        .ok_or_else(|| {
                            TGVError::AlignmentParseError(format!(
                                "CIGAR exceeds quality length for read {id}"
                            ))
                        })?
                        .to_vec(),
                )
            });
            query_cursor = end;
        } else {
            run_seq.push(None);
            run_qual.push(None);
        }
        if kind.consumes_reference() {
            reference_cursor = reference_cursor.map(|cursor| cursor + len as u64);
        }
    }
    Ok(())
}

fn append_unmapped(
    record: &RecordBuf,
    id: u64,
    sequence: &str,
    unmapped_read_id: &mut Vec<u64>,
    base_count: &mut Vec<u32>,
    unmapped_seq: &mut Vec<Option<String>>,
    unmapped_qual: &mut Vec<Option<Vec<u8>>>,
) {
    let quality = record.quality_scores().as_ref();
    unmapped_read_id.push(id);
    base_count.push(record.sequence().len() as u32);
    unmapped_seq.push((!sequence.is_empty()).then(|| sequence.to_owned()));
    unmapped_qual.push((!quality.is_empty()).then(|| quality.to_vec()));
}

/// The read-level table schema.
///
/// `read_id` is the zero-based row index in the original loaded reads table.
/// Filtering and sorting preserve this ID so the original read remains directly
/// addressable. `ref_id` is the alignment header's reference ordinal, not the
/// merged tgv contig index. `pos` and `next_pos` are one-based.
/// Missing names and mapping qualities are null, rather than SAM sentinel values.
/// `stacking_start` and `stacking_end` are nullable, one-based, inclusive
/// display bounds, including soft clips. Both are null for unpositioned reads.
pub struct ReadSchema;

impl ReadSchema {
    pub const READ_ID: &'static str = "read_id";
    pub const QNAME: &'static str = "qname";
    pub const REF_ID: &'static str = "ref_id";
    pub const POS: &'static str = "pos";
    pub const MAPQ: &'static str = "mapq";
    pub const NEXT_REF_ID: &'static str = "next_ref_id";
    pub const NEXT_POS: &'static str = "next_pos";
    pub const TLEN: &'static str = "tlen";
    pub const STACKING_START: &'static str = "stacking_start";
    pub const STACKING_END: &'static str = "stacking_end";
    pub const PAIRED: &'static str = "paired";
    pub const PROPER_PAIR: &'static str = "proper_pair";
    pub const UNMAPPED: &'static str = "unmapped";
    pub const MATE_UNMAPPED: &'static str = "mate_unmapped";
    pub const REVERSE: &'static str = "reverse";
    pub const MATE_REVERSE: &'static str = "mate_reverse";
    pub const FIRST_SEGMENT: &'static str = "first_segment";
    pub const LAST_SEGMENT: &'static str = "last_segment";
    pub const SECONDARY: &'static str = "secondary";
    pub const QC_FAILED: &'static str = "qc_failed";
    pub const DUPLICATE: &'static str = "duplicate";
    pub const SUPPLEMENTARY: &'static str = "supplementary";
    pub const SHOW: &'static str = "show";
    pub const Y: &'static str = "y";

    /// Temporary column used while sorting reads and pairs.
    pub const SORT_KEY: &'static str = "sort_key";
}

impl TableSchema for ReadSchema {
    fn schema() -> SchemaRef {
        let mut schema = Schema::with_capacity(24);
        schema.insert(Self::READ_ID.into(), DataType::UInt64);
        schema.insert(Self::QNAME.into(), DataType::String);
        schema.insert(Self::REF_ID.into(), DataType::UInt32);
        schema.insert(Self::POS.into(), DataType::UInt64);
        schema.insert(Self::MAPQ.into(), DataType::UInt8);
        schema.insert(Self::NEXT_REF_ID.into(), DataType::UInt32);
        schema.insert(Self::NEXT_POS.into(), DataType::UInt64);
        schema.insert(Self::TLEN.into(), DataType::Int32);
        schema.insert(Self::STACKING_START.into(), DataType::UInt64);
        schema.insert(Self::STACKING_END.into(), DataType::UInt64);
        schema.insert(Self::PAIRED.into(), DataType::Boolean);
        schema.insert(Self::PROPER_PAIR.into(), DataType::Boolean);
        schema.insert(Self::UNMAPPED.into(), DataType::Boolean);
        schema.insert(Self::MATE_UNMAPPED.into(), DataType::Boolean);
        schema.insert(Self::REVERSE.into(), DataType::Boolean);
        schema.insert(Self::MATE_REVERSE.into(), DataType::Boolean);
        schema.insert(Self::FIRST_SEGMENT.into(), DataType::Boolean);
        schema.insert(Self::LAST_SEGMENT.into(), DataType::Boolean);
        schema.insert(Self::SECONDARY.into(), DataType::Boolean);
        schema.insert(Self::QC_FAILED.into(), DataType::Boolean);
        schema.insert(Self::DUPLICATE.into(), DataType::Boolean);
        schema.insert(Self::SUPPLEMENTARY.into(), DataType::Boolean);
        schema.insert(Self::SHOW.into(), DataType::Boolean);
        schema.insert(Self::Y.into(), DataType::UInt64);
        Arc::new(schema)
    }

    fn column_docs() -> &'static [ColumnDoc] {
        &[
            ColumnDoc {
                name: Self::READ_ID,
                description: "The read ID within the loaded reads.",
            },
            ColumnDoc {
                name: Self::QNAME,
                description: "The read name, or null.",
            },
            ColumnDoc {
                name: Self::POS,
                description: "The 1-based alignment start, or null for unpositioned reads.",
            },
            ColumnDoc {
                name: Self::MAPQ,
                description: "The mapping quality, or null when unavailable.",
            },
            ColumnDoc {
                name: Self::NEXT_POS,
                description: "The mate's 1-based alignment start, or null.",
            },
            ColumnDoc {
                name: Self::TLEN,
                description: "The observed template length; negative for the rightmost segment.",
            },
            ColumnDoc {
                name: Self::PAIRED,
                description: "SAM flag 0x1: the template has multiple segments.",
            },
            ColumnDoc {
                name: Self::PROPER_PAIR,
                description: "SAM flag 0x2: each segment is properly aligned.",
            },
            ColumnDoc {
                name: Self::UNMAPPED,
                description: "SAM flag 0x4: the read is unmapped.",
            },
            ColumnDoc {
                name: Self::MATE_UNMAPPED,
                description: "SAM flag 0x8: the mate is unmapped.",
            },
            ColumnDoc {
                name: Self::REVERSE,
                description: "SAM flag 0x10: the read is reverse complemented.",
            },
            ColumnDoc {
                name: Self::MATE_REVERSE,
                description: "SAM flag 0x20: the mate is reverse complemented.",
            },
            ColumnDoc {
                name: Self::FIRST_SEGMENT,
                description: "SAM flag 0x40: the read is the first segment.",
            },
            ColumnDoc {
                name: Self::LAST_SEGMENT,
                description: "SAM flag 0x80: the read is the last segment.",
            },
            ColumnDoc {
                name: Self::SECONDARY,
                description: "SAM flag 0x100: the alignment is secondary.",
            },
            ColumnDoc {
                name: Self::QC_FAILED,
                description: "SAM flag 0x200: the read fails quality checks.",
            },
            ColumnDoc {
                name: Self::DUPLICATE,
                description: "SAM flag 0x400: the read is a PCR or optical duplicate.",
            },
            ColumnDoc {
                name: Self::SUPPLEMENTARY,
                description: "SAM flag 0x800: the alignment is supplementary.",
            },
        ]
    }
}

/// All CIGAR runs, with nullable SEQ, qualities, and one-based reference and display bounds.
///
/// `kind` uses the explicit BAM operation codes declared on this schema.
/// `ref_start` retains the reference cursor. Insertions have equal display bounds;
/// hard clips, padding, and unpositioned operations have null display bounds.
/// Stored `run_offset` accounts for leading soft-clip bases before coordinate 1.
/// Viewport queries clip the display bounds further and advance this offset.
pub struct CigarSchema;

impl CigarSchema {
    /// CIGAR `M`.
    pub const MATCH: u8 = 0;
    /// CIGAR `I`.
    pub const INSERTION: u8 = 1;
    /// CIGAR `D`.
    pub const DELETION: u8 = 2;
    /// CIGAR `N`.
    pub const REFERENCE_SKIP: u8 = 3;
    /// CIGAR `S`.
    pub const SOFT_CLIP: u8 = 4;
    /// CIGAR `H`.
    pub const HARD_CLIP: u8 = 5;
    /// CIGAR `P`.
    pub const PADDING: u8 = 6;
    /// CIGAR `=`.
    pub const SEQUENCE_MATCH: u8 = 7;
    /// CIGAR `X`.
    pub const SEQUENCE_MISMATCH: u8 = 8;

    pub const READ_ID: &'static str = ReadSchema::READ_ID;
    pub const OP_INDEX: &'static str = "op_index";
    pub const KIND: &'static str = "kind";
    pub const REF_ID: &'static str = "ref_id";
    pub const REF_START: &'static str = "ref_start";
    pub const OP_LEN: &'static str = "op_len";
    pub const SEQ: &'static str = "seq";
    pub const QUAL: &'static str = "qual";
    pub const DISPLAY_START: &'static str = "display_start";
    pub const DISPLAY_END: &'static str = "display_end";
    pub const RUN_OFFSET: &'static str = "run_offset";
}

impl TableSchema for CigarSchema {
    fn schema() -> SchemaRef {
        let mut schema = Schema::with_capacity(11);
        schema.insert(Self::READ_ID.into(), DataType::UInt64);
        schema.insert(Self::OP_INDEX.into(), DataType::UInt32);
        schema.insert(Self::KIND.into(), DataType::UInt8);
        schema.insert(Self::REF_ID.into(), DataType::UInt32);
        schema.insert(Self::REF_START.into(), DataType::UInt64);
        schema.insert(Self::OP_LEN.into(), DataType::UInt32);
        schema.insert(Self::SEQ.into(), DataType::String);
        schema.insert(Self::QUAL.into(), DataType::Binary);
        schema.insert(Self::DISPLAY_START.into(), DataType::UInt64);
        schema.insert(Self::DISPLAY_END.into(), DataType::UInt64);
        schema.insert(Self::RUN_OFFSET.into(), DataType::UInt32);
        Arc::new(schema)
    }

    fn column_docs() -> &'static [ColumnDoc] {
        &[
            ColumnDoc {
                name: Self::READ_ID,
                description: "The read that owns the operation.",
            },
            ColumnDoc {
                name: Self::OP_INDEX,
                description: "The zero-based operation index within the CIGAR string.",
            },
            ColumnDoc {
                name: Self::KIND,
                description: "The CIGAR operation, as a BAM operation code.",
            },
            ColumnDoc {
                name: Self::REF_START,
                description: "The 1-based reference position where the operation starts. For operations that do not consume the reference, it is the next reference position, so an insertion lies between `ref_start - 1` and `ref_start`.",
            },
            ColumnDoc {
                name: Self::OP_LEN,
                description: "The operation length.",
            },
            ColumnDoc {
                name: Self::SEQ,
                description: "The read bases of operations that consume the read; otherwise null.",
            },
            ColumnDoc {
                name: Self::QUAL,
                description: "The base qualities, aligned with `seq`.",
            },
        ]
    }
}

/// The schema for records without CIGAR operations.
///
/// Missing SEQ and quality scores are null.
pub struct UnmappedReadSchema;

impl UnmappedReadSchema {
    pub const READ_ID: &'static str = ReadSchema::READ_ID;
    pub const BASE_COUNT: &'static str = "base_count";
    pub const SEQ: &'static str = "seq";
    pub const QUAL: &'static str = "qual";
}

impl TableSchema for UnmappedReadSchema {
    fn schema() -> SchemaRef {
        let mut schema = Schema::with_capacity(4);
        schema.insert(Self::READ_ID.into(), DataType::UInt64);
        schema.insert(Self::BASE_COUNT.into(), DataType::UInt32);
        schema.insert(Self::SEQ.into(), DataType::String);
        schema.insert(Self::QUAL.into(), DataType::Binary);
        Arc::new(schema)
    }
}

/// Sparse reference mismatches within M runs.
pub struct ReferenceMismatchSchema;

impl ReferenceMismatchSchema {
    pub const READ_ID: &'static str = ReadSchema::READ_ID;
    pub const OP_INDEX: &'static str = CigarSchema::OP_INDEX;
    pub const RUN_OFFSET: &'static str = CigarSchema::RUN_OFFSET;
    pub const REF_POS: &'static str = "ref_pos";
    pub const BASE: &'static str = "base";
}

impl TableSchema for ReferenceMismatchSchema {
    fn schema() -> SchemaRef {
        let mut schema = Schema::with_capacity(5);
        schema.insert(Self::READ_ID.into(), DataType::UInt64);
        schema.insert(Self::OP_INDEX.into(), DataType::UInt32);
        schema.insert(Self::RUN_OFFSET.into(), DataType::UInt32);
        schema.insert(Self::REF_POS.into(), DataType::UInt64);
        schema.insert(Self::BASE.into(), DataType::UInt8);
        Arc::new(schema)
    }

    fn column_docs() -> &'static [ColumnDoc] {
        &[
            ColumnDoc {
                name: Self::READ_ID,
                description: "The read containing the mismatch.",
            },
            ColumnDoc {
                name: Self::OP_INDEX,
                description: "The `M` operation containing the base.",
            },
            ColumnDoc {
                name: Self::REF_POS,
                description: "The 1-based reference position.",
            },
            ColumnDoc {
                name: Self::BASE,
                description: "The read base, which differs from the reference.",
            },
        ]
    }
}

/// Sparse MM/ML annotations, with exactly one of code and ChEBI ID populated.
///
/// Positions are one-based display coordinates, including projected soft clips.
/// `source_order` preserves MM/ML order for deterministic probability ties.
/// Missing ML probabilities are null; a recorded probability of 255 remains 255.
pub struct BaseModificationSchema;

impl BaseModificationSchema {
    pub const READ_ID: &'static str = ReadSchema::READ_ID;
    pub const OP_INDEX: &'static str = CigarSchema::OP_INDEX;
    pub const RUN_OFFSET: &'static str = CigarSchema::RUN_OFFSET;
    pub const DISPLAY_POS: &'static str = "display_pos";
    pub const CODE: &'static str = "code";
    pub const CHEBI_ID: &'static str = "chebi_id";
    pub const PROBABILITY: &'static str = "probability";
    pub const SOURCE_ORDER: &'static str = "source_order";
}

impl TableSchema for BaseModificationSchema {
    fn schema() -> SchemaRef {
        let mut schema = Schema::with_capacity(8);
        schema.insert(Self::READ_ID.into(), DataType::UInt64);
        schema.insert(Self::OP_INDEX.into(), DataType::UInt32);
        schema.insert(Self::RUN_OFFSET.into(), DataType::UInt32);
        schema.insert(Self::DISPLAY_POS.into(), DataType::UInt64);
        schema.insert(Self::CODE.into(), DataType::UInt8);
        schema.insert(Self::CHEBI_ID.into(), DataType::UInt32);
        schema.insert(Self::PROBABILITY.into(), DataType::UInt8);
        schema.insert(Self::SOURCE_ORDER.into(), DataType::UInt64);
        Arc::new(schema)
    }

    fn column_docs() -> &'static [ColumnDoc] {
        &[
            ColumnDoc {
                name: Self::READ_ID,
                description: "The read carrying the modification call.",
            },
            ColumnDoc {
                name: Self::DISPLAY_POS,
                description: "The 1-based reference position, with soft-clipped bases projected beside the alignment.",
            },
            ColumnDoc {
                name: Self::CODE,
                description: "The modification code, such as `m` for 5mC; null when `chebi_id` is set.",
            },
            ColumnDoc {
                name: Self::CHEBI_ID,
                description: "The ChEBI ID of the modification; null when `code` is set.",
            },
            ColumnDoc {
                name: Self::PROBABILITY,
                description: "The ML probability from 0 to 255, or null when absent.",
            },
        ]
    }
}

pub(crate) fn reference_mismatches(
    runs: &DataFrame,
    reference: &Sequence,
    contig_index: usize,
) -> Result<DataFrame, TGVError> {
    if reference.contig_index != contig_index || reference.sequence.is_empty() {
        return Ok(ReferenceMismatchSchema::empty());
    }
    let runs = runs
        .clone()
        .lazy()
        .filter(col(CigarSchema::KIND).eq(lit(CigarSchema::MATCH)))
        .collect()?;
    let ids = runs.column(CigarSchema::READ_ID)?.u64()?;
    let indexes = runs.column(CigarSchema::OP_INDEX)?.u32()?;
    let starts = runs.column(CigarSchema::REF_START)?.u64()?;
    let lengths = runs.column(CigarSchema::OP_LEN)?.u32()?;
    let sequences = runs.column(CigarSchema::SEQ)?.str()?;
    let mut read_id = Vec::new();
    let mut op_index = Vec::new();
    let mut run_offset = Vec::new();
    let mut ref_pos = Vec::new();
    let mut base = Vec::new();
    for row in 0..runs.height() {
        let Some(start) = starts.get(row) else {
            continue;
        };
        let Some(sequence) = sequences.get(row) else {
            continue;
        };
        let sequence = sequence.as_bytes();
        let end = start + u64::from(lengths.get(row).expect("run lengths are non-null"));
        let left = start.max(reference.start);
        let right = end.min(reference.end() + 1);
        for pos in left..right {
            let offset = (pos - start) as u32;
            let read_base = sequence[offset as usize];
            let reference_base = reference
                .base_at(pos)
                .expect("clipped reference coordinates are loaded");
            if !matches_base(read_base, reference_base) {
                read_id.push(ids.get(row).expect("run read IDs are non-null"));
                op_index.push(indexes.get(row).expect("run indexes are non-null"));
                run_offset.push(offset);
                ref_pos.push(pos);
                base.push(read_base);
            }
        }
    }
    Ok(DataFrame::new(
        read_id.len(),
        vec![
            Column::new(ReferenceMismatchSchema::READ_ID.into(), read_id),
            Column::new(ReferenceMismatchSchema::OP_INDEX.into(), op_index),
            Column::new(ReferenceMismatchSchema::RUN_OFFSET.into(), run_offset),
            Column::new(ReferenceMismatchSchema::REF_POS.into(), ref_pos),
            Column::new(ReferenceMismatchSchema::BASE.into(), base),
        ],
    )?)
}

pub(super) fn locate_query_base(
    pos: u64,
    alignment_start: u64,
    cigars: &[Op],
) -> Option<(u32, u32, u64)> {
    let mut query_cursor = 0u64;
    let mut reference_cursor = alignment_start;
    for (index, op) in cigars.iter().enumerate() {
        let len = op.len() as u64;
        if op.kind().consumes_read() {
            let query_end = query_cursor + len;
            if (query_cursor..query_end).contains(&pos) {
                let offset = pos - query_cursor;
                let position = match op.kind() {
                    Kind::Insertion => return None,
                    Kind::SoftClip
                        if cigars[..index]
                            .iter()
                            .all(|op| matches!(op.kind(), Kind::HardClip | Kind::Pad)) =>
                    {
                        let projected = reference_cursor as i128 + offset as i128 - len as i128;
                        if projected <= 0 {
                            return None;
                        }
                        projected as u64
                    }
                    _ => reference_cursor + offset,
                };
                if position == 0 {
                    return None;
                }
                return Some((index as u32, offset as u32, position));
            }
            query_cursor = query_end;
        }
        if op.kind().consumes_reference() {
            reference_cursor += len;
        }
    }
    None
}

pub(super) fn extract_base_modifications(
    mm_string: &str,
    ml_bytes: Option<&[u8]>,
    flags: Flags,
    sequence: &sam::alignment::record_buf::Sequence,
    cigars: &[Op],
    alignment_start: u64,
    read_id: u64,
) -> Result<Vec<(u32, u32, u64, Modification, Option<u8>)>, TGVError> {
    let groups = BaseModifications::parse(
        mm_string.as_bytes(),
        flags.is_reverse_complemented(),
        sequence,
    )
    .map_err(|error| TGVError::AlignmentBaseModifications {
        read_id,
        message: error.to_string(),
    })?;
    let mut probabilities = ml_bytes.unwrap_or_default().iter().copied();
    let mut annotations = Vec::new();
    for group in groups.as_ref() {
        for position in group.positions() {
            for modification in group.modifications() {
                let probability = probabilities.next();
                if let Some((index, offset, position)) =
                    locate_query_base(*position as u64, alignment_start, cigars)
                {
                    annotations.push((index, offset, position, *modification, probability));
                }
            }
        }
    }
    Ok(annotations)
}

pub(super) fn base_modifications(
    records: &[RecordBuf],
    read_id_offset: u64,
) -> Result<DataFrame, TGVError> {
    let mut read_id = Vec::new();
    let mut op_index = Vec::new();
    let mut run_offset = Vec::new();
    let mut display_pos = Vec::new();
    let mut code = Vec::new();
    let mut chebi_id = Vec::new();
    let mut probability = Vec::new();
    let mut source_order = Vec::new();
    for (index, record) in records.iter().enumerate() {
        let id = read_id_offset + index as u64;
        let Some(start) = record.alignment_start() else {
            continue;
        };
        if record.sequence().is_empty() {
            continue;
        }
        let data = record.data();
        let Some(Value::String(mm)) = data
            .get(&Tag::BASE_MODIFICATIONS)
            .or_else(|| data.get(&Tag::new(b'M', b'm')))
        else {
            continue;
        };
        let mm = std::str::from_utf8(mm.as_ref()).map_err(|error| {
            TGVError::AlignmentBaseModifications {
                read_id: id,
                message: error.to_string(),
            }
        })?;
        let ml = match data
            .get(&Tag::BASE_MODIFICATION_PROBABILITIES)
            .or_else(|| data.get(&Tag::new(b'M', b'l')))
        {
            Some(Value::Array(Array::UInt8(values))) => Some(values.as_slice()),
            _ => None,
        };
        let annotations = extract_base_modifications(
            mm,
            ml,
            record.flags(),
            record.sequence(),
            record.cigar().as_ref(),
            start.get() as u64,
            id,
        )?;
        for (order, (index, offset, position, modification, prob)) in
            annotations.into_iter().enumerate()
        {
            read_id.push(id);
            op_index.push(index);
            run_offset.push(offset);
            display_pos.push(position);
            let (mod_code, mod_chebi) = match modification {
                Modification::Code(value) => (Some(value), None),
                Modification::ChebiId(value) => (None, Some(value)),
            };
            code.push(mod_code);
            chebi_id.push(mod_chebi);
            probability.push(prob);
            source_order.push(order as u64);
        }
    }
    Ok(DataFrame::new(
        read_id.len(),
        vec![
            Column::new(BaseModificationSchema::READ_ID.into(), read_id),
            Column::new(BaseModificationSchema::OP_INDEX.into(), op_index),
            Column::new(BaseModificationSchema::RUN_OFFSET.into(), run_offset),
            Column::new(BaseModificationSchema::DISPLAY_POS.into(), display_pos),
            Column::new(BaseModificationSchema::CODE.into(), code),
            Column::new(BaseModificationSchema::CHEBI_ID.into(), chebi_id),
            Column::new(BaseModificationSchema::PROBABILITY.into(), probability),
            Column::new(BaseModificationSchema::SOURCE_ORDER.into(), source_order),
        ],
    )?)
}

fn matches_base(base1: u8, base2: u8) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;
    use noodles::sam::{
        self,
        alignment::{
            record::cigar::{Op, op::Kind},
            record_buf::Cigar,
        },
        record::data::field::value::base_modifications::group::modification,
    };

    use crate::{
        alignment::{Alignment, CoverageSchema, tables},
        sequence::Sequence,
    };
    use noodles::sam::alignment::{
        record::{Flags, data::field::Tag},
        record_buf::data::{Data, field::Value},
    };

    fn extract_base_modifications(
        mm: String,
        ml: Option<Vec<u8>>,
        flags: &Flags,
        sequence: &sam::alignment::record_buf::Sequence,
        cigars: &[Op],
        start: u64,
    ) -> Result<
        Vec<(
            u64,
            noodles::sam::record::data::field::value::base_modifications::group::Modification,
            Option<u8>,
        )>,
        TGVError,
    > {
        Ok(tables::extract_base_modifications(
            &mm,
            ml.as_deref(),
            *flags,
            sequence,
            cigars,
            start,
            0,
        )?
        .into_iter()
        .map(|(_, _, pos, modification, probability)| (pos, modification, probability))
        .collect())
    }

    fn get_reference_position_from_seq_position(
        pos: u64,
        start: u64,
        cigars: &[Op],
    ) -> Option<u64> {
        tables::locate_query_base(pos, start, cigars).map(|(_, _, pos)| pos)
    }

    use rstest::rstest;

    fn read_from_parts(
        start: u64,
        cigar_ops: impl IntoIterator<Item = (Kind, usize)>,
        sequence: &[u8],
    ) -> RecordBuf {
        let cigar: Cigar = cigar_ops
            .into_iter()
            .map(|(kind, len)| Op::new(kind, len))
            .collect();

        let record = sam::alignment::RecordBuf::builder()
            .set_alignment_start(noodles::core::Position::try_from(start as usize).unwrap())
            .set_cigar(cigar)
            .set_sequence(sam::alignment::record_buf::Sequence::from(sequence))
            .build();

        record
    }

    #[test]
    fn base_at_returns_reference_aligned_bases_only() {
        let record = read_from_parts(
            10,
            [
                (Kind::SoftClip, 1),
                (Kind::Match, 2),
                (Kind::Insertion, 1),
                (Kind::SequenceMatch, 1),
                (Kind::SequenceMismatch, 1),
                (Kind::Deletion, 1),
                (Kind::Match, 1),
                (Kind::SoftClip, 1),
            ],
            b"SATIGCRZ",
        );
        let alignment = Alignment::from_records(
            vec![record],
            0,
            (1, 100),
            &Sequence {
                start: 1,
                sequence: vec![b'A'; 100],
                contig_index: 0,
            },
        )
        .unwrap();

        let base_at = |pos: u64| {
            let kind = col(CigarSchema::KIND);
            let bases = alignment
                .tables
                .cigar_runs
                .clone()
                .lazy()
                .filter(
                    kind.clone()
                        .eq(lit(CigarSchema::MATCH))
                        .or(kind.clone().eq(lit(CigarSchema::SEQUENCE_MATCH)))
                        .or(kind.eq(lit(CigarSchema::SEQUENCE_MISMATCH)))
                        .and(col(CigarSchema::DISPLAY_START).lt_eq(lit(pos)))
                        .and(col(CigarSchema::DISPLAY_END).gt_eq(lit(pos))),
                )
                .select([col(CigarSchema::SEQ).str().slice(
                    (lit(pos as i128) - col(CigarSchema::REF_START).cast(DataType::Int128))
                        .cast(DataType::Int64),
                    lit(1u64),
                )])
                .collect()
                .unwrap();
            bases
                .column(CigarSchema::SEQ)
                .unwrap()
                .str()
                .unwrap()
                .iter()
                .next()
                .flatten()
                .map(|base| base.as_bytes()[0])
        };

        assert_eq!(base_at(9), None);
        assert_eq!(base_at(10), Some(b'A'));
        assert_eq!(base_at(11), Some(b'T'));
        assert_eq!(base_at(12), Some(b'G'));
        assert_eq!(base_at(13), Some(b'C'));
        assert_eq!(base_at(14), None);
        assert_eq!(base_at(15), Some(b'R'));
        assert_eq!(base_at(16), None);
    }

    #[test]
    fn is_deletion_at_detects_deletions_and_reference_skips() {
        let record = read_from_parts(
            10,
            [
                (Kind::Match, 2),
                (Kind::Deletion, 2),
                (Kind::Match, 1),
                (Kind::Skip, 1),
                (Kind::Match, 1),
            ],
            b"AAAA",
        );
        let alignment = Alignment::from_records(
            vec![record],
            0,
            (1, 100),
            &Sequence {
                start: 1,
                sequence: vec![b'A'; 100],
                contig_index: 0,
            },
        )
        .unwrap();

        let is_deletion_at = |pos: u64| {
            let kind = col(CigarSchema::KIND);
            alignment
                .tables
                .cigar_runs
                .clone()
                .lazy()
                .filter(
                    kind.clone()
                        .eq(lit(CigarSchema::DELETION))
                        .or(kind.eq(lit(CigarSchema::REFERENCE_SKIP)))
                        .and(col(CigarSchema::DISPLAY_START).lt_eq(lit(pos)))
                        .and(col(CigarSchema::DISPLAY_END).gt_eq(lit(pos))),
                )
                .collect()
                .unwrap()
                .height()
                > 0
        };

        assert!(!is_deletion_at(9));
        assert!(!is_deletion_at(10));
        assert!(!is_deletion_at(11));
        assert!(is_deletion_at(12));
        assert!(is_deletion_at(13));
        assert!(!is_deletion_at(14));
        assert!(is_deletion_at(15));
        assert!(!is_deletion_at(16));
        assert!(!is_deletion_at(17));
    }

    #[test]
    fn has_insertion_at_detects_insertion_anchors() {
        let record = read_from_parts(
            10,
            [
                (Kind::Match, 2),
                (Kind::Insertion, 2),
                (Kind::Match, 1),
                (Kind::Insertion, 1),
            ],
            b"AAIITI",
        );
        let alignment = Alignment::from_records(
            vec![record],
            0,
            (1, 100),
            &Sequence {
                start: 1,
                sequence: vec![b'A'; 100],
                contig_index: 0,
            },
        )
        .unwrap();

        let has_insertion_at = |pos: u64| {
            alignment
                .tables
                .cigar_runs
                .clone()
                .lazy()
                .filter(
                    col(CigarSchema::KIND)
                        .eq(lit(CigarSchema::INSERTION))
                        .and(col(CigarSchema::REF_START).eq(lit(pos))),
                )
                .collect()
                .unwrap()
                .height()
                > 0
        };

        assert!(!has_insertion_at(11));
        assert!(has_insertion_at(12));
        assert!(has_insertion_at(13));
        assert!(!has_insertion_at(14));
    }

    #[test]
    fn is_softclip_at_detects_leading_and_trailing_softclips() {
        let record = read_from_parts(
            10,
            [(Kind::SoftClip, 2), (Kind::Match, 3), (Kind::SoftClip, 1)],
            b"SSAATZ",
        );
        let alignment = Alignment::from_records(
            vec![record],
            0,
            (1, 100),
            &Sequence {
                start: 1,
                sequence: vec![b'A'; 100],
                contig_index: 0,
            },
        )
        .unwrap();

        let is_softclip_at = |pos| {
            alignment
                .tables
                .cigar_runs
                .clone()
                .lazy()
                .filter(
                    col(CigarSchema::KIND)
                        .eq(lit(CigarSchema::SOFT_CLIP))
                        .and(col(CigarSchema::DISPLAY_START).lt_eq(lit(pos)))
                        .and(col(CigarSchema::DISPLAY_END).gt_eq(lit(pos))),
                )
                .collect()
                .unwrap()
                .height()
                > 0
        };

        assert!(!is_softclip_at(7));
        assert!(is_softclip_at(8));
        assert!(is_softclip_at(9));
        assert!(!is_softclip_at(10));
        assert!(!is_softclip_at(12));
        assert!(is_softclip_at(13));
        assert!(!is_softclip_at(14));
    }

    #[test]
    fn extract_base_modifications_preserves_missing_probabilities_for_each_position() {
        let cigars = vec![Op::new(Kind::Match, 3)];
        let sequence = sam::alignment::record_buf::Sequence::from(b"CCC");

        let modifications = extract_base_modifications(
            "C+m,0,0,0;".to_string(),
            None,
            &Flags::default(),
            &sequence,
            &cigars,
            10,
        )
        .unwrap();

        assert_eq!(
            modifications,
            vec![
                (10, modification::FIVE_METHYLCYTOSINE, None),
                (11, modification::FIVE_METHYLCYTOSINE, None),
                (12, modification::FIVE_METHYLCYTOSINE, None),
            ]
        );
    }

    #[test]
    fn extract_base_modifications_consumes_probability_for_each_position_and_modification() {
        let cigars = vec![Op::new(Kind::Match, 2)];
        let sequence = sam::alignment::record_buf::Sequence::from(b"CC");

        let modifications = extract_base_modifications(
            "C+mh,0,0;".to_string(),
            Some(vec![10, 200, 180, 20]),
            &Flags::default(),
            &sequence,
            &cigars,
            10,
        )
        .unwrap();

        assert_eq!(
            modifications,
            vec![
                (10, modification::FIVE_METHYLCYTOSINE, Some(10)),
                (10, modification::FIVE_HYDROXYMETHYLCYTOSINE, Some(200)),
                (11, modification::FIVE_METHYLCYTOSINE, Some(180)),
                (11, modification::FIVE_HYDROXYMETHYLCYTOSINE, Some(20)),
            ]
        );
    }

    #[test]
    fn get_reference_position_from_seq_position_handles_cigar_boundaries() {
        let cigars = vec![
            Op::new(Kind::SoftClip, 2),
            Op::new(Kind::Match, 2),
            Op::new(Kind::Insertion, 1),
            Op::new(Kind::Match, 2),
            Op::new(Kind::SoftClip, 1),
        ];

        assert_eq!(
            get_reference_position_from_seq_position(0, 10, &cigars),
            Some(8)
        );
        assert_eq!(
            get_reference_position_from_seq_position(2, 10, &cigars),
            Some(10)
        );
        assert_eq!(
            get_reference_position_from_seq_position(4, 10, &cigars),
            None
        );
        assert_eq!(
            get_reference_position_from_seq_position(5, 10, &cigars),
            Some(12)
        );
        assert_eq!(
            get_reference_position_from_seq_position(7, 10, &cigars),
            Some(14)
        );
    }

    #[test]
    fn alignment_tables_store_base_modification_annotations() {
        let mut data = Data::default();
        data.insert(Tag::new(b'M', b'm'), Value::from("C+m,0,0,0;"));
        data.insert(Tag::new(b'M', b'l'), Value::from(vec![255u8, 80, 20]));
        let record = sam::alignment::RecordBuf::builder()
            .set_alignment_start(noodles::core::Position::try_from(10).unwrap())
            .set_cigar([Op::new(Kind::Match, 3)].into_iter().collect())
            .set_sequence(sam::alignment::record_buf::Sequence::from(b"CCC"))
            .set_data(data)
            .build();
        let alignment =
            Alignment::from_records(vec![record], 0, (1, 100), &Sequence::default()).unwrap();
        assert_eq!(
            alignment.records[0].data().get(&Tag::new(b'M', b'm')),
            Some(&Value::from("C+m,0,0,0;")),
        );
        assert_eq!(
            alignment.records[0].data().get(&Tag::new(b'M', b'l')),
            Some(&Value::from(vec![255u8, 80, 20])),
        );
        let table = &alignment.tables.base_modifications;
        assert_eq!(
            table
                .column(BaseModificationSchema::DISPLAY_POS)
                .unwrap()
                .u64()
                .unwrap()
                .into_no_null_iter()
                .collect::<Vec<_>>(),
            vec![10, 11, 12]
        );
        assert_eq!(
            table
                .column(BaseModificationSchema::PROBABILITY)
                .unwrap()
                .u8()
                .unwrap()
                .into_no_null_iter()
                .collect::<Vec<_>>(),
            vec![255, 80, 20]
        );
        assert_eq!(
            table
                .column(BaseModificationSchema::CODE)
                .unwrap()
                .u8()
                .unwrap()
                .into_no_null_iter()
                .collect::<Vec<_>>(),
            vec![b'm'; 3]
        );
        assert_eq!(
            table
                .column(BaseModificationSchema::OP_INDEX)
                .unwrap()
                .u32()
                .unwrap()
                .into_no_null_iter()
                .collect::<Vec<_>>(),
            vec![0; 3]
        );
        assert_eq!(
            table
                .column(BaseModificationSchema::RUN_OFFSET)
                .unwrap()
                .u32()
                .unwrap()
                .into_no_null_iter()
                .collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
    }

    #[rstest]
    #[case(10, vec![(Kind::Match, 3)],  b"ATT", false,Sequence::default(), vec![(Kind::Match, 10, 12, vec![], None)])]
    // Test reverse strand
    #[case(10, vec![(Kind::Match, 3)],  b"ATT", true, Sequence::default(), vec![(Kind::Match, 10, 12, vec![], None)])]
    // Test deletion
    #[case(10, vec![(Kind::Match, 3),(Kind::Deletion, 2), (Kind::Match, 3)], b"AAATTT", true, Sequence::default(), vec![(Kind::Match, 10, 12, vec![], None), (Kind::Deletion, 13, 14, vec![], None), (Kind::Match, 15, 17, vec![], None)])]
    // Test RefSkip
    #[case(10, vec![(Kind::Match, 3),(Kind::Skip, 2)], b"AAA", false, Sequence::default(), vec![(Kind::Match, 10, 12, vec![], None), (Kind::Deletion, 13, 14, vec![], None)])]
    // Test insertion
    #[case(10, vec![(Kind::Match, 3), (Kind::Insertion, 2), (Kind::Match, 3)], b"AAATTCCC", false, Sequence::default(), vec![(Kind::Match, 10, 12, vec![], None), (Kind::Match, 13, 15, vec![], None)])]
    // Test soft clips
    #[case(10, vec![(Kind::SoftClip, 2), (Kind::Match, 3), (Kind::SoftClip, 1)], b"GGATTC", true, Sequence::default(), vec![
        (Kind::SoftClip, 8, 8, vec![], Some(b'G')),
        (Kind::SoftClip, 9, 9, vec![], Some(b'G')),
        (Kind::Match, 10, 12, vec![], None),
        (Kind::SoftClip, 13, 13, vec![], Some(b'C'))
    ])]
    // Test Equal cigar (matches current implementation with query pivot)
    #[case(10, vec![(Kind::SequenceMatch, 3)], b"ATT", false, Sequence::default(), vec![(Kind::Match, 10, 12, vec![], None)])]
    // Test Diff cigar (explicit mismatch)
    #[case(10, vec![(Kind::SequenceMismatch, 3)], b"ATT", false, Sequence::default(), vec![(Kind::Match, 10, 12, vec![(10, b'A'),(11, b'T'),(12, b'T')], None)])]
    // Test complex cigar: soft clip + match + insertion + match + deletion + match
    #[case(10, vec![(Kind::SoftClip, 1), (Kind::Match, 2), (Kind::Insertion, 1), (Kind::Match, 2), (Kind::Deletion, 3), (Kind::Match, 2)],
           b"GATCGAAA", false, Sequence::default(), vec![
        (Kind::SoftClip, 9, 9, vec![], Some(b'G')),
        (Kind::Match, 10, 11, vec![], None),
        (Kind::Match, 12, 13, vec![], None),
        (Kind::Deletion, 14, 16, vec![], None),
        (Kind::Match, 17, 18, vec![], None)
    ])]
    // Test soft clips
    #[case(10, vec![(Kind::SoftClip, 2), (Kind::Match, 3), (Kind::SoftClip, 1)], b"GGATTC", true, Sequence{start: 10, sequence: b"AATG".to_vec(), contig_index: 0}, vec![
        (Kind::SoftClip, 8, 8, vec![], Some(b'G')),
        (Kind::SoftClip, 9, 9, vec![], Some(b'G')),
        (Kind::Match, 10, 12, vec![(11, b'T')], None),
        (Kind::SoftClip, 13, 13, vec![], Some(b'C'))
    ])]
    fn cigar_runs_preserve_displayable_cigar_operations(
        #[case] reference_start: u64,
        #[case] cigars: Vec<(Kind, usize)>,
        #[case] seq: &[u8],
        #[case] is_reverse: bool,
        #[case] reference_sequence: Sequence,
        #[case] expected: Vec<(Kind, u64, u64, Vec<(u64, u8)>, Option<u8>)>,
    ) {
        let flags = if is_reverse {
            Flags::REVERSE_COMPLEMENTED
        } else {
            Flags::default()
        };
        let record = sam::alignment::RecordBuf::builder()
            .set_alignment_start(
                noodles::core::Position::try_from(reference_start as usize).unwrap(),
            )
            .set_flags(flags)
            .set_cigar(
                cigars
                    .into_iter()
                    .map(|(kind, len)| Op::new(kind, len))
                    .collect(),
            )
            .set_sequence(sam::alignment::record_buf::Sequence::from(seq))
            .build();
        let alignment =
            Alignment::from_records(vec![record], 0, (1, 100), &reference_sequence).unwrap();
        assert_eq!(
            alignment
                .tables
                .reads
                .column(ReadSchema::POS)
                .unwrap()
                .u64()
                .unwrap()
                .get(0),
            Some(reference_start)
        );
        assert_eq!(
            alignment
                .tables
                .cigar_runs
                .column(CigarSchema::REF_START)
                .unwrap()
                .u64()
                .unwrap()
                .get(0),
            Some(reference_start)
        );
        assert_eq!(alignment.coverage.data.schema(), &CoverageSchema::schema());
        assert_eq!(
            alignment
                .coverage
                .query(reference_start, reference_start)
                .unwrap()
                .column(CoverageSchema::TOTAL)
                .unwrap()
                .u64()
                .unwrap()
                .sum()
                .unwrap_or(0),
            1
        );
        assert_eq!(
            alignment
                .coverage
                .query(100, 100)
                .unwrap()
                .column(CoverageSchema::TOTAL)
                .unwrap()
                .u64()
                .unwrap()
                .sum()
                .unwrap_or(0),
            0
        );
        let mut actual = Vec::new();
        let frame = &alignment.tables.cigar_runs;
        let kinds = frame.column(CigarSchema::KIND).unwrap().u8().unwrap();
        for row in 0..frame.height() {
            let kind = kinds.get(row).unwrap();
            if matches!(
                kind,
                CigarSchema::INSERTION | CigarSchema::HARD_CLIP | CigarSchema::PADDING
            ) {
                continue;
            }
            let start = frame
                .column(CigarSchema::DISPLAY_START)
                .unwrap()
                .u64()
                .unwrap()
                .get(row)
                .unwrap();
            let end = frame
                .column(CigarSchema::DISPLAY_END)
                .unwrap()
                .u64()
                .unwrap()
                .get(row)
                .unwrap();
            let index = frame
                .column(CigarSchema::OP_INDEX)
                .unwrap()
                .u32()
                .unwrap()
                .get(row)
                .unwrap();
            let mut mismatches = Vec::new();
            for annotation in 0..alignment.tables.reference_mismatches.height() {
                let table = &alignment.tables.reference_mismatches;
                if table
                    .column(ReferenceMismatchSchema::OP_INDEX)
                    .unwrap()
                    .u32()
                    .unwrap()
                    .get(annotation)
                    == Some(index)
                {
                    mismatches.push((
                        table
                            .column(ReferenceMismatchSchema::REF_POS)
                            .unwrap()
                            .u64()
                            .unwrap()
                            .get(annotation)
                            .unwrap(),
                        table
                            .column(ReferenceMismatchSchema::BASE)
                            .unwrap()
                            .u8()
                            .unwrap()
                            .get(annotation)
                            .unwrap(),
                    ));
                }
            }
            let offset = frame
                .column(CigarSchema::RUN_OFFSET)
                .unwrap()
                .u32()
                .unwrap()
                .get(row)
                .unwrap() as usize;
            let sequence = if matches!(
                kind,
                CigarSchema::MATCH
                    | CigarSchema::INSERTION
                    | CigarSchema::SOFT_CLIP
                    | CigarSchema::SEQUENCE_MATCH
                    | CigarSchema::SEQUENCE_MISMATCH
            ) {
                frame
                    .column(CigarSchema::SEQ)
                    .unwrap()
                    .str()
                    .unwrap()
                    .get(row)
                    .unwrap()
                    .as_bytes()
            } else {
                &[]
            };
            if kind == CigarSchema::SEQUENCE_MISMATCH {
                mismatches.extend(
                    (start..=end).map(|pos| (pos, sequence[offset + (pos - start) as usize])),
                );
            }
            if kind == CigarSchema::SOFT_CLIP {
                for pos in start..=end {
                    actual.push((
                        Kind::SoftClip,
                        pos,
                        pos,
                        vec![],
                        sequence.get(offset + (pos - start) as usize).copied(),
                    ));
                }
            } else {
                actual.push((
                    if matches!(kind, CigarSchema::DELETION | CigarSchema::REFERENCE_SKIP) {
                        Kind::Deletion
                    } else {
                        Kind::Match
                    },
                    start,
                    end,
                    mismatches,
                    None,
                ));
            }
        }
        actual.sort_by_key(|value| value.1);
        assert_eq!(actual, expected);
        assert_eq!(
            alignment
                .tables
                .reads
                .column(ReadSchema::REVERSE)
                .unwrap()
                .bool()
                .unwrap()
                .get(0),
            Some(is_reverse)
        );
    }
}

//! Columnar alignment data, schemas, construction, and sparse base annotations.
//!
//! The core schemas follow the LAMF alignment layout without its Lance compression
//! metadata. Polars schemas specify column names and types, but do not enforce
//! whether values may be null. Optional tags remain in the original records.

use crate::{alignment::read::matches_base, error::TGVError, sequence::Sequence};
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
    /// Alignment matches, including mismatches (CIGAR `M`).
    pub r#match: DataFrame,
    /// Sequence matches (CIGAR `=`).
    pub sequence_match: DataFrame,
    /// Sequence mismatches (CIGAR `X`).
    pub mismatch: DataFrame,
    /// Insertions into the reference (CIGAR `I`).
    pub insertion: DataFrame,
    /// Deletions from the reference (CIGAR `D`).
    pub deletion: DataFrame,
    /// Skipped reference regions (CIGAR `N`).
    pub reference_skip: DataFrame,
    /// Soft-clipped bases (CIGAR `S`).
    pub soft_clip: DataFrame,
    /// Hard-clipped bases (CIGAR `H`).
    pub hard_clip: DataFrame,
    /// Padding (CIGAR `P`).
    pub padding: DataFrame,
    pub unmapped: DataFrame,
    pub reference_mismatches: DataFrame,
    pub base_modifications: DataFrame,
}

impl Default for AlignmentTables {
    fn default() -> Self {
        Self {
            reads: DataFrame::full_null(&reads_schema(), 0),
            r#match: DataFrame::full_null(&run_schema(Kind::Match), 0),
            sequence_match: DataFrame::full_null(&run_schema(Kind::SequenceMatch), 0),
            mismatch: DataFrame::full_null(&run_schema(Kind::SequenceMismatch), 0),
            insertion: DataFrame::full_null(&run_schema(Kind::Insertion), 0),
            deletion: DataFrame::full_null(&run_schema(Kind::Deletion), 0),
            reference_skip: DataFrame::full_null(&run_schema(Kind::Skip), 0),
            soft_clip: DataFrame::full_null(&run_schema(Kind::SoftClip), 0),
            hard_clip: DataFrame::full_null(&run_schema(Kind::HardClip), 0),
            padding: DataFrame::full_null(&run_schema(Kind::Pad), 0),
            unmapped: DataFrame::full_null(&unmapped_schema(), 0),
            reference_mismatches: DataFrame::full_null(&reference_mismatches_schema(), 0),
            base_modifications: DataFrame::full_null(&base_modifications_schema(), 0),
        }
    }
}

impl AlignmentTables {
    pub fn run(&self, kind: Kind) -> &DataFrame {
        match kind {
            Kind::Match => &self.r#match,
            Kind::SequenceMatch => &self.sequence_match,
            Kind::SequenceMismatch => &self.mismatch,
            Kind::Insertion => &self.insertion,
            Kind::Deletion => &self.deletion,
            Kind::Skip => &self.reference_skip,
            Kind::SoftClip => &self.soft_clip,
            Kind::HardClip => &self.hard_clip,
            Kind::Pad => &self.padding,
        }
    }

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
        let batch = Self::build_batch(records, reference_sequence, contig_index, offset)?;
        self.reads.vstack_mut(&batch.reads)?;
        self.r#match.vstack_mut(&batch.r#match)?;
        self.sequence_match.vstack_mut(&batch.sequence_match)?;
        self.mismatch.vstack_mut(&batch.mismatch)?;
        self.insertion.vstack_mut(&batch.insertion)?;
        self.deletion.vstack_mut(&batch.deletion)?;
        self.reference_skip.vstack_mut(&batch.reference_skip)?;
        self.soft_clip.vstack_mut(&batch.soft_clip)?;
        self.hard_clip.vstack_mut(&batch.hard_clip)?;
        self.padding.vstack_mut(&batch.padding)?;
        self.unmapped.vstack_mut(&batch.unmapped)?;
        self.reference_mismatches
            .vstack_mut(&batch.reference_mismatches)?;
        self.base_modifications
            .vstack_mut(&batch.base_modifications)?;
        Ok(self)
    }

    fn build_batch(
        records: &[RecordBuf],
        reference_sequence: &Sequence,
        contig_index: usize,
        read_id_offset: u64,
    ) -> Result<Self, TGVError> {
        let mut read_id: Vec<u64> = Vec::with_capacity(records.len());
        let mut qname: Vec<Option<String>> = Vec::with_capacity(records.len());
        let mut ref_id: Vec<Option<u32>> = Vec::with_capacity(records.len());
        let mut pos: Vec<Option<u32>> = Vec::with_capacity(records.len());
        let mut mapq: Vec<Option<u8>> = Vec::with_capacity(records.len());
        let mut next_ref_id: Vec<Option<u32>> = Vec::with_capacity(records.len());
        let mut next_pos: Vec<Option<u32>> = Vec::with_capacity(records.len());
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
        let mut run_read_id: [Vec<u64>; 9] = std::array::from_fn(|_| Vec::new());
        let mut run_op_index: [Vec<u32>; 9] = std::array::from_fn(|_| Vec::new());
        let mut run_ref_id: [Vec<Option<u32>>; 9] = std::array::from_fn(|_| Vec::new());
        let mut run_ref_start: [Vec<Option<u64>>; 9] = std::array::from_fn(|_| Vec::new());
        let mut run_op_len: [Vec<u32>; 9] = std::array::from_fn(|_| Vec::new());
        let mut run_seq: [Vec<Option<String>>; 9] = std::array::from_fn(|_| Vec::new());
        let mut run_qual: [Vec<Option<Vec<u8>>>; 9] = std::array::from_fn(|_| Vec::new());
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
                &mut run_ref_id,
                &mut run_ref_start,
                &mut run_op_len,
                &mut run_seq,
                &mut run_qual,
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
            Column::new("read_id".into(), read_id),
            Column::new("qname".into(), qname),
            Column::new("ref_id".into(), ref_id),
            Column::new("pos".into(), pos),
            Column::new("mapq".into(), mapq),
            Column::new("next_ref_id".into(), next_ref_id),
            Column::new("next_pos".into(), next_pos),
            Column::new("tlen".into(), tlen),
            Column::new("stacking_start".into(), stacking_start),
            Column::new("stacking_end".into(), stacking_end),
            Column::new("paired".into(), paired),
            Column::new("proper_pair".into(), proper_pair),
            Column::new("unmapped".into(), unmapped),
            Column::new("mate_unmapped".into(), mate_unmapped),
            Column::new("reverse".into(), reverse),
            Column::new("mate_reverse".into(), mate_reverse),
            Column::new("first_segment".into(), first_segment),
            Column::new("last_segment".into(), last_segment),
            Column::new("secondary".into(), secondary),
            Column::new("qc_failed".into(), qc_failed),
            Column::new("duplicate".into(), duplicate),
            Column::new("supplementary".into(), supplementary),
        ];
        let reads = DataFrame::new(height, read_columns)?;
        let [
            r#match,
            sequence_match,
            mismatch,
            insertion,
            deletion,
            reference_skip,
            soft_clip,
            hard_clip,
            padding,
        ] = [
            (0, Kind::Match),
            (1, Kind::SequenceMatch),
            (2, Kind::SequenceMismatch),
            (3, Kind::Insertion),
            (4, Kind::Deletion),
            (5, Kind::Skip),
            (6, Kind::SoftClip),
            (7, Kind::HardClip),
            (8, Kind::Pad),
        ]
        .map(|(index, kind)| -> Result<DataFrame, TGVError> {
            let height = run_read_id[index].len();
            let mut columns = vec![
                Column::new("read_id".into(), std::mem::take(&mut run_read_id[index])),
                Column::new("op_index".into(), std::mem::take(&mut run_op_index[index])),
                Column::new("ref_id".into(), std::mem::take(&mut run_ref_id[index])),
                Column::new(
                    "ref_start".into(),
                    std::mem::take(&mut run_ref_start[index]),
                ),
                Column::new("op_len".into(), std::mem::take(&mut run_op_len[index])),
            ];
            if kind.consumes_read() {
                columns.push(Column::new(
                    "seq".into(),
                    std::mem::take(&mut run_seq[index]),
                ));
                columns.push(binary_column("qual", &run_qual[index]));
            }
            Ok(DataFrame::new(height, columns)?)
        });
        let r#match = r#match?;
        let sequence_match = sequence_match?;
        let mismatch = mismatch?;
        let insertion = insertion?;
        let deletion = deletion?;
        let reference_skip = reference_skip?;
        let soft_clip = soft_clip?;
        let hard_clip = hard_clip?;
        let padding = padding?;
        let unmapped = DataFrame::new(
            unmapped_read_id.len(),
            vec![
                Column::new("read_id".into(), unmapped_read_id),
                Column::new("base_count".into(), base_count),
                Column::new("seq".into(), unmapped_seq),
                binary_column("qual", &unmapped_qual),
            ],
        )?;
        let reference_mismatches =
            reference_mismatches(&r#match, reference_sequence, contig_index)?;
        let base_modifications = base_modifications(records, read_id_offset)?;
        Ok(Self {
            reads,
            r#match,
            sequence_match,
            mismatch,
            insertion,
            deletion,
            reference_skip,
            soft_clip,
            hard_clip,
            padding,
            unmapped,
            reference_mismatches,
            base_modifications,
        })
    }
}

fn append_read(
    record: &RecordBuf,
    id: u64,
    read_id: &mut Vec<u64>,
    qname: &mut Vec<Option<String>>,
    ref_id: &mut Vec<Option<u32>>,
    pos: &mut Vec<Option<u32>>,
    mapq: &mut Vec<Option<u8>>,
    next_ref_id: &mut Vec<Option<u32>>,
    next_pos: &mut Vec<Option<u32>>,
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
    pos.push(record.alignment_start().map(|p| p.get() as u32));
    mapq.push(record.mapping_quality().map(|q| q.get()));
    next_ref_id.push(record.mate_reference_sequence_id().map(|id| id as u32));
    next_pos.push(record.mate_alignment_start().map(|p| p.get() as u32));
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
    run_read_id: &mut [Vec<u64>; 9],
    run_op_index: &mut [Vec<u32>; 9],
    run_ref_id: &mut [Vec<Option<u32>>; 9],
    run_ref_start: &mut [Vec<Option<u64>>; 9],
    run_op_len: &mut [Vec<u32>; 9],
    run_seq: &mut [Vec<Option<String>>; 9],
    run_qual: &mut [Vec<Option<Vec<u8>>>; 9],
) -> Result<(), TGVError> {
    let quality = record.quality_scores().as_ref();
    let reference_id = record.reference_sequence_id().map(|id| id as u32);
    let mut query_cursor = 0usize;
    let mut reference_cursor = record.alignment_start().map(|p| p.get() as u64);
    for (op_idx, op) in record.cigar().as_ref().iter().enumerate() {
        let kind = op.kind();
        let len = op.len();
        let index = match kind {
            Kind::Match => 0,
            Kind::SequenceMatch => 1,
            Kind::SequenceMismatch => 2,
            Kind::Insertion => 3,
            Kind::Deletion => 4,
            Kind::Skip => 5,
            Kind::SoftClip => 6,
            Kind::HardClip => 7,
            Kind::Pad => 8,
        };
        run_read_id[index].push(id);
        run_op_index[index].push(op_idx as u32);
        run_ref_id[index].push(reference_id);
        run_ref_start[index].push(reference_cursor);
        run_op_len[index].push(len as u32);
        if kind.consumes_read() {
            let end = query_cursor + len;
            if end > record.sequence().len() && !record.sequence().is_empty() {
                return Err(TGVError::AlignmentParseError(format!(
                    "CIGAR exceeds sequence length for read {id}"
                )));
            }
            run_seq[index].push(if sequence.is_empty() {
                None
            } else {
                Some(sequence[query_cursor..end].to_owned())
            });
            run_qual[index].push(if quality.is_empty() {
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

/// The table name for a noodles CIGAR operation kind.
pub const fn run_table_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Match => "match",
        Kind::SequenceMatch => "sequence_match",
        Kind::SequenceMismatch => "mismatch",
        Kind::Insertion => "insertion",
        Kind::Deletion => "deletion",
        Kind::Skip => "reference_skip",
        Kind::SoftClip => "soft_clip",
        Kind::HardClip => "hard_clip",
        Kind::Pad => "padding",
    }
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
pub fn reads_schema() -> SchemaRef {
    let mut schema = Schema::with_capacity(22);
    schema.insert("read_id".into(), DataType::UInt64);
    schema.insert("qname".into(), DataType::String);
    schema.insert("ref_id".into(), DataType::UInt32);
    schema.insert("pos".into(), DataType::UInt32);
    schema.insert("mapq".into(), DataType::UInt8);
    schema.insert("next_ref_id".into(), DataType::UInt32);
    schema.insert("next_pos".into(), DataType::UInt32);
    schema.insert("tlen".into(), DataType::Int32);
    schema.insert("stacking_start".into(), DataType::UInt64);
    schema.insert("stacking_end".into(), DataType::UInt64);
    schema.insert("paired".into(), DataType::Boolean);
    schema.insert("proper_pair".into(), DataType::Boolean);
    schema.insert("unmapped".into(), DataType::Boolean);
    schema.insert("mate_unmapped".into(), DataType::Boolean);
    schema.insert("reverse".into(), DataType::Boolean);
    schema.insert("mate_reverse".into(), DataType::Boolean);
    schema.insert("first_segment".into(), DataType::Boolean);
    schema.insert("last_segment".into(), DataType::Boolean);
    schema.insert("secondary".into(), DataType::Boolean);
    schema.insert("qc_failed".into(), DataType::Boolean);
    schema.insert("duplicate".into(), DataType::Boolean);
    schema.insert("supplementary".into(), DataType::Boolean);
    Arc::new(schema)
}

/// The schema for one CIGAR operation kind.
///
/// `ref_start` is one-based. For reference-consuming operations, the exclusive
/// end is `ref_start + op_len`; other operations retain the reference cursor
/// without advancing it. Each `read_id` indexes the original loaded reads table.
/// Read-consuming operations store their ASCII SEQ substring as a UTF-8 string.
/// Missing SEQ and quality scores are null.
pub fn run_schema(kind: Kind) -> SchemaRef {
    let mut schema = Schema::with_capacity(if kind.consumes_read() { 7 } else { 5 });
    schema.insert("read_id".into(), DataType::UInt64);
    schema.insert("op_index".into(), DataType::UInt32);
    schema.insert("ref_id".into(), DataType::UInt32);
    schema.insert("ref_start".into(), DataType::UInt64);
    schema.insert("op_len".into(), DataType::UInt32);
    if kind.consumes_read() {
        schema.insert("seq".into(), DataType::String);
        schema.insert("qual".into(), DataType::Binary);
    }
    Arc::new(schema)
}

/// The schema for records without CIGAR operations.
///
/// Missing SEQ and quality scores are null.
pub fn unmapped_schema() -> SchemaRef {
    let mut schema = Schema::with_capacity(4);
    schema.insert("read_id".into(), DataType::UInt64);
    schema.insert("base_count".into(), DataType::UInt32);
    schema.insert("seq".into(), DataType::String);
    schema.insert("qual".into(), DataType::Binary);
    Arc::new(schema)
}

/// Sparse reference mismatches within M runs.
pub fn reference_mismatches_schema() -> SchemaRef {
    let mut schema = Schema::with_capacity(5);
    schema.insert("read_id".into(), DataType::UInt64);
    schema.insert("op_index".into(), DataType::UInt32);
    schema.insert("run_offset".into(), DataType::UInt32);
    schema.insert("ref_pos".into(), DataType::UInt64);
    schema.insert("base".into(), DataType::UInt8);
    Arc::new(schema)
}

/// Sparse MM/ML annotations, with exactly one of code and ChEBI ID populated.
///
/// Positions are one-based display coordinates, including projected soft clips.
/// `source_order` preserves MM/ML order for deterministic probability ties.
/// Missing ML probabilities are null; a recorded probability of 255 remains 255.
pub fn base_modifications_schema() -> SchemaRef {
    let mut schema = Schema::with_capacity(8);
    schema.insert("read_id".into(), DataType::UInt64);
    schema.insert("op_index".into(), DataType::UInt32);
    schema.insert("run_offset".into(), DataType::UInt32);
    schema.insert("display_pos".into(), DataType::UInt64);
    schema.insert("code".into(), DataType::UInt8);
    schema.insert("chebi_id".into(), DataType::UInt32);
    schema.insert("probability".into(), DataType::UInt8);
    schema.insert("source_order".into(), DataType::UInt64);
    Arc::new(schema)
}

pub(super) fn reference_mismatches(
    runs: &DataFrame,
    reference: &Sequence,
    contig_index: usize,
) -> Result<DataFrame, TGVError> {
    if reference.contig_index != contig_index || reference.sequence.is_empty() {
        return Ok(DataFrame::full_null(&reference_mismatches_schema(), 0));
    }
    let ids = runs.column("read_id")?.u64()?;
    let indexes = runs.column("op_index")?.u32()?;
    let starts = runs.column("ref_start")?.u64()?;
    let lengths = runs.column("op_len")?.u32()?;
    let sequences = runs.column("seq")?.str()?;
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
            Column::new("read_id".into(), read_id),
            Column::new("op_index".into(), op_index),
            Column::new("run_offset".into(), run_offset),
            Column::new("ref_pos".into(), ref_pos),
            Column::new("base".into(), base),
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
            Column::new("read_id".into(), read_id),
            Column::new("op_index".into(), op_index),
            Column::new("run_offset".into(), run_offset),
            Column::new("display_pos".into(), display_pos),
            Column::new("code".into(), code),
            Column::new("chebi_id".into(), chebi_id),
            Column::new("probability".into(), probability),
            Column::new("source_order".into(), source_order),
        ],
    )?)
}

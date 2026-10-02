//! Polars schemas for the alignment tables and their shared column conventions.
//!
//! The schemas follow the LAMF alignment layout without its Lance compression
//! metadata. Polars schemas specify column names and types, but do not enforce
//! whether values may be null.

use noodles::sam::alignment::record::{Flags, cigar::op::Kind};
use polars::prelude::{DataType, Schema, SchemaRef};
use std::sync::Arc;

/// The SAM flag columns in the order they appear in the reads table.
pub const FLAG_FIELDS: [(&str, Flags); 12] = [
    ("paired", Flags::SEGMENTED),
    ("proper_pair", Flags::PROPERLY_SEGMENTED),
    ("unmapped", Flags::UNMAPPED),
    ("mate_unmapped", Flags::MATE_UNMAPPED),
    ("reverse", Flags::REVERSE_COMPLEMENTED),
    ("mate_reverse", Flags::MATE_REVERSE_COMPLEMENTED),
    ("first_segment", Flags::FIRST_SEGMENT),
    ("last_segment", Flags::LAST_SEGMENT),
    ("secondary", Flags::SECONDARY),
    ("qc_failed", Flags::QC_FAIL),
    ("duplicate", Flags::DUPLICATE),
    ("supplementary", Flags::SUPPLEMENTARY),
];

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
/// merged tgv contig index. `pos` and `next_pos` are zero-based.
pub fn reads_schema() -> SchemaRef {
    let mut schema = Schema::with_capacity(8 + FLAG_FIELDS.len());
    schema.insert("read_id".into(), DataType::UInt64);
    schema.insert("qname".into(), DataType::String);
    schema.insert("ref_id".into(), DataType::UInt32);
    schema.insert("pos".into(), DataType::UInt32);
    schema.insert("mapq".into(), DataType::UInt8);
    schema.insert("next_ref_id".into(), DataType::UInt32);
    schema.insert("next_pos".into(), DataType::UInt32);
    schema.insert("tlen".into(), DataType::Int32);
    for (name, _) in FLAG_FIELDS {
        schema.insert(name.into(), DataType::Boolean);
    }
    Arc::new(schema)
}

// /// The alignment header's reference dictionary schema.
// pub fn references_schema() -> SchemaRef {
//     let mut schema = Schema::with_capacity(11);
//     schema.insert("ref_id".into(), DataType::UInt32);
//     schema.insert("name".into(), DataType::String);
//     schema.insert("length".into(), DataType::UInt32);
//     schema.insert("md5".into(), DataType::String);
//     schema.insert("uri".into(), DataType::String);
//     schema.insert("assembly".into(), DataType::String);
//     schema.insert("species".into(), DataType::String);
//     schema.insert("topology".into(), DataType::String);
//     schema.insert("aliases".into(), DataType::List(Box::new(DataType::String)));
//     schema.insert("alt_locus".into(), DataType::String);
//     schema.insert("description".into(), DataType::String);
//     Arc::new(schema)
// }

/// The schema for one CIGAR operation kind.
///
/// `ref_start` is zero-based. For reference-consuming operations, the exclusive
/// end is `ref_start + op_len`; other operations retain the reference cursor
/// without advancing it. Each `read_id` indexes the original loaded reads table.
pub fn run_schema(kind: Kind) -> SchemaRef {
    let mut schema = Schema::with_capacity(if kind.consumes_read() { 7 } else { 5 });
    schema.insert("read_id".into(), DataType::UInt64);
    schema.insert("op_index".into(), DataType::UInt32);
    schema.insert("ref_id".into(), DataType::UInt32);
    schema.insert("ref_start".into(), DataType::UInt64);
    schema.insert("op_len".into(), DataType::UInt32);
    if kind.consumes_read() {
        schema.insert("seq".into(), DataType::Binary);
        schema.insert("qual".into(), DataType::Binary);
    }
    Arc::new(schema)
}

/// The optional SAM tag table schema.
///
/// `data_tag` contains the two original ASCII tag bytes. Polars represents
/// these values as variable-length binary because it has no fixed-size binary
/// column type. `data_value` preserves the original BAM-encoded payload.
pub fn tags_schema() -> SchemaRef {
    let mut schema = Schema::with_capacity(4);
    schema.insert("read_id".into(), DataType::UInt64);
    schema.insert("data_tag".into(), DataType::Binary);
    schema.insert("data_type".into(), DataType::UInt8);
    schema.insert("data_value".into(), DataType::BinaryOffset);
    Arc::new(schema)
}

/// The schema for records without CIGAR operations.
pub fn unmapped_schema() -> SchemaRef {
    let mut schema = Schema::with_capacity(4);
    schema.insert("read_id".into(), DataType::UInt64);
    schema.insert("base_count".into(), DataType::UInt32);
    schema.insert("seq".into(), DataType::Binary);
    schema.insert("qual".into(), DataType::Binary);
    Arc::new(schema)
}

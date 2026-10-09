//! SQL functions that read one CIGAR operation at a reference position.
//!
//! Per-base questions, such as allele counts or base qualities at a site, otherwise need index
//! arithmetic over `cigar_ops.seq` and `cigar_ops.qual`. These functions take the packed
//! `cigar_ops.op` column and a 1-based reference position, and return null where the operation
//! does not cover the position. They are evaluated row by row on the operations that survive a
//! query's filters, so no per-base table is built.
//!
//! Each function checks its arguments when Polars plans the query, so a wrong column or a
//! missing position fails before any data is read.

use crate::tables::{PackedColumn, PackedType};
use gv_core::alignment::tables::CigarSchema;
use polars::{
    prelude::*,
    sql::{FunctionOptions, FunctionRegistry},
};
use serde::Serialize;

/// Names the fields of the packed `cigar_ops.op` column.
pub(crate) struct OpStruct;

impl OpStruct {
    pub const NAME: &'static str = "op";
    pub const KIND: &'static str = CigarSchema::KIND;
    pub const REF_START: &'static str = CigarSchema::REF_START;
    pub const REF_END: &'static str = crate::tables::AddedColumns::REF_END;
    pub const SEQ: &'static str = CigarSchema::SEQ;
    pub const QUAL: &'static str = CigarSchema::QUAL;
    pub const READ_OFFSET: &'static str = CigarSchema::READ_OFFSET;

    pub const COLUMN: PackedColumn = PackedColumn {
        name: Self::NAME,
        fields: &[
            (Self::KIND, PackedType::String),
            (Self::REF_START, PackedType::Int64),
            (Self::REF_END, PackedType::Int64),
            (Self::SEQ, PackedType::String),
            (Self::QUAL, PackedType::UInt8List),
            (Self::READ_OFFSET, PackedType::Int64),
        ],
        description: "The operation's `kind`, `ref_start`, `ref_end`, `seq`, `qual`, and `read_offset`, packed for `allele_at`, `qual_at`, `offset_at`, and `insertion_after`.",
    };
}

/// A SQL function over one CIGAR operation at a reference position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpFunction {
    AlleleAt,
    QualAt,
    OffsetAt,
    InsertionAfter,
}

/// Describes one SQL function in the catalog.
#[derive(Serialize)]
pub struct CatalogFunction {
    pub name: &'static str,
    pub signature: &'static str,
    pub returns: String,
    pub description: &'static str,
}

impl OpFunction {
    pub const ALL: [Self; 4] = [
        Self::AlleleAt,
        Self::QualAt,
        Self::OffsetAt,
        Self::InsertionAfter,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::AlleleAt => "allele_at",
            Self::QualAt => "qual_at",
            Self::OffsetAt => "offset_at",
            Self::InsertionAfter => "insertion_after",
        }
    }

    fn returns(self) -> DataType {
        match self {
            Self::AlleleAt | Self::InsertionAfter => DataType::String,
            Self::QualAt => DataType::UInt8,
            Self::OffsetAt => DataType::Int64,
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::AlleleAt => {
                "The uppercase read base at `pos` for `M`, `=`, and `X` operations, or `*` where a `D` operation covers `pos`. Null otherwise, or when the read has no SEQ."
            }
            Self::QualAt => {
                "The Phred base quality at `pos`, from 0 to 93, for `M`, `=`, and `X` operations. Null otherwise, or when the read has no qualities."
            }
            Self::OffsetAt => {
                "The zero-based offset in the read's stored SEQ of the base at `pos`, for `M`, `=`, and `X` operations. Null otherwise. For a reverse-strand read, the sequencing cycle counts from the other end."
            }
            Self::InsertionAfter => {
                "The inserted bases when the operation is an insertion between `pos` and `pos + 1`. Null otherwise."
            }
        }
    }

    pub fn catalog(self) -> CatalogFunction {
        CatalogFunction {
            name: self.name(),
            signature: match self {
                Self::AlleleAt => "allele_at(op, pos)",
                Self::QualAt => "qual_at(op, pos)",
                Self::OffsetAt => "offset_at(op, pos)",
                Self::InsertionAfter => "insertion_after(op, pos)",
            },
            returns: self.returns().to_string(),
            description: self.description(),
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|function| function.name() == name)
    }

    /// Checks the argument types at planning time and returns the output field.
    fn output_field(self, fields: &[Field]) -> PolarsResult<Field> {
        let usage = || {
            polars_err!(
                SQLInterface: "{} expects `cigar_ops.op` and an integer reference position, as in `{}(c.op, 88108)`",
                self.name(),
                self.name()
            )
        };
        let [op, pos] = fields else {
            return Err(usage());
        };
        let position_is_integer = pos.dtype().is_integer()
            || matches!(pos.dtype(), DataType::Unknown(UnknownKind::Int(_)));
        if op.dtype() != &OpStruct::COLUMN.dtype() || !position_is_integer {
            return Err(usage());
        }
        Ok(Field::new(self.name().into(), self.returns()))
    }

    fn udf(self) -> UserDefinedFunction {
        let mut function = UserDefinedFunction::new(
            self.name().into(),
            BaseColumnUdf::new(
                move |columns: &mut [Column]| self.evaluate(columns),
                move |_: &Schema, fields: &[Field]| self.output_field(fields),
            ),
        );
        // Elementwise functions let Polars apply filters, such as an overlap test, first.
        function.options = FunctionOptions::elementwise();
        function
    }

    fn evaluate(self, columns: &[Column]) -> PolarsResult<Column> {
        let [op, pos] = columns else {
            polars_bail!(SQLInterface: "{} expects two arguments", self.name());
        };
        let length = match (op.len(), pos.len()) {
            (op_len, pos_len) if op_len == pos_len => op_len,
            (1, pos_len) => pos_len,
            (op_len, 1) => op_len,
            (op_len, pos_len) => {
                polars_bail!(ShapeMismatch:
                    "{} expects equal argument lengths or a scalar, got {} operations and {} positions",
                    self.name(), op_len, pos_len
                );
            }
        };
        // Aggregated operations and literal positions can each arrive as a single value.
        let op = if op.len() != length {
            op.new_from_index(0, length)
        } else {
            op.clone()
        };
        let op = op.as_materialized_series().struct_()?;
        let field = |name: &str| op.field_by_name(name);
        let kind = field(OpStruct::KIND)?;
        let ref_start = field(OpStruct::REF_START)?;
        let ref_end = field(OpStruct::REF_END)?;
        let pos = pos.cast(&DataType::Int64)?;
        let pos = if pos.len() != length {
            pos.new_from_index(0, length)
        } else {
            pos
        };
        let ops = kind
            .str()?
            .iter()
            .zip(ref_start.i64()?.iter())
            .zip(ref_end.i64()?.iter())
            .zip(pos.i64()?.iter())
            .map(|(((kind, ref_start), ref_end), pos)| Op {
                kind: kind.and_then(|kind| kind.bytes().next()),
                ref_start,
                ref_end,
                pos,
            });
        let name = PlSmallStr::from_static(self.name());
        let output = match self {
            Self::AlleleAt => {
                let seq = field(OpStruct::SEQ)?;
                let mut builder = StringChunkedBuilder::new(name, length);
                for (op, seq) in ops.zip(seq.str()?.iter()) {
                    if op.deletion_covers() {
                        builder.append_value("*");
                        continue;
                    }
                    match op
                        .base_index()
                        .and_then(|index| seq?.as_bytes().get(index).copied())
                    {
                        Some(base) => {
                            let base = [base.to_ascii_uppercase()];
                            builder.append_value(std::str::from_utf8(&base).unwrap_or("N"));
                        }
                        None => builder.append_null(),
                    }
                }
                builder.finish().into_series()
            }
            Self::QualAt => {
                let qual = field(OpStruct::QUAL)?;
                let mut values = Vec::with_capacity(length);
                for (op, scores) in ops.zip(qual.list()?.amortized_iter()) {
                    values.push(match (op.base_index(), scores) {
                        (Some(index), Some(scores)) => scores.as_ref().u8()?.get(index),
                        _ => None,
                    });
                }
                UInt8Chunked::from_iter_options(name, values.into_iter()).into_series()
            }
            Self::OffsetAt => {
                let read_offset = field(OpStruct::READ_OFFSET)?;
                let values = ops
                    .zip(read_offset.i64()?.iter())
                    .map(|(op, read_offset)| Some(read_offset? + op.base_index()? as i64));
                Int64Chunked::from_iter_options(name, values).into_series()
            }
            Self::InsertionAfter => {
                let seq = field(OpStruct::SEQ)?;
                let values = ops
                    .zip(seq.str()?.iter())
                    .map(|(op, seq)| op.insertion_follows().then_some(seq).flatten());
                StringChunked::from_iter_options(name, values).into_series()
            }
        };
        Ok(output.into_column())
    }
}

/// One CIGAR operation and the reference position a function reads it at.
struct Op {
    kind: Option<u8>,
    ref_start: Option<i64>,
    ref_end: Option<i64>,
    pos: Option<i64>,
}

impl Op {
    /// Whether the position lies within the operation's reference span.
    fn covers(&self) -> bool {
        matches!(
            (self.ref_start, self.ref_end, self.pos),
            (Some(start), Some(end), Some(pos)) if start <= pos && pos <= end
        )
    }

    fn deletion_covers(&self) -> bool {
        self.kind == Some(b'D') && self.covers()
    }

    /// The zero-based index into the operation's SEQ of the base aligned to the position.
    fn base_index(&self) -> Option<usize> {
        let aligned = matches!(self.kind, Some(b'M' | b'=' | b'X'));
        if !aligned || !self.covers() {
            return None;
        }
        Some((self.pos? - self.ref_start?) as usize)
    }

    /// Whether the operation is an insertion between the position and the next one, since an
    /// insertion's `ref_start` is the next reference position.
    fn insertion_follows(&self) -> bool {
        self.kind == Some(b'I')
            && matches!(
                (self.ref_start, self.pos),
                (Some(start), Some(pos)) if pos.checked_add(1) == Some(start)
            )
    }
}

/// Resolves the op-at-position functions for a SQL context.
pub(crate) struct OpFunctions;

impl FunctionRegistry for OpFunctions {
    fn register(&mut self, name: &str, _fun: UserDefinedFunction) -> PolarsResult<()> {
        polars_bail!(SQLInterface: "functions cannot be registered, including '{name}'")
    }

    fn get_udf(&self, name: &str) -> PolarsResult<Option<UserDefinedFunction>> {
        Ok(OpFunction::from_name(name).map(OpFunction::udf))
    }

    fn contains(&self, name: &str) -> bool {
        OpFunction::from_name(name).is_some()
    }
}

//! Independent columnar coverage storage, construction, and queries.

use super::tables::CigarSchema;
use crate::{error::TGVError, sequence::Sequence, table_schema::TableSchema};
use polars::prelude::*;
use std::{collections::BTreeMap, sync::Arc};

/// Sparse coverage by one-based position, independent of the alignment tables.
#[derive(Debug)]
pub struct CoverageTable {
    /// Rows are sorted by position, and every column is non-null.
    pub data: DataFrame,
}

impl Default for CoverageTable {
    fn default() -> Self {
        Self {
            data: CoverageSchema::empty(),
        }
    }
}

impl CoverageTable {
    /// Construct coverage from a lazy query of visible CIGAR runs.
    pub fn from_runs(
        runs: LazyFrame,
        contig_index: usize,
        reference_sequence: &Sequence,
    ) -> Result<Self, TGVError> {
        // TODO: used only once. Consolidate.
        let mut coverage: BTreeMap<u64, [u64; 7]> = BTreeMap::new();
        let kind = col(CigarSchema::KIND);
        let runs = runs
            .filter(
                kind.clone()
                    .eq(lit(CigarSchema::MATCH))
                    .or(kind.clone().eq(lit(CigarSchema::SEQUENCE_MATCH)))
                    .or(kind.clone().eq(lit(CigarSchema::SEQUENCE_MISMATCH)))
                    .or(kind.eq(lit(CigarSchema::SOFT_CLIP)))
                    .and(col(CigarSchema::DISPLAY_START).is_not_null())
                    .and(col(CigarSchema::DISPLAY_END).is_not_null()),
            )
            .collect()?;
        let kinds = runs.column(CigarSchema::KIND)?.u8()?;
        let starts = runs.column(CigarSchema::DISPLAY_START)?.u64()?;
        let ends = runs.column(CigarSchema::DISPLAY_END)?.u64()?;
        let offsets = runs.column(CigarSchema::RUN_OFFSET)?.u32()?;
        let sequences = runs.column(CigarSchema::SEQ)?.str()?;
        for row in 0..runs.height() {
            let kind = kinds.get(row).expect("CIGAR kinds are non-null");
            let start = starts.get(row).expect("queried runs have display bounds");
            let end = ends.get(row).expect("queried runs have display bounds");
            let offset = offsets.get(row).expect("queried runs have offsets") as usize;
            let Some(sequence) = sequences.get(row) else {
                continue;
            };
            let sequence = sequence.as_bytes();
            for position in start..=end {
                let base = sequence[offset + (position - start) as usize];
                let counts = coverage.entry(position).or_default();
                if kind == CigarSchema::SOFT_CLIP {
                    counts[6] += 1;
                } else {
                    let index = match base {
                        b'A' | b'a' => 0,
                        b'T' | b't' => 1,
                        b'C' | b'c' => 2,
                        b'G' | b'g' => 3,
                        _ => 4,
                    };
                    counts[index] += 1;
                    counts[5] += 1;
                }
            }
        }
        let mut positions = Vec::with_capacity(coverage.len());
        let mut a = Vec::with_capacity(coverage.len());
        let mut t = Vec::with_capacity(coverage.len());
        let mut c = Vec::with_capacity(coverage.len());
        let mut g = Vec::with_capacity(coverage.len());
        let mut n = Vec::with_capacity(coverage.len());
        let mut total = Vec::with_capacity(coverage.len());
        let mut softclip = Vec::with_capacity(coverage.len());
        let mut reference_base = Vec::with_capacity(coverage.len());
        for (position, coverage) in coverage {
            positions.push(position);
            a.push(coverage[0]);
            t.push(coverage[1]);
            c.push(coverage[2]);
            g.push(coverage[3]);
            n.push(coverage[4]);
            total.push(coverage[5]);
            softclip.push(coverage[6]);
            reference_base.push(if reference_sequence.contig_index == contig_index {
                reference_sequence.base_at(position).unwrap_or(b'N')
            } else {
                b'N'
            });
        }
        let data = DataFrame::new(
            positions.len(),
            vec![
                Column::new(CoverageSchema::POS.into(), positions),
                Column::new(CoverageSchema::A.into(), a),
                Column::new(CoverageSchema::T.into(), t),
                Column::new(CoverageSchema::C.into(), c),
                Column::new(CoverageSchema::G.into(), g),
                Column::new(CoverageSchema::N.into(), n),
                Column::new(CoverageSchema::TOTAL.into(), total),
                Column::new(CoverageSchema::SOFTCLIP.into(), softclip),
                Column::new(CoverageSchema::REFERENCE_BASE.into(), reference_base),
            ],
        )?;

        Ok(Self { data })
    }

    /// Select sparse rows within a one-based, inclusive interval.
    pub fn query(&self, start: u64, end: u64) -> Result<DataFrame, TGVError> {
        Ok(self
            .data
            .clone()
            .lazy()
            .filter(
                col(CoverageSchema::POS)
                    .gt_eq(lit(start))
                    .and(col(CoverageSchema::POS).lt_eq(lit(end))),
            )
            .collect()?)
    }
}

/// Sparse per-position counts, positions, and reference bases.
///
/// `pos` is one-based. Rows are sorted by position and contain no null values.
/// Positions without read or soft-clip coverage have no row.
pub struct CoverageSchema;

impl CoverageSchema {
    pub const POS: &'static str = "pos";
    pub const A: &'static str = "A";
    pub const T: &'static str = "T";
    pub const C: &'static str = "C";
    pub const G: &'static str = "G";
    pub const N: &'static str = "N";
    pub const TOTAL: &'static str = "total";
    pub const SOFTCLIP: &'static str = "softclip";
    pub const REFERENCE_BASE: &'static str = "reference_base";
}

impl TableSchema for CoverageSchema {
    fn schema() -> SchemaRef {
        let mut schema = Schema::with_capacity(9);
        schema.insert(Self::POS.into(), DataType::UInt64);
        schema.insert(Self::A.into(), DataType::UInt64);
        schema.insert(Self::T.into(), DataType::UInt64);
        schema.insert(Self::C.into(), DataType::UInt64);
        schema.insert(Self::G.into(), DataType::UInt64);
        schema.insert(Self::N.into(), DataType::UInt64);
        schema.insert(Self::TOTAL.into(), DataType::UInt64);
        schema.insert(Self::SOFTCLIP.into(), DataType::UInt64);
        schema.insert(Self::REFERENCE_BASE.into(), DataType::UInt8);
        Arc::new(schema)
    }
}

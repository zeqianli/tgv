//! Independent columnar coverage storage, construction, and queries.

use crate::{alignment::AlignmentViewport, error::TGVError, sequence::Sequence};
use noodles::sam::alignment::record::cigar::op::Kind;
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
            data: DataFrame::full_null(&Self::schema(), 0),
        }
    }
}

impl CoverageTable {
    /// Construct coverage from projected, visible CIGAR runs.
    pub fn from_runs(
        viewport: &AlignmentViewport,
        contig_index: usize,
        reference_sequence: &Sequence,
    ) -> Result<Self, TGVError> {
        let mut coverage: BTreeMap<u64, [u64; 7]> = BTreeMap::new();
        for (kind, runs) in &viewport.runs {
            if !matches!(
                kind,
                Kind::Match | Kind::SequenceMatch | Kind::SequenceMismatch | Kind::SoftClip
            ) {
                continue;
            }
            let starts = runs.column("display_start")?.u64()?;
            let ends = runs.column("display_end")?.u64()?;
            let offsets = runs.column("run_offset")?.u32()?;
            let sequences = runs.column("seq")?.str()?;
            for row in 0..runs.height() {
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
                    if *kind == Kind::SoftClip {
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
                Column::new("pos".into(), positions),
                Column::new("A".into(), a),
                Column::new("T".into(), t),
                Column::new("C".into(), c),
                Column::new("G".into(), g),
                Column::new("N".into(), n),
                Column::new("total".into(), total),
                Column::new("softclip".into(), softclip),
                Column::new("reference_base".into(), reference_base),
            ],
        )?;

        Ok(Self { data })
    }

    /// Sum coverage counts across a one-based, inclusive interval.
    pub fn query(&self, start: u64, end: u64) -> Result<Coverage, TGVError> {
        let table = self
            .data
            .clone()
            .lazy()
            .filter(col("pos").gt_eq(lit(start)).and(col("pos").lt_eq(lit(end))))
            .collect()?;
        Ok(Coverage {
            A: table.column("A")?.u64()?.sum().unwrap_or(0),
            T: table.column("T")?.u64()?.sum().unwrap_or(0),
            C: table.column("C")?.u64()?.sum().unwrap_or(0),
            G: table.column("G")?.u64()?.sum().unwrap_or(0),
            N: table.column("N")?.u64()?.sum().unwrap_or(0),
            total: table.column("total")?.u64()?.sum().unwrap_or(0),
            softclip: table.column("softclip")?.u64()?.sum().unwrap_or(0),
        })
    }

    /// Select sparse rows within a one-based, inclusive interval.
    pub fn query_rows(&self, start: u64, end: u64) -> Result<DataFrame, TGVError> {
        Ok(self
            .data
            .clone()
            .lazy()
            .filter(col("pos").gt_eq(lit(start)).and(col("pos").lt_eq(lit(end))))
            .collect()?)
    }

    /// Sparse per-position counts, positions, and reference bases.
    ///
    /// `pos` is one-based. Rows are sorted by position and contain no null values.
    /// Positions without read or soft-clip coverage have no row.
    pub fn schema() -> SchemaRef {
        let mut schema = Schema::with_capacity(9);
        schema.insert("pos".into(), DataType::UInt64);
        schema.insert("A".into(), DataType::UInt64);
        schema.insert("T".into(), DataType::UInt64);
        schema.insert("C".into(), DataType::UInt64);
        schema.insert("G".into(), DataType::UInt64);
        schema.insert("N".into(), DataType::UInt64);
        schema.insert("total".into(), DataType::UInt64);
        schema.insert("softclip".into(), DataType::UInt64);
        schema.insert("reference_base".into(), DataType::UInt8);
        Arc::new(schema)
    }
}

/// Temporary summed counts from an interval coverage query.
#[derive(Clone, Debug, Default)]
#[allow(non_snake_case)]
pub struct Coverage {
    pub A: u64,
    pub T: u64,
    pub C: u64,
    pub G: u64,
    pub N: u64,
    /// Total coverage, excluding soft clips.
    pub total: u64,
    /// Soft-clip count.
    pub softclip: u64,
}

impl Coverage {
    pub const MAX_DISPLAY_ALLELE_FREQUENCY_RECIPROCOL: u64 = 100;

    /// Calculate the displayed alternate depth relative to a reference base.
    pub fn max_alt_depth(&self, reference_base: u8) -> u64 {
        match reference_base {
            b'A' | b'a' | b'G' | b'g' => self.C.max(self.T),
            b'T' | b't' => self.A.max(self.C),
            b'C' | b'c' => self.A.max(self.T),
            _ => 0,
        }
    }

    /// Describe the summed counts across the queried interval.
    pub fn describe(&self) -> String {
        format!(
            "A:{}, T:{}, C:{}, G:{}, N:{}, total:{}",
            self.A, self.T, self.C, self.G, self.N, self.total
        )
    }
}

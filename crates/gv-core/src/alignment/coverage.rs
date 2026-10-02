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
        let mut coverage = BTreeMap::new();
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
                    let coordinate = position;
                    let reference_base = if reference_sequence.contig_index == contig_index {
                        reference_sequence.base_at(coordinate).unwrap_or(b'N')
                    } else {
                        b'N'
                    };
                    let entry = coverage
                        .entry(coordinate)
                        .or_insert_with(|| BaseCoverage::new(reference_base));
                    if *kind == Kind::SoftClip {
                        entry.update_softclip(base)
                    } else {
                        entry.update(base)
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
            a.push(coverage.A as u64);
            t.push(coverage.T as u64);
            c.push(coverage.C as u64);
            g.push(coverage.G as u64);
            n.push(coverage.N as u64);
            total.push(coverage.total as u64);
            softclip.push(coverage.softclip as u64);
            reference_base.push(coverage.reference_base);
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

    /// Basewise coverage at position.
    /// 1-based, inclusive.
    pub fn at(&self, pos: u64) -> Result<BaseCoverage, TGVError> {
        let table = &self.data;
        let positions = table.column("pos")?.u64()?;
        let mut left = 0;
        let mut right = positions.len();
        while left < right {
            let middle = left + (right - left) / 2;
            if positions
                .get(middle)
                .expect("coverage positions are non-null")
                < pos
            {
                left = middle + 1;
            } else {
                right = middle;
            }
        }
        if left == positions.len() || positions.get(left) != Some(pos) {
            return Ok(BaseCoverage::default());
        }
        Ok(BaseCoverage {
            A: table
                .column("A")?
                .u64()?
                .get(left)
                .expect("coverage counts are non-null") as usize,
            T: table
                .column("T")?
                .u64()?
                .get(left)
                .expect("coverage counts are non-null") as usize,
            C: table
                .column("C")?
                .u64()?
                .get(left)
                .expect("coverage counts are non-null") as usize,
            G: table
                .column("G")?
                .u64()?
                .get(left)
                .expect("coverage counts are non-null") as usize,
            N: table
                .column("N")?
                .u64()?
                .get(left)
                .expect("coverage counts are non-null") as usize,
            total: table
                .column("total")?
                .u64()?
                .get(left)
                .expect("coverage counts are non-null") as usize,
            softclip: table
                .column("softclip")?
                .u64()?
                .get(left)
                .expect("coverage counts are non-null") as usize,
            reference_base: table
                .column("reference_base")?
                .u8()?
                .get(left)
                .expect("reference bases are non-null"),
        })
    }

    /// Select sparse coverage rows within a one-based, inclusive interval.
    pub fn query(&self, start: u64, end: u64) -> Result<DataFrame, TGVError> {
        let table = &self.data;
        let positions = table.column("pos")?.u64()?;
        Ok(table.filter(&(positions.gt_eq(start) & positions.lt_eq(end)))?)
    }

    /// Sparse per-position coverage, with columns matching `BaseCoverage` fields.
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

#[derive(Clone, Debug)]
#[allow(non_snake_case)]
pub struct BaseCoverage {
    pub A: usize,
    pub T: usize,
    pub C: usize,
    pub G: usize,

    pub N: usize,

    /// Total coverage, excluding soft clips.
    pub total: usize,

    /// Soft-clip count.
    pub softclip: usize,

    /// The reference base.
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

    pub fn update_softclip(&mut self, _base: u8) {
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

    pub fn describe(&self) -> String {
        format!(
            "A:{}, T:{}, C:{}, G:{}, N:{}, total:{}",
            self.A, self.T, self.C, self.G, self.N, self.total
        )
    }
}

impl Default for BaseCoverage {
    fn default() -> Self {
        Self::new(b'N')
    }
}

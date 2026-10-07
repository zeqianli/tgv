//! Queryable VCF core fields for a loaded region, and their table schema.

use crate::{
    error::TGVError,
    intervals::{IntervalSchema, IntervalTable, Region},
    table_schema::{ColumnDoc, TableSchema},
};
use noodles::vcf;
use polars::prelude::*;
use std::sync::Arc;

/// Variants overlapping the loaded region of one track.
#[derive(Debug)]
pub struct VariantTable {
    /// Rows are sorted by contig, start, end, and row ID.
    pub data: DataFrame,
    pub contig_index: usize,
    data_complete_left_bound: u64,
    data_complete_right_bound: u64,
}

/// The ordered columns for VCF core fields.
pub struct VariantSchema;

impl VariantSchema {
    pub const ROW_ID: &'static str = IntervalSchema::ROW_ID;
    pub const CONTIG_INDEX: &'static str = IntervalSchema::CONTIG_INDEX;
    pub const START: &'static str = IntervalSchema::START;
    pub const END: &'static str = IntervalSchema::END;
    pub const IDS: &'static str = "ids";
    pub const REFERENCE: &'static str = "reference";
    pub const ALTERNATE: &'static str = "alternate";
    pub const QUALITY_SCORE: &'static str = "quality_score";
    pub const FILTERS: &'static str = "filters";
}

impl TableSchema for VariantSchema {
    fn schema() -> SchemaRef {
        let mut schema = Schema::with_capacity(9);
        schema.insert(Self::ROW_ID.into(), DataType::UInt64);
        schema.insert(Self::CONTIG_INDEX.into(), DataType::UInt64);
        schema.insert(Self::START.into(), DataType::UInt64);
        schema.insert(Self::END.into(), DataType::UInt64);
        schema.insert(Self::IDS.into(), DataType::List(Box::new(DataType::String)));
        schema.insert(Self::REFERENCE.into(), DataType::String);
        schema.insert(
            Self::ALTERNATE.into(),
            DataType::List(Box::new(DataType::String)),
        );
        schema.insert(Self::QUALITY_SCORE.into(), DataType::Float32);
        schema.insert(
            Self::FILTERS.into(),
            DataType::List(Box::new(DataType::String)),
        );
        Arc::new(schema)
    }

    fn column_docs() -> &'static [ColumnDoc] {
        &[
            ColumnDoc {
                name: Self::ROW_ID,
                description: "The zero-based record index within the loaded data, in file order: within the whole file for plain VCF files, and within the loaded region for indexed VCF and BCF files.",
            },
            ColumnDoc {
                name: Self::START,
                description: "The 1-based VCF POS.",
            },
            ColumnDoc {
                name: Self::END,
                description: "The last reference position covered by the reference allele.",
            },
            ColumnDoc {
                name: Self::IDS,
                description: "The VCF ID values.",
            },
            ColumnDoc {
                name: Self::REFERENCE,
                description: "The reference allele.",
            },
            ColumnDoc {
                name: Self::ALTERNATE,
                description: "The alternate alleles.",
            },
            ColumnDoc {
                name: Self::QUALITY_SCORE,
                description: "The VCF QUAL, or null.",
            },
            ColumnDoc {
                name: Self::FILTERS,
                description: "The VCF FILTER values, such as `PASS`.",
            },
        ]
    }
}

impl Default for VariantTable {
    fn default() -> Self {
        Self {
            data: VariantSchema::empty(),
            contig_index: 0,
            data_complete_left_bound: u64::MAX,
            data_complete_right_bound: 0,
        }
    }
}

impl IntervalTable for VariantTable {
    fn query(&self, contig_index: usize, start: u64, end: u64) -> Result<DataFrame, TGVError> {
        if start == 0 {
            return Err(TGVError::ValueError(
                "Interval queries require a positive start.".into(),
            ));
        }
        if start > end {
            return Ok(DataFrame::full_null(self.data.schema(), 0));
        }
        Ok(self
            .data
            .clone()
            .lazy()
            .filter(
                col(VariantSchema::CONTIG_INDEX)
                    .eq(lit(contig_index as u64))
                    .and(col(VariantSchema::START).lt_eq(lit(end)))
                    .and(col(VariantSchema::END).gt_eq(lit(start))),
            )
            .collect()?)
    }
}

impl VariantTable {
    /// Whether the table holds every record overlapping the region.
    pub fn has_complete_data(&self, region: &Region) -> bool {
        region.contig_index() == self.contig_index
            && region.start() >= self.data_complete_left_bound
            && region.end() <= self.data_complete_right_bound
    }

    /// Builds records into a frame in record order, numbering rows from 0.
    pub(super) fn records_frame(
        records: &[vcf::variant::RecordBuf],
        contig_index: usize,
    ) -> Result<DataFrame, TGVError> {
        let mut row_ids = Vec::with_capacity(records.len());
        let mut contigs = Vec::with_capacity(records.len());
        let mut starts = Vec::with_capacity(records.len());
        let mut ends = Vec::with_capacity(records.len());
        let mut reference = Vec::with_capacity(records.len());
        let mut quality = Vec::with_capacity(records.len());
        let mut ids =
            ListStringChunkedBuilder::new(VariantSchema::IDS.into(), records.len(), records.len());
        let mut alternate = ListStringChunkedBuilder::new(
            VariantSchema::ALTERNATE.into(),
            records.len(),
            records.len(),
        );
        let mut filters = ListStringChunkedBuilder::new(
            VariantSchema::FILTERS.into(),
            records.len(),
            records.len(),
        );
        for (id, record) in records.iter().enumerate() {
            let start = record
                .variant_start()
                .ok_or_else(|| {
                    TGVError::ValueError(format!("VCF row {id} has no positive start."))
                })?
                .get() as u64;
            let bases = record.reference_bases();
            if bases.is_empty() {
                return Err(TGVError::ValueError(format!(
                    "VCF row {id} has no reference bases."
                )));
            }
            row_ids.push(id as u64);
            contigs.push(contig_index as u64);
            starts.push(start);
            ends.push(start + bases.len() as u64 - 1);
            reference.push(bases);
            quality.push(record.quality_score());
            let record_ids = record.ids().as_ref();
            if record_ids.is_empty() {
                ids.append_null();
            } else {
                ids.append_values_iter(record_ids.iter().map(String::as_str));
            }
            let alleles = record.alternate_bases().as_ref();
            if alleles.is_empty() {
                alternate.append_null();
            } else {
                alternate.append_values_iter(alleles.iter().map(String::as_str));
            }
            let record_filters = record.filters().as_ref();
            if record_filters.is_empty() {
                filters.append_null();
            } else {
                filters.append_values_iter(record_filters.iter().map(String::as_str));
            }
        }
        Ok(DataFrame::new(
            records.len(),
            vec![
                Column::new(VariantSchema::ROW_ID.into(), row_ids),
                Column::new(VariantSchema::CONTIG_INDEX.into(), contigs),
                Column::new(VariantSchema::START.into(), starts),
                Column::new(VariantSchema::END.into(), ends),
                ids.finish().into_series().into(),
                Column::new(VariantSchema::REFERENCE.into(), reference),
                alternate.finish().into_series().into(),
                Column::new(VariantSchema::QUALITY_SCORE.into(), quality),
                filters.finish().into_series().into(),
            ],
        )?)
    }

    /// Builds a table from records on one contig, numbering rows from 0 in record order.
    pub(super) fn from_records(
        records: &[vcf::variant::RecordBuf],
        contig_index: usize,
        loaded_bounds: (u64, u64),
    ) -> Result<Self, TGVError> {
        let data = Self::records_frame(records, contig_index)?
            .lazy()
            .sort(
                [
                    VariantSchema::CONTIG_INDEX,
                    VariantSchema::START,
                    VariantSchema::END,
                    VariantSchema::ROW_ID,
                ],
                SortMultipleOptions::default(),
            )
            .collect()?;
        Ok(Self {
            data,
            contig_index,
            data_complete_left_bound: loaded_bounds.0,
            data_complete_right_bound: loaded_bounds.1,
        })
    }

    /// Wraps a frame holding every record of one contig, read before contig indexes were known.
    pub(super) fn whole_contig(data: &DataFrame, contig_index: usize) -> Result<Self, TGVError> {
        Ok(Self {
            data: data
                .clone()
                .lazy()
                .with_columns([lit(contig_index as u64).alias(VariantSchema::CONTIG_INDEX)])
                .collect()?,
            contig_index,
            data_complete_left_bound: 1,
            data_complete_right_bound: u64::MAX,
        })
    }
}

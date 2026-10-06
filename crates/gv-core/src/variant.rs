//! Queryable VCF core fields with the original records and header preserved.

use crate::{
    contig_header::ContigHeader,
    error::TGVError,
    intervals::{IntervalSchema, IntervalTable},
    table_schema::TableSchema,
};
use noodles::vcf::{
    self,
    variant::record::{AlternateBases as _, Filters as _, Ids as _},
};
use polars::prelude::*;
use std::sync::Arc;

#[derive(Debug)]
pub struct VariantTable {
    pub data: DataFrame,
    pub records: Vec<vcf::Record>,
    pub header: vcf::Header,
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
}

impl Default for VariantTable {
    fn default() -> Self {
        Self {
            data: VariantSchema::empty(),
            records: Vec::new(),
            header: vcf::Header::default(),
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
    pub fn add_records(
        mut self,
        records: &[vcf::Record],
        contig_header: &ContigHeader,
    ) -> Result<Self, TGVError> {
        if records.is_empty() {
            return Ok(self);
        }
        let offset = self.records.len() as u64;
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
        for (index, record) in records.iter().enumerate() {
            let id = offset + index as u64;
            let contig = contig_header.try_get_index_by_str(record.reference_sequence_name())?;
            let start = record
                .variant_start()
                .transpose()?
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
            row_ids.push(id);
            contigs.push(contig as u64);
            starts.push(start);
            ends.push(start + bases.len() as u64 - 1);
            reference.push(bases);
            quality.push(record.quality_score().transpose()?);
            let record_ids = record.ids();
            if record_ids.is_empty() {
                ids.append_null();
            } else {
                ids.append_values_iter(record_ids.iter());
            }
            let record_alternates = record.alternate_bases();
            let alleles = record_alternates.iter().collect::<Result<Vec<_>, _>>()?;
            if alleles.is_empty() {
                alternate.append_null();
            } else {
                alternate.append_values_iter(alleles.into_iter());
            }
            let record_filters = record.filters();
            if record_filters.is_empty() {
                filters.append_null();
            } else {
                let values = record_filters
                    .iter(&self.header)
                    .collect::<Result<Vec<_>, _>>()?;
                filters.append_values_iter(values.into_iter());
            }
        }
        let batch = DataFrame::new(
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
        )?;
        self.data = concat([self.data.lazy(), batch.lazy()], UnionArgs::default())?
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
        self.records.extend_from_slice(records);
        Ok(self)
    }
}

pub struct VariantRepository {
    pub vcf_path: String,
}

impl VariantRepository {
    pub fn read_variants(&self, contig_header: &ContigHeader) -> Result<VariantTable, TGVError> {
        let mut reader = vcf::io::reader::Builder::default().build_from_path(&self.vcf_path)?;
        let header = reader.read_header()?;
        let mut table = VariantTable {
            header,
            ..VariantTable::default()
        };
        let mut batch = Vec::with_capacity(1024);
        for record in reader.records() {
            batch.push(record?);
            if batch.len() == 1024 {
                table = table.add_records(&batch, contig_header)?;
                batch.clear();
            }
        }
        table.add_records(&batch, contig_header)
    }
}

//! Columnar BED intervals with the original noodles records in source order.

use crate::{contig_header::ContigHeader, error::TGVError, intervals::IntervalTable};
use noodles::bed;
use polars::prelude::*;
use std::sync::Arc;

#[derive(Debug)]
pub struct BedTable {
    pub data: DataFrame,
    pub records: Vec<bed::Record<3>>,
}

impl Default for BedTable {
    fn default() -> Self {
        let mut schema = Schema::with_capacity(4);
        schema.insert("row_id".into(), DataType::UInt64);
        schema.insert("contig_index".into(), DataType::UInt64);
        schema.insert("start".into(), DataType::UInt64);
        schema.insert("end".into(), DataType::UInt64);
        Self {
            data: DataFrame::full_null(&Arc::new(schema), 0),
            records: Vec::new(),
        }
    }
}

impl IntervalTable for BedTable {
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
                col("contig_index")
                    .eq(lit(contig_index as u64))
                    .and(col("start").lt_eq(lit(end)))
                    .and(col("end").gt_eq(lit(start))),
            )
            .collect()?)
    }
}

impl BedTable {
    pub fn add_records(
        mut self,
        records: &[bed::Record<3>],
        contig_header: &ContigHeader,
    ) -> Result<Self, TGVError> {
        if records.is_empty() {
            return Ok(self);
        }
        let offset = self.records.len() as u64;
        let mut ids = Vec::with_capacity(records.len());
        let mut contigs = Vec::with_capacity(records.len());
        let mut starts = Vec::with_capacity(records.len());
        let mut ends = Vec::with_capacity(records.len());
        for (index, record) in records.iter().enumerate() {
            let id = offset + index as u64;
            let contig = contig_header
                .try_get_index_by_str(&record.reference_sequence_name().to_string())?;
            let start = record.feature_start()?.get() as u64;
            let end = record
                .feature_end()
                .transpose()?
                .map_or(start, |p| p.get() as u64);
            if start == 0 || end < start {
                return Err(TGVError::ValueError(format!(
                    "Invalid BED interval [{start}, {end}] at row {id}."
                )));
            }
            ids.push(id);
            contigs.push(contig as u64);
            starts.push(start);
            ends.push(end);
        }
        let batch = DataFrame::new(
            records.len(),
            vec![
                Column::new("row_id".into(), ids),
                Column::new("contig_index".into(), contigs),
                Column::new("start".into(), starts),
                Column::new("end".into(), ends),
            ],
        )?;
        self.data = concat([self.data.lazy(), batch.lazy()], UnionArgs::default())?
            .sort(
                ["contig_index", "start", "end", "row_id"],
                SortMultipleOptions::default(),
            )
            .collect()?;
        self.records.extend_from_slice(records);
        Ok(self)
    }
}

#[derive(Debug, Clone)]
pub struct BedRepository {
    pub bed_path: String,
}

impl BedRepository {
    pub fn read_bed(&self, contig_header: &ContigHeader) -> Result<BedTable, TGVError> {
        let mut reader = bed::io::reader::Builder::<3>.build_from_path(&self.bed_path)?;
        let mut record = bed::Record::default();
        let mut batch = Vec::with_capacity(1024);
        let mut table = BedTable::default();
        while reader.read_record(&mut record)? != 0 {
            batch.push(record.clone());
            if batch.len() == 1024 {
                table = table.add_records(&batch, contig_header)?;
                batch.clear();
            }
        }
        table.add_records(&batch, contig_header)
    }
}

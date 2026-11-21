//! Queryable VCF core fields for a loaded region, and their table schema.

use crate::{
    error::Error,
    intervals::{IntervalSchema, IntervalTable, Region},
    table_schema::{ColumnDoc, TableSchema},
};
use noodles::vcf;
use polars::prelude::*;
use std::sync::Arc;

/// Variants overlapping the loaded region of one track.
#[derive(Debug, Clone)]
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
    fn query(&self, contig_index: usize, start: u64, end: u64) -> Result<DataFrame, Error> {
        if start == 0 {
            return Err(Error::ValueError(
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
    ) -> Result<DataFrame, Error> {
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
                .ok_or_else(|| Error::ValueError(format!("VCF row {id} has no positive start.")))?
                .get() as u64;
            let bases = record.reference_bases();
            if bases.is_empty() {
                return Err(Error::ValueError(format!(
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
    ) -> Result<Self, Error> {
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
    pub(super) fn whole_contig(data: &DataFrame, contig_index: usize) -> Result<Self, Error> {
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

#[derive(Debug, Clone, PartialEq)]
pub struct Variant {
    pub start: u64,
    pub end: u64,
    pub reference: String,
    pub alternates: Vec<String>,
    pub quality: Option<f32>,
}

impl VariantTable {
    pub fn variants(
        &self,
        contig_index: usize,
        start: u64,
        end: u64,
    ) -> Result<Vec<Variant>, Error> {
        let rows = crate::intervals::IntervalTable::query(self, contig_index, start, end)?;
        let starts = rows.column(VariantSchema::START)?.u64()?;
        let ends = rows.column(VariantSchema::END)?.u64()?;
        let references = rows.column(VariantSchema::REFERENCE)?.str()?;
        let alternates = rows.column(VariantSchema::ALTERNATE)?.list()?;
        let qualities = rows.column(VariantSchema::QUALITY_SCORE)?.f32()?;
        (0..rows.height())
            .map(|row| {
                let alternates = match alternates.get_as_series(row) {
                    Some(alleles) => alleles
                        .str()?
                        .iter()
                        .map(|allele| {
                            allele.map(str::to_string).ok_or(Error::NullValue {
                                column: VariantSchema::ALTERNATE,
                            })
                        })
                        .collect::<Result<Vec<_>, Error>>()?,
                    None => Vec::new(),
                };
                Ok(Variant {
                    start: starts.get(row).ok_or(Error::NullValue {
                        column: VariantSchema::START,
                    })?,
                    end: ends.get(row).ok_or(Error::NullValue {
                        column: VariantSchema::END,
                    })?,
                    reference: references
                        .get(row)
                        .ok_or(Error::NullValue {
                            column: VariantSchema::REFERENCE,
                        })?
                        .to_string(),
                    alternates,
                    quality: qualities.get(row),
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn alleles(values: &[Option<&str>]) -> Series {
        Series::new("".into(), values)
    }

    fn table(references: Vec<Option<&str>>, alternates: Vec<Series>) -> VariantTable {
        let height = references.len();
        let data = DataFrame::new(
            height,
            vec![
                Column::new(
                    VariantSchema::CONTIG_INDEX.into(),
                    (0..height as u64).map(|row| row / 3).collect::<Vec<_>>(),
                ),
                Column::new(
                    VariantSchema::START.into(),
                    [100u64, 101, 200, 100][..height].to_vec(),
                ),
                Column::new(
                    VariantSchema::END.into(),
                    [100u64, 103, 200, 100][..height].to_vec(),
                ),
                Column::new(VariantSchema::REFERENCE.into(), references),
                Column::new(VariantSchema::ALTERNATE.into(), alternates),
                Column::new(
                    VariantSchema::QUALITY_SCORE.into(),
                    [Some(30.0f32), None, Some(10.0), Some(5.0)][..height].to_vec(),
                ),
            ],
        )
        .unwrap();
        VariantTable {
            data,
            ..VariantTable::default()
        }
    }

    fn sample() -> VariantTable {
        table(
            vec![Some("A"), Some("AC"), Some("G"), Some("T")],
            vec![
                alleles(&[Some("T")]),
                alleles(&[Some("G"), Some("C")]),
                alleles(&[]),
                alleles(&[Some("A")]),
            ],
        )
    }

    fn variant(
        start: u64,
        end: u64,
        reference: &str,
        alternates: &[&str],
        quality: Option<f32>,
    ) -> Variant {
        Variant {
            start,
            end,
            reference: reference.to_string(),
            alternates: alternates.iter().map(|allele| allele.to_string()).collect(),
            quality,
        }
    }

    #[rstest]
    #[case::whole_contig(0, 1, 1000, vec![
        variant(100, 100, "A", &["T"], Some(30.0)),
        variant(101, 103, "AC", &["G", "C"], None),
        variant(200, 200, "G", &[], Some(10.0)),
    ])]
    #[case::overlap_by_end(0, 102, 150, vec![variant(101, 103, "AC", &["G", "C"], None)])]
    #[case::single_base(0, 200, 200, vec![variant(200, 200, "G", &[], Some(10.0))])]
    #[case::between_variants(0, 104, 199, vec![])]
    #[case::other_contig(1, 1, 1000, vec![variant(100, 100, "T", &["A"], Some(5.0))])]
    #[case::reversed_bounds(0, 200, 100, vec![])]
    fn variants(
        #[case] contig_index: usize,
        #[case] start: u64,
        #[case] end: u64,
        #[case] expected: Vec<Variant>,
    ) {
        assert_eq!(
            sample().variants(contig_index, start, end).unwrap(),
            expected
        );
    }

    #[test]
    fn variants_reject_a_zero_start() {
        assert!(sample().variants(0, 0, 100).is_err());
    }

    #[test]
    fn variants_report_a_missing_reference() {
        let table = table(vec![None], vec![alleles(&[Some("T")])]);
        assert!(matches!(
            table.variants(0, 1, 1000),
            Err(Error::NullValue {
                column: VariantSchema::REFERENCE
            })
        ));
    }

    #[test]
    fn variants_report_a_missing_alternate_allele() {
        let table = table(vec![Some("A")], vec![alleles(&[Some("T"), None])]);
        assert!(matches!(
            table.variants(0, 1, 1000),
            Err(Error::NullValue {
                column: VariantSchema::ALTERNATE
            })
        ));
    }
}

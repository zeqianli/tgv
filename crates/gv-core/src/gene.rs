//! Columnar gene storage, loaded-region state, and genomic navigation.

use crate::{
    error::TGVError,
    intervals::{GenomeInterval, IntervalSchema, IntervalTable, Region},
    table_schema::{ColumnDoc, TableSchema},
};
use polars::prelude::*;
use std::sync::Arc;

#[derive(Debug)]
pub struct GeneTable {
    /// Rows are sorted by contig, transcription bounds, and stable row ID.
    pub data: DataFrame,
    pub contig_index: usize,
    data_complete_left_bound: u64,
    data_complete_right_bound: u64,
}

impl Default for GeneTable {
    fn default() -> Self {
        Self {
            data: GeneSchema::empty(),
            contig_index: 0,
            data_complete_left_bound: u64::MAX,
            data_complete_right_bound: 0,
        }
    }
}

impl IntervalTable for GeneTable {
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
                col(GeneSchema::CONTIG_INDEX)
                    .eq(lit(contig_index as u64))
                    .and(col(GeneSchema::START).lt_eq(lit(end)))
                    .and(col(GeneSchema::END).gt_eq(lit(start))),
            )
            .collect()?)
    }
}

impl GeneTable {
    pub(crate) fn from_data(
        data: DataFrame,
        contig_index: usize,
        loaded_bounds: (u64, u64),
    ) -> Result<Self, TGVError> {
        Ok(Self {
            data: data
                .lazy()
                .sort(
                    [
                        GeneSchema::CONTIG_INDEX,
                        GeneSchema::START,
                        GeneSchema::END,
                        GeneSchema::ROW_ID,
                    ],
                    SortMultipleOptions::default(),
                )
                .collect()?,
            contig_index,
            data_complete_left_bound: loaded_bounds.0,
            data_complete_right_bound: loaded_bounds.1,
        })
    }

    pub fn has_complete_data(&self, region: &Region) -> bool {
        region.contig_index() == self.contig_index
            && region.start() >= self.data_complete_left_bound
            && region.end() <= self.data_complete_right_bound
    }

    pub fn gene_by_name(&self, name: &str) -> Result<DataFrame, TGVError> {
        let rows = self
            .data
            .clone()
            .lazy()
            .filter(col(GeneSchema::NAME).eq(lit(name)))
            .limit(1)
            .collect()?;
        Ok(rows)
    }

    pub fn get_gene_at(&self, position: u64) -> Result<DataFrame, TGVError> {
        if position == 0 {
            return Err(TGVError::ValueError(
                "Interval queries require a positive start.".into(),
            ));
        }
        Ok(self
            .data
            .clone()
            .lazy()
            .filter(
                col(GeneSchema::START)
                    .lt_eq(lit(position))
                    .and(col(GeneSchema::END).gt_eq(lit(position))),
            )
            .limit(1)
            .collect()?)
    }

    pub fn get_k_genes_before(&self, position: u64, k: usize) -> Result<DataFrame, TGVError> {
        navigate_genes(self, position, k, false, false)
    }
    pub fn get_k_genes_after(&self, position: u64, k: usize) -> Result<DataFrame, TGVError> {
        navigate_genes(self, position, k, true, false)
    }
    pub fn get_saturating_k_genes_before(
        &self,
        position: u64,
        k: usize,
    ) -> Result<DataFrame, TGVError> {
        navigate_genes(self, position, k, false, true)
    }
    pub fn get_saturating_k_genes_after(
        &self,
        position: u64,
        k: usize,
    ) -> Result<DataFrame, TGVError> {
        navigate_genes(self, position, k, true, true)
    }
    pub fn get_k_exons_before(&self, position: u64, k: usize) -> Result<DataFrame, TGVError> {
        navigate_exons(self, position, k, false, false)
    }
    pub fn get_k_exons_after(&self, position: u64, k: usize) -> Result<DataFrame, TGVError> {
        navigate_exons(self, position, k, true, false)
    }
    pub fn get_saturating_k_exons_before(
        &self,
        position: u64,
        k: usize,
    ) -> Result<DataFrame, TGVError> {
        navigate_exons(self, position, k, false, true)
    }
    pub fn get_saturating_k_exons_after(
        &self,
        position: u64,
        k: usize,
    ) -> Result<DataFrame, TGVError> {
        navigate_exons(self, position, k, true, true)
    }
}

fn navigate_genes(
    table: &GeneTable,
    position: u64,
    k: usize,
    after: bool,
    saturating: bool,
) -> Result<DataFrame, TGVError> {
    if k == 0 {
        return if saturating {
            Ok(DataFrame::full_null(table.data.schema(), 0))
        } else {
            table.get_gene_at(position)
        };
    }
    let bound = if after {
        GeneSchema::START
    } else {
        GeneSchema::END
    };
    let predicate = if after {
        col(bound).gt(lit(position))
    } else {
        col(bound).lt(lit(position))
    };
    let rows = table
        .data
        .clone()
        .lazy()
        .filter(predicate)
        .sort(
            [
                bound,
                if after {
                    GeneSchema::END
                } else {
                    GeneSchema::START
                },
                GeneSchema::ROW_ID,
            ],
            SortMultipleOptions::default()
                .with_order_descending(!after)
                .with_maintain_order(true),
        )
        .slice((k - 1) as i64, 1)
        .collect()?;
    if rows.height() > 0 {
        return Ok(rows);
    }
    if !saturating {
        return Ok(rows);
    }
    let rows = table
        .data
        .clone()
        .lazy()
        .sort(
            [GeneSchema::START, GeneSchema::END, GeneSchema::ROW_ID],
            SortMultipleOptions::default().with_order_descending(after),
        )
        .limit(1)
        .collect()?;
    Ok(rows)
}

fn navigate_exons(
    table: &GeneTable,
    position: u64,
    k: usize,
    after: bool,
    saturating: bool,
) -> Result<DataFrame, TGVError> {
    let after = k == 0 || after;
    let exons = table
        .data
        .clone()
        .lazy()
        .filter(
            col(GeneSchema::HAS_EXONS).and(col(GeneSchema::EXON_STARTS).list().len().gt(lit(0u32))),
        )
        .select([
            col(GeneSchema::ROW_ID),
            col(GeneSchema::CONTIG_INDEX),
            col(GeneSchema::EXON_STARTS).alias(GeneSchema::START),
            col(GeneSchema::EXON_ENDS).alias(GeneSchema::END),
        ])
        .explode(
            cols([GeneSchema::START, GeneSchema::END]),
            ExplodeOptions {
                empty_as_null: false,
                keep_nulls: false,
            },
        );
    if k == 0 && saturating {
        return Ok(exons.limit(0).collect()?);
    }
    let predicate = if k == 0 {
        col(GeneSchema::START)
            .lt_eq(lit(position))
            .and(col(GeneSchema::END).gt_eq(lit(position)))
    } else if after {
        col(GeneSchema::START).gt(lit(position))
    } else {
        col(GeneSchema::END).lt_eq(lit(position))
    };
    let mut rows = exons
        .clone()
        .filter(predicate)
        .sort(
            [
                if after {
                    GeneSchema::START
                } else {
                    GeneSchema::END
                },
                if after {
                    GeneSchema::END
                } else {
                    GeneSchema::START
                },
                GeneSchema::ROW_ID,
            ],
            SortMultipleOptions::default()
                .with_order_descending(!after)
                .with_maintain_order(true),
        )
        .slice(k.saturating_sub(1) as i64, 1)
        .collect()?;
    if rows.height() == 0 && saturating {
        rows = exons
            .sort(
                [GeneSchema::START, GeneSchema::END, GeneSchema::ROW_ID],
                SortMultipleOptions::default().with_order_descending(after),
            )
            .limit(1)
            .collect()?;
    }
    Ok(rows)
}

/// Derive viewport drawing segments without materializing gene or exon objects.
/// Exon numbers refer to complete transcripts, before CDS splitting or viewport filtering.
pub fn query_segments(genes: DataFrame, start: u64, end: u64) -> Result<DataFrame, TGVError> {
    use polars::lazy::dsl::{max_horizontal, min_horizontal};

    let exons = genes
        .lazy()
        .filter(
            col(GeneSchema::HAS_EXONS).and(col(GeneSchema::EXON_STARTS).list().len().gt(lit(0u32))),
        )
        .select([
            col(GeneSchema::ROW_ID).alias(GeneSegmentSchema::GENE_ROW_ID),
            col(GeneSchema::STRAND),
            col(GeneSchema::CDS_START).cast(DataType::Int128),
            col(GeneSchema::CDS_END).cast(DataType::Int128),
            col(GeneSchema::EXON_STARTS).alias(GeneSegmentSchema::START),
            col(GeneSchema::EXON_ENDS).alias(GeneSegmentSchema::END),
        ])
        .explode(
            cols([GeneSegmentSchema::START, GeneSegmentSchema::END]),
            ExplodeOptions {
                empty_as_null: false,
                keep_nulls: false,
            },
        )
        .with_columns([
            col(GeneSegmentSchema::START)
                .cum_count(false)
                .over([col(GeneSegmentSchema::GENE_ROW_ID)])?
                .cast(DataType::UInt64)
                .alias(GeneSegmentSchema::EXON_ORDINAL),
            len()
                .over([col(GeneSegmentSchema::GENE_ROW_ID)])?
                .cast(DataType::UInt64)
                .alias(GeneSegmentSchema::EXON_COUNT),
            col(GeneSegmentSchema::END)
                .shift(lit(1i64))
                .over([col(GeneSegmentSchema::GENE_ROW_ID)])?
                .cast(DataType::Int128)
                .alias(GeneSegmentSchema::PREVIOUS_END),
            col(GeneSegmentSchema::START).cast(DataType::Int128),
            col(GeneSegmentSchema::END).cast(DataType::Int128),
        ])
        .with_columns([when(col(GeneSchema::STRAND).eq(lit("+")))
            .then(col(GeneSegmentSchema::EXON_ORDINAL))
            .otherwise(
                col(GeneSegmentSchema::EXON_COUNT) - col(GeneSegmentSchema::EXON_ORDINAL)
                    + lit(1u64),
            )
            .alias(GeneSegmentSchema::FEATURE_INDEX)]);
    let coding = col(GeneSchema::CDS_START).lt_eq(col(GeneSchema::CDS_END));
    let columns = [
        col(GeneSegmentSchema::GENE_ROW_ID),
        col(GeneSegmentSchema::START),
        col(GeneSegmentSchema::END),
        col(GeneSegmentSchema::KIND),
        col(GeneSegmentSchema::FEATURE_INDEX),
    ];
    let coding_exons = exons
        .clone()
        .filter(coding.clone())
        .with_columns([
            max_horizontal([col(GeneSegmentSchema::START), col(GeneSchema::CDS_START)])?
                .alias(GeneSegmentSchema::START),
            min_horizontal([col(GeneSegmentSchema::END), col(GeneSchema::CDS_END)])?
                .alias(GeneSegmentSchema::END),
            lit("coding_exon").alias(GeneSegmentSchema::KIND),
        ])
        .select(columns.clone());
    let before_cds = exons
        .clone()
        .filter(coding.clone())
        .with_columns([
            min_horizontal([
                col(GeneSegmentSchema::END),
                col(GeneSchema::CDS_START) - lit(1i128),
            ])?
            .alias(GeneSegmentSchema::END),
            lit("noncoding_exon").alias(GeneSegmentSchema::KIND),
        ])
        .select(columns.clone());
    let after_cds = exons
        .clone()
        .filter(coding.clone())
        .with_columns([
            max_horizontal([
                col(GeneSegmentSchema::START),
                col(GeneSchema::CDS_END) + lit(1i128),
            ])?
            .alias(GeneSegmentSchema::START),
            lit("noncoding_exon").alias(GeneSegmentSchema::KIND),
        ])
        .select(columns.clone());
    let noncoding = exons
        .clone()
        .filter(coding.not())
        .with_columns([lit("noncoding_exon").alias(GeneSegmentSchema::KIND)])
        .select(columns.clone());
    let introns = exons
        .with_columns([
            (col(GeneSegmentSchema::PREVIOUS_END) + lit(1i128)).alias(GeneSegmentSchema::START),
            (col(GeneSegmentSchema::START) - lit(1i128)).alias(GeneSegmentSchema::END),
            lit("intron").alias(GeneSegmentSchema::KIND),
            when(col(GeneSchema::STRAND).eq(lit("+")))
                .then(col(GeneSegmentSchema::EXON_ORDINAL) - lit(1u64))
                .otherwise(col(GeneSegmentSchema::FEATURE_INDEX))
                .alias(GeneSegmentSchema::FEATURE_INDEX),
        ])
        .select(columns);
    Ok(concat(
        [coding_exons, before_cds, after_cds, noncoding, introns],
        UnionArgs::default(),
    )?
    .filter(
        col(GeneSegmentSchema::START)
            .lt_eq(col(GeneSegmentSchema::END))
            .and(col(GeneSegmentSchema::START).lt_eq(lit(end as i128)))
            .and(col(GeneSegmentSchema::END).gt_eq(lit(start as i128))),
    )
    .with_columns([
        col(GeneSegmentSchema::START).cast(DataType::UInt64),
        col(GeneSegmentSchema::END).cast(DataType::UInt64),
    ])
    .sort(
        [
            GeneSegmentSchema::GENE_ROW_ID,
            GeneSegmentSchema::START,
            GeneSegmentSchema::END,
        ],
        SortMultipleOptions::default(),
    )
    .collect()?)
}

/// Transcript bounds, CDS bounds, paired exon lists, and gene metadata.
/// Coordinates are one-based and inclusive; empty exon lists retain their element type.
pub struct GeneSchema;

impl GeneSchema {
    pub const ROW_ID: &'static str = IntervalSchema::ROW_ID;
    pub const CONTIG_INDEX: &'static str = IntervalSchema::CONTIG_INDEX;
    pub const START: &'static str = IntervalSchema::START;
    pub const END: &'static str = IntervalSchema::END;
    pub const ID: &'static str = "id";
    pub const NAME: &'static str = "name";
    pub const STRAND: &'static str = "strand";
    pub const CDS_START: &'static str = "cds_start";
    pub const CDS_END: &'static str = "cds_end";
    pub const EXON_STARTS: &'static str = "exon_starts";
    pub const EXON_ENDS: &'static str = "exon_ends";
    pub const HAS_EXONS: &'static str = "has_exons";
}

impl TableSchema for GeneSchema {
    fn schema() -> SchemaRef {
        let mut schema = Schema::with_capacity(12);
        schema.insert(Self::ROW_ID.into(), DataType::UInt64);
        schema.insert(Self::CONTIG_INDEX.into(), DataType::UInt64);
        schema.insert(Self::START.into(), DataType::UInt64);
        schema.insert(Self::END.into(), DataType::UInt64);
        schema.insert(Self::ID.into(), DataType::String);
        schema.insert(Self::NAME.into(), DataType::String);
        schema.insert(Self::STRAND.into(), DataType::String);
        schema.insert(Self::CDS_START.into(), DataType::UInt64);
        schema.insert(Self::CDS_END.into(), DataType::UInt64);
        schema.insert(
            Self::EXON_STARTS.into(),
            DataType::List(Box::new(DataType::UInt64)),
        );
        schema.insert(
            Self::EXON_ENDS.into(),
            DataType::List(Box::new(DataType::UInt64)),
        );
        schema.insert(Self::HAS_EXONS.into(), DataType::Boolean);
        Arc::new(schema)
    }

    fn column_docs() -> &'static [ColumnDoc] {
        &[
            ColumnDoc {
                name: Self::ROW_ID,
                description: "The transcript row ID within the loaded annotations.",
            },
            ColumnDoc {
                name: Self::START,
                description: "The transcript start.",
            },
            ColumnDoc {
                name: Self::END,
                description: "The transcript end, inclusive.",
            },
            ColumnDoc {
                name: Self::ID,
                description: "The transcript accession, such as `NM_153325.4`.",
            },
            ColumnDoc {
                name: Self::NAME,
                description: "The gene name, such as `DEFB125`.",
            },
            ColumnDoc {
                name: Self::STRAND,
                description: "`+` or `-`.",
            },
            ColumnDoc {
                name: Self::CDS_START,
                description: "The first coding position; greater than `cds_end` for noncoding transcripts.",
            },
            ColumnDoc {
                name: Self::CDS_END,
                description: "The last coding position.",
            },
            ColumnDoc {
                name: Self::EXON_STARTS,
                description: "The exon starts, in genomic order.",
            },
            ColumnDoc {
                name: Self::EXON_ENDS,
                description: "The exon ends, inclusive and paired with `exon_starts`.",
            },
            ColumnDoc {
                name: Self::HAS_EXONS,
                description: "Whether the annotation source provides exons.",
            },
        ]
    }
}

/// Derived coding exon, noncoding exon, and intron drawing segments.
/// Exon indexes refer to the complete transcript before viewport filtering.
pub struct GeneSegmentSchema;

impl GeneSegmentSchema {
    pub const GENE_ROW_ID: &'static str = "gene_row_id";
    pub const START: &'static str = "start";
    pub const END: &'static str = "end";
    pub const KIND: &'static str = "kind";
    pub const FEATURE_INDEX: &'static str = "feature_index";

    // Temporary columns used while deriving query results.
    pub const EXON_ORDINAL: &'static str = "exon_ordinal";
    pub const EXON_COUNT: &'static str = "exon_count";
    pub const PREVIOUS_END: &'static str = "previous_end";
}

impl TableSchema for GeneSegmentSchema {
    fn schema() -> SchemaRef {
        let mut schema = Schema::with_capacity(5);
        schema.insert(Self::GENE_ROW_ID.into(), DataType::UInt64);
        schema.insert(Self::START.into(), DataType::UInt64);
        schema.insert(Self::END.into(), DataType::UInt64);
        schema.insert(Self::KIND.into(), DataType::String);
        schema.insert(Self::FEATURE_INDEX.into(), DataType::UInt64);
        Arc::new(schema)
    }

    fn column_docs() -> &'static [ColumnDoc] {
        &[
            ColumnDoc {
                name: Self::GENE_ROW_ID,
                description: "The transcript row ID in the gene table.",
            },
            ColumnDoc {
                name: Self::START,
                description: "The segment start.",
            },
            ColumnDoc {
                name: Self::END,
                description: "The segment end, inclusive.",
            },
            ColumnDoc {
                name: Self::KIND,
                description: "`coding_exon`, `noncoding_exon`, or `intron`.",
            },
            ColumnDoc {
                name: Self::FEATURE_INDEX,
                description: "The exon or intron number in transcription order, starting at 1.",
            },
        ]
    }
}

#[cfg(test)]
mod tests {

    use crate::intervals::Focus;

    /// Test table: [gene1: [2,5], [8,10]], [gene_no_exon (21-30)], [gene2: [41,50]].
    fn get_test_track() -> GeneTable {
        let data = DataFrame::new(
            3,
            vec![
                Column::new(GeneSchema::ROW_ID.into(), [0u64, 1, 2]),
                Column::new(GeneSchema::CONTIG_INDEX.into(), [0u64; 3]),
                Column::new(GeneSchema::START.into(), [2u64, 21, 41]),
                Column::new(GeneSchema::END.into(), [10u64, 30, 50]),
                Column::new(GeneSchema::ID.into(), ["gene1", "gene_no_exon", "gene2"]),
                Column::new(GeneSchema::NAME.into(), ["gene1", "gene_no_exon", "gene2"]),
                Column::new(GeneSchema::STRAND.into(), ["+"; 3]),
                Column::new(GeneSchema::CDS_START.into(), [2u64, 25, 45]),
                Column::new(GeneSchema::CDS_END.into(), [10u64, 25, 50]),
                Column::new(
                    GeneSchema::EXON_STARTS.into(),
                    vec![
                        Series::new("".into(), [2u64, 8]),
                        Series::new("".into(), Vec::<u64>::new()),
                        Series::new("".into(), [41u64]),
                    ],
                ),
                Column::new(
                    GeneSchema::EXON_ENDS.into(),
                    vec![
                        Series::new("".into(), [5u64, 10]),
                        Series::new("".into(), Vec::<u64>::new()),
                        Series::new("".into(), [50u64]),
                    ],
                ),
                Column::new(GeneSchema::HAS_EXONS.into(), [true, false, true]),
            ],
        )
        .unwrap();
        GeneTable::from_data(data, 0, (1, 100)).unwrap()
    }

    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case(1, None)]
    #[case(2, Some("gene1"))]
    #[case(5, Some("gene1"))]
    #[case(10, Some("gene1"))]
    #[case(42, Some("gene2"))]
    #[case(51, None)]
    fn test_get_genes_at(#[case] position: u64, #[case] expected: Option<&str>) {
        let track = get_test_track();
        match expected {
            Some(gene_name) => assert_eq!(
                track
                    .get_gene_at(position)
                    .unwrap()
                    .column(GeneSchema::NAME)
                    .unwrap()
                    .str()
                    .unwrap()
                    .get(0)
                    .unwrap(),
                gene_name
            ),
            None => assert!(track.get_gene_at(position).unwrap().height() == 0),
        }
    }

    #[rstest]
    #[case(2, 0, Some("gene1"))]
    #[case(2, 1, None)]
    #[case(11, 1, Some("gene1"))]
    #[case(35, 1, Some("gene_no_exon"))]
    #[case(51, 0, None)]
    #[case(51, 1, Some("gene2"))]
    fn test_get_k_genes_before(
        #[case] position: u64,
        #[case] k: usize,
        #[case] expected: Option<&str>,
    ) {
        let track = get_test_track();
        match expected {
            Some(gene_name) => assert_eq!(
                track
                    .get_k_genes_before(position, k)
                    .unwrap()
                    .column(GeneSchema::NAME)
                    .unwrap()
                    .str()
                    .unwrap()
                    .get(0)
                    .unwrap(),
                gene_name
            ),
            None => assert!(track.get_k_genes_before(position, k).unwrap().height() == 0),
        }
    }

    #[rstest]
    #[case(2, 0, Some("gene1"))]
    #[case(2, 1, Some("gene_no_exon"))]
    #[case(2, 2, Some("gene2"))]
    #[case(2, 3, None)]
    #[case(11, 1, Some("gene_no_exon"))]
    #[case(51, 1, None)]
    #[case(1, 1, Some("gene1"))]
    #[case(1, 0, None)]
    fn test_get_k_genes_after(
        #[case] position: u64,
        #[case] k: usize,
        #[case] expected: Option<&str>,
    ) {
        let track = get_test_track();
        match expected {
            Some(gene_name) => {
                assert_eq!(
                    track
                        .get_k_genes_after(position, k)
                        .unwrap()
                        .column(GeneSchema::NAME)
                        .unwrap()
                        .str()
                        .unwrap()
                        .get(0)
                        .unwrap(),
                    gene_name
                )
            }
            None => assert!(track.get_k_genes_after(position, k).unwrap().height() == 0),
        }
    }

    #[rstest]
    #[case(1, None)]
    #[case(5, Some(2))]
    #[case(15, None)]
    #[case(25, None)]
    #[case(51, None)]
    fn test_get_exon_at(#[case] position: u64, #[case] expected: Option<u64>) {
        let track = get_test_track();
        match expected {
            Some(exon_idx) => assert_eq!(
                track
                    .get_k_exons_after(position, 0)
                    .unwrap()
                    .column(GeneSchema::START)
                    .unwrap()
                    .u64()
                    .unwrap()
                    .get(0)
                    .unwrap(),
                exon_idx
            ),
            None => assert!(track.get_k_exons_after(position, 0).unwrap().height() == 0),
        }
    }

    #[rstest]
    #[case(1, 0, None)]
    #[case(2, 0, Some(2))]
    #[case(2, 1, None)]
    #[case(35, 1, Some(8))]
    #[case(51, 1, Some(41))]
    #[case(51, 2, Some(8))]
    fn test_get_k_exons_before(
        #[case] position: u64,
        #[case] k: usize,
        #[case] expected: Option<u64>,
    ) {
        let track = get_test_track();
        match expected {
            Some(exon_idx) => assert_eq!(
                track
                    .get_k_exons_before(position, k)
                    .unwrap()
                    .column(GeneSchema::START)
                    .unwrap()
                    .u64()
                    .unwrap()
                    .get(0)
                    .unwrap(),
                exon_idx
            ),
            None => assert!(track.get_k_exons_before(position, k).unwrap().height() == 0),
        }
    }

    #[rstest]
    #[case(1, 0, None)]
    #[case(1, 2, Some(8))]
    #[case(2, 0, Some(2))]
    #[case(2, 1, Some(8))]
    #[case(35, 1, Some(41))]
    #[case(35, 2, None)]
    #[case(51, 0, None)]
    #[case(51, 1, None)]
    fn test_get_k_exons_after(
        #[case] position: u64,
        #[case] k: usize,
        #[case] expected: Option<u64>,
    ) {
        let track = get_test_track();
        match expected {
            Some(exon_idx) => assert_eq!(
                track
                    .get_k_exons_after(position, k)
                    .unwrap()
                    .column(GeneSchema::START)
                    .unwrap()
                    .u64()
                    .unwrap()
                    .get(0)
                    .unwrap(),
                exon_idx
            ),
            None => assert!(track.get_k_exons_after(position, k).unwrap().height() == 0),
        }
    }

    #[test]
    fn test_has_complete_data_uses_loaded_region() {
        let track = GeneTable::from_data(GeneTable::default().data, 0, (100, 200)).unwrap();

        assert!(track.has_complete_data(&Region {
            focus: Focus {
                contig_index: 0,
                position: 150,
            },
            half_width: 25,
        }));
        assert!(!track.has_complete_data(&Region {
            focus: Focus {
                contig_index: 0,
                position: 95,
            },
            half_width: 10,
        }));
        assert!(!track.has_complete_data(&Region {
            focus: Focus {
                contig_index: 1,
                position: 150,
            },
            half_width: 25,
        }));
    }
}

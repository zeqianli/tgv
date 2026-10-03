//! Columnar gene storage, loaded-region state, and genomic navigation.

use crate::{
    error::TGVError,
    intervals::{GenomeInterval, IntervalTable, Region},
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
            data: DataFrame::full_null(&gene_schema(), 0),
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
                col("contig_index")
                    .eq(lit(contig_index as u64))
                    .and(col("start").lt_eq(lit(end)))
                    .and(col("end").gt_eq(lit(start))),
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
                    ["contig_index", "start", "end", "row_id"],
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
            .filter(col("name").eq(lit(name)))
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
                col("start")
                    .lt_eq(lit(position))
                    .and(col("end").gt_eq(lit(position))),
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
    let bound = if after { "start" } else { "end" };
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
            [bound, if after { "end" } else { "start" }, "row_id"],
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
            ["start", "end", "row_id"],
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
        .filter(col("has_exons").and(col("exon_starts").list().len().gt(lit(0u32))))
        .select([
            col("row_id"),
            col("contig_index"),
            col("exon_starts").alias("start"),
            col("exon_ends").alias("end"),
        ])
        .explode(
            cols(["start", "end"]),
            ExplodeOptions {
                empty_as_null: false,
                keep_nulls: false,
            },
        );
    if k == 0 && saturating {
        return Ok(exons.limit(0).collect()?);
    }
    let predicate = if k == 0 {
        col("start")
            .lt_eq(lit(position))
            .and(col("end").gt_eq(lit(position)))
    } else if after {
        col("start").gt(lit(position))
    } else {
        col("end").lt_eq(lit(position))
    };
    let mut rows = exons
        .clone()
        .filter(predicate)
        .sort(
            [
                if after { "start" } else { "end" },
                if after { "end" } else { "start" },
                "row_id",
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
                ["start", "end", "row_id"],
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
        .filter(col("has_exons").and(col("exon_starts").list().len().gt(lit(0u32))))
        .select([
            col("row_id").alias("gene_row_id"),
            col("strand"),
            col("cds_start").cast(DataType::Int128),
            col("cds_end").cast(DataType::Int128),
            col("exon_starts").alias("start"),
            col("exon_ends").alias("end"),
        ])
        .explode(
            cols(["start", "end"]),
            ExplodeOptions {
                empty_as_null: false,
                keep_nulls: false,
            },
        )
        .with_columns([
            col("start")
                .cum_count(false)
                .over([col("gene_row_id")])?
                .cast(DataType::UInt64)
                .alias("exon_ordinal"),
            len()
                .over([col("gene_row_id")])?
                .cast(DataType::UInt64)
                .alias("exon_count"),
            col("end")
                .shift(lit(1i64))
                .over([col("gene_row_id")])?
                .cast(DataType::Int128)
                .alias("previous_end"),
            col("start").cast(DataType::Int128),
            col("end").cast(DataType::Int128),
        ])
        .with_columns([when(col("strand").eq(lit("+")))
            .then(col("exon_ordinal"))
            .otherwise(col("exon_count") - col("exon_ordinal") + lit(1u64))
            .alias("feature_index")]);
    let coding = col("cds_start").lt_eq(col("cds_end"));
    let columns = [
        col("gene_row_id"),
        col("start"),
        col("end"),
        col("kind"),
        col("feature_index"),
    ];
    let coding_exons = exons
        .clone()
        .filter(coding.clone())
        .with_columns([
            max_horizontal([col("start"), col("cds_start")])?.alias("start"),
            min_horizontal([col("end"), col("cds_end")])?.alias("end"),
            lit("coding_exon").alias("kind"),
        ])
        .select(columns.clone());
    let before_cds = exons
        .clone()
        .filter(coding.clone())
        .with_columns([
            min_horizontal([col("end"), col("cds_start") - lit(1i128)])?.alias("end"),
            lit("noncoding_exon").alias("kind"),
        ])
        .select(columns.clone());
    let after_cds = exons
        .clone()
        .filter(coding.clone())
        .with_columns([
            max_horizontal([col("start"), col("cds_end") + lit(1i128)])?.alias("start"),
            lit("noncoding_exon").alias("kind"),
        ])
        .select(columns.clone());
    let noncoding = exons
        .clone()
        .filter(coding.not())
        .with_columns([lit("noncoding_exon").alias("kind")])
        .select(columns.clone());
    let introns = exons
        .with_columns([
            (col("previous_end") + lit(1i128)).alias("start"),
            (col("start") - lit(1i128)).alias("end"),
            lit("intron").alias("kind"),
            when(col("strand").eq(lit("+")))
                .then(col("exon_ordinal") - lit(1u64))
                .otherwise(col("feature_index"))
                .alias("feature_index"),
        ])
        .select(columns);
    Ok(concat(
        [coding_exons, before_cds, after_cds, noncoding, introns],
        UnionArgs::default(),
    )?
    .filter(
        col("start")
            .lt_eq(col("end"))
            .and(col("start").lt_eq(lit(end as i128)))
            .and(col("end").gt_eq(lit(start as i128))),
    )
    .with_columns([
        col("start").cast(DataType::UInt64),
        col("end").cast(DataType::UInt64),
    ])
    .sort(
        ["gene_row_id", "start", "end"],
        SortMultipleOptions::default(),
    )
    .collect()?)
}

fn gene_schema() -> SchemaRef {
    let mut schema = Schema::with_capacity(13);
    schema.insert("row_id".into(), DataType::UInt64);
    schema.insert("contig_index".into(), DataType::UInt64);
    schema.insert("start".into(), DataType::UInt64);
    schema.insert("end".into(), DataType::UInt64);
    schema.insert("id".into(), DataType::String);
    schema.insert("name".into(), DataType::String);
    schema.insert("strand".into(), DataType::String);
    schema.insert("cds_start".into(), DataType::UInt64);
    schema.insert("cds_end".into(), DataType::UInt64);
    schema.insert(
        "exon_starts".into(),
        DataType::List(Box::new(DataType::UInt64)),
    );
    schema.insert(
        "exon_ends".into(),
        DataType::List(Box::new(DataType::UInt64)),
    );
    schema.insert("has_exons".into(), DataType::Boolean);
    Arc::new(schema)
}
#[cfg(test)]
mod tests {

    use crate::intervals::Focus;

    /// Test table: [gene1: [2,5], [8,10]], [gene_no_exon (21-30)], [gene2: [41,50]].
    fn get_test_track() -> GeneTable {
        let data = DataFrame::new(
            3,
            vec![
                Column::new("row_id".into(), [0u64, 1, 2]),
                Column::new("contig_index".into(), [0u64; 3]),
                Column::new("start".into(), [2u64, 21, 41]),
                Column::new("end".into(), [10u64, 30, 50]),
                Column::new("id".into(), ["gene1", "gene_no_exon", "gene2"]),
                Column::new("name".into(), ["gene1", "gene_no_exon", "gene2"]),
                Column::new("strand".into(), ["+"; 3]),
                Column::new("cds_start".into(), [2u64, 25, 45]),
                Column::new("cds_end".into(), [10u64, 25, 50]),
                Column::new(
                    "exon_starts".into(),
                    vec![
                        Series::new("".into(), [2u64, 8]),
                        Series::new("".into(), Vec::<u64>::new()),
                        Series::new("".into(), [41u64]),
                    ],
                ),
                Column::new(
                    "exon_ends".into(),
                    vec![
                        Series::new("".into(), [5u64, 10]),
                        Series::new("".into(), Vec::<u64>::new()),
                        Series::new("".into(), [50u64]),
                    ],
                ),
                Column::new("has_exons".into(), [true, false, true]),
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
                    .column("name")
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
                    .column("name")
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
                        .column("name")
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
                    .column("start")
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
                    .column("start")
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
                    .column("start")
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

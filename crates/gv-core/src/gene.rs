//! Columnar gene storage, loaded-region state, and genomic navigation.

use crate::{
    error::TGVError,
    feature::{Gene, SubGeneFeature, SubGeneFeatureType},
    intervals::{GenomeInterval, IntervalTable, Region},
    strand::Strand,
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
    pub fn from_genes(
        genes: Vec<Gene>,
        contig_index: usize,
        loaded_bounds: (u64, u64),
    ) -> Result<Self, TGVError> {
        for gene in &genes {
            if gene.contig_index != contig_index
                || gene.transcription_start == 0
                || gene.transcription_end < gene.transcription_start
                || gene.cds_start == 0
            {
                return Err(TGVError::ValueError(format!(
                    "Invalid coordinates or contig for gene {}.",
                    gene.id
                )));
            }
            if gene.exon_starts.len() != gene.exon_ends.len() {
                return Err(TGVError::ValueError(format!(
                    "Gene {} has mismatched exon starts and ends.",
                    gene.id
                )));
            }
            for (&start, &end) in gene.exon_starts.iter().zip(&gene.exon_ends) {
                if start < gene.transcription_start || end > gene.transcription_end || end < start {
                    return Err(TGVError::ValueError(format!(
                        "Invalid exon [{start}, {end}] for gene {}.",
                        gene.id
                    )));
                }
            }
        }
        let data = if genes.is_empty() {
            DataFrame::full_null(&gene_schema(), 0)
        } else {
            DataFrame::new(
                genes.len(),
                vec![
                    Column::new("row_id".into(), (0..genes.len() as u64).collect::<Vec<_>>()),
                    Column::new(
                        "contig_index".into(),
                        genes
                            .iter()
                            .map(|g| g.contig_index as u64)
                            .collect::<Vec<_>>(),
                    ),
                    Column::new(
                        "start".into(),
                        genes
                            .iter()
                            .map(|g| g.transcription_start)
                            .collect::<Vec<_>>(),
                    ),
                    Column::new(
                        "end".into(),
                        genes
                            .iter()
                            .map(|g| g.transcription_end)
                            .collect::<Vec<_>>(),
                    ),
                    Column::new(
                        "id".into(),
                        genes.iter().map(|g| g.id.as_str()).collect::<Vec<_>>(),
                    ),
                    Column::new(
                        "name".into(),
                        genes.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(),
                    ),
                    Column::new(
                        "strand".into(),
                        genes
                            .iter()
                            .map(|g| g.strand.to_string())
                            .collect::<Vec<_>>(),
                    ),
                    Column::new(
                        "cds_start".into(),
                        genes.iter().map(|g| g.cds_start).collect::<Vec<_>>(),
                    ),
                    Column::new(
                        "cds_end".into(),
                        genes.iter().map(|g| g.cds_end).collect::<Vec<_>>(),
                    ),
                    Column::new(
                        "exon_starts".into(),
                        genes
                            .iter()
                            .map(|g| Series::new("".into(), &g.exon_starts))
                            .collect::<Vec<_>>(),
                    ),
                    Column::new(
                        "exon_ends".into(),
                        genes
                            .iter()
                            .map(|g| Series::new("".into(), &g.exon_ends))
                            .collect::<Vec<_>>(),
                    ),
                    Column::new(
                        "has_exons".into(),
                        genes.iter().map(|g| g.has_exons).collect::<Vec<_>>(),
                    ),
                ],
            )?
            .lazy()
            .sort(
                ["contig_index", "start", "end", "row_id"],
                SortMultipleOptions::default(),
            )
            .collect()?
        };
        Ok(Self {
            data,
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

    pub fn gene_by_name(&self, name: &str) -> Result<Option<Gene>, TGVError> {
        let rows = self
            .data
            .clone()
            .lazy()
            .filter(col("name").eq(lit(name)))
            .limit(1)
            .collect()?;
        Ok(genes_from_rows(&rows)?.into_iter().next())
    }

    pub fn get_gene_at(&self, position: u64) -> Result<Option<Gene>, TGVError> {
        let rows = self.query(self.contig_index, position, position)?;
        Ok(genes_from_rows(&rows)?.into_iter().next())
    }

    pub fn get_k_genes_before(&self, position: u64, k: usize) -> Result<Option<Gene>, TGVError> {
        navigate_genes(self, position, k, false, false)
    }
    pub fn get_k_genes_after(&self, position: u64, k: usize) -> Result<Option<Gene>, TGVError> {
        navigate_genes(self, position, k, true, false)
    }
    pub fn get_saturating_k_genes_before(
        &self,
        position: u64,
        k: usize,
    ) -> Result<Option<Gene>, TGVError> {
        navigate_genes(self, position, k, false, true)
    }
    pub fn get_saturating_k_genes_after(
        &self,
        position: u64,
        k: usize,
    ) -> Result<Option<Gene>, TGVError> {
        navigate_genes(self, position, k, true, true)
    }
    pub fn get_exon_at(&self, position: u64) -> Result<Option<SubGeneFeature>, TGVError> {
        navigate_exons(self, position, 0, true, false)
    }
    pub fn get_k_exons_before(
        &self,
        position: u64,
        k: usize,
    ) -> Result<Option<SubGeneFeature>, TGVError> {
        navigate_exons(self, position, k, false, false)
    }
    pub fn get_k_exons_after(
        &self,
        position: u64,
        k: usize,
    ) -> Result<Option<SubGeneFeature>, TGVError> {
        navigate_exons(self, position, k, true, false)
    }
    pub fn get_saturating_k_exons_before(
        &self,
        position: u64,
        k: usize,
    ) -> Result<Option<SubGeneFeature>, TGVError> {
        navigate_exons(self, position, k, false, true)
    }
    pub fn get_saturating_k_exons_after(
        &self,
        position: u64,
        k: usize,
    ) -> Result<Option<SubGeneFeature>, TGVError> {
        navigate_exons(self, position, k, true, true)
    }
}

fn navigate_genes(
    table: &GeneTable,
    position: u64,
    k: usize,
    after: bool,
    saturating: bool,
) -> Result<Option<Gene>, TGVError> {
    if k == 0 {
        return if saturating {
            Ok(None)
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
        return Ok(genes_from_rows(&rows)?.into_iter().next());
    }
    if !saturating {
        return Ok(None);
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
    Ok(genes_from_rows(&rows)?.into_iter().next())
}

fn navigate_exons(
    table: &GeneTable,
    position: u64,
    k: usize,
    after: bool,
    saturating: bool,
) -> Result<Option<SubGeneFeature>, TGVError> {
    if k == 0 && saturating {
        return Ok(None);
    }
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
    if rows.height() == 0 {
        return Ok(None);
    }
    Ok(Some(SubGeneFeature {
        contig_index: rows
            .column("contig_index")?
            .u64()?
            .get(0)
            .expect("exon contigs are non-null") as usize,
        start: rows
            .column("start")?
            .u64()?
            .get(0)
            .expect("exon starts are non-null"),
        end: rows
            .column("end")?
            .u64()?
            .get(0)
            .expect("exon ends are non-null"),
        feature_type: SubGeneFeatureType::Exon,
    }))
}

/// Materialize temporary gene values from queried rows, without retaining a second representation.
pub fn genes_from_rows(data: &DataFrame) -> Result<Vec<Gene>, TGVError> {
    let ids = data.column("id")?.str()?;
    let names = data.column("name")?.str()?;
    let strands = data.column("strand")?.str()?;
    let contigs = data.column("contig_index")?.u64()?;
    let starts = data.column("start")?.u64()?;
    let ends = data.column("end")?.u64()?;
    let cds_starts = data.column("cds_start")?.u64()?;
    let cds_ends = data.column("cds_end")?.u64()?;
    let exon_starts = data.column("exon_starts")?.list()?;
    let exon_ends = data.column("exon_ends")?.list()?;
    let has_exons = data.column("has_exons")?.bool()?;
    (0..data.height())
        .map(|row| {
            Ok(Gene {
                id: ids.get(row).expect("gene IDs are non-null").to_owned(),
                name: names.get(row).expect("gene names are non-null").to_owned(),
                strand: Strand::from_str(
                    strands
                        .get(row)
                        .expect("gene strands are non-null")
                        .to_owned(),
                )?,
                contig_index: contigs.get(row).expect("gene contigs are non-null") as usize,
                transcription_start: starts.get(row).expect("gene starts are non-null"),
                transcription_end: ends.get(row).expect("gene ends are non-null"),
                cds_start: cds_starts.get(row).expect("CDS starts are non-null"),
                cds_end: cds_ends.get(row).expect("CDS ends are non-null"),
                exon_starts: exon_starts
                    .get_as_series(row)
                    .expect("exon lists are non-null")
                    .u64()?
                    .into_no_null_iter()
                    .collect(),
                exon_ends: exon_ends
                    .get_as_series(row)
                    .expect("exon lists are non-null")
                    .u64()?
                    .into_no_null_iter()
                    .collect(),
                has_exons: has_exons.get(row).expect("exon flags are non-null"),
            })
        })
        .collect()
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
    use crate::strand::Strand;

    /// Test table: [gene1: [2,5], [8,10]], [gene_no_exon (21-30)], [gene2: [41,50]].
    fn get_test_track() -> GeneTable {
        let genes = vec![
            Gene {
                id: "gene1".to_string(),
                name: "gene1".to_string(),
                strand: Strand::Forward,
                contig_index: 0,
                transcription_start: 2,
                transcription_end: 10,
                cds_start: 2,
                cds_end: 10,
                exon_starts: vec![2, 8],
                exon_ends: vec![5, 10],
                has_exons: true,
            },
            Gene {
                id: "gene_no_exon".to_string(),
                name: "gene_no_exon".to_string(),
                strand: Strand::Forward,
                contig_index: 0,
                transcription_start: 21,
                transcription_end: 30,
                cds_start: 25,
                cds_end: 25,
                exon_starts: vec![],
                exon_ends: vec![],
                has_exons: false,
            },
            Gene {
                id: "gene2".to_string(),
                name: "gene2".to_string(),
                strand: Strand::Forward,
                contig_index: 0,
                transcription_start: 41,
                transcription_end: 50,
                cds_start: 45,
                cds_end: 50,
                exon_starts: vec![41],
                exon_ends: vec![50],
                has_exons: true,
            },
        ];

        GeneTable::from_genes(genes, 0, (1, 100)).unwrap()
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
                track.get_gene_at(position).unwrap().unwrap().name,
                gene_name
            ),
            None => assert!(track.get_gene_at(position).unwrap().is_none()),
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
                track.get_k_genes_before(position, k).unwrap().unwrap().name,
                gene_name
            ),
            None => assert!(track.get_k_genes_before(position, k).unwrap().is_none()),
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
                    track.get_k_genes_after(position, k).unwrap().unwrap().name,
                    gene_name
                )
            }
            None => assert!(track.get_k_genes_after(position, k).unwrap().is_none()),
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
                track.get_exon_at(position).unwrap().unwrap().start(),
                exon_idx
            ),
            None => assert!(track.get_exon_at(position).unwrap().is_none()),
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
                    .unwrap()
                    .start(),
                exon_idx
            ),
            None => assert!(track.get_k_exons_before(position, k).unwrap().is_none()),
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
                    .unwrap()
                    .start(),
                exon_idx
            ),
            None => assert!(track.get_k_exons_after(position, k).unwrap().is_none()),
        }
    }

    #[test]
    fn test_has_complete_data_uses_loaded_region() {
        let track = GeneTable::from_genes(Vec::new(), 0, (100, 200)).unwrap();

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

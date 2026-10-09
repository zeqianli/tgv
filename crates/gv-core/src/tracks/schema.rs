use crate::{
    contig_header::{Contig, ContigHeader},
    cytoband::{CytobandBand, CytobandTable},
    error::TGVError,
    gene::{GeneSchema, GeneTable},
    reference::Reference,
    strand::Strand,
};
use polars::prelude::*;
use serde::Deserialize;
use sqlx::{FromRow, Row, mysql::MySqlRow, sqlite::SqliteRow};
use std::collections::HashMap;

/// Deserialization target for a row in the gene table.
/// Coordinates retain the UCSC convention until table ingestion.
#[allow(non_snake_case)]
#[derive(Debug)]
pub struct UcscGeneRow {
    pub name: String,
    pub chrom: String,
    pub strand: String,
    pub txStart: u64,
    pub txEnd: u64,
    pub cdsStart: u64,
    pub cdsEnd: u64,
    pub name2: Option<String>,
    pub exonStarts: Vec<u8>,
    pub exonEnds: Vec<u8>,
    pub has_exons: bool,
}

#[allow(non_snake_case)]
impl FromRow<'_, SqliteRow> for UcscGeneRow {
    fn from_row(row: &SqliteRow) -> sqlx::Result<Self> {
        let txStart: i64 = row.try_get("txStart")?;
        let txEnd: i64 = row.try_get("txEnd")?;
        let cdsStart: i64 = row.try_get("cdsStart")?;
        let cdsEnd: i64 = row.try_get("cdsEnd")?;

        Ok(UcscGeneRow {
            name: row.try_get("name")?,
            chrom: row.try_get("chrom")?,
            strand: row.try_get("strand")?,
            txStart: txStart as u64,
            txEnd: txEnd as u64,
            cdsStart: cdsStart as u64,
            cdsEnd: cdsEnd as u64,
            name2: row.try_get("name2")?,
            exonStarts: row.try_get("exonStarts")?,
            exonEnds: row.try_get("exonEnds")?,
            has_exons: true,
        })
    }
}

impl FromRow<'_, MySqlRow> for UcscGeneRow {
    fn from_row(row: &MySqlRow) -> sqlx::Result<Self> {
        Ok(UcscGeneRow {
            name: row.try_get("name")?,
            chrom: row.try_get("chrom")?,
            strand: row.try_get("strand")?,
            txStart: row.try_get("txStart")?,
            txEnd: row.try_get("txEnd")?,
            cdsStart: row.try_get("cdsStart")?,
            cdsEnd: row.try_get("cdsEnd")?,
            name2: row.try_get("name2")?,
            exonStarts: row.try_get("exonStarts")?,
            exonEnds: row.try_get("exonEnds")?,
            has_exons: true,
        })
    }
}

#[allow(non_snake_case)]
impl UcscGeneRow {
    fn parse_blob_to_coords(blob: &[u8]) -> Result<Vec<u64>, TGVError> {
        let coords = std::str::from_utf8(blob)?.trim_end_matches(',');
        if coords.is_empty() {
            return Ok(Vec::new());
        }
        coords.split(',').map(|v| Ok(v.parse::<u64>()?)).collect()
    }
}

impl GeneTable {
    /// Build gene columns directly from UCSC rows, converting starts to one-based coordinates.
    pub fn from_gene_rows(
        gene_rows: Vec<UcscGeneRow>,
        contig_index: usize,
        contig_header: &ContigHeader,
        loaded_bounds: Option<(u64, u64)>,
    ) -> Result<Self, TGVError> {
        let mut ids = Vec::with_capacity(gene_rows.len());
        let mut names = Vec::with_capacity(gene_rows.len());
        let mut strands = Vec::with_capacity(gene_rows.len());
        let mut starts = Vec::with_capacity(gene_rows.len());
        let mut ends = Vec::with_capacity(gene_rows.len());
        let mut cds_starts = Vec::with_capacity(gene_rows.len());
        let mut cds_ends = Vec::with_capacity(gene_rows.len());
        let mut exon_starts = ListPrimitiveChunkedBuilder::<UInt64Type>::new(
            GeneSchema::EXON_STARTS.into(),
            gene_rows.len(),
            0,
            DataType::UInt64,
        );
        let mut exon_ends = ListPrimitiveChunkedBuilder::<UInt64Type>::new(
            GeneSchema::EXON_ENDS.into(),
            gene_rows.len(),
            0,
            DataType::UInt64,
        );
        let mut has_exons = Vec::with_capacity(gene_rows.len());
        for row in gene_rows {
            if contig_header.try_get_index_by_str(&row.chrom)? != contig_index
                || row.txStart >= row.txEnd
                || row.cdsStart == u64::MAX
                || row.cdsStart > row.cdsEnd
                || row.cdsStart < row.txStart
                || row.cdsEnd > row.txEnd
            {
                return Err(TGVError::ValueError(format!(
                    "Invalid UCSC bounds or contig for gene {}.",
                    row.name,
                )));
            }
            let raw_starts = UcscGeneRow::parse_blob_to_coords(&row.exonStarts)?;
            let raw_ends = UcscGeneRow::parse_blob_to_coords(&row.exonEnds)?;
            if raw_starts.len() != raw_ends.len() {
                return Err(TGVError::ValueError(format!(
                    "Gene {} has mismatched exon starts and ends.",
                    row.name,
                )));
            }
            let mut previous_end = row.txStart;
            for (&start, &end) in raw_starts.iter().zip(&raw_ends) {
                if start < previous_end || start >= end || end > row.txEnd {
                    return Err(TGVError::ValueError(format!(
                        "Invalid UCSC exon [{start}, {end}) for gene {}.",
                        row.name,
                    )));
                }
                previous_end = end;
            }
            starts.push(row.txStart + 1);
            ends.push(row.txEnd);
            cds_starts.push(row.cdsStart + 1);
            cds_ends.push(row.cdsEnd);
            names.push(row.name2.unwrap_or_else(|| row.name.clone()));
            strands.push(Strand::from_str(row.strand)?.to_string());
            ids.push(row.name);
            exon_starts.append_values_iter(raw_starts.into_iter().map(|start| start + 1));
            exon_ends.append_slice(&raw_ends);
            has_exons.push(row.has_exons);
        }
        let height = ids.len();
        let data = DataFrame::new(
            height,
            vec![
                Column::new(
                    GeneSchema::ROW_ID.into(),
                    (0..height as u64).collect::<Vec<_>>(),
                ),
                Column::new(
                    GeneSchema::CONTIG_INDEX.into(),
                    vec![contig_index as u64; height],
                ),
                Column::new(GeneSchema::START.into(), starts),
                Column::new(GeneSchema::END.into(), ends),
                Column::new(GeneSchema::ID.into(), ids),
                Column::new(GeneSchema::NAME.into(), names),
                Column::new(GeneSchema::STRAND.into(), strands),
                Column::new(GeneSchema::CDS_START.into(), cds_starts),
                Column::new(GeneSchema::CDS_END.into(), cds_ends),
                exon_starts.finish().into_column(),
                exon_ends.finish().into_column(),
                Column::new(GeneSchema::HAS_EXONS.into(), has_exons),
            ],
        )?;
        let bounds = loaded_bounds.unwrap_or((
            data.column(GeneSchema::START)?
                .u64()?
                .min()
                .unwrap_or(u64::MAX),
            data.column(GeneSchema::END)?.u64()?.max().unwrap_or(0),
        ));
        Self::from_data(data, contig_index, bounds)
    }
}

#[allow(non_snake_case)]
#[derive(Debug, Deserialize)]
pub struct CytobandSegmentRow {
    chrom: String,
    chromStart: u64, // sqlite doesn't support unsigned int
    chromEnd: u64,
    name: String,
    gieStain: String,
}

#[allow(non_snake_case)]
impl FromRow<'_, SqliteRow> for CytobandSegmentRow {
    fn from_row(row: &SqliteRow) -> sqlx::Result<Self> {
        let chromStart: i64 = row.try_get("chromStart")?;
        let chromEnd: i64 = row.try_get("chromEnd")?;
        Ok(CytobandSegmentRow {
            chrom: row.try_get("chrom")?,
            chromStart: chromStart as u64,
            chromEnd: chromEnd as u64,
            name: row.try_get("name")?,
            gieStain: row.try_get("gieStain")?,
        })
    }
}

#[allow(non_snake_case)]
impl FromRow<'_, MySqlRow> for CytobandSegmentRow {
    fn from_row(row: &MySqlRow) -> sqlx::Result<Self> {
        Ok(CytobandSegmentRow {
            chrom: row.try_get("chrom")?,
            chromStart: row.try_get("chromStart")?,
            chromEnd: row.try_get("chromEnd")?,
            name: row.try_get("name")?,
            gieStain: row.try_get("gieStain")?,
        })
    }
}

impl CytobandSegmentRow {
    /// Builds the cytoband table, converting UCSC's 0-based starts to 1-based. Bands on
    /// contigs the header doesn't know are skipped.
    pub fn into_table(
        rows: impl IntoIterator<Item = Self>,
        contig_header: &ContigHeader,
    ) -> Result<CytobandTable, TGVError> {
        CytobandTable::from_bands(
            rows.into_iter()
                .filter_map(|row| {
                    Some(CytobandBand {
                        contig_index: contig_header.try_get_index_by_str(&row.chrom).ok()?,
                        start: row.chromStart + 1,
                        end: row.chromEnd,
                        name: row.name,
                        stain: row.gieStain,
                    })
                })
                .collect(),
        )
    }
}

#[allow(non_snake_case)]
#[derive(Debug)]
pub struct ContigRow {
    pub chrom: String,
    pub size: u64,
    pub aliases: String,
}

impl FromRow<'_, SqliteRow> for ContigRow {
    fn from_row(row: &SqliteRow) -> sqlx::Result<Self> {
        let size: i64 = row.try_get("size")?;
        Ok(ContigRow {
            chrom: row.try_get("chrom")?,
            size: size as u64,
            aliases: row.try_get("aliases").unwrap_or("".to_string()),
        })
    }
}

impl FromRow<'_, MySqlRow> for ContigRow {
    fn from_row(row: &MySqlRow) -> sqlx::Result<Self> {
        Ok(ContigRow {
            chrom: row.try_get("chrom")?,
            size: row.try_get("size")?,
            aliases: row.try_get("aliases").unwrap_or("".to_string()),
        })
    }
}

impl ContigRow {
    pub fn to_contig(self) -> Result<Contig, TGVError> {
        let mut contig = Contig::new(&self.chrom, Some(self.size));
        for alias in self.aliases.split(',') {
            contig.add_alias(alias);
        }
        Ok(contig)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[allow(non_snake_case)]
#[serde(untagged)]
pub enum UcscGeneResponse {
    GeneResponse1 {
        name: String,
        name2: Option<String>,

        strand: String,

        txStart: u64,
        txEnd: u64,
        cdsStart: u64,

        cdsEnd: u64,
        exonStarts: String,
        exonEnds: String,
    },

    GeneResponse2 {
        /*

        Example responsse:

        {
        "chrom": "NC_072398.2",
        "chromStart": 130929426,
        "chromEnd": 130985030,
        "name": "NM_001142759.1",
        "score": 0,
        "strand": "+",
        "thickStart": 130929440,
        "thickEnd": 130982945,
        "reserved": "0",
        "blockCount": 13,
        "blockSizes": "65,124,76,182,122,217,167,126,78,192,72,556,374,",
        "chromStarts": "0,8926,14265,18877,31037,33561,34781,36127,39014,43150,43484,53351,55230,",
        "name2": "DBT",
        "cdsStartStat": "cmpl",
        "cdsEndStat": "cmpl",
        "exonFrames": "0,0,1,2,1,0,1,0,0,0,0,0,-1,",
        "type": "",
        "geneName": "NM_001142759.1",
        "geneName2": "DBT",
        "geneType": ""
        }

        I'm not sure if the implementation is correct.

        */
        chromStart: u64,
        chromEnd: u64,
        name: String,
        strand: String,
        thickStart: u64,
        thickEnd: u64,
    },
}

#[allow(non_snake_case)]
impl UcscGeneResponse {
    /// Normalize API fields to the database row format without changing coordinates.
    pub fn into_gene_row(self, chrom: String) -> UcscGeneRow {
        match self {
            Self::GeneResponse1 {
                name,
                name2,
                strand,
                txStart,
                txEnd,
                cdsStart,
                cdsEnd,
                exonStarts,
                exonEnds,
            } => UcscGeneRow {
                name,
                name2,
                chrom,
                strand,
                txStart,
                txEnd,
                cdsStart,
                cdsEnd,
                exonStarts: exonStarts.into_bytes(),
                exonEnds: exonEnds.into_bytes(),
                has_exons: true,
            },
            Self::GeneResponse2 {
                name,
                strand,
                chromStart,
                chromEnd,
                thickStart,
                thickEnd,
            } => UcscGeneRow {
                name,
                name2: None,
                chrom,
                strand,
                txStart: chromStart,
                txEnd: chromEnd,
                cdsStart: thickStart,
                cdsEnd: thickEnd,
                exonStarts: Vec::new(),
                exonEnds: Vec::new(),
                has_exons: false,
            },
        }
    }
}

///
/// Example response:
/// {
///    ...
///     "genarkGenomes": {
///       "GCF_028858775.2": {
///         "hubUrl": "GCF/028/858/775/GCF_028858775.2/hub.txt",
///         "asmName": "NHGRI_mPanTro3-v2.0_pri",
///         "scientificName": "Pan troglodytes",
///         "commonName": "chimpanzee (v2 AG18354 primary hap 2024 refseq)",
///         "taxId": 9598,
///         "priority": 138,
///         "clade": "primates"
///       }
///     },
///
///   }
#[derive(Debug, Clone, Deserialize)]
#[allow(non_snake_case)]
pub struct UcscApiHubUrlResponse {
    genarkGenomes: HashMap<String, GenarkGenome>,
}

impl UcscApiHubUrlResponse {
    pub fn get_hub_url(&self, accession: &str) -> Result<String, TGVError> {
        Ok(format!(
            "https://hgdownload.soe.ucsc.edu/hubs/{}",
            self.genarkGenomes[accession].hubUrl
        ))
    }
}

#[derive(Debug, Clone, Deserialize)]
#[allow(non_snake_case)]
pub struct GenarkGenome {
    hubUrl: String,
}

/// Example response:
/// {
///   ...,
///   "chromosomes": {
///     "chr1": 197195432,
///     "chr16_random": 3994,
///     "chrM": 16299,
///     "chr3_random": 41899,
///     ...
///    }
/// }

#[allow(non_snake_case)]
#[derive(Debug, Clone, Deserialize)]
pub struct UcscListChromosomeResponse {
    pub chromosomes: HashMap<String, u64>,
}

/// The UCSC API's cytoband response. Without a `chrom` parameter, the API groups the bands by
/// contig:
///
/// ```json
/// { "cytoBandIdeo": { "chr1": [{ "chrom": "chr1", "chromStart": 0, "chromEnd": 2300000,
///   "name": "p36.33", "gieStain": "gneg" }, ...], ... } }
/// ```
#[allow(non_snake_case)]
#[derive(Debug, Deserialize, Default)]
pub struct UcscApiCytobandResponse {
    #[serde(default)]
    cytoBandIdeo: Option<UcscApiCytobands>,
}

/// Bands grouped by contig, or one flat list, which some hubs may return for single-contig
/// genomes.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum UcscApiCytobands {
    ByContig(HashMap<String, Vec<CytobandSegmentRow>>),
    Flat(Vec<CytobandSegmentRow>),
}

impl UcscApiCytobandResponse {
    pub fn into_table(self, contig_header: &ContigHeader) -> Result<CytobandTable, TGVError> {
        let rows: Vec<CytobandSegmentRow> = match self.cytoBandIdeo {
            Some(UcscApiCytobands::ByContig(by_contig)) => {
                by_contig.into_values().flatten().collect()
            }
            Some(UcscApiCytobands::Flat(rows)) => rows,
            None => Vec::new(),
        };
        CytobandSegmentRow::into_table(rows, contig_header)
    }
}

use crate::{
    contig_header::{Contig, ContigHeader},
    cytoband::{Cytoband, CytobandSegment, Stain},
    error::TGVError,
    feature::{Gene, SubGeneFeature},
    reference::Reference,
    strand::Strand,
    track::Track,
};
use serde::Deserialize;
use sqlx::FromRow;
use std::collections::HashMap;

/// Deserialization target for a row in the gene table.
/// Converting to Gene needs the header information and is done downstream.
#[derive(Debug, FromRow)]
pub struct UcscGeneRow {
    pub name: String,
    pub chrom: String,
    pub strand: String,
    pub tx_start: i64,
    pub tx_end: i64,
    pub cds_start: i64,
    pub cds_end: i64,
    pub name2: Option<String>,
    pub exon_starts_blob: Vec<u8>,
    pub exon_ends_blob: Vec<u8>,
}

impl UcscGeneRow {
    // Helper function to parse BLOB of comma-separated coordinates
    fn parse_blob_to_coords(blob: &[u8]) -> Vec<usize> {
        let coords_str = String::from_utf8_lossy(blob);
        coords_str
            .trim_end_matches(',')
            .split(',')
            .filter_map(|s| s.parse::<usize>().ok())
            .collect()
    }

    pub fn to_gene(self, contig_header: &ContigHeader) -> Result<Gene, TGVError> {
        // USCS coordinates are 0-based, half-open
        // https://genome-blog.gi.ucsc.edu/blog/2016/12/12/the-ucsc-genome-browser-coordinate-counting-systems/
        Ok(Gene {
            id: self.name.clone(),
            name: self.name2.unwrap_or(self.name.clone()),
            strand: Strand::from_str(self.strand)?,
            contig_index: contig_header.get_index_by_str(&self.chrom)?,
            transcription_start: self.tx_start as usize + 1,
            transcription_end: self.tx_end as usize,
            cds_start: self.cds_start as usize + 1,
            cds_end: self.cds_end as usize,
            exon_starts: Self::parse_blob_to_coords(&self.exon_starts_blob)
                .iter()
                .map(|v| v + 1)
                .collect(),
            exon_ends: Self::parse_blob_to_coords(&self.exon_ends_blob),
            has_exons: true,
        })
    }
}

impl Track<Gene> {
    pub fn from_gene_rows(
        gene_rows: Vec<UcscGeneRow>,
        contig_index: usize,
        contig_header: &ContigHeader,
    ) -> Result<Self, TGVError> {
        if gene_rows.is_empty() {
            return Err(TGVError::IOError("No genes found".to_string()));
        }

        let genes = gene_rows
            .into_iter()
            .map(|row| row.to_gene(contig_header))
            .collect::<Result<Vec<Gene>, TGVError>>()?;
        Track::from_genes(genes, contig_index)
    }
}

#[derive(Debug, FromRow, Deserialize)]
pub struct CytobandSegmentRow {
    chromStart: i64, // sqlite doesn't support unsigned int
    chromEnd: i64,
    name: String,
    gieStain: String,
}

impl CytobandSegmentRow {
    pub fn to_cytoband_segment(self, header: &ContigHeader) -> Result<CytobandSegment, TGVError> {
        Ok(CytobandSegment {
            contig_index: header.get_index_by_str(&self.name)?,
            start: self.chromStart as usize + 1,
            end: self.chromEnd as usize,
            name: self.name,
            stain: Stain::from(&self.gieStain)?,
        })
    }
}

#[derive(Debug, FromRow)]
pub struct ContigRow {
    pub chrom: String,
    pub size: i64,
    pub aliases: String,
}

impl ContigRow {
    pub fn to_contig(self) -> Result<Contig, TGVError> {
        let mut contig = Contig::new(&self.chrom, Some(self.size as usize));
        for alias in self.aliases.split(',') {
            contig.add_alias(alias);
        }
        Ok(contig)
    }
}

/// Custom deserializer for comma-separated lists in UCSC response
fn parse_comma_separated_list(s: &str) -> Result<Vec<usize>, TGVError> {
    s.trim_end_matches(',')
        .split(',')
        .filter(|s| !s.is_empty())
        .map(|num| {
            num.parse::<usize>()
                .map_err(|_| TGVError::ValueError(format!("Failed to parse {}", num)))
        })
        .collect()
}

#[derive(Debug, Clone, Deserialize)]
#[allow(non_snake_case)]
pub enum UcscGeneResponse {
    GeneResponse1 {
        name: String,
        name2: Option<String>,

        strand: String,

        txStart: usize,
        txEnd: usize,
        cdsStart: usize,

        cdsEnd: usize,
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
        chromStart: usize,
        chromEnd: usize,
        name: String,
        strand: String,
        thickStart: usize,
        thickEnd: usize,
    },
}

impl UcscGeneResponse {
    /// Custom deserializer for strand field
    pub fn to_gene(self, contig_index: usize) -> Result<Gene, TGVError> {
        match self {
            UcscGeneResponse::GeneResponse1 {
                name,
                name2,
                strand,
                txStart,
                txEnd,
                cdsStart,
                cdsEnd,
                exonStarts,
                exonEnds,
            } => Ok(Gene {
                id: name.clone(),
                name: name2.unwrap_or(name.clone()),
                strand: Strand::from_str(strand)?,
                contig_index: contig_index,
                transcription_start: txStart,
                transcription_end: txEnd,
                cds_start: cdsStart,
                cds_end: cdsEnd,
                exon_starts: parse_comma_separated_list(&exonStarts)?,
                exon_ends: parse_comma_separated_list(&exonEnds)?,
                has_exons: true,
            }),

            UcscGeneResponse::GeneResponse2 {
                chromStart,
                chromEnd,
                name,
                strand,
                thickStart,
                thickEnd,
            } => Ok(Gene {
                id: name.clone(),
                name: name,
                strand: Strand::from_str(strand)?,
                contig_index: contig_index,
                transcription_start: chromStart,
                transcription_end: chromEnd,
                cds_start: thickStart,
                cds_end: thickEnd,
                exon_starts: vec![],
                exon_ends: vec![],
                has_exons: false,
            }),
        }
    }
}

// {
//   "downloadTime": "2025:08:12T17:08:55Z",
//   "downloadTimeStamp": 1755018535,
//   "genome": "hg38",
//   "dataTime": "2022-10-18T23:39:31",
//   "dataTimeStamp": 1666161571,
//   "trackType": "bed 3 +",
//   "track": "gold",
//   "chrom": "chrM",
//   "start": 0,
//   "end": 16569,
//   "gold": [
//     {
//       "bin": 585,
//       "chrom": "chrM",
//       "chromStart": 0,
//       "chromEnd": 16569,
//       "ix": 1,
//       "type": "O",
//       "frag": "J01415.2",
//       "fragStart": 0,
//       "fragEnd": 16569,
//       "strand": "+"
//     }
//   ],
//   "itemsReturned": 1
// }
#[derive(Debug, Clone, Deserialize)]
pub struct UcscApiListGeneResponse {
    #[serde(flatten)]
    pub genes: Vec<UcscGeneResponse>,
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
    genearkGenomes: HashMap<String, GenarkGenome>,
}

impl UcscApiHubUrlResponse {
    pub fn get_hub_url(&self, accession: &str) -> Result<String, TGVError> {
        Ok(format!(
            "https://hgdownload.soe.ucsc.edu/hubs/{}",
            self.genearkGenomes[accession].hubUrl
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

#[derive(Debug, Clone, Deserialize)]
pub struct UcscListChromosomeResponse {
    pub chromosomes: HashMap<String, usize>,
}

///{
//   ...
//   "cytoBandIdeo": [
//     {
//       "chrom": "chr1",
//       "chromStart": 0,
//       "chromEnd": 8918386,
//       "name": "qA1",
//       "gieStain": "gpos100"
//     },
#[derive(Debug, Deserialize)]
pub struct UcscApiCytobandResponse {
    cytoBandIdeo: Vec<CytobandSegmentRow>,
}

impl Default for UcscApiCytobandResponse {
    fn default() -> Self {
        UcscApiCytobandResponse {
            cytoBandIdeo: vec![],
        }
    }
}

impl UcscApiCytobandResponse {
    pub fn to_cytoband(
        self,
        reference: &Reference,
        contig_index: usize,
        contig_header: &ContigHeader,
    ) -> Result<Option<Cytoband>, TGVError> {
        if self.cytoBandIdeo.is_empty() {
            Ok(None)
        } else {
            Ok(Some(Cytoband {
                reference: Some(reference.clone()),
                contig_index: contig_index,
                segments: self
                    .cytoBandIdeo
                    .into_iter()
                    .map(|cytoband| cytoband.to_cytoband_segment(contig_header))
                    .collect::<Result<Vec<CytobandSegment>, TGVError>>()?,
            }))
        }
    }
}

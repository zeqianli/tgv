use crate::{
    contig_collection::{Contig, ContigHeader},
    cytoband::{Cytoband, CytobandSegment, Stain},
    error::TGVError,
    feature::{Gene, SubGeneFeature},
    intervals::GenomeInterval,
    reference::Reference,
    region::Region,
    strand::Strand,
    track::Track,
    ucsc::UcscHost,
};
use async_trait::async_trait;
use sqlx::{
    mysql::{MySqlPoolOptions, MySqlRow},
    Column, FromRow, MySqlPool, Row,
};
use std::collections::HashMap;
use std::sync::Arc;

/// Deserialization target for a row in the gene table.
/// Converting to Gene needs the header information and is done downstream.
#[derive(Debug, FromRow)]
pub struct UcscGeneRow {
    pub name: String,
    pub chrom: String,
    pub strand: String,
    pub tx_start: u64,
    pub tx_end: u64,
    pub cds_start: u64,
    pub cds_end: u64,
    pub name2: Option<String>,
    pub exon_starts: Vec<u8>,
    pub exon_ends: Vec<u8>,
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

    fn to_gene(self, contig_header: &ContigHeader) -> Result<Gene, TGVError> {
        // USCS coordinates are 0-based, half-open
        // https://genome-blog.gi.ucsc.edu/blog/2016/12/12/the-ucsc-genome-browser-coordinate-counting-systems/
        Ok(Gene {
            id: self.name,
            name: self.name2.unwrap_or(self.name),
            strand: Strand::from_str(self.strand)?,
            contig: Contig::new(&self.chrom),
            transcription_start: self.tx_start as usize + 1,
            transcription_end: tx_end as usize,
            cds_start: cds_start as usize + 1,
            cds_end: cds_end as usize,
            exon_starts: Self::parse_blob_to_coords(&exon_starts_blob)
                .iter()
                .map(|v| v + 1)
                .collect(),
            exon_ends: Self::parse_blob_to_coords(&exon_ends_blob),
            has_exons: true,
        })
    }
}

#[derive(Debug, FromRow)]
struct CytobandSegmentRow {
    chromStart: u64,
    chromEnd: u64,
    name: String,
    gieStain: String,
}

impl CytobandSegmentRow {
    fn to_cytoband_segment(self, header: &ContigHeader) -> Result<CytobandSegment, TGVError> {
        Ok(CytobandSegment {
            contig: Contig::new(&self.chrom, header.length),
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
    pub size: u32,
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

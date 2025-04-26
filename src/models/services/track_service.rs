use crate::error::TGVError;
use crate::models::{
    contig::Contig,
    cytoband::{Cytoband, CytobandSegment, Stain},
    reference::Reference,
    region::Region,
    strand::Strand,
    track::{
        feature::{Gene, SubGeneFeature, SubGeneFeatureType},
        track::Track,
    },
};
use crate::traits::GenomeInterval;
use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use serde::de::Error as _;
use serde_json;
use sqlx::{mysql::MySqlPoolOptions, MySqlPool, Row};
use std::collections::HashMap;
use std::sync::Arc;

/// Holds cache for track service queries.
/// Can be returned or pass into queries.
pub struct TrackCache {
    /// Contig name -> Track
    track_by_contig: HashMap<String, Option<Track<Gene>>>,

    /// Gene name -> Option<Gene>.
    /// If the gene name is not found, the value is None.
    gene_by_name: HashMap<String, Option<Gene>>,
}

impl TrackCache {
    pub fn new() -> Self {
        Self {
            track_by_contig: HashMap::new(),
            gene_by_name: HashMap::new(),
        }
    }

    pub fn includes_contig(&self, contig: &Contig) -> bool {
        self.track_by_contig.contains_key(&contig.full_name())
    }

    /// Note that this returns None both when the contig is not queried,
    ///    and returns Some(None) when the contig is queried but the track data is not found.
    pub fn get_track(&self, contig: &Contig) -> Option<Option<&Track<Gene>>> {
        self.track_by_contig
            .get(&contig.full_name())
            .map(|track| track.as_ref())
    }

    pub fn includes_gene(&self, gene_name: &str) -> bool {
        self.gene_by_name.contains_key(gene_name)
    }

    /// Note that this returns None both when the gene is not queried,
    ///    and returns Some(None) when the gene is queried but the gene data is not found.
    pub fn get_gene(&self, gene_name: &str) -> Option<Option<&Gene>> {
        self.gene_by_name.get(gene_name).map(|gene| gene.as_ref())
    }

    pub fn add_track(&mut self, contig: &Contig, track: Track<Gene>) {
        for (i, gene) in track.genes().iter().enumerate() {
            self.gene_by_name
                .insert(gene.name.clone(), Some(gene.clone()));
        }
        self.track_by_contig.insert(contig.full_name(), Some(track));
    }
}

#[async_trait]
pub trait TrackService {
    // Basics

    /// Close the track service.
    async fn close(&self) -> Result<(), TGVError>;

    // Contigs and cytobands

    // Return all contigs given a reference.
    async fn get_all_contigs(
        &self,
        reference: &Reference,
    ) -> Result<Vec<(Contig, usize)>, TGVError>;

    // Return the cytoband data given a reference and a contig.
    async fn get_cytoband(
        &self,
        reference: &Reference,
        contig: &Contig,
    ) -> Result<Option<Cytoband>, TGVError>;

    // Genes and tracks

    /// Return a Track<Gene> that covers a region.
    async fn query_gene_track(
        &self,
        reference: &Reference,
        region: &Region,
        cache: &mut TrackCache,
    ) -> Result<Track<Gene>, TGVError> {
        let genes = self
            .query_genes_overlapping(reference, region, cache)
            .await?;
        Track::from_genes(genes, region.contig.clone())
    }

    /// Given a reference, return the prefered track name.
    async fn get_prefered_track_name(
        &self,
        reference: &Reference,
    ) -> Result<Option<String>, TGVError>;

    /// Return a list of genes that overlap with a region.
    async fn query_genes_overlapping(
        &self,
        reference: &Reference,
        region: &Region,
        cache: &mut TrackCache,
    ) -> Result<Vec<Gene>, TGVError>;

    /// Return the Gene covering a contig:coordinate.
    async fn query_gene_covering(
        &self,
        reference: &Reference,
        contig: &Contig,
        coord: usize,
        cache: &mut TrackCache,
    ) -> Result<Option<Gene>, TGVError>;

    async fn query_gene_name(
        &self,
        reference: &Reference,
        gene_id: &String,
        cache: &mut TrackCache,
    ) -> Result<Gene, TGVError>;

    /// Return the k-th gene after a contig:coordinate.
    async fn query_k_genes_after(
        &self,
        reference: &Reference,
        contig: &Contig,
        coord: usize,
        k: usize,
        cache: &mut TrackCache,
    ) -> Result<Gene, TGVError>;

    /// Return the k-th gene before a contig:coordinate.
    async fn query_k_genes_before(
        &self,
        reference: &Reference,
        contig: &Contig,
        coord: usize,
        k: usize,
        cache: &mut TrackCache,
    ) -> Result<Gene, TGVError>;

    /// Return the k-th exon after a contig:coordinate.
    async fn query_k_exons_after(
        &self,
        reference: &Reference,
        contig: &Contig,
        coord: usize,
        k: usize,
        cache: &mut TrackCache,
    ) -> Result<SubGeneFeature, TGVError>;

    /// Return the k-th exon before a contig:coordinate.
    async fn query_k_exons_before(
        &self,
        reference: &Reference,
        contig: &Contig,
        coord: usize,
        k: usize,
        cache: &mut TrackCache,
    ) -> Result<SubGeneFeature, TGVError>;
}

#[derive(Debug)]
pub struct UcscDbTrackService {
    pool: Arc<MySqlPool>,
}

impl UcscDbTrackService {
    /// Initialize the database connect.
    /// Reference is needed to find the corresponding schema.
    pub async fn new(reference: &Reference) -> Result<Self, TGVError> {
        let mysql_url = UcscDbTrackService::get_mysql_url(reference)?;
        let pool = MySqlPoolOptions::new()
            .max_connections(5)
            .connect(&mysql_url)
            .await?;

        Ok(Self {
            pool: Arc::new(pool),
        })
    }

    fn get_mysql_url(reference: &Reference) -> Result<String, TGVError> {
        match reference {
            Reference::Hg19 => Ok("mysql://genome@genome-mysql.soe.ucsc.edu/hg19".to_string()),
            Reference::Hg38 => Ok("mysql://genome@genome-mysql.soe.ucsc.edu/hg38".to_string()),
            Reference::UcscGenome(genome) => Ok(format!(
                "mysql://genome@genome-mysql.soe.ucsc.edu/{}",
                genome
            )),
            _ => Err(TGVError::ValueError(format!(
                "Unsupported reference: {}",
                reference
            ))),
        }
    }
}

#[async_trait]
impl TrackService for UcscDbTrackService {
    async fn close(&self) -> Result<(), TGVError> {
        self.pool.close().await;
        Ok(())
    }

    async fn get_all_contigs(
        &self,
        reference: &Reference,
    ) -> Result<Vec<(Contig, usize)>, TGVError> {
        let rows = sqlx::query("SELECT chrom, size FROM chromInfo ORDER BY chrom")
            .fetch_all(&*self.pool)
            .await?;

        let mut contigs = Vec::new();
        for row in rows {
            let chrom: String = row.try_get("chrom")?;
            let size: u32 = row.try_get("size")?;

            // Create a Contig based on reference type
            let contig = match reference {
                Reference::Hg19 | Reference::Hg38 => Contig::chrom(&chrom),
                _ => Contig::contig(&chrom),
            };

            contigs.push((contig, size as usize));
        }

        Ok(contigs)
    }

    async fn get_cytoband(
        &self,
        reference: &Reference,
        contig: &Contig,
    ) -> Result<Option<Cytoband>, TGVError> {
        let rows = sqlx::query(
            "SELECT chrom, chromStart, chromEnd, name, gieStain FROM cytoBandIdeo WHERE chrom = ?",
        )
        .bind(contig.full_name()) // Assuming full_name includes "chr" prefix if needed
        .fetch_all(&*self.pool)
        .await?;

        if rows.is_empty() {
            return Ok(None);
        }

        let mut segments = Vec::with_capacity(rows.len());
        for row in rows {
            let chrom_start: u32 = row.try_get("chromStart")?;
            let chrom_end: u32 = row.try_get("chromEnd")?;
            let name: String = row.try_get("name")?;
            let gie_stain_str: String = row.try_get("gieStain")?;

            let stain = Stain::from(&gie_stain_str)?;

            segments.push(CytobandSegment {
                contig: contig.clone(),          // Use the input contig
                start: chrom_start as usize + 1, // 0-based to 1-based
                end: chrom_end as usize,
                name,
                stain,
            });
        }

        Ok(Some(Cytoband {
            reference: Some(reference.clone()),
            contig: contig.clone(),
            segments,
        }))
    }

    async fn get_prefered_track_name(
        &self,
        _reference: &Reference,
    ) -> Result<Option<String>, TGVError> {
        let gene_track_rows = sqlx::query("SELECT tableName FROM trackDb WHERE grp = 'gene'")
            .fetch_all(&*self.pool)
            .await?;

        let available_gene_tracks: Vec<String> = gene_track_rows
            .into_iter()
            .map(|row| row.try_get("tableName"))
            .collect::<Result<_, _>>()?;

        let preferences = [
            "ncbiRefSeqSelect",
            "ncbiRefSeqCurated",
            "ncbiRefSeq",
            "refGenes",
        ];

        for pref in preferences {
            if available_gene_tracks.contains(&pref.to_string()) {
                return Ok(Some(pref.to_string()));
            }
        }

        Ok(None)
    }

    async fn query_genes_overlapping(&self, region: &Region) -> Result<Vec<Gene>, TGVError> {
        let rows = sqlx::query(
            "SELECT name, chrom, strand, txStart, txEnd, name2, exonStarts, exonEnds, cdsStart, cdsEnd
             FROM ncbiRefSeqSelect 
             WHERE chrom = ? AND (txStart <= ?) AND (txEnd >= ?)",
        )
        .bind(region.contig.full_name()) // Requires "chr" prefix?
        .bind(u64::try_from(region.end).unwrap()) // end is 1-based inclusive, UCSC is 0-based exclusive
        .bind(u64::try_from(region.start.saturating_sub(1)).unwrap()) // start is 1-based inclusive, UCSC is 0-based inclusive
        .fetch_all(&*self.pool)
        .await?;

        let mut genes = Vec::new();
        for row in rows {
            let name: String = row.try_get("name")?;
            let chrom: String = row.try_get("chrom")?;
            let strand_str: String = row.try_get("strand")?;
            let tx_start: u64 = row.try_get("txStart")?;
            let tx_end: u64 = row.try_get("txEnd")?;
            let cds_start: u64 = row.try_get("cdsStart")?;
            let cds_end: u64 = row.try_get("cdsEnd")?;
            let name2: String = row.try_get("name2")?;
            let exon_starts_blob: Vec<u8> = row.try_get("exonStarts")?;
            let exon_ends_blob: Vec<u8> = row.try_get("exonEnds")?;

            // USCS coordinates are 0-based, half-open
            // https://genome-blog.gi.ucsc.edu/blog/2016/12/12/the-ucsc-genome-browser-coordinate-counting-systems/

            genes.push(Gene {
                id: name,
                name: name2,
                strand: Strand::from_str(strand_str).unwrap(),
                contig: Contig::chrom(&chrom),
                transcription_start: tx_start as usize + 1,
                transcription_end: tx_end as usize,
                cds_start: cds_start as usize + 1,
                cds_end: cds_end as usize,
                exon_starts: parse_blob_to_coords(&exon_starts_blob)
                    .iter()
                    .map(|v| v + 1)
                    .collect(),
                exon_ends: parse_blob_to_coords(&exon_ends_blob),
            });
        }

        Ok(genes)
    }

    async fn query_gene_covering(
        &self,
        contig: &Contig,
        coord: usize,
    ) -> Result<Option<Gene>, TGVError> {
        let row = sqlx::query(
            "SELECT name, chrom, strand, txStart, txEnd, name2, exonStarts, exonEnds, cdsStart, cdsEnd
             FROM ncbiRefSeqSelect 
             WHERE chrom = ? AND txStart <= ? AND txEnd >= ?",
        )
        .bind(contig.full_name())
        .bind(u32::try_from(coord.saturating_sub(1)).unwrap()) // coord is 1-based inclusive, UCSC is 0-based inclusive
        .bind(u32::try_from(coord).unwrap()) // coord is 1-based inclusive, UCSC is 0-based exclusive
        .fetch_optional(&*self.pool)
        .await?;

        if let Some(row) = row {
            let name: String = row.try_get("name")?;
            let chrom: String = row.try_get("chrom")?;
            let strand_str: String = row.try_get("strand")?;
            let tx_start: u32 = row.try_get("txStart")?;
            let tx_end: u32 = row.try_get("txEnd")?;
            let cds_start: u32 = row.try_get("cdsStart")?;
            let cds_end: u32 = row.try_get("cdsEnd")?;
            let name2: String = row.try_get("name2")?;
            let exon_starts_blob: Vec<u8> = row.try_get("exonStarts")?;
            let exon_ends_blob: Vec<u8> = row.try_get("exonEnds")?;

            // USCS coordinates are 0-based, half-open
            // https://genome-blog.gi.ucsc.edu/blog/2016/12/12/the-ucsc-genome-browser-coordinate-counting-systems/

            Ok(Some(Gene {
                id: name,
                name: name2,
                strand: Strand::from_str(strand_str).unwrap(),
                contig: Contig::chrom(&chrom),
                transcription_start: tx_start as usize + 1,
                transcription_end: tx_end as usize,
                cds_start: cds_start as usize + 1,
                cds_end: cds_end as usize,
                exon_starts: parse_blob_to_coords(&exon_starts_blob)
                    .iter()
                    .map(|v| v + 1)
                    .collect(),
                exon_ends: parse_blob_to_coords(&exon_ends_blob),
            }))
        } else {
            Ok(None)
        }
    }

    async fn query_gene_name(&self, gene_id: &String) -> Result<Gene, TGVError> {
        let row = sqlx::query(
            "SELECT name, name2, strand, chrom, txStart, txEnd, exonStarts, exonEnds, cdsStart, cdsEnd
            FROM ncbiRefSeqSelect 
            WHERE name2 = ?",
        )
        .bind(gene_id)
        .fetch_optional(&*self.pool)
        .await?;

        if let Some(row) = row {
            // USCS coordinates are -1-based, half-open
            // https://genome-blog.gi.ucsc.edu/blog/2015/12/12/the-ucsc-genome-browser-coordinate-counting-systems/

            let name: String = row.try_get("name")?;
            let name2: String = row.try_get("name2")?;
            let strand_str: String = row.try_get("strand")?;
            let chrom: String = row.try_get("chrom")?;
            let tx_start: u32 = row.try_get("txStart")?;
            let tx_end: u32 = row.try_get("txEnd")?;
            let exon_starts_blob: Vec<u8> = row.try_get("exonStarts")?;
            let exon_ends_blob: Vec<u8> = row.try_get("exonEnds")?;
            let cds_start: u32 = row.try_get("cdsStart")?;
            let cds_end: u32 = row.try_get("cdsEnd")?;

            // USCS coordinates are -1-based, half-open
            // https://genome-blog.gi.ucsc.edu/blog/2015/12/12/the-ucsc-genome-browser-coordinate-counting-systems/

            Ok(Gene {
                id: name,
                name: name2,
                strand: Strand::from_str(strand_str).unwrap(),
                contig: Contig::chrom(&chrom),
                transcription_start: tx_start as usize + 1,
                transcription_end: tx_end as usize,
                cds_start: cds_start as usize + 1,
                cds_end: cds_end as usize,
                exon_starts: parse_blob_to_coords(&exon_starts_blob)
                    .iter()
                    .map(|v| v + 1)
                    .collect(),
                exon_ends: parse_blob_to_coords(&exon_ends_blob),
            })
        } else {
            Err(TGVError::IOError(format!(
                "Failed to query gene: {}",
                gene_id
            )))
        }
    }

    async fn query_k_genes_after(
        &self,
        contig: &Contig,
        coord: usize,
        k: usize,
    ) -> Result<Gene, TGVError> {
        if k == 0 {
            return Err(TGVError::ValueError("k cannot be 0".to_string()));
        }

        let rows = sqlx::query(
            "SELECT name, chrom, strand, txStart, txEnd, name2, exonStarts, exonEnds, cdsStart, cdsEnd
             FROM ncbiRefSeqSelect 
             WHERE chrom = ? AND txEnd >= ? 
             ORDER BY txEnd ASC LIMIT ?",
        )
        .bind(contig.full_name())
        .bind(u32::try_from(coord).unwrap()) // coord is 1-based inclusive, UCSC is 0-based exclusive
        .bind(u32::try_from(k+1).unwrap())
        .fetch_all(&*self.pool)
        .await?;

        if rows.is_empty() {
            return Err(TGVError::IOError("No genes found".to_string()));
        }

        let mut genes = Vec::new();

        for row in rows {
            let name: String = row.try_get("name")?;
            let chrom: String = row.try_get("chrom")?;
            let strand_str: String = row.try_get("strand")?;
            let tx_start: u32 = row.try_get("txStart")?;
            let tx_end: u32 = row.try_get("txEnd")?;
            let name2: String = row.try_get("name2")?;
            let exon_starts_blob: Vec<u8> = row.try_get("exonStarts")?;
            let exon_ends_blob: Vec<u8> = row.try_get("exonEnds")?;
            let cds_start: u32 = row.try_get("cdsStart")?;
            let cds_end: u32 = row.try_get("cdsEnd")?;

            // USCS coordinates are 0-based, half-open
            // https://genome-blog.gi.ucsc.edu/blog/2016/12/12/the-ucsc-genome-browser-coordinate-counting-systems/

            let gene = Gene {
                id: name,
                name: name2,
                strand: Strand::from_str(strand_str).unwrap(),
                contig: Contig::chrom(&chrom),
                transcription_start: tx_start as usize + 1,
                transcription_end: tx_end as usize,
                cds_start: cds_start as usize + 1,
                cds_end: cds_end as usize,
                exon_starts: parse_blob_to_coords(&exon_starts_blob)
                    .iter()
                    .map(|v| v + 1)
                    .collect(),
                exon_ends: parse_blob_to_coords(&exon_ends_blob),
            };

            genes.push(gene);
        }

        let track = Track::from_genes(genes, contig.clone())?;

        track
            .get_saturating_k_genes_after(coord, k)
            .cloned()
            .ok_or(TGVError::IOError("No genes found".to_string()))
    }

    async fn query_k_genes_before(
        &self,
        contig: &Contig,
        coord: usize,
        k: usize,
    ) -> Result<Gene, TGVError> {
        if k == 0 {
            return Err(TGVError::ValueError("k cannot be 0".to_string()));
        }

        let rows = sqlx::query(
            "SELECT name, chrom, strand, txStart, txEnd, name2, exonStarts, exonEnds, cdsStart, cdsEnd
             FROM ncbiRefSeqSelect 
             WHERE chrom = ? AND txStart <= ? 
             ORDER BY txStart DESC LIMIT ?",
        )
        .bind(contig.full_name())
        .bind(u32::try_from(coord.saturating_sub(1)).unwrap()) // coord is 1-based inclusive, UCSC is 0-based inclusive
        .bind(u32::try_from(k+1).unwrap())
        .fetch_all(&*self.pool)
        .await?;

        if rows.is_empty() {
            return Err(TGVError::IOError("No genes found".to_string()));
        }

        let mut genes = Vec::new();

        for row in rows {
            let name: String = row.try_get("name")?;
            let chrom: String = row.try_get("chrom")?;
            let strand_str: String = row.try_get("strand")?;
            let tx_start: u32 = row.try_get("txStart")?;
            let tx_end: u32 = row.try_get("txEnd")?;
            let name2: String = row.try_get("name2")?;
            let exon_starts_blob: Vec<u8> = row.try_get("exonStarts")?;
            let exon_ends_blob: Vec<u8> = row.try_get("exonEnds")?;
            let cds_start: u32 = row.try_get("cdsStart")?;
            let cds_end: u32 = row.try_get("cdsEnd")?;

            // USCS coordinates are 0-based, half-open
            // https://genome-blog.gi.ucsc.edu/blog/2016/12/12/the-ucsc-genome-browser-coordinate-counting-systems/

            let gene = Gene {
                id: name,
                name: name2,
                strand: Strand::from_str(strand_str).unwrap(),
                contig: Contig::chrom(&chrom),
                transcription_start: tx_start as usize + 1,
                transcription_end: tx_end as usize,
                cds_start: cds_start as usize + 1,
                cds_end: cds_end as usize,
                exon_starts: parse_blob_to_coords(&exon_starts_blob)
                    .iter()
                    .map(|v| v + 1)
                    .collect(),
                exon_ends: parse_blob_to_coords(&exon_ends_blob),
            };

            genes.push(gene);
        }

        let track = Track::from_genes(genes, contig.clone())?;

        track
            .get_saturating_k_genes_before(coord, k)
            .cloned()
            .ok_or(TGVError::IOError("No genes found".to_string()))
    }

    async fn query_k_exons_after(
        &self,
        contig: &Contig,
        coord: usize,
        k: usize,
    ) -> Result<SubGeneFeature, TGVError> {
        if k == 0 {
            return Err(TGVError::ValueError("k cannot be 0".to_string()));
        }

        let rows = sqlx::query(
            "SELECT name, chrom, strand, txStart, txEnd, name2, exonStarts, exonEnds, cdsStart, cdsEnd
             FROM ncbiRefSeqSelect 
             WHERE chrom = ? AND txEnd >= ? 
             ORDER BY txEnd ASC LIMIT ?",
        )
        .bind(contig.full_name())
        .bind(u32::try_from(coord).unwrap()) // coord is 1-based inclusive, UCSC is 0-based exclusive
        .bind(u32::try_from(k+1).unwrap())
        .fetch_all(&*self.pool)
        .await?;

        let mut genes = Vec::new();
        for row in rows {
            let name: String = row.try_get("name")?;
            let chrom: String = row.try_get("chrom")?;
            let strand_str: String = row.try_get("strand")?;
            let tx_start: u32 = row.try_get("txStart")?;
            let tx_end: u32 = row.try_get("txEnd")?;
            let name2: String = row.try_get("name2")?;
            let exon_starts_blob: Vec<u8> = row.try_get("exonStarts")?;
            let exon_ends_blob: Vec<u8> = row.try_get("exonEnds")?;
            let cds_start: u32 = row.try_get("cdsStart")?;
            let cds_end: u32 = row.try_get("cdsEnd")?;

            // USCS coordinates are 0-based, half-open
            // https://genome-blog.gi.ucsc.edu/blog/2016/12/12/the-ucsc-genome-browser-coordinate-counting-systems/

            let gene = Gene {
                id: name,
                name: name2,
                strand: Strand::from_str(strand_str).unwrap(),
                contig: Contig::chrom(&chrom),
                transcription_start: tx_start as usize + 1,
                transcription_end: tx_end as usize,
                cds_start: cds_start as usize + 1,
                cds_end: cds_end as usize,
                exon_starts: parse_blob_to_coords(&exon_starts_blob)
                    .iter()
                    .map(|v| v + 1)
                    .collect(),
                exon_ends: parse_blob_to_coords(&exon_ends_blob),
            };

            genes.push(gene);
        }

        let track = Track::from_genes(genes, contig.clone())?;

        track
            .get_saturating_k_exons_after(coord, k)
            .ok_or(TGVError::IOError("No exons found".to_string()))
    }

    async fn query_k_exons_before(
        &self,
        contig: &Contig,
        coord: usize,
        k: usize,
    ) -> Result<SubGeneFeature, TGVError> {
        if k == 0 {
            return Err(TGVError::ValueError("k cannot be 0".to_string()));
        }

        let rows = sqlx::query(
            "SELECT name, chrom, strand, txStart, txEnd, name2, exonStarts, exonEnds, cdsStart, cdsEnd
             FROM ncbiRefSeqSelect 
             WHERE chrom = ? AND txStart <= ? 
             ORDER BY txStart DESC LIMIT ?",
        )
        .bind(contig.full_name())
        .bind(u32::try_from(coord.saturating_sub(1)).unwrap()) // coord is 1-based inclusive, UCSC is 0-based inclusive
        .bind(u32::try_from(k+1).unwrap())
        .fetch_all(&*self.pool)
        .await?;

        let mut genes = Vec::new();
        for row in rows {
            let name: String = row.try_get("name")?;
            let chrom: String = row.try_get("chrom")?;
            let strand_str: String = row.try_get("strand")?;
            let tx_start: u32 = row.try_get("txStart")?;
            let tx_end: u32 = row.try_get("txEnd")?;
            let name2: String = row.try_get("name2")?;
            let exon_starts_blob: Vec<u8> = row.try_get("exonStarts")?;
            let exon_ends_blob: Vec<u8> = row.try_get("exonEnds")?;
            let cds_start: u32 = row.try_get("cdsStart")?;
            let cds_end: u32 = row.try_get("cdsEnd")?;

            // USCS coordinates are 0-based, half-open
            // https://genome-blog.gi.ucsc.edu/blog/2016/12/12/the-ucsc-genome-browser-coordinate-counting-systems/

            let gene = Gene {
                id: name,
                name: name2,
                strand: Strand::from_str(strand_str).unwrap(),
                contig: Contig::chrom(&chrom),
                transcription_start: tx_start as usize + 1,
                transcription_end: tx_end as usize,
                cds_start: cds_start as usize + 1,
                cds_end: cds_end as usize,
                exon_starts: parse_blob_to_coords(&exon_starts_blob)
                    .iter()
                    .map(|v| v + 1)
                    .collect(),
                exon_ends: parse_blob_to_coords(&exon_ends_blob),
            };

            genes.push(gene);
        }

        let track = Track::from_genes(genes, contig.clone())?;

        track
            .get_saturating_k_exons_before(coord, k)
            .ok_or(TGVError::IOError("No exons found".to_string()))
    }
}

// Helper function to parse BLOB of comma-separated coordinates
fn parse_blob_to_coords(blob: &[u8]) -> Vec<usize> {
    let coords_str = String::from_utf8_lossy(blob);
    coords_str
        .trim_end_matches(',')
        .split(',')
        .filter_map(|s| s.parse::<usize>().ok())
        .collect()
}

// TODO: improved pattern:
// Service doesn't save anything. No reference, no cache.
// Ask these things to be passed in. And return them to store in the state.

#[derive(Debug)]
pub struct UcscApiTrackService {
    client: Client,
}

impl UcscApiTrackService {
    pub fn new() -> Result<Self, TGVError> {
        Ok(Self {
            client: Client::new(),
        })
    }

    /// Query the API to download the gene track data for a contig.
    pub async fn query_track_by_contig(
        &self,
        reference: &Reference,
        contig: &Contig,
    ) -> Result<Track<Gene>, TGVError> {
        let preferred_track =
            self.get_prefered_track_name(reference)
                .await?
                .ok_or(TGVError::IOError(format!(
                    "Failed to get prefered track from UCSC API"
                )))?; // TODO: proper handling

        let query_url = self.get_track_data_url(reference, contig, preferred_track.clone())?;
        let response = self.client.get(query_url).send().await?.text().await?;
        let mut value: serde_json::Value = serde_json::from_str(&response)?;
        let mut genes: Vec<Gene> = serde_json::from_value(value[preferred_track].take())?;

        for gene in genes.iter_mut() {
            gene.contig = contig.clone();
        }

        Track::from_genes(genes, contig.clone())
    }

    // /// Check if the contig's track is already cached. If not, load it.
    // /// Return true if loading is performed.
    // pub async fn check_or_load_contig_to_cache(
    //     &self,
    //     reference: &Reference,
    //     contig: &Contig,
    //     cache: &mut TrackCache,
    // ) -> Result<bool, TGVError> {
    //     if cache.queried_contig(contig) {
    //         return Ok(false);
    //     }

    //

    //     cache.track_by_contig.insert(contig.full_name(), track);

    //     Ok(true)
    // }

    // /// Load all contigs until the gene is found.
    // pub async fn check_or_load_gene(
    //     &self,
    //     reference: &Reference,
    //     gene_name: &String,
    //     cache: &mut TrackCache,
    // ) -> Result<bool, TGVError> {
    //     if self.gene_name_lookup.contains_key(gene_name) {
    //         return Ok(false);
    //     }

    //     for (contig, _) in self.get_all_contigs(reference).await?.iter() {
    //         self.check_or_load_contig(reference, contig).await?;
    //         if self.gene_name_lookup.contains_key(gene_name) {
    //             return Ok(true);
    //         }
    //     }

    //     Err(TGVError::IOError(format!(
    //         "Gene {} not found in contigs",
    //         gene_name
    //     )))
    // }

    /// Return

    const CYTOBAND_TRACK: &str = "cytoBandIdeo";

    fn get_track_data_url(
        &self,
        reference: &Reference,
        contig: &Contig,
        track_name: String,
    ) -> Result<String, TGVError> {
        match reference {
            Reference::Hg19 | Reference::Hg38 | Reference::UcscGenome(_) => Ok(format!(
                "https://api.genome.ucsc.edu/getData/track?genome={}&track={}&chrom={}",
                reference.to_string(),
                track_name,
                contig.full_name()
            )),
            _ => Err(TGVError::IOError("Unsupported reference".to_string())),
        }
    }
}

/// Get the preferred track name recursively.
fn get_prefered_track_name(
    key: &str,
    content: &serde_json::Value,
    preferred_track_name: Option<String>,
    is_top_level: bool,
) -> Result<Option<String>, TGVError> {
    let mut prefered_track_name = preferred_track_name;

    let err = TGVError::IOError(format!("Failed to get genome from UCSC API"));

    if content.get("compositeContainer").is_some() || is_top_level {
        // do this recursively
        for (track_name, track_content) in content.as_object().ok_or(err)?.iter() {
            if track_content.is_object() {
                prefered_track_name =
                    get_prefered_track_name(track_name, track_content, prefered_track_name, false)?;
            }
        }
    } else {
        if prefered_track_name == Some("ncbiRefSeqSelect".to_string()) {
        } else if key == "ncbiRefSeqSelect".to_string() {
            prefered_track_name = Some(key.to_string());
        } else if key == "ncbiRefSeqCurated".to_string() {
            prefered_track_name = Some(key.to_string());
        } else if *key == "ncbiRefSeq".to_string()
            && prefered_track_name != Some("ncbiRefSeqCurated".to_string())
        {
            prefered_track_name = Some(key.to_string());
        } else if *key == "refGene".to_string()
            && (prefered_track_name != Some("ncbiRefSeqCurated".to_string())
                && prefered_track_name != Some("ncbiRefSeq".to_string()))
        {
            prefered_track_name = Some(key.to_string());
        }
    }

    Ok(prefered_track_name)
}

#[async_trait]
impl TrackService for UcscApiTrackService {
    async fn close(&self) -> Result<(), TGVError> {
        // reqwest client dones't need closing
        Ok(())
    }

    async fn get_all_contigs(
        &self,
        reference: &Reference,
    ) -> Result<Vec<(Contig, usize)>, TGVError> {
        match reference {
            Reference::Hg19 | Reference::Hg38 => reference.contigs_and_lengths(), // TODO: query
            Reference::UcscGenome(genome) => {
                let query_url = format!(
                    "https://api.genome.ucsc.edu/list/chromosomes?genome={}",
                    genome
                );

                let response = self.client.get(query_url).send().await?.text().await?;

                let err =
                    TGVError::IOError(format!("Failed to deserialize chromosomes for {}", genome));

                // schema: {..., "chromosomes": [{"__name__", len}]}

                let value: serde_json::Value = serde_json::from_str(&response)?;

                let mut output = Vec::new();
                for (k, v) in value["chromosomes"].as_object().ok_or(err)?.iter() {
                    // TODO: save length
                    output.push((Contig::chrom(k), v.as_u64().unwrap() as usize));
                }

                Ok(output)
            }
            _ => Err(TGVError::IOError("Unsupported reference".to_string())),
        }
    }

    async fn get_cytoband(
        &self,
        reference: &Reference,
        contig: &Contig,
    ) -> Result<Option<Cytoband>, TGVError> {
        let query_url = format!(
            "https://api.genome.ucsc.edu/getData/track?genome={}&track={}&chrom={}",
            reference.to_string(),
            Self::CYTOBAND_TRACK,
            contig.full_name()
        );

        let response = self.client.get(query_url).send().await?;

        if response.status() != StatusCode::OK {
            return Ok(None); // Some genome doesn't have cytobands
        }

        let response_text = response.text().await?;
        let value: serde_json::Value =
            serde_json::from_str(&response_text).map_err(TGVError::JsonSerializationError)?;

        // Extract the array of segments from the "cytoBandIdeo" field
        let segments_value = value.get(Self::CYTOBAND_TRACK).ok_or_else(|| {
            TGVError::JsonSerializationError(serde_json::Error::custom(format!(
                "Missing '{}' field in UCSC API response",
                Self::CYTOBAND_TRACK
            )))
        })?;

        // Deserialize the segments array
        let segments: Vec<CytobandSegment> = serde_json::from_value(segments_value.clone())
            .map_err(TGVError::JsonSerializationError)?;

        if segments.is_empty() {
            return Ok(None);
        }

        // Construct the *single* Cytoband object for this contig
        let cytoband = Cytoband {
            reference: Some(reference.clone()),
            contig: contig.clone(),
            segments,
        };

        // Return the single Cytoband wrapped in Option
        Ok(Some(cytoband))
    }

    async fn get_prefered_track_name(
        &self,
        reference: &Reference,
    ) -> Result<Option<String>, TGVError> {
        match reference.clone() {
            Reference::Hg19 | Reference::Hg38 => Ok(Some("ncbiRefSeqSelect".to_string())),
            Reference::UcscGenome(genome) => {
                let query_url =
                    format!("https://api.genome.ucsc.edu/list/tracks?genome={}", genome);
                let response = reqwest::get(query_url)
                    .await?
                    .json::<serde_json::Value>()
                    .await?;

                let prefered_track = get_prefered_track_name(
                    genome.as_str(),
                    response
                        .get(genome.clone())
                        .ok_or(TGVError::IOError(format!(
                            "Failed to get genome from UCSC API"
                        )))?,
                    None,
                    true,
                )?;

                Ok(prefered_track)
            }
            _ => Err(TGVError::IOError(
                "Failed to get prefered track from UCSC API".to_string(),
            )),
        }
    }

    async fn query_genes_overlapping(
        &self,
        reference: &Reference,
        region: &Region,
        cache: &mut TrackCache,
    ) -> Result<Vec<Gene>, TGVError> {
        if !cache.includes_contig(&region.contig()) {
            let track = self
                .query_track_by_contig(reference, &region.contig())
                .await?;
            cache.add_track(region.contig(), track);
        }

        if let Some(track) = self.cached_tracks.get(&region.contig().full_name()) {
            Ok(track
                .get_features_overlapping(region)
                .iter()
                .map(|g| (*g).clone())
                .collect())
        } else {
            Err(TGVError::IOError(format!(
                "Track not found for contig {}",
                region.contig().full_name()
            )))
        }
    }

    async fn query_gene_covering(
        &self,
        reference: &Reference,
        contig: &Contig,
        position: usize,
    ) -> Result<Option<Gene>, TGVError> {
        if let Some(track) = self.cached_tracks.get(&contig.full_name()) {
            Ok(track.get_gene_at(position).map(|g| (*g).clone()))
        } else {
            Err(TGVError::IOError(format!(
                "Track not found for contig {}",
                contig.full_name()
            )))
        }
    }

    async fn query_gene_name(&self, name: &String) -> Result<Gene, TGVError> {
        if let Some((contig, gene_index)) = self.gene_name_lookup.get(name) {
            return Ok(self
                .cached_tracks
                .get(&contig.full_name())
                .unwrap() // should never error out
                .genes()[*gene_index]
                .clone());
        } else {
            Err(TGVError::IOError("Gene not found".to_string()))
        }
    }

    async fn query_k_genes_after(
        &self,
        contig: &Contig,
        position: usize,
        k: usize,
    ) -> Result<Gene, TGVError> {
        if let Some(track) = self.cached_tracks.get(&contig.full_name()) {
            Ok(track
                .get_saturating_k_genes_after(position, k)
                .ok_or(TGVError::IOError("No genes found".to_string()))?
                .clone())
        } else {
            Err(TGVError::IOError(format!(
                "Track not found for contig {}",
                contig.full_name()
            )))
        }
    }

    async fn query_k_genes_before(
        &self,
        contig: &Contig,
        position: usize,
        k: usize,
    ) -> Result<Gene, TGVError> {
        if let Some(track) = self.cached_tracks.get(&contig.full_name()) {
            Ok(track
                .get_saturating_k_genes_before(position, k)
                .ok_or(TGVError::IOError("No genes found".to_string()))?
                .clone())
        } else {
            Err(TGVError::IOError(format!(
                "Track not found for contig {}",
                contig.full_name()
            )))
        }
    }

    async fn query_k_exons_after(
        &self,
        contig: &Contig,
        position: usize,
        k: usize,
    ) -> Result<SubGeneFeature, TGVError> {
        if let Some(track) = self.cached_tracks.get(&contig.full_name()) {
            Ok(track
                .get_saturating_k_exons_after(position, k)
                .ok_or(TGVError::IOError("No exons found".to_string()))?)
        } else {
            Err(TGVError::IOError(format!(
                "Track not found for contig {}",
                contig.full_name()
            )))
        }
    }

    async fn query_k_exons_before(
        &self,
        contig: &Contig,
        position: usize,
        k: usize,
    ) -> Result<SubGeneFeature, TGVError> {
        if let Some(track) = self.cached_tracks.get(&contig.full_name()) {
            Ok(track
                .get_saturating_k_exons_before(position, k)
                .ok_or(TGVError::IOError("No exons found".to_string()))?)
        } else {
            Err(TGVError::IOError(format!(
                "Track not found for contig {}",
                contig.full_name()
            )))
        }
    }
}

// --- Enum Wrapper ---

/// Enum to hold different TrackService implementations
#[derive(Debug)]
pub enum TrackServiceEnum {
    Api(UcscApiTrackService),
    Db(UcscDbTrackService),
}

// Implement TrackService for the enum, dispatching calls
#[async_trait]
impl TrackService for TrackServiceEnum {
    async fn close(&self) -> Result<(), TGVError> {
        match self {
            TrackServiceEnum::Api(service) => service.close().await,
            TrackServiceEnum::Db(service) => service.close().await,
        }
    }

    async fn get_all_contigs(
        &self,
        reference: &Reference,
    ) -> Result<Vec<(Contig, usize)>, TGVError> {
        match self {
            TrackServiceEnum::Api(service) => service.get_all_contigs(reference).await,
            TrackServiceEnum::Db(service) => service.get_all_contigs(reference).await,
        }
    }

    async fn get_cytoband(
        &self,
        reference: &Reference,
        contig: &Contig,
    ) -> Result<Option<Cytoband>, TGVError> {
        match self {
            TrackServiceEnum::Api(service) => service.get_cytoband(reference, contig).await,
            TrackServiceEnum::Db(service) => service.get_cytoband(reference, contig).await,
        }
    }

    async fn get_prefered_track_name(
        &self,
        reference: &Reference,
    ) -> Result<Option<String>, TGVError> {
        match self {
            TrackServiceEnum::Api(service) => service.get_prefered_track_name(reference).await,
            TrackServiceEnum::Db(service) => service.get_prefered_track_name(reference).await,
        }
    }

    async fn query_genes_overlapping(&self, region: &Region) -> Result<Vec<Gene>, TGVError> {
        match self {
            TrackServiceEnum::Api(service) => service.query_genes_overlapping(region).await,
            TrackServiceEnum::Db(service) => service.query_genes_overlapping(region).await,
        }
    }

    async fn query_gene_covering(
        &self,
        contig: &Contig,
        coord: usize,
    ) -> Result<Option<Gene>, TGVError> {
        match self {
            TrackServiceEnum::Api(service) => service.query_gene_covering(contig, coord).await,
            TrackServiceEnum::Db(service) => service.query_gene_covering(contig, coord).await,
        }
    }

    async fn query_gene_name(&self, gene_id: &String) -> Result<Gene, TGVError> {
        match self {
            TrackServiceEnum::Api(service) => service.query_gene_name(gene_id).await,
            TrackServiceEnum::Db(service) => service.query_gene_name(gene_id).await,
        }
    }

    async fn query_k_genes_after(
        &self,
        contig: &Contig,
        coord: usize,
        k: usize,
    ) -> Result<Gene, TGVError> {
        match self {
            TrackServiceEnum::Api(service) => service.query_k_genes_after(contig, coord, k).await,
            TrackServiceEnum::Db(service) => service.query_k_genes_after(contig, coord, k).await,
        }
    }

    async fn query_k_genes_before(
        &self,
        contig: &Contig,
        coord: usize,
        k: usize,
    ) -> Result<Gene, TGVError> {
        match self {
            TrackServiceEnum::Api(service) => service.query_k_genes_before(contig, coord, k).await,
            TrackServiceEnum::Db(service) => service.query_k_genes_before(contig, coord, k).await,
        }
    }

    async fn query_k_exons_after(
        &self,
        contig: &Contig,
        coord: usize,
        k: usize,
    ) -> Result<SubGeneFeature, TGVError> {
        match self {
            TrackServiceEnum::Api(service) => service.query_k_exons_after(contig, coord, k).await,
            TrackServiceEnum::Db(service) => service.query_k_exons_after(contig, coord, k).await,
        }
    }

    async fn query_k_exons_before(
        &self,
        contig: &Contig,
        coord: usize,
        k: usize,
    ) -> Result<SubGeneFeature, TGVError> {
        match self {
            TrackServiceEnum::Api(service) => service.query_k_exons_before(contig, coord, k).await,
            TrackServiceEnum::Db(service) => service.query_k_exons_before(contig, coord, k).await,
        }
    }

    // Default helper methods delegate
    async fn query_gene_track(&self, region: &Region) -> Result<Track<Gene>, TGVError> {
        match self {
            TrackServiceEnum::Api(service) => service.query_gene_track(region).await,
            TrackServiceEnum::Db(service) => service.query_gene_track(region).await,
        }
    }
}

// NOTE: Default methods `check_or_load_contig` and `check_or_load_gene`
// are not reimplemented here as they were likely intended to be part of the
// concrete types' logic, not the trait itself if using `&mut self`.
// If they were meant to be dispatched, they would need `&mut self` in the enum impl too.

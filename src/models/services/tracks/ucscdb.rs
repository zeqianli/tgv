use crate::error::TGVError;
use crate::models::{
    contig::Contig,
    cytoband::{Cytoband, CytobandSegment, Stain},
    reference::Reference,
    region::Region,
    services::tracks::TrackService,
    strand::Strand,
    track::{
        feature::{Gene, SubGeneFeature, SubGeneFeatureType},
        track::Track,
    },
};
use sqlx::{mysql::MySqlPoolOptions, MySqlPool, Row};
use std::sync::Arc;

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

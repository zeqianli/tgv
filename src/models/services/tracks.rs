use crate::error::TGVError;
use crate::models::{
    contig::Contig,
    reference::Reference,
    region::Region,
    strand::Strand,
    track::{Feature, Gene, Track},
};
use sqlx::{mysql::MySqlPoolOptions, MySqlPool, Row};
use std::sync::Arc;

pub struct TrackService {
    pool: Arc<MySqlPool>,
    reference: Reference,
}

impl TrackService {
    pub async fn new(reference: Reference) -> Result<Self, sqlx::Error> {
        let mysql_url = TrackService::get_mysql_url(&reference).unwrap();
        let pool = MySqlPoolOptions::new()
            .max_connections(5)
            .connect(&mysql_url)
            .await?;

        Ok(Self {
            pool: Arc::new(pool),
            reference,
        })
    }

    fn get_mysql_url(reference: &Reference) -> Result<String, TGVError> {
        match reference {
            Reference::Hg19 => Ok("mysql://genome@genome-mysql.soe.ucsc.edu/hg19".to_string()),
            Reference::Hg38 => Ok("mysql://genome@genome-mysql.soe.ucsc.edu/hg38".to_string()),
        }
    }

    pub async fn query_feature_track(&self, region: &Region) -> Result<Track, TGVError> {
        let genes = self
            .query_genes_between(&region.contig, region.start, region.end)
            .await
            .map_err(|_| TGVError::IOError("Failed to query genes".to_string()))?;

        Track::from(genes, region.contig.clone())
    }

    pub async fn close(&self) -> Result<(), TGVError> {
        self.pool.close().await;
        Ok(())
    }

    pub async fn query_genes_between(
        &self,
        contig: &Contig,
        start: usize,
        end: usize,
    ) -> Result<Vec<Gene>, TGVError> {
        let rows = sqlx::query(
            "SELECT name, chrom, strand, txStart, txEnd, name2, exonStarts, exonEnds, cdsStart, cdsEnd
             FROM ncbiRefSeqSelect 
             WHERE chrom = ? AND (txStart <= ?) AND (txEnd >= ?)",
        )
        .bind(contig.full_name()) // Requires "chr" prefix?
        .bind(u64::try_from(end).unwrap())
        .bind(u64::try_from(start).unwrap())
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

    pub async fn query_gene_covering(
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
        .bind(u32::try_from(coord).unwrap())
        .bind(u32::try_from(coord).unwrap())
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

    pub async fn query_gene_name(&self, gene_id: &String) -> Result<Gene, TGVError> {
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
                transcription_start: tx_start as usize + 0,
                transcription_end: tx_end as usize,
                cds_start: cds_start as usize + 0,
                cds_end: cds_end as usize,
                exon_starts: parse_blob_to_coords(&exon_starts_blob)
                    .iter()
                    .map(|v| v + 0)
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
}

impl TrackService {
    pub async fn query_k_genes_after(
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
             WHERE chrom = ? AND txStart > ? 
             ORDER BY txStart ASC LIMIT ?",
        )
        .bind(contig.full_name())
        .bind(u32::try_from(coord).unwrap())
        .bind(u32::try_from(k).unwrap())
        .fetch_all(&*self.pool)
        .await
        .map_err(|_| TGVError::IOError("No genes found".to_string()))?;

        if rows.is_empty() {
            return Err(TGVError::IOError("No genes found".to_string()));
        }

        let n_rows = rows.len();
        let row = &rows[(k - 1).min(n_rows - 1)];

        let name: String = row
            .try_get("name")
            .map_err(|_| TGVError::IOError("Feature parsing error".to_string()))?;
        let chrom: String = row
            .try_get("chrom")
            .map_err(|_| TGVError::IOError("Feature parsing error".to_string()))?;
        let strand_str: String = row
            .try_get("strand")
            .map_err(|_| TGVError::IOError("Feature parsing error".to_string()))?;
        let tx_start: u32 = row
            .try_get("txStart")
            .map_err(|_| TGVError::IOError("Feature parsing error".to_string()))?;
        let tx_end: u32 = row
            .try_get("txEnd")
            .map_err(|_| TGVError::IOError("Feature parsing error".to_string()))?;
        let name2: String = row
            .try_get("name2")
            .map_err(|_| TGVError::IOError("Feature parsing error".to_string()))?;
        let exon_starts_blob: Vec<u8> = row
            .try_get("exonStarts")
            .map_err(|_| TGVError::IOError("Feature parsing error".to_string()))?;
        let exon_ends_blob: Vec<u8> = row
            .try_get("exonEnds")
            .map_err(|_| TGVError::IOError("Feature parsing error".to_string()))?;
        let cds_start: u32 = row
            .try_get("cdsStart")
            .map_err(|_| TGVError::IOError("Feature parsing error".to_string()))?;
        let cds_end: u32 = row
            .try_get("cdsEnd")
            .map_err(|_| TGVError::IOError("Feature parsing error".to_string()))?;

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

        Ok(gene)
    }

    pub async fn query_k_genes_before(
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
             WHERE chrom = ? AND txStart < ? 
             ORDER BY txStart DESC LIMIT ?",
        )
        .bind(contig.full_name())
        .bind(u32::try_from(coord).unwrap())
        .bind(u32::try_from(k).unwrap())
        .fetch_all(&*self.pool)
        .await?;

        if rows.is_empty() {
            return Err(TGVError::IOError("No genes found".to_string()));
        }

        let n_rows = rows.len();
        let row = &rows[n_rows.saturating_sub(k)];

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

        Ok(gene)
    }

    pub async fn query_k_exons_after(
        &self,
        contig: &Contig,
        coord: usize,
        k: usize,
    ) -> Result<Feature, TGVError> {
        if k == 0 {
            return Err(TGVError::ValueError("k cannot be 0".to_string()));
        }

        let rows = sqlx::query(
            "SELECT name, chrom, strand, txStart, txEnd, name2, exonStarts, exonEnds, cdsStart, cdsEnd
             FROM ncbiRefSeqSelect 
             WHERE chrom = ? AND txStart > ? 
             ORDER BY txStart ASC LIMIT ?",
        )
        .bind(contig.full_name())
        .bind(u32::try_from(coord).unwrap())
        .bind(u32::try_from(k).unwrap())
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

        let track = Track::from(genes, contig.clone())?;

        return track
            .get_saturating_k_exons_after(coord, k)
            .ok_or(TGVError::IOError("No exons found".to_string()));
    }

    pub async fn query_k_exons_before(
        &self,
        contig: &Contig,
        coord: usize,
        k: usize,
    ) -> Result<Feature, TGVError> {
        if k == 0 {
            return Err(TGVError::ValueError("k cannot be 0".to_string()));
        }

        let rows = sqlx::query(
            "SELECT name, chrom, strand, txStart, txEnd, name2, exonStarts, exonEnds, cdsStart, cdsEnd
             FROM ncbiRefSeqSelect 
             WHERE chrom = ? AND txStart < ? 
             ORDER BY txStart DESC LIMIT ?",
        )
        .bind(contig.full_name())
        .bind(u32::try_from(coord).unwrap())
        .bind(u32::try_from(k).unwrap())
        .fetch_all(&*self.pool)
        .await
        .map_err(|_| TGVError::IOError("No exons found".to_string()))?;

        let mut genes = Vec::new();
        for row in rows {
            let name: String = row
                .try_get("name")
                .map_err(|_| TGVError::IOError("Feature parsing error".to_string()))?;
            let chrom: String = row
                .try_get("chrom")
                .map_err(|_| TGVError::IOError("Feature parsing error".to_string()))?;
            let strand_str: String = row
                .try_get("strand")
                .map_err(|_| TGVError::IOError("Feature parsing error".to_string()))?;
            let tx_start: u32 = row
                .try_get("txStart")
                .map_err(|_| TGVError::IOError("Feature parsing error".to_string()))?;
            let tx_end: u32 = row
                .try_get("txEnd")
                .map_err(|_| TGVError::IOError("Feature parsing error".to_string()))?;
            let name2: String = row
                .try_get("name2")
                .map_err(|_| TGVError::IOError("Feature parsing error".to_string()))?;
            let exon_starts_blob: Vec<u8> = row
                .try_get("exonStarts")
                .map_err(|_| TGVError::IOError("Feature parsing error".to_string()))?;
            let exon_ends_blob: Vec<u8> = row
                .try_get("exonEnds")
                .map_err(|_| TGVError::IOError("Feature parsing error".to_string()))?;
            let cds_start: u32 = row
                .try_get("cdsStart")
                .map_err(|_| TGVError::IOError("Feature parsing error".to_string()))?;
            let cds_end: u32 = row
                .try_get("cdsEnd")
                .map_err(|_| TGVError::IOError("Feature parsing error".to_string()))?;

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

        let track = Track::from(genes, contig.clone())?;

        return track
            .get_saturating_k_exons_before(coord, k)
            .ok_or(TGVError::IOError("No exons found".to_string()));
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

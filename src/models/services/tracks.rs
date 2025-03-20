use crate::models::{
    contig::Contig,
    region::Region,
    strand::Strand,
    track::{Feature, Track},
};
use sqlx::{mysql::MySqlPoolOptions, MySqlPool, Row};
use std::sync::Arc;

pub struct TrackService {
    pool: Arc<MySqlPool>,
    reference: String,
}

impl TrackService {
    pub async fn new(reference: String) -> Result<Self, sqlx::Error> {
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

    fn get_mysql_url(reference: &str) -> Result<String, ()> {
        match reference {
            "hg19" => Ok("mysql://genome@genome-mysql.soe.ucsc.edu/hg19".to_string()),
            "hg38" => Ok("mysql://genome@genome-mysql.soe.ucsc.edu/hg38".to_string()),
            _ => Err(()),
        }
    }

    pub async fn query_feature_track(&self, region: &Region) -> Result<Track, sqlx::Error> {
        let genes = self
            .query_genes_between(&region.contig, region.start, region.end)
            .await?;

        let mut features = Vec::new();
        for gene in genes {
            match gene.expand() {
                Ok(expanded) => features.extend(expanded),
                Err(_) => return Err(sqlx::Error::RowNotFound), // TODO: proper error type
            }
        }

        match Track::from(features, region.contig.clone()) {
            Ok(track) => Ok(track),
            Err(_) => Err(sqlx::Error::RowNotFound), // TODO: proper error type
        }
    }

    pub async fn query_genes_between(
        &self,
        contig: &Contig,
        start: usize,
        end: usize,
    ) -> Result<Vec<Feature>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT name, chrom, strand, txStart, txEnd, name2, exonStarts, exonEnds 
             FROM ncbiRefSeqSelect 
             WHERE chrom = ? AND (txStart <= ?) AND (txEnd >= ?)",
        )
        .bind(contig.full_name()) // Requires "chr" prefix?
        .bind(u32::try_from(end).unwrap())
        .bind(u32::try_from(start).unwrap())
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

            genes.push(Feature::Gene {
                id: name,
                name: name2,
                strand: Strand::from_str(strand_str).unwrap(),
                contig: Contig::chrom(&chrom),
                start: tx_start.try_into().unwrap(),
                end: tx_end.try_into().unwrap(),
                exon_starts: parse_blob_to_coords(&exon_starts_blob),
                exon_ends: parse_blob_to_coords(&exon_ends_blob),
            });
        }

        Ok(genes)
    }

    pub async fn query_gene_covering(
        &self,
        contig: &Contig,
        coord: usize,
    ) -> Result<Option<Feature>, sqlx::Error> {
        let row = sqlx::query(
            "SELECT name, chrom, strand, txStart, txEnd, name2, exonStarts, exonEnds 
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
            let name2: String = row.try_get("name2")?;
            let exon_starts_blob: Vec<u8> = row.try_get("exonStarts")?;
            let exon_ends_blob: Vec<u8> = row.try_get("exonEnds")?;

            Ok(Some(Feature::Gene {
                id: name,
                name: name2,
                strand: Strand::from_str(strand_str).unwrap(),
                contig: Contig::chrom(&chrom),
                start: tx_start.try_into().unwrap(),
                end: tx_end.try_into().unwrap(),
                exon_starts: parse_blob_to_coords(&exon_starts_blob),
                exon_ends: parse_blob_to_coords(&exon_ends_blob),
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn query_genes_after(
        &self,
        contig: &Contig,
        coord: usize,
        k: usize,
    ) -> Result<Vec<Feature>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT name, chrom, strand, txStart, txEnd, name2, exonStarts, exonEnds 
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

            genes.push(Feature::Gene {
                id: name,
                name: name2,
                strand: Strand::from_str(strand_str).unwrap(),
                contig: Contig::chrom(&chrom),
                start: tx_start.try_into().unwrap(),
                end: tx_end.try_into().unwrap(),
                exon_starts: parse_blob_to_coords(&exon_starts_blob),
                exon_ends: parse_blob_to_coords(&exon_ends_blob),
            });
        }

        Ok(genes)
    }

    pub async fn query_genes_before(
        &self,
        contig: &Contig,
        coord: usize,
        k: usize,
    ) -> Result<Vec<Feature>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT name, chrom, strand, txStart, txEnd, name2, exonStarts, exonEnds 
             FROM ncbiRefSeqSelect 
             WHERE chrom = ? AND txStart < ? 
             ORDER BY txStart DESC LIMIT ?",
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

            genes.push(Feature::Gene {
                id: name,
                name: name2,
                strand: Strand::from_str(strand_str).unwrap(),
                contig: Contig::chrom(&chrom),
                start: tx_start.try_into().unwrap(),
                end: tx_end.try_into().unwrap(),
                exon_starts: parse_blob_to_coords(&exon_starts_blob),
                exon_ends: parse_blob_to_coords(&exon_ends_blob),
            });
        }

        Ok(genes)
    }

    pub async fn query_gene_name(&self, gene_id: &String) -> Result<Feature, sqlx::Error> {
        let row = sqlx::query(
            "SELECT name, name2, strand, chrom, txStart, txEnd, exonStarts, exonEnds 
            FROM ncbiRefSeqSelect 
            WHERE name2 = ?",
        )
        .bind(gene_id)
        .fetch_optional(&*self.pool)
        .await?;

        if let Some(row) = row {
            let name: String = row.try_get("name")?;
            let name2: String = row.try_get("name2")?;
            let strand_str: String = row.try_get("strand")?;
            let chrom: String = row.try_get("chrom")?;
            let tx_start: u32 = row.try_get("txStart")?;
            let tx_end: u32 = row.try_get("txEnd")?;
            let exon_starts_blob: Vec<u8> = row.try_get("exonStarts")?;
            let exon_ends_blob: Vec<u8> = row.try_get("exonEnds")?;

            Ok(Feature::Gene {
                id: name,
                name: name2,
                strand: Strand::from_str(strand_str).unwrap(),
                contig: Contig::chrom(&chrom),
                start: tx_start.try_into().unwrap(),
                end: tx_end.try_into().unwrap(),
                exon_starts: parse_blob_to_coords(&exon_starts_blob),
                exon_ends: parse_blob_to_coords(&exon_ends_blob),
            })
        } else {
            Err(sqlx::Error::RowNotFound)
        }
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

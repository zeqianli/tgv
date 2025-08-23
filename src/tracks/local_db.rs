use crate::tracks::{TrackCache, TrackService, TRACK_PREFERENCES};
use crate::{
    contig_header::{Contig, ContigHeader},
    cytoband::{Cytoband, CytobandSegment},
    error::TGVError,
    feature::{Gene, SubGeneFeature},
    intervals::GenomeInterval,
    reference::Reference,
    region::Region,
    track::Track,
    tracks::schema::*,
};
use async_trait::async_trait;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions},
    Column, Row,
};
use std::collections::HashMap;
use std::sync::Arc;

/// Local database track service that reads from SQLite files created by UcscDownloader
#[derive(Debug)]
pub struct LocalDbTrackService {
    pool: Arc<SqlitePool>,
}

impl LocalDbTrackService {
    /// Initialize the database connections using SQLite cache files
    pub async fn new(reference: &Reference, cache_dir: &str) -> Result<Self, TGVError> {
        let expanded_cache_dir = shellexpand::full(cache_dir).map_err(|e| {
            TGVError::ValueError(format!("Failed to expand cache directory path: {}", e))
        })?;

        let db_path = std::path::Path::new(expanded_cache_dir.as_ref())
            .join(reference.to_string())
            .join("tracks.sqlite");

        if !db_path.exists() {
            return Err(TGVError::IOError(format!(
                "SQLite cache not found at {}. Please run download command first.",
                db_path.display()
            )));
        }

        let options = SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(false);

        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;

        Ok(Self {
            pool: Arc::new(pool),
        })
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

    async fn get_preferred_track_name_with_cache(
        &self,
        reference: &Reference,
        cache: &mut TrackCache,
    ) -> Result<String, TGVError> {
        match &cache.preferred_track_name {
            None => {
                let preferred_track = self.get_preferred_track_name(reference, cache).await?;
                cache.set_preferred_track_name(preferred_track.clone());
                preferred_track
            }
            Some(track) => track.clone(),
        }
        .ok_or(TGVError::IOError("No preferred track found".to_string()))
    }

    /// chrom name -> 2bit file name.
    /// Used for initailzing the local cache service.
    pub async fn get_contig_2bit_file_lookup(
        &self,
        reference: &Reference,
        contig_header: &ContigHeader,
    ) -> Result<HashMap<usize, Option<String>>, TGVError> {
        let rows_with_alias = sqlx::query(
            "SELECT chrom, fileName FROM chromInfo WHERE chrom NOT LIKE 'chr%\\_%' ESCAPE '\\'",
        )
        .fetch_all(&*self.pool)
        .await?;

        let mut filename_hashmap: HashMap<usize, Option<String>> = HashMap::new();
        for row in rows_with_alias {
            let chrom: String = row.try_get("chrom")?;
            let file_name: String = row.try_get("fileName")?;

            let basename = if file_name.trim().is_empty() {
                None
            } else {
                Some(
                    file_name
                        .split("/")
                        .last()
                        .ok_or(TGVError::IOError(
                            "Failed to get basename from file name".to_string(),
                        ))?
                        .to_string(),
                )
            };

            filename_hashmap.insert(contig_header.get_index_by_str(&chrom)?, basename);
        }

        Ok(filename_hashmap)
    }
}

#[async_trait]
impl TrackService for LocalDbTrackService {
    async fn close(&self) -> Result<(), TGVError> {
        self.pool.close().await;
        Ok(())
    }

    async fn get_all_contigs(
        &self,
        reference: &Reference,
        cache: &mut TrackCache,
    ) -> Result<Vec<Contig>, TGVError> {
        let contigs: Vec<ContigRow> = sqlx::query_as(
            "SELECT 
                chromInfo.chrom as chrom, 
                chromInfo.size as size,
                GROUP_CONCAT(chromAlias.alias, ',') as aliases
            FROM chromInfo 
            LEFT JOIN chromAlias ON chromAlias.chrom = chromInfo.chrom
            WHERE chromInfo.chrom NOT LIKE 'chr%\\_%' ESCAPE '\\' 
            GROUP BY chromInfo.chrom
            ORDER BY chromInfo.chrom;
            ",
        )
        .fetch_all(&*self.pool)
        .await
        .unwrap_or({
            sqlx::query_as(
                "SELECT 
                    chromInfo.chrom as chrom, 
                    chromInfo.size as size
                FROM chromInfo 
                WHERE chromInfo.chrom NOT LIKE 'chr%\\_%' ESCAPE '\\'
                ORDER BY chromInfo.chrom",
            )
            .fetch_all(&*self.pool)
            .await?
        });

        let mut contigs = contigs
            .into_iter()
            .map(|row| row.to_contig())
            .collect::<Result<Vec<Contig>, TGVError>>()?;

        contigs.sort_by(|a, b| {
            if a.name.starts_with("chr") || b.name.starts_with("chr") {
                Contig::contigs_compare(a, b)
            } else {
                b.length().cmp(&a.length()) // Sort by length in descending order
            }
        });

        Ok(contigs)
    }

    async fn get_cytoband(
        &self,
        reference: &Reference,
        contig_index: usize,
        cache: &mut TrackCache,
        contig_header: &ContigHeader,
    ) -> Result<Option<Cytoband>, TGVError> {
        let contig_name = contig_header.get_name(contig_index)?;
        let cytoband_segment_rows: Vec<CytobandSegmentRow> = sqlx::query_as(
            "SELECT chrom, chromStart, chromEnd, name, gieStain FROM cytoBandIdeo WHERE chrom = ?",
        )
        .bind(contig_name)
        .fetch_all(&*self.pool)
        .await?;

        if cytoband_segment_rows.is_empty() {
            return Ok(None);
        }

        // Cytoband table is not available.
        Ok(Some(Cytoband {
            reference: Some(reference.clone()),
            contig_index,
            segments: cytoband_segment_rows
                .into_iter()
                .map(|segment| segment.to_cytoband_segment(contig_index))
                .collect::<Result<Vec<CytobandSegment>, TGVError>>()?,
        }))
    }

    async fn get_preferred_track_name(
        &self,
        reference: &Reference,
        cache: &mut TrackCache,
    ) -> Result<Option<String>, TGVError> {
        match reference {
            // Speed up for human genomes
            Reference::Hg19 | Reference::Hg38 => return Ok(Some("ncbiRefSeqSelect".to_string())),
            _ => {}
        }

        let gene_track_rows = sqlx::query(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT IN ('chromInfo', 'chromAlias', 'cytoBandIdeo')"
        ).fetch_all(&*self.pool).await?;

        let available_gene_tracks: Vec<String> = gene_track_rows
            .into_iter()
            .map(|row| row.try_get::<String, &str>("name"))
            .collect::<Result<Vec<String>, sqlx::Error>>()?;

        for pref in TRACK_PREFERENCES {
            if available_gene_tracks.contains(&pref.to_string()) {
                return Ok(Some(pref.to_string()));
            }
        }

        Ok(None)
    }

    async fn query_genes_overlapping(
        &self,
        reference: &Reference,
        region: &Region,
        cache: &mut TrackCache,
        contig_header: &ContigHeader,
    ) -> Result<Vec<Gene>, TGVError> {
        let contig_name = contig_header.get_name(region.contig_index())?;
        let rows: Vec<UcscGeneRow> = sqlx::query_as(
            format!(
                "SELECT * FROM {} 
             WHERE chrom = ? AND (txStart <= ?) AND (txEnd >= ?)",
                self.get_preferred_track_name_with_cache(reference, cache)
                    .await?
            )
            .as_str(),
        )
        .bind(contig_name)
        .bind(region.end as i64) // end is 1-based inclusive, UCSC is 0-based exclusive
        .bind(region.start.saturating_sub(1) as i64) // start is 1-based inclusive, UCSC is 0-based inclusive
        .fetch_all(&*self.pool)
        .await?;

        rows.into_iter()
            .map(|row| row.to_gene(contig_header))
            .collect::<Result<Vec<Gene>, TGVError>>()
    }

    async fn query_gene_covering(
        &self,
        reference: &Reference,
        contig_index: usize,
        coord: usize,
        cache: &mut TrackCache,
        contig_header: &ContigHeader,
    ) -> Result<Option<Gene>, TGVError> {
        let contig_name = contig_header.get_name(contig_index)?;
        let gene_row: Option<UcscGeneRow> = sqlx::query_as(
            format!(
                "SELECT *
             FROM {} 
             WHERE chrom = ? AND txStart <= ? AND txEnd >= ?",
                self.get_preferred_track_name_with_cache(reference, cache)
                    .await?,
            )
            .as_str(),
        )
        .bind(contig_name)
        .bind(coord.saturating_sub(1) as i64) // coord is 1-based inclusive, UCSC is 0-based inclusive
        .bind(coord as i64) // coord is 1-based inclusive, UCSC is 0-based exclusive
        .fetch_optional(&*self.pool)
        .await?;

        gene_row.map(|row| row.to_gene(contig_header)).transpose()
    }

    async fn query_gene_name(
        &self,
        reference: &Reference,
        gene_name: &String,
        cache: &mut TrackCache,
        contig_header: &ContigHeader,
    ) -> Result<Gene, TGVError> {
        let gene_row: Option<UcscGeneRow> = sqlx::query_as(
            format!(
                "SELECT *
            FROM {} 
            WHERE name2 = ?",
                self.get_preferred_track_name_with_cache(reference, cache)
                    .await?
            )
            .as_str(),
        )
        .bind(gene_name)
        .fetch_optional(&*self.pool)
        .await?;

        gene_row
            .ok_or(TGVError::IOError(format!(
                "Failed to query gene: {}",
                gene_name
            )))?
            .to_gene(contig_header)
    }

    async fn query_k_genes_after(
        &self,
        reference: &Reference,
        contig_index: usize,
        coord: usize,
        k: usize,
        cache: &mut TrackCache,
        contig_header: &ContigHeader,
    ) -> Result<Gene, TGVError> {
        let contig_name = contig_header.get_name(contig_index)?;
        if k == 0 {
            return Err(TGVError::ValueError("k cannot be 0".to_string()));
        }

        let gene_rows: Vec<UcscGeneRow> = sqlx::query_as(
            format!(
                "SELECT *
             FROM {} 
             WHERE chrom = ? AND txEnd >= ? 
             ORDER BY txEnd ASC LIMIT ?",
                self.get_preferred_track_name_with_cache(reference, cache)
                    .await?,
            )
            .as_str(),
        )
        .bind(contig_name)
        .bind(coord as i64) // coord is 1-based inclusive, UCSC is 0-based exclusive
        .bind((k + 1) as i64)
        .fetch_all(&*self.pool)
        .await?;

        Track::from_gene_rows(gene_rows, contig_index, contig_header)?
            .get_saturating_k_genes_after(coord, k)
            .cloned()
            .ok_or(TGVError::IOError("No genes found".to_string()))
    }

    async fn query_k_genes_before(
        &self,
        reference: &Reference,
        contig_index: usize,
        coord: usize,
        k: usize,
        cache: &mut TrackCache,
        contig_header: &ContigHeader,
    ) -> Result<Gene, TGVError> {
        let contig_name = contig_header.get_name(contig_index)?;
        if k == 0 {
            return Err(TGVError::ValueError("k cannot be 0".to_string()));
        }

        let gene_rows: Vec<UcscGeneRow> = sqlx::query_as(
            format!(
                "SELECT *
             FROM {} 
             WHERE chrom = ? AND txStart <= ? 
             ORDER BY txStart DESC LIMIT ?",
                self.get_preferred_track_name_with_cache(reference, cache)
                    .await?,
            )
            .as_str(),
        )
        .bind(contig_name)
        .bind(coord.saturating_sub(1) as i64) // coord is 1-based inclusive, UCSC is 0-based inclusive
        .bind((k + 1) as i64)
        .fetch_all(&*self.pool)
        .await?;

        Track::from_gene_rows(gene_rows, contig_index, contig_header)?
            .get_saturating_k_genes_before(coord, k)
            .cloned()
            .ok_or(TGVError::IOError("No genes found".to_string()))
    }

    async fn query_k_exons_after(
        &self,
        reference: &Reference,
        contig_index: usize,
        coord: usize,
        k: usize,
        cache: &mut TrackCache,
        contig_header: &ContigHeader,
    ) -> Result<SubGeneFeature, TGVError> {
        let contig_name = contig_header.get_name(contig_index)?;
        if k == 0 {
            return Err(TGVError::ValueError("k cannot be 0".to_string()));
        }

        let gene_rows: Vec<UcscGeneRow> = sqlx::query_as(
            format!(
                "SELECT *
             FROM {} 
             WHERE chrom = ? AND txEnd >= ? 
             ORDER BY txEnd ASC LIMIT ?",
                self.get_preferred_track_name_with_cache(reference, cache)
                    .await?,
            )
            .as_str(),
        )
        .bind(contig_name)
        .bind(coord as i64) // coord is 1-based inclusive, UCSC is 0-based exclusive
        .bind((k + 1) as i64)
        .fetch_all(&*self.pool)
        .await?;

        Track::from_gene_rows(gene_rows, contig_index, contig_header)?
            .get_saturating_k_exons_after(coord, k)
            .ok_or(TGVError::IOError("No exons found".to_string()))
    }

    async fn query_k_exons_before(
        &self,
        reference: &Reference,
        contig_index: usize,
        coord: usize,
        k: usize,
        cache: &mut TrackCache,
        contig_header: &ContigHeader,
    ) -> Result<SubGeneFeature, TGVError> {
        let contig_name = contig_header.get_name(contig_index)?;
        if k == 0 {
            return Err(TGVError::ValueError("k cannot be 0".to_string()));
        }

        let gene_rows: Vec<UcscGeneRow> = sqlx::query_as(
            format!(
                "SELECT *
             FROM {} 
             WHERE chrom = ? AND txStart <= ? 
             ORDER BY txStart DESC LIMIT ?",
                self.get_preferred_track_name_with_cache(reference, cache)
                    .await?,
            )
            .as_str(),
        )
        .bind(contig_name)
        .bind(coord.saturating_sub(1) as i64) // coord is 1-based inclusive, UCSC is 0-based inclusive
        .bind((k + 1) as i64)
        .fetch_all(&*self.pool)
        .await?;

        Track::from_gene_rows(gene_rows, contig_index, contig_header)?
            .get_saturating_k_exons_before(coord, k)
            .ok_or(TGVError::IOError("No exons found".to_string()))
    }
}

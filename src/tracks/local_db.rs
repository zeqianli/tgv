use crate::tracks::{TrackCache, TrackService, TRACK_PREFERENCES};
use crate::{
    contig::Contig,
    cytoband::{Cytoband, CytobandSegment, Stain},
    error::TGVError,
    feature::{Gene, SubGeneFeature},
    reference::Reference,
    region::Region,
    strand::Strand,
    track::Track,
    traits::GenomeInterval,
    ucsc::UcscHost,
};
use async_trait::async_trait;
use bigtools::BigBedRead;
use reqwest::{Client, StatusCode};
use serde::de::Error as _;
use serde::Deserialize;
use sqlx::{
    mysql::{MySqlPoolOptions, MySqlRow},
    sqlite::{Sqlite, SqliteConnectOptions, SqlitePool, SqlitePoolOptions, SqliteRow},
    Column, MySqlPool, Pool, Row,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
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

    fn parse_gene_rows(&self, rows: Vec<SqliteRow>) -> Result<Vec<Gene>, TGVError> {
        let mut genes = Vec::new();
        for row in rows {
            let name: String = row.try_get("name")?;
            let chrom: String = row.try_get("chrom")?;
            let strand_str: String = row.try_get("strand")?;
            let tx_start: i64 = row.try_get("txStart")?;
            let tx_end: i64 = row.try_get("txEnd")?;
            let cds_start: i64 = row.try_get("cdsStart")?;
            let cds_end: i64 = row.try_get("cdsEnd")?;

            let name2: String = match row.try_get("name2") {
                Ok(name2) => name2,
                Err(_) => name.clone(),
            };
            let exon_starts_blob: Vec<u8> = row.try_get("exonStarts")?;
            let exon_ends_blob: Vec<u8> = row.try_get("exonEnds")?;

            // USCS coordinates are 0-based, half-open
            // https://genome-blog.gi.ucsc.edu/blog/2016/12/12/the-ucsc-genome-browser-coordinate-counting-systems/

            genes.push(Gene {
                id: name,
                name: name2,
                strand: Strand::from_str(strand_str).unwrap(),
                contig: Contig::new(&chrom),
                transcription_start: tx_start as usize + 1,
                transcription_end: tx_end as usize,
                cds_start: cds_start as usize + 1,
                cds_end: cds_end as usize,
                exon_starts: Self::parse_blob_to_coords(&exon_starts_blob)
                    .iter()
                    .map(|v| v + 1)
                    .collect(),
                exon_ends: Self::parse_blob_to_coords(&exon_ends_blob),
                has_exons: true,
            });
        }

        Ok(genes)
    }

    async fn get_preferred_track_name_with_cache(
        &self,
        reference: &Reference,
        cache: &mut TrackCache,
    ) -> Result<String, TGVError> {
        if cache.get_preferred_track_name().is_none() {
            let preferred_track = self.get_preferred_track_name(reference, cache).await?;
            cache.set_preferred_track_name(preferred_track);
        }

        let preferred_track = match cache.get_preferred_track_name().unwrap() {
            Some(track) => track,
            None => return Err(TGVError::IOError("No preferred track found".to_string())),
        };

        Ok(preferred_track)
    }

    /// chrom name -> 2bit file name.
    /// Used for initailzing the local cache service.
    pub async fn get_contig_2bit_file_lookup(
        &self,
        reference: &Reference,
    ) -> Result<HashMap<String, Option<String>>, TGVError> {
        let rows_with_alias = sqlx::query("SELECT chrom, fileName FROM chromInfo")
            .fetch_all(&*self.pool)
            .await?;

        let mut filename_hashmap: HashMap<String, Option<String>> = HashMap::new();
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
            filename_hashmap.insert(chrom, basename);
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
    ) -> Result<Vec<(Contig, usize)>, TGVError> {
        if let Ok(rows_with_alias) = sqlx::query(
            "SELECT chromInfo.chrom as chrom, chromInfo.size as size, chromAlias.alias as alias
             FROM chromInfo 
             LEFT JOIN chromAlias ON chromAlias.chrom = chromInfo.chrom
             WHERE chromInfo.chrom NOT LIKE 'chr%\\_%'
             ORDER BY chromInfo.chrom",
        )
        .fetch_all(&*self.pool)
        .await
        {
            let mut contigs_hashmap: HashMap<String, (Contig, usize)> = HashMap::new();
            for row in rows_with_alias {
                let chrom: String = row.try_get("chrom")?;
                let size: i64 = row.try_get("size")?;
                let alias: String = row.try_get("alias")?;

                match contigs_hashmap.get_mut(&chrom) {
                    Some((ref mut contig, _)) => {
                        contig.alias(&alias);
                    }
                    None => {
                        let mut contig = Contig::new(&chrom);
                        contig.alias(&alias);
                        contigs_hashmap.insert(chrom.clone(), (contig, size as usize));
                    }
                }
            }
            let mut contigs = contigs_hashmap
                .values()
                .cloned()
                .collect::<Vec<(Contig, usize)>>();
            contigs.sort_by(|(a, length_a), (b, length_b)| {
                if a.name.starts_with("chr") || b.name.starts_with("chr") {
                    Contig::contigs_compare(a, b)
                } else {
                    length_b.cmp(length_a) // Sort by length in descending order
                }
            });

            return Ok(contigs);
        } else {
            let rows = sqlx::query(
                "SELECT chromInfo.chrom as chrom, chromInfo.size as size
                 FROM chromInfo
                 WHERE chromInfo.chrom NOT LIKE 'chr%\\_%'
                 ORDER BY chromInfo.chrom",
            )
            .fetch_all(&*self.pool)
            .await?;

            let mut contigs = rows
                .into_iter()
                .map(|row| {
                    let chrom: String = row.try_get("chrom")?;
                    let size: i64 = row.try_get("size")?;
                    Ok((Contig::new(&chrom), size as usize))
                })
                .collect::<Result<Vec<(Contig, usize)>, TGVError>>()?;

            contigs.sort_by(|(a, length_a), (b, length_b)| {
                if a.name.starts_with("chr") || b.name.starts_with("chr") {
                    Contig::contigs_compare(a, b)
                } else {
                    length_b.cmp(length_a) // Sort by length in descending order
                }
            });

            return Ok(contigs);
        }
    }

    async fn get_cytoband(
        &self,
        reference: &Reference,
        contig: &Contig,
        cache: &mut TrackCache,
    ) -> Result<Option<Cytoband>, TGVError> {
        if let Ok(rows) = sqlx::query(
            "SELECT chrom, chromStart, chromEnd, name, gieStain FROM cytoBandIdeo WHERE chrom = ?",
        )
        .bind(contig.name.clone())
        .fetch_all(&*self.pool)
        .await
        {
            if rows.is_empty() {
                return Ok(None);
            }

            let mut segments = Vec::with_capacity(rows.len());
            for row in rows {
                let chrom_start: i64 = row.try_get("chromStart")?;
                let chrom_end: i64 = row.try_get("chromEnd")?;
                let name: String = row.try_get("name")?;
                let gie_stain_str: String = row.try_get("gieStain")?;

                let stain = Stain::from(&gie_stain_str)?;

                segments.push(CytobandSegment {
                    contig: contig.clone(),
                    start: chrom_start as usize + 1, // 0-based to 1-based
                    end: chrom_end as usize,
                    name,
                    stain,
                });
            }

            return Ok(Some(Cytoband {
                reference: Some(reference.clone()),
                contig: contig.clone(),
                segments,
            }));
        } else {
            /// Cytoband table is not available.
            return Ok(None);
        }
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

        let preferred_track = get_preferred_track_name_from_vec(&available_gene_tracks)?;

        Ok(preferred_track)
    }

    async fn query_genes_overlapping(
        &self,
        reference: &Reference,
        region: &Region,
        cache: &mut TrackCache,
    ) -> Result<Vec<Gene>, TGVError> {
        let rows = sqlx::query(
            format!(
                "SELECT * FROM {} 
             WHERE chrom = ? AND (txStart <= ?) AND (txEnd >= ?)",
                self.get_preferred_track_name_with_cache(reference, cache)
                    .await?
            )
            .as_str(),
        )
        .bind(region.contig.name.clone())
        .bind(region.end as i64) // end is 1-based inclusive, UCSC is 0-based exclusive
        .bind(region.start.saturating_sub(1) as i64) // start is 1-based inclusive, UCSC is 0-based inclusive
        .fetch_all(&*self.pool)
        .await?;

        self.parse_gene_rows(rows)
    }

    async fn query_gene_covering(
        &self,
        reference: &Reference,
        contig: &Contig,
        coord: usize,
        cache: &mut TrackCache,
    ) -> Result<Option<Gene>, TGVError> {
        let row = sqlx::query(
            format!(
                "SELECT *
             FROM {} 
             WHERE chrom = ? AND txStart <= ? AND txEnd >= ?",
                self.get_preferred_track_name_with_cache(reference, cache)
                    .await?,
            )
            .as_str(),
        )
        .bind(contig.name.clone())
        .bind(coord.saturating_sub(1) as i64) // coord is 1-based inclusive, UCSC is 0-based inclusive
        .bind(coord as i64) // coord is 1-based inclusive, UCSC is 0-based exclusive
        .fetch_optional(&*self.pool)
        .await?;

        if let Some(row) = row {
            Ok(self.parse_gene_rows(vec![row])?.first().cloned())
        } else {
            Ok(None)
        }
    }

    async fn query_gene_name(
        &self,
        reference: &Reference,
        gene_id: &String,
        cache: &mut TrackCache,
    ) -> Result<Gene, TGVError> {
        let row = sqlx::query(
            format!(
                "SELECT *
            FROM {} 
            WHERE name2 = ?",
                self.get_preferred_track_name_with_cache(reference, cache)
                    .await?
            )
            .as_str(),
        )
        .bind(gene_id)
        .fetch_optional(&*self.pool)
        .await?;

        if let Some(row) = row {
            self.parse_gene_rows(vec![row])?
                .first()
                .cloned()
                .ok_or(TGVError::IOError(format!(
                    "Failed to query gene: {}",
                    gene_id
                )))
        } else {
            Err(TGVError::IOError(format!(
                "Failed to query gene: {}",
                gene_id
            )))
        }
    }

    async fn query_k_genes_after(
        &self,
        reference: &Reference,
        contig: &Contig,
        coord: usize,
        k: usize,
        cache: &mut TrackCache,
    ) -> Result<Gene, TGVError> {
        if k == 0 {
            return Err(TGVError::ValueError("k cannot be 0".to_string()));
        }

        let rows = sqlx::query(
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
        .bind(contig.name.clone())
        .bind(coord as i64) // coord is 1-based inclusive, UCSC is 0-based exclusive
        .bind((k + 1) as i64)
        .fetch_all(&*self.pool)
        .await?;

        if rows.is_empty() {
            return Err(TGVError::IOError("No genes found".to_string()));
        }

        Track::from_genes(self.parse_gene_rows(rows)?, contig.clone())?
            .get_saturating_k_genes_after(coord, k)
            .cloned()
            .ok_or(TGVError::IOError("No genes found".to_string()))
    }

    async fn query_k_genes_before(
        &self,
        reference: &Reference,
        contig: &Contig,
        coord: usize,
        k: usize,
        cache: &mut TrackCache,
    ) -> Result<Gene, TGVError> {
        if k == 0 {
            return Err(TGVError::ValueError("k cannot be 0".to_string()));
        }

        let rows = sqlx::query(
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
        .bind(contig.name.clone())
        .bind(coord.saturating_sub(1) as i64) // coord is 1-based inclusive, UCSC is 0-based inclusive
        .bind((k + 1) as i64)
        .fetch_all(&*self.pool)
        .await?;

        if rows.is_empty() {
            return Err(TGVError::IOError("No genes found".to_string()));
        }

        Track::from_genes(self.parse_gene_rows(rows)?, contig.clone())?
            .get_saturating_k_genes_before(coord, k)
            .cloned()
            .ok_or(TGVError::IOError("No genes found".to_string()))
    }

    async fn query_k_exons_after(
        &self,
        reference: &Reference,
        contig: &Contig,
        coord: usize,
        k: usize,
        cache: &mut TrackCache,
    ) -> Result<SubGeneFeature, TGVError> {
        if k == 0 {
            return Err(TGVError::ValueError("k cannot be 0".to_string()));
        }

        let rows = sqlx::query(
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
        .bind(contig.name.clone())
        .bind(coord as i64) // coord is 1-based inclusive, UCSC is 0-based exclusive
        .bind((k + 1) as i64)
        .fetch_all(&*self.pool)
        .await?;

        let genes = self.parse_gene_rows(rows)?;

        let track = Track::from_genes(genes, contig.clone())?;

        track
            .get_saturating_k_exons_after(coord, k)
            .ok_or(TGVError::IOError("No exons found".to_string()))
    }

    async fn query_k_exons_before(
        &self,
        reference: &Reference,
        contig: &Contig,
        coord: usize,
        k: usize,
        cache: &mut TrackCache,
    ) -> Result<SubGeneFeature, TGVError> {
        if k == 0 {
            return Err(TGVError::ValueError("k cannot be 0".to_string()));
        }

        let rows = sqlx::query(
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
        .bind(contig.name.clone())
        .bind(coord.saturating_sub(1) as i64) // coord is 1-based inclusive, UCSC is 0-based inclusive
        .bind((k + 1) as i64)
        .fetch_all(&*self.pool)
        .await?;

        let genes = self.parse_gene_rows(rows)?;

        Track::from_genes(genes, contig.clone())?
            .get_saturating_k_exons_before(coord, k)
            .ok_or(TGVError::IOError("No exons found".to_string()))
    }
}

fn get_preferred_track_name_from_vec(names: &Vec<String>) -> Result<Option<String>, TGVError> {
    for pref in TRACK_PREFERENCES {
        if names.contains(&pref.to_string()) {
            return Ok(Some(pref.to_string()));
        }
    }

    Ok(None)
}

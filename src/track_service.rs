use crate::error::TGVError;
use crate::reference;
use crate::traits::GenomeInterval;
use crate::{
    contig::Contig,
    cytoband::{Cytoband, CytobandSegment, Stain},
    feature::{Gene, SubGeneFeature},
    reference::Reference,
    region::Region,
    strand::Strand,
    track::Track,
    ucsc::UcscHost,
};
use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use serde::de::Error as _;
use serde::Deserialize;
use sqlx::mysql::MySqlRow;
use sqlx::sqlite::Sqlite;
use sqlx::sqlite::SqliteRow;
use sqlx::Pool;
use sqlx::{
    mysql::MySqlPoolOptions,
    sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions},
    Column, MySqlPool, Row,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Holds cache for track service queries.
/// Can be returned or pass into queries.
pub struct TrackCache {
    /// Contig name/aliases -> Track
    tracks: Vec<Option<Track<Gene>>>,

    /// Contig name/aliases -> Index
    tracks_by_contig: HashMap<String, usize>,

    /// Gene name -> Option<Gene>.
    /// If the gene name is not found, the value is None.
    gene_by_name: HashMap<String, Option<Gene>>,

    /// Prefered track name.
    /// None: Not initialized.
    /// Some(None): Queried but not found.
    /// Some(Some(name)): Queried and found.
    preferred_track_name: Option<Option<String>>,

    /// hub_url for UCSC accessions.
    /// None: Not initialized.
    /// Some(url): Queried and found.
    hub_url: Option<String>,
}

impl Default for TrackCache {
    fn default() -> Self {
        Self::new()
    }
}

impl TrackCache {
    pub fn new() -> Self {
        Self {
            tracks: Vec::new(),
            tracks_by_contig: HashMap::new(),
            gene_by_name: HashMap::new(),
            preferred_track_name: None,
            hub_url: None,
        }
    }

    pub fn get_track_index(&self, contig: &Contig) -> Option<usize> {
        match self.tracks_by_contig.get(&contig.name) {
            Some(index) => Some(*index),
            None => {
                for alias in contig.aliases.iter() {
                    if let Some(index) = self.tracks_by_contig.get(alias) {
                        return Some(*index);
                    }
                }
                None
            }
        }
    }

    pub fn includes_contig(&self, contig: &Contig) -> bool {
        self.get_track_index(contig).is_some()
    }

    /// Note that this returns None both when the contig is not queried,
    ///    and returns Some(None) when the contig is queried but the track data is not found.
    pub fn get_track(&self, contig: &Contig) -> Option<Option<&Track<Gene>>> {
        self.get_track_index(contig)
            .map(|index| self.tracks[index].as_ref())
    }

    pub fn includes_gene(&self, gene_name: &str) -> bool {
        self.gene_by_name.contains_key(gene_name)
    }

    /// Note that this returns None both when the gene is not queried,
    ///    and returns Some(None) when the gene is queried but the gene data is not found.
    pub fn get_gene(&self, gene_name: &str) -> Option<Option<&Gene>> {
        self.gene_by_name.get(gene_name).map(|gene| gene.as_ref())
    }

    pub fn add_track(&mut self, contig: &Contig, track: Option<Track<Gene>>) {
        if let Some(track) = &track {
            for (i, gene) in track.genes().iter().enumerate() {
                self.gene_by_name
                    .insert(gene.name.clone(), Some(gene.clone()));
            }
        }
        self.tracks.push(track);
        self.tracks_by_contig
            .insert(contig.name.clone(), self.tracks.len() - 1);
        for alias in contig.aliases.iter() {
            self.tracks_by_contig
                .insert(alias.clone(), self.tracks.len() - 1);
        }
    }

    pub fn get_preferred_track_name(&self) -> Option<Option<String>> {
        self.preferred_track_name.clone()
    }

    pub fn set_preferred_track_name(&mut self, preferred_track_name: Option<String>) {
        self.preferred_track_name = Some(preferred_track_name);
    }
}

#[async_trait]
pub trait TrackService {
    // Basics

    /// Close the track service.
    async fn close(&self) -> Result<(), TGVError>;

    // Return all contigs given a reference.
    async fn get_all_contigs(
        &self,
        reference: &Reference,
        cache: &mut TrackCache,
    ) -> Result<Vec<(Contig, usize)>, TGVError>;

    // Return the cytoband data given a reference and a contig.
    async fn get_cytoband(
        &self,
        reference: &Reference,
        contig: &Contig,
        cache: &mut TrackCache,
    ) -> Result<Option<Cytoband>, TGVError>;

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
    async fn get_preferred_track_name(
        &self,
        reference: &Reference,
        cache: &mut TrackCache,
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
    // Initialize the database connections. Reference is needed to find the corresponding schema.
    pub async fn new(reference: &Reference, ucsc_host: &UcscHost) -> Result<Self, TGVError> {
        let mysql_url = UcscDbTrackService::get_mysql_url(reference, ucsc_host)?;
        let pool = MySqlPoolOptions::new()
            .max_connections(1)
            .connect(&mysql_url)
            .await?;

        Ok(Self {
            pool: Arc::new(pool),
        })
    }

    pub fn get_mysql_url(reference: &Reference, ucsc_host: &UcscHost) -> Result<String, TGVError> {
        match reference {
            Reference::Hg19 | Reference::Hg38 | Reference::UcscGenome(_) => {
                Ok(format!("mysql://genome@{}/{}", ucsc_host.url(), reference))
            }
            _ => Err(TGVError::ValueError(format!(
                "Unsupported reference: {}",
                reference
            ))),
        }
    }

    pub async fn list_assemblies(n: Option<usize>) -> Result<Vec<(String, String)>, TGVError> {
        let connection = MySqlPoolOptions::new()
            .max_connections(5)
            .connect("mysql://genome@genome-mysql.soe.ucsc.edu/hgcentral")
            .await?;

        let rows = if let Some(n) = n {
            sqlx::query("SELECT name, organism FROM dbDb LIMIT ?")
                .bind(n as i32)
                .fetch_all(&connection)
                .await?
        } else {
            sqlx::query("SELECT name, organism FROM dbDb")
                .fetch_all(&connection)
                .await?
        };

        let mut assemblies = Vec::new();
        for row in rows {
            let name: String = row.try_get("name")?;
            let organism: String = row.try_get("organism")?;
            assemblies.push((name, organism));
        }

        Ok(assemblies)
    }

    pub async fn list_accessions(
        n: usize,
        offset: usize,
    ) -> Result<Vec<(String, String)>, TGVError> {
        let connection = MySqlPoolOptions::new()
            .max_connections(5)
            .connect("mysql://genome@genome-mysql.soe.ucsc.edu/hgcentral")
            .await?;

        let rows =
            sqlx::query("SELECT name, organism FROM dbDb ORDER BY organism, name LIMIT ? OFFSET ?")
                .bind(n as i32)
                .bind(offset as i32)
                .fetch_all(&connection)
                .await?;

        let mut assemblies = Vec::new();
        for row in rows {
            let name: String = row.try_get("name")?;
            let common_name: String = row.try_get("commonName")?;
            assemblies.push((name, common_name));
        }
        Ok(assemblies)
    }

    fn parse_gene_rows(&self, rows: Vec<MySqlRow>) -> Result<Vec<Gene>, TGVError> {
        let mut genes = Vec::new();
        for row in rows {
            let name: String = row.try_get("name")?;
            let chrom: String = row.try_get("chrom")?;
            let strand_str: String = row.try_get("strand")?;
            let tx_start: u64 = row.try_get("txStart")?;
            let tx_end: u64 = row.try_get("txEnd")?;
            let cds_start: u64 = row.try_get("cdsStart")?;
            let cds_end: u64 = row.try_get("cdsEnd")?;

            let name2: String = match row.try_get("name2") {
                Ok(name2) => name2,
                Err(e) => name.clone(),
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
                exon_starts: parse_blob_to_coords(&exon_starts_blob)
                    .iter()
                    .map(|v| v + 1)
                    .collect(),
                exon_ends: parse_blob_to_coords(&exon_ends_blob),
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
    async fn get_contig_2bit_file_lookup(
        &self,
        reference: &Reference,
    ) -> Result<HashMap<String, String>, TGVError> {
        let rows_with_alias = sqlx::query("SELECT chrom, fileName FROM chromInfo")
            .fetch_all(&*self.pool)
            .await?;

        let mut filename_hashmap: HashMap<String, String> = HashMap::new();
        for row in rows_with_alias {
            let chrom: String = row.try_get("chrom")?;
            let file_name: String = row.try_get("fileName")?;

            let basename = file_name.split("/").last().ok_or(TGVError::IOError(
                "Failed to get basename from file name".to_string(),
            ))?;
            filename_hashmap.insert(chrom, basename.to_string());
        }

        Ok(filename_hashmap)
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
                let size: u32 = row.try_get("size")?;
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
                    let size: u32 = row.try_get("size")?;
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

        let gene_track_rows = sqlx::query("SHOW TABLES").fetch_all(&*self.pool).await?;

        let available_gene_tracks: Vec<String> = gene_track_rows
            .into_iter()
            .map(|row| row.try_get::<String, usize>(0))
            .collect::<Result<Vec<String>, sqlx::Error>>()?;

        get_preferred_track_name_from_vec(&available_gene_tracks)
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
        .bind(u64::try_from(region.end).unwrap()) // end is 1-based inclusive, UCSC is 0-based exclusive
        .bind(u64::try_from(region.start.saturating_sub(1)).unwrap()) // start is 1-based inclusive, UCSC is 0-based inclusive
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
        .bind(u32::try_from(coord.saturating_sub(1)).unwrap()) // coord is 1-based inclusive, UCSC is 0-based inclusive
        .bind(u32::try_from(coord).unwrap()) // coord is 1-based inclusive, UCSC is 0-based exclusive
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
        .bind(u32::try_from(coord).unwrap()) // coord is 1-based inclusive, UCSC is 0-based exclusive
        .bind(u32::try_from(k + 1).unwrap())
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
        .bind(u32::try_from(coord.saturating_sub(1)).unwrap()) // coord is 1-based inclusive, UCSC is 0-based inclusive
        .bind(u32::try_from(k + 1).unwrap())
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
        .bind(u32::try_from(coord).unwrap()) // coord is 1-based inclusive, UCSC is 0-based exclusive
        .bind(u32::try_from(k + 1).unwrap())
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
        .bind(u32::try_from(coord.saturating_sub(1)).unwrap()) // coord is 1-based inclusive, UCSC is 0-based inclusive
        .bind(u32::try_from(k + 1).unwrap())
        .fetch_all(&*self.pool)
        .await?;

        let genes = self.parse_gene_rows(rows)?;

        Track::from_genes(genes, contig.clone())?
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
                exon_starts: parse_blob_to_coords(&exon_starts_blob)
                    .iter()
                    .map(|v| v + 1)
                    .collect(),
                exon_ends: parse_blob_to_coords(&exon_ends_blob),
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
    async fn get_contig_2bit_file_lookup(
        &self,
        reference: &Reference,
    ) -> Result<HashMap<String, String>, TGVError> {
        let rows_with_alias = sqlx::query("SELECT chrom, fileName FROM chromInfo")
            .fetch_all(&*self.pool)
            .await?;

        let mut filename_hashmap: HashMap<String, String> = HashMap::new();
        for row in rows_with_alias {
            let chrom: String = row.try_get("chrom")?;
            let file_name: String = row.try_get("fileName")?;

            let basename = file_name.split("/").last().ok_or(TGVError::IOError(
                "Failed to get basename from file name".to_string(),
            ))?;
            filename_hashmap.insert(chrom, basename.to_string());
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

        get_preferred_track_name_from_vec(&available_gene_tracks)
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

// TODO: improved pattern:
// Service doesn't save anything. No reference, no cache.
// Ask these things to be passed in. And return them to store in the state.

#[derive(Debug)]
pub struct UcscApiTrackService {
    client: Client,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(non_snake_case)]
struct GeneResponse1 {
    name: String,
    name2: String,

    strand: String,

    txStart: usize,
    txEnd: usize,
    cdsStart: usize,
    cdsEnd: usize,
    exonStarts: String,
    exonEnds: String,
}

impl GeneResponse1 {
    /// Custom deserializer for strand field
    fn gene(self, contig: &Contig) -> Result<Gene, TGVError> {
        Ok(Gene {
            id: self.name,
            name: self.name2,
            strand: Strand::from_str(self.strand)?,
            contig: contig.clone(),
            transcription_start: self.txStart,
            transcription_end: self.txEnd,
            cds_start: self.cdsStart,
            cds_end: self.cdsEnd,
            exon_starts: parse_comma_separated_list(&self.exonStarts)?,
            exon_ends: parse_comma_separated_list(&self.exonEnds)?,
            has_exons: true,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
#[allow(non_snake_case)]
struct GeneResponse2 {
    name: String,

    strand: String,
    txStart: usize,
    txEnd: usize,
    cdsStart: usize,
    cdsEnd: usize,
    exonStarts: String,
    exonEnds: String,
}

impl GeneResponse2 {
    /// Custom deserializer for strand field
    fn gene(self, contig: &Contig) -> Result<Gene, TGVError> {
        Ok(Gene {
            id: self.name.clone(),
            name: self.name.clone(),
            strand: Strand::from_str(self.strand)?,
            contig: contig.clone(),
            transcription_start: self.txStart,
            transcription_end: self.txEnd,
            cds_start: self.cdsStart,
            cds_end: self.cdsEnd,
            exon_starts: parse_comma_separated_list(&self.exonStarts)?,
            exon_ends: parse_comma_separated_list(&self.exonEnds)?,
            has_exons: true,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
#[allow(non_snake_case)]
struct GeneResponse3 {
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
}

impl GeneResponse3 {
    /// TODO: I'm not sure if this is correct.
    fn gene(self, contig: &Contig) -> Result<Gene, TGVError> {
        Ok(Gene {
            id: self.name.clone(),
            name: self.name.clone(),
            strand: Strand::from_str(self.strand)?,
            contig: contig.clone(),
            transcription_start: self.chromStart,
            transcription_end: self.chromEnd,
            cds_start: self.thickStart,
            cds_end: self.thickEnd,
            exon_starts: vec![],
            exon_ends: vec![],
            has_exons: false,
        })
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
        cache: &mut TrackCache,
    ) -> Result<Track<Gene>, TGVError> {
        if cache.get_preferred_track_name().is_none() {
            let preferred_track = self
                .get_preferred_track_name(reference, cache)
                .await?
                .ok_or(TGVError::IOError(format!(
                    "Failed to get prefered track for {} from UCSC API",
                    contig.name
                )))?; // TODO: proper handling

            cache.set_preferred_track_name(Some(preferred_track));
        }

        let preferred_track = cache.get_preferred_track_name().unwrap();

        let preferred_track = preferred_track.ok_or(TGVError::IOError(format!(
            "Failed to get prefered track for {} from UCSC API",
            contig.name
        )))?;

        let query_url = self
            .get_track_data_url(reference, contig, preferred_track.clone(), cache)
            .await?;

        let response_value: serde_json::Value =
            self.client.get(query_url).send().await?.json().await?;

        let genes_array_value = response_value.get(&preferred_track).ok_or_else(|| {
            TGVError::JsonSerializationError(serde_json::Error::custom(format!(
                "Track key \'{}\' not found in UCSC API response. Full response: {:?}",
                preferred_track, response_value
            )))
        })?;

        let mut genes: Vec<Gene> = Vec::new();
        let mut deserialized_successfully = false;

        // Attempt 1: GeneResponse1
        if let Ok(gene_responses) =
            serde_json::from_value::<Vec<GeneResponse1>>(genes_array_value.clone())
        {
            for gr in gene_responses {
                genes.push(gr.gene(contig)?);
            }
            deserialized_successfully = true;
        }

        // Attempt 2: GeneResponse2
        if !deserialized_successfully {
            if let Ok(gene_responses) =
                serde_json::from_value::<Vec<GeneResponse2>>(genes_array_value.clone())
            {
                for gr in gene_responses {
                    genes.push(gr.gene(contig)?);
                }
                deserialized_successfully = true;
            }
        }

        // Attempt 3: Direct Gene deserialization (handles complex format via GeneHelper in feature.rs)
        if !deserialized_successfully {
            if let Ok(gene_responses) =
                serde_json::from_value::<Vec<GeneResponse3>>(genes_array_value.clone())
            {
                for gr in gene_responses {
                    genes.push(gr.gene(contig)?);
                }
                deserialized_successfully = true;
            }
        }

        if !deserialized_successfully {
            return Err(TGVError::JsonSerializationError(serde_json::Error::custom(
                format!(
                    "Failed to deserialize gene data from UCSC API for track \'{}\' using any known format. Gene array value: {:?}",
                    preferred_track, genes_array_value
                )
            )));
        }

        Track::from_genes(genes, contig.clone())
    }

    const CYTOBAND_TRACK: &str = "cytoBandIdeo";

    async fn get_track_data_url(
        &self,
        reference: &Reference,
        contig: &Contig,
        track_name: String,
        cache: &mut TrackCache,
    ) -> Result<String, TGVError> {
        match reference {
            Reference::Hg19 | Reference::Hg38 | Reference::UcscGenome(_) => Ok(format!(
                "https://api.genome.ucsc.edu/getData/track?genome={}&track={}&chrom={}",
                reference.to_string(),
                track_name,
                contig.name
            )),
            Reference::UcscAccession(genome) => {
                if cache.hub_url.is_none() {
                    let hub_url = self.get_hub_url_for_genark_accession(genome).await?;
                    cache.hub_url = Some(hub_url);
                }
                let hub_url = cache.hub_url.as_ref().unwrap();
                Ok(format!(
                    "https://api.genome.ucsc.edu/getData/track?hubUrl={}&genome={}&track={}&chrom={}",
                    hub_url, genome, track_name, contig.name
                ))
            }
        }
    }

    async fn get_hub_url_for_genark_accession(&self, accession: &str) -> Result<String, TGVError> {
        let query_url = format!(
            "https://api.genome.ucsc.edu/list/genarkGenomes?genome={}",
            accession
        );
        let response = self.client.get(query_url).send().await?;

        // Example response:
        // {
        //     "downloadTime": "2025:05:06T03:46:07Z",
        //     "downloadTimeStamp": 1746503167,
        //     "dataTime": "2025-04-29T10:42:00",
        //     "dataTimeStamp": 1745948520,
        //     "hubUrlPrefix": "/gbdb/genark",
        //     "genarkGenomes": {
        //       "GCF_028858775.2": {
        //         "hubUrl": "GCF/028/858/775/GCF_028858775.2/hub.txt",
        //         "asmName": "NHGRI_mPanTro3-v2.0_pri",
        //         "scientificName": "Pan troglodytes",
        //         "commonName": "chimpanzee (v2 AG18354 primary hap 2024 refseq)",
        //         "taxId": 9598,
        //         "priority": 138,
        //         "clade": "primates"
        //       }
        //     },
        //     "totalAssemblies": 5691,
        //     "itemsReturned": 1
        //   }

        let response_text = response.text().await?;
        let value: serde_json::Value = serde_json::from_str(&response_text)?;

        Ok(format!(
            "https://hgdownload.soe.ucsc.edu/hubs/{}",
            value["genarkGenomes"][accession]["hubUrl"]
                .as_str()
                .ok_or(TGVError::IOError(format!(
                    "Failed to get hub url for {}",
                    accession
                )))?
        ))
    }
}

/// Get the preferred track name recursively.
fn get_all_track_names(content: &serde_json::Value) -> Result<Vec<String>, TGVError> {
    let err = TGVError::IOError("Failed to get genome from UCSC API".to_string());

    let mut names = Vec::new();

    for (key, value) in content.as_object().ok_or(err)?.iter() {
        if value.get("compositeContainer").is_some() {
            // do this recursively
            names.extend(get_all_track_names(value)?);
        } else {
            names.push(key.to_string());
        }
    }

    Ok(names)
}

fn get_preferred_track_name_from_vec(names: &Vec<String>) -> Result<Option<String>, TGVError> {
    let preferences = [
        "ncbiRefSeqSelect",
        "ncbiRefSeqCurated",
        "ncbiRefSeq",
        "ncbiGene",
        "refGenes",
    ];

    for pref in preferences {
        if names.contains(&pref.to_string()) {
            return Ok(Some(pref.to_string()));
        }
    }

    Ok(None)
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
        track_cache: &mut TrackCache,
    ) -> Result<Vec<(Contig, usize)>, TGVError> {
        match reference {
            Reference::Hg19 | Reference::Hg38 | Reference::UcscGenome(_) => {
                let query_url = format!(
                    "https://api.genome.ucsc.edu/list/chromosomes?genome={}",
                    reference.to_string()
                );

                let response = self.client.get(query_url).send().await?.text().await?;

                let err = TGVError::IOError(format!(
                    "Failed to deserialize chromosomes for {}",
                    reference.to_string()
                ));

                // schema: {..., "chromosomes": [{"__name__", len}]}

                let value: serde_json::Value = serde_json::from_str(&response)?;

                let mut output = Vec::new();
                for (k, v) in value["chromosomes"].as_object().ok_or(err)?.iter() {
                    // TODO: save length
                    output.push((Contig::new(k), v.as_u64().unwrap() as usize));
                }

                output.sort_by(|(a, length_a), (b, length_b)| {
                    if a.name.starts_with("chr") || b.name.starts_with("chr") {
                        Contig::contigs_compare(a, b)
                    } else {
                        length_b.cmp(length_a) // Sort by length in descending order
                    }
                });

                Ok(output)
            }
            Reference::UcscAccession(genome) => {
                if track_cache.hub_url.is_none() {
                    let hub_url = self.get_hub_url_for_genark_accession(genome).await?;
                    track_cache.hub_url = Some(hub_url);
                }
                let hub_url = track_cache.hub_url.as_ref().unwrap();

                let query_url = format!(
                    "https://api.genome.ucsc.edu/list/chromosomes?hubUrl={};genome={}",
                    hub_url, genome
                );

                let response = self
                    .client
                    .get(query_url)
                    .send()
                    .await?
                    .json::<serde_json::Value>()
                    .await?;

                let mut output = Vec::new();

                for (k, v) in response
                    .get("chromosomes")
                    .ok_or(TGVError::IOError(format!(
                        "Failed to parse response for chromosomes for UCSC accession {}. Response: {:?}",
                        genome, response
                    )))?
                    .as_object()
                    .ok_or(TGVError::IOError(format!(
                        "Failed to parse response for chromosomes for UCSC accession {}. Response: {:?}",
                        genome, response
                    )))?
                    .iter()
                {
                    output.push((
                        Contig::new(k),
                        v.as_u64()
                            .ok_or(TGVError::IOError(format!(
                                "Failed to get contig {} length for UCSC accession {}. Response: {:?}",
                                k, genome, response
                            )))?
                            as usize,
                    ));
                }

                // Longest contig first
                // These contigs are likely not well-named, so longest contig first.
                // Note that this is different from the UCSC assemblies.
                output.sort_by(|(a, a_len), (b, b_len)| b_len.cmp(a_len));

                Ok(output)
            }
        }
    }

    async fn get_cytoband(
        &self,
        reference: &Reference,
        contig: &Contig,
        cache: &mut TrackCache,
    ) -> Result<Option<Cytoband>, TGVError> {
        let query_url = match reference {
            Reference::Hg19 | Reference::Hg38 | Reference::UcscGenome(_) => format!(
                "https://api.genome.ucsc.edu/getData/track?genome={}&track={}&chrom={}",
                reference.to_string(),
                Self::CYTOBAND_TRACK,
                contig.name
            ),
            Reference::UcscAccession(genome) => {
                if cache.hub_url.is_none() {
                    let hub_url = self.get_hub_url_for_genark_accession(genome).await?;
                    cache.hub_url = Some(hub_url);
                }
                let hub_url = cache.hub_url.as_ref().unwrap();
                format!(
                    "https://api.genome.ucsc.edu/getData/track?hubUrl={}&genome={}&track={}&chrom={}",
                    hub_url, genome, Self::CYTOBAND_TRACK, contig.name
                )
            }
        };

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

    async fn get_preferred_track_name(
        &self,
        reference: &Reference,
        cache: &mut TrackCache,
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

                let track_names = get_all_track_names(response.get(genome.clone()).ok_or(
                    TGVError::IOError("Failed to get genome from UCSC API".to_string()),
                )?)?;

                let prefered_track = get_preferred_track_name_from_vec(&track_names)?;

                Ok(prefered_track)
            }
            Reference::UcscAccession(genome) => {
                if cache.hub_url.is_none() {
                    let hub_url = self
                        .get_hub_url_for_genark_accession(genome.as_str())
                        .await?;
                    cache.hub_url = Some(hub_url);
                }
                let hub_url = cache.hub_url.as_ref().unwrap();
                let query_url = format!(
                    "https://api.genome.ucsc.edu/list/tracks?hubUrl={}&genome={}",
                    hub_url, genome
                );
                let response = reqwest::get(query_url)
                    .await?
                    .json::<serde_json::Value>()
                    .await?;
                let track_names = get_all_track_names(response.get(genome.clone()).ok_or(
                    TGVError::IOError("Failed to get genome from UCSC API".to_string()),
                )?)?;

                let prefered_track = get_preferred_track_name_from_vec(&track_names)?;
                Ok(prefered_track)
            }
        }
    }

    async fn query_genes_overlapping(
        &self,
        reference: &Reference,
        region: &Region,
        cache: &mut TrackCache,
    ) -> Result<Vec<Gene>, TGVError> {
        if !cache.includes_contig(region.contig()) {
            let track = self
                .query_track_by_contig(reference, region.contig(), cache)
                .await?;
            cache.add_track(region.contig(), Some(track));
        }
        // TODO: now I don't really handle empty query results

        if let Some(Some(track)) = cache.get_track(region.contig()) {
            Ok(track
                .get_features_overlapping(region)
                .iter()
                .map(|g| (*g).clone())
                .collect())
        } else {
            Err(TGVError::IOError(format!(
                "Track not found for contig {}",
                region.contig().name
            )))
        }
    }

    async fn query_gene_covering(
        &self,
        reference: &Reference,
        contig: &Contig,
        position: usize,
        cache: &mut TrackCache,
    ) -> Result<Option<Gene>, TGVError> {
        if !cache.includes_contig(contig) {
            let track = self.query_track_by_contig(reference, contig, cache).await?;
            cache.add_track(contig, Some(track));
        }
        if let Some(Some(track)) = cache.get_track(contig) {
            Ok(track.get_gene_at(position).map(|g| (*g).clone()))
        } else {
            Err(TGVError::IOError(format!(
                "Track not found for contig {}",
                contig.name
            )))
        }
    }

    async fn query_gene_name(
        &self,
        reference: &Reference,
        name: &String,
        cache: &mut TrackCache,
    ) -> Result<Gene, TGVError> {
        if !cache.includes_gene(name) {
            // query all possible tracks until the gene is found
            for (contig, _) in self.get_all_contigs(reference, cache).await? {
                let track = self
                    .query_track_by_contig(reference, &contig, cache)
                    .await?;
                cache.add_track(&contig, Some(track));

                if let Some(Some(gene)) = cache.get_gene(name) {
                    break;
                }
            }
        }

        if let Some(Some(gene)) = cache.get_gene(name) {
            Ok(gene.clone())
        } else {
            Err(TGVError::IOError(format!("Gene {} not found", name)))
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
        if !cache.includes_contig(contig) {
            let track = self.query_track_by_contig(reference, contig, cache).await?;
            cache.add_track(contig, Some(track));
        }

        if let Some(Some(track)) = cache.get_track(contig) {
            Ok(track
                .get_saturating_k_genes_after(coord, k)
                .ok_or(TGVError::IOError("No genes found".to_string()))?
                .clone())
        } else {
            Err(TGVError::IOError(format!(
                "Track not found for contig {}",
                contig.name
            )))
        }
    }

    async fn query_k_genes_before(
        &self,
        reference: &Reference,
        contig: &Contig,
        coord: usize,
        k: usize,
        cache: &mut TrackCache,
    ) -> Result<Gene, TGVError> {
        if !cache.includes_contig(contig) {
            let track = self.query_track_by_contig(reference, contig, cache).await?;
            cache.add_track(contig, Some(track));
        }
        if let Some(Some(track)) = cache.get_track(contig) {
            Ok(track
                .get_saturating_k_genes_before(coord, k)
                .ok_or(TGVError::IOError("No genes found".to_string()))?
                .clone())
        } else {
            Err(TGVError::IOError(format!(
                "Track not found for contig {}",
                contig.name
            )))
        }
    }

    async fn query_k_exons_after(
        &self,
        reference: &Reference,
        contig: &Contig,
        coord: usize,
        k: usize,
        cache: &mut TrackCache,
    ) -> Result<SubGeneFeature, TGVError> {
        if !cache.includes_contig(contig) {
            let track = self.query_track_by_contig(reference, contig, cache).await?;
            cache.add_track(contig, Some(track));
        }
        if let Some(Some(track)) = cache.get_track(contig) {
            Ok(track
                .get_saturating_k_exons_after(coord, k)
                .ok_or(TGVError::IOError("No exons found".to_string()))?)
        } else {
            Err(TGVError::IOError(format!(
                "Track not found for contig {}",
                contig.name
            )))
        }
    }

    async fn query_k_exons_before(
        &self,
        reference: &Reference,
        contig: &Contig,
        coord: usize,
        k: usize,
        cache: &mut TrackCache,
    ) -> Result<SubGeneFeature, TGVError> {
        if !cache.includes_contig(contig) {
            let track = self.query_track_by_contig(reference, contig, cache).await?;
            cache.add_track(contig, Some(track));
        }
        if let Some(Some(track)) = cache.get_track(contig) {
            Ok(track
                .get_saturating_k_exons_before(coord, k)
                .ok_or(TGVError::IOError("No exons found".to_string()))?)
        } else {
            Err(TGVError::IOError(format!(
                "Track not found for contig {}",
                contig.name
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
    LocalDb(LocalDbTrackService),
}

impl TrackServiceEnum {
    pub async fn get_contig_2bit_file_lookup(
        &self,
        reference: &Reference,
    ) -> Result<HashMap<String, String>, TGVError> {
        match self {
            TrackServiceEnum::Api(_) => Err(TGVError::IOError(
                "get_contig_2bit_file_lookup is not supported for UcscApiTrackService".to_string(),
            )),
            TrackServiceEnum::Db(service) => service.get_contig_2bit_file_lookup(reference).await,
            TrackServiceEnum::LocalDb(service) => {
                service.get_contig_2bit_file_lookup(reference).await
            }
        }
    }
}

// Implement TrackService for the enum, dispatching calls
#[async_trait]
impl TrackService for TrackServiceEnum {
    async fn close(&self) -> Result<(), TGVError> {
        match self {
            TrackServiceEnum::Api(service) => service.close().await,
            TrackServiceEnum::Db(service) => service.close().await,
            TrackServiceEnum::LocalDb(service) => service.close().await,
        }
    }

    async fn get_all_contigs(
        &self,
        reference: &Reference,
        cache: &mut TrackCache,
    ) -> Result<Vec<(Contig, usize)>, TGVError> {
        match self {
            TrackServiceEnum::Api(service) => service.get_all_contigs(reference, cache).await,
            TrackServiceEnum::Db(service) => service.get_all_contigs(reference, cache).await,
            TrackServiceEnum::LocalDb(service) => service.get_all_contigs(reference, cache).await,
        }
    }

    async fn get_cytoband(
        &self,
        reference: &Reference,
        contig: &Contig,
        cache: &mut TrackCache,
    ) -> Result<Option<Cytoband>, TGVError> {
        match self {
            TrackServiceEnum::Api(service) => service.get_cytoband(reference, contig, cache).await,
            TrackServiceEnum::Db(service) => service.get_cytoband(reference, contig, cache).await,
            TrackServiceEnum::LocalDb(service) => {
                service.get_cytoband(reference, contig, cache).await
            }
        }
    }

    async fn get_preferred_track_name(
        &self,
        reference: &Reference,
        cache: &mut TrackCache,
    ) -> Result<Option<String>, TGVError> {
        match self {
            TrackServiceEnum::Api(service) => {
                service.get_preferred_track_name(reference, cache).await
            }
            TrackServiceEnum::Db(service) => {
                service.get_preferred_track_name(reference, cache).await
            }
            TrackServiceEnum::LocalDb(service) => {
                service.get_preferred_track_name(reference, cache).await
            }
        }
    }

    async fn query_genes_overlapping(
        &self,
        reference: &Reference,
        region: &Region,
        cache: &mut TrackCache,
    ) -> Result<Vec<Gene>, TGVError> {
        match self {
            TrackServiceEnum::Api(service) => {
                service
                    .query_genes_overlapping(reference, region, cache)
                    .await
            }
            TrackServiceEnum::Db(service) => {
                service
                    .query_genes_overlapping(reference, region, cache)
                    .await
            }
            TrackServiceEnum::LocalDb(service) => {
                service
                    .query_genes_overlapping(reference, region, cache)
                    .await
            }
        }
    }

    async fn query_gene_covering(
        &self,
        reference: &Reference,
        contig: &Contig,
        coord: usize,
        cache: &mut TrackCache,
    ) -> Result<Option<Gene>, TGVError> {
        match self {
            TrackServiceEnum::Api(service) => {
                service
                    .query_gene_covering(reference, contig, coord, cache)
                    .await
            }
            TrackServiceEnum::Db(service) => {
                service
                    .query_gene_covering(reference, contig, coord, cache)
                    .await
            }
            TrackServiceEnum::LocalDb(service) => {
                service
                    .query_gene_covering(reference, contig, coord, cache)
                    .await
            }
        }
    }

    async fn query_gene_name(
        &self,
        reference: &Reference,
        gene_id: &String,
        cache: &mut TrackCache,
    ) -> Result<Gene, TGVError> {
        match self {
            TrackServiceEnum::Api(service) => {
                service.query_gene_name(reference, gene_id, cache).await
            }
            TrackServiceEnum::Db(service) => {
                service.query_gene_name(reference, gene_id, cache).await
            }
            TrackServiceEnum::LocalDb(service) => {
                service.query_gene_name(reference, gene_id, cache).await
            }
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
        match self {
            TrackServiceEnum::Api(service) => {
                service
                    .query_k_genes_after(reference, contig, coord, k, cache)
                    .await
            }
            TrackServiceEnum::Db(service) => {
                service
                    .query_k_genes_after(reference, contig, coord, k, cache)
                    .await
            }
            TrackServiceEnum::LocalDb(service) => {
                service
                    .query_k_genes_after(reference, contig, coord, k, cache)
                    .await
            }
        }
    }

    async fn query_k_genes_before(
        &self,
        reference: &Reference,
        contig: &Contig,
        coord: usize,
        k: usize,
        cache: &mut TrackCache,
    ) -> Result<Gene, TGVError> {
        match self {
            TrackServiceEnum::Api(service) => {
                service
                    .query_k_genes_before(reference, contig, coord, k, cache)
                    .await
            }
            TrackServiceEnum::Db(service) => {
                service
                    .query_k_genes_before(reference, contig, coord, k, cache)
                    .await
            }
            TrackServiceEnum::LocalDb(service) => {
                service
                    .query_k_genes_before(reference, contig, coord, k, cache)
                    .await
            }
        }
    }

    async fn query_k_exons_after(
        &self,
        reference: &Reference,
        contig: &Contig,
        coord: usize,
        k: usize,
        cache: &mut TrackCache,
    ) -> Result<SubGeneFeature, TGVError> {
        match self {
            TrackServiceEnum::Api(service) => {
                service
                    .query_k_exons_after(reference, contig, coord, k, cache)
                    .await
            }
            TrackServiceEnum::Db(service) => {
                service
                    .query_k_exons_after(reference, contig, coord, k, cache)
                    .await
            }
            TrackServiceEnum::LocalDb(service) => {
                service
                    .query_k_exons_after(reference, contig, coord, k, cache)
                    .await
            }
        }
    }

    async fn query_k_exons_before(
        &self,
        reference: &Reference,
        contig: &Contig,
        coord: usize,
        k: usize,
        cache: &mut TrackCache,
    ) -> Result<SubGeneFeature, TGVError> {
        match self {
            TrackServiceEnum::Api(service) => {
                service
                    .query_k_exons_before(reference, contig, coord, k, cache)
                    .await
            }
            TrackServiceEnum::Db(service) => {
                service
                    .query_k_exons_before(reference, contig, coord, k, cache)
                    .await
            }
            TrackServiceEnum::LocalDb(service) => {
                service
                    .query_k_exons_before(reference, contig, coord, k, cache)
                    .await
            }
        }
    }
    // Default helper methods delegate
    async fn query_gene_track(
        &self,
        reference: &Reference,
        region: &Region,
        cache: &mut TrackCache,
    ) -> Result<Track<Gene>, TGVError> {
        match self {
            TrackServiceEnum::Api(service) => {
                service.query_gene_track(reference, region, cache).await
            }
            TrackServiceEnum::Db(service) => {
                service.query_gene_track(reference, region, cache).await
            }
            TrackServiceEnum::LocalDb(service) => {
                service.query_gene_track(reference, region, cache).await
            }
        }
    }
}

/// Download data from UCSC mariaDB to a local sqlite file.
pub struct UCSCDownloader {
    reference: Reference,
    cache_dir: String,
}

/// UCSC column type. Used to map MySQL types to SQLite types.
enum UCSCColumnType {
    UnsignedInt,
    Int,
    Float,
    Blob,
    String,
}

impl UCSCColumnType {
    fn to_sqlite_type(&self) -> &str {
        match self {
            UCSCColumnType::UnsignedInt => "INTEGER",
            UCSCColumnType::Int => "INTEGER",
            UCSCColumnType::Float => "REAL",
            UCSCColumnType::Blob => "BLOB",
            UCSCColumnType::String => "TEXT",
        }
    }
}

impl UCSCDownloader {
    pub fn new(reference: Reference, cache_dir: String) -> Result<Self, TGVError> {
        // Expand the cache directory path (handles tilde, env vars, etc.)
        let expanded_cache_dir = shellexpand::full(&cache_dir)
            .map(|expanded| expanded.into_owned())
            .map_err(|e| {
                TGVError::IOError(format!(
                    "Failed to expand cache directory '{}': {}",
                    cache_dir, e
                ))
            })?;

        Ok(Self {
            reference,
            cache_dir: expanded_cache_dir,
        })
    }

    fn reference_cache_dir(&self, reference: &Reference) -> Result<PathBuf, TGVError> {
        let reference_cache_dir = Path::new(&self.cache_dir).join(reference.to_string());

        // Ensure genome directory exists
        std::fs::create_dir_all(&reference_cache_dir)
            .map_err(|e| TGVError::IOError(format!("Failed to create genome directory: {}", e)))?;
        Ok(reference_cache_dir)
    }

    /// Download these tables: chromInfo, chromAlias, cytoBandIdeo, and gene tracks
    pub async fn download(&self) -> Result<(), TGVError> {
        use std::fs;

        // Create SQLite database file path: cache_dir/reference_name/tracks.sqlite
        let db_dir = self.reference_cache_dir(&self.reference)?;
        fs::create_dir_all(&db_dir)?;
        let db_path = db_dir.join("tracks.sqlite");

        // Connect to MariaDB
        let mysql_url = UcscDbTrackService::get_mysql_url(&self.reference, &UcscHost::Us)?;
        let mysql_pool = MySqlPoolOptions::new()
            .max_connections(5)
            .connect(&mysql_url)
            .await?;

        let sqlite_pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(&db_path)
                    .create_if_missing(true),
            )
            .await?;

        self.transfer_table(&mysql_pool, &sqlite_pool, "chromInfo")
            .await?
            .transfer_table(&mysql_pool, &sqlite_pool, "chromAlias")
            .await?
            .transfer_table(&mysql_pool, &sqlite_pool, "cytoBandIdeo")
            .await?
            .transfer_gene_tracks(&mysql_pool, &sqlite_pool)
            .await?;

        mysql_pool.close().await;

        self.download_genomes(&sqlite_pool).await?;
        sqlite_pool.close().await;

        println!(
            "Successfully downloaded {} data to {}",
            self.reference,
            db_path.display()
        );
        Ok(())
    }

    /// Download data for UCSC accessions
    /// Use UCSC API get
    /// 1. 2bit file url
    /// 2. hub url, prefered track name, and big data url for the prefered track
    async fn download_for_ucsc_accession(
        &self,
        accession: &str,
        local_path: &str,
    ) -> Result<(), TGVError> {
        let ucsc_api_service = UcscApiTrackService::new()?;
        let track_cache = TrackCache::new();
        let hub_url = ucsc_api_service
            .get_hub_url_for_genark_accession(accession)
            .await?;

        // Example response:
        // {
        //     "downloadTime": "2025:07:12T15:57:02Z",
        //     "downloadTimeStamp": 1752335822,
        //     "hubUrl": "http://hgdownload.soe.ucsc.edu/hubs/GCF/028/858/775/GCF_028858775.2/hub.txt",
        //     "genomes": {
        //       "GCF_028858775.2": {
        //         "organism": "NHGRI_mPanTro3-v2.0_pri Jan. 2024",
        //         "description": "chimpanzee (v2 AG18354 primary hap 2024 refseq)",
        //         "trackDbFile": "http://hgdownload.soe.ucsc.edu/hubs/GCF/028/858/775/GCF_028858775.2/hub.txt",
        //         "twoBitPath": "http://hgdownload.soe.ucsc.edu/hubs/GCF/028/858/775/GCF_028858775.2/GCF_028858775.2.2bit",
        //         "groups": "http://hgdownload.soe.ucsc.edu/hubs/GCF/028/858/775/GCF_028858775.2/groups.txt",
        //         "defaultPos": "NC_072398.2:77099816-77109816",
        //         "orderKey": 0
        //       }
        //     }
        //   }
        let query_url = format!(
            "https://api.genome.ucsc.edu/list/hubGenomes?hubUrl={}",
            hub_url
        );
        let response = ucsc_api_service.client.get(query_url).send().await?;
        let value: serde_json::Value = response.json().await?;
        // let track_db_path = value["genomes"][accession]["trackDbFile"].as_str().ok_or(TGVError::IOError(format!(
        //     "Failed to get hub url for {}",
        //     accession
        // )))?.to_string();
        let twobit_path = value["genomes"][accession]["twoBitPath"]
            .as_str()
            .ok_or(TGVError::IOError(format!(
                "Failed to get hub url for {}",
                accession
            )))?
            .to_string();

        let local_twobit_path = self
            .reference_cache_dir(&self.reference)?
            .join(twobit_path.clone());

        self.download_file(&twobit_path.clone(), &local_twobit_path.to_str().unwrap())
            .await?;

        // Example response:
        // {
        //  "downloadTime": "2025:07:12T16:27:23Z",
        // "downloadTimeStamp": 1752337643,
        // "hubUrl": "http://hgdownload.soe.ucsc.edu/hubs/mouseStrains/hub.txt",
        // "CAST_EiJ": {
        //     "ensGene": {
        //     "shortLabel": "Ensembl genes",
        //     "type": "bigBed 12 .",
        //     "longLabel": "Ensembl genes v86",
        //     "visibility": "pack",
        //     "group": "genes",
        //     "polished": "polished",
        //     "html": "html/GCA_001624445.1_CAST_EiJ_v1.ensGene",
        //     "searchIndex": "name",
        //     "priority": "40",
        //     "searchTrix": "http://hgdownload.soe.ucsc.edu/hubs/mouseStrains/GCA_001624445.1_CAST_EiJ_v1/ixIxx/GCA_001624445.1_CAST_EiJ_v1.ensGene.name.ix",
        //     "bigDataUrl": "http://hgdownload.soe.ucsc.edu/hubs/mouseStrains/GCA_001624445.1_CAST_EiJ_v1/bbi/GCA_001624445.1_CAST_EiJ_v1.ensGene.bb",
        //     "color": "150,0,0"
        //     },
        //     "allGaps": {
        //       ...
        //     }
        // }

        let response = ucsc_api_service
            .client
            .get(format!(
                "https://api.genome.ucsc.edu/list/tracks?hubUrl={}&genome={}",
                hub_url, accession
            ))
            .send()
            .await?;
        let value: serde_json::Value = response.json().await?;
        let track_names = get_all_track_names(value.get(accession.clone()).ok_or(
            TGVError::IOError("Failed to get genome from UCSC API".to_string()),
        )?)?;

        let prefered_track = get_preferred_track_name_from_vec(&track_names)?.ok_or(
            TGVError::IOError(format!("Failed to find prefered track for {}", accession)),
        )?;

        let big_data_path = value[accession][prefered_track]["bigDataUrl"]
            .as_str()
            .ok_or(TGVError::IOError(format!(
                "Failed to get big data url for {}",
                accession
            )))?;

        let local_big_data_path = self
            .reference_cache_dir(&self.reference)?
            .join(big_data_path);
        self.download_file(&big_data_path, &local_big_data_path.to_str().unwrap())
            .await?;

        BigBedConverter::new(big_data_path, reference.clone())?
            .covert_with_track_name(prefered_track, &sqlite_pool)?;

        Ok(())
    }

    async fn transfer_table(
        &self,
        mysql_pool: &MySqlPool,
        sqlite_pool: &SqlitePool,
        table_name: &str,
    ) -> Result<&Self, TGVError> {
        let mut column_types: HashMap<String, UCSCColumnType> = HashMap::new();

        // Check if table exists and get its structure
        let columns_info = sqlx::query(&format!("SHOW COLUMNS FROM {}", table_name))
            .fetch_all(mysql_pool)
            .await?;
        if columns_info.is_empty() {
            return Err(TGVError::IOError(format!(
                "{} table has no columns.",
                table_name
            )));
        }

        // Map MySQL types to SQLite types
        let mut column_defs: Vec<String> = Vec::new();
        let mut valid_columns = Vec::new();

        for col_info in &columns_info {
            let field_name: String = col_info.try_get("Field")?;
            let mysql_type: String = col_info.try_get("Type")?;

            match mysql_type.to_lowercase() {
                t if t.contains("int")
                    || t.contains("tinyint")
                    || t.contains("smallint")
                    || t.contains("mediumint")
                    || t.contains("bigint") =>
                {
                    if t.contains("unsigned") {
                        column_types.insert(field_name.clone(), UCSCColumnType::UnsignedInt);
                    } else {
                        column_types.insert(field_name.clone(), UCSCColumnType::Int);
                    }
                }
                t if t.contains("float")
                    || t.contains("double")
                    || t.contains("decimal")
                    || t.contains("numeric") =>
                {
                    column_types.insert(field_name.clone(), UCSCColumnType::Float);
                }
                t if t.contains("blob") || t.contains("binary") => {
                    column_types.insert(field_name.clone(), UCSCColumnType::Blob);
                }
                t if t.contains("char")
                    || t.contains("text")
                    || t.contains("varchar")
                    || t.contains("enum")
                    || t.contains("set") =>
                {
                    column_types.insert(field_name.clone(), UCSCColumnType::String);
                }
                _ => {
                    return Err(TGVError::IOError(format!(
                        "Skipping unsupported column type: {} {}",
                        field_name, mysql_type
                    )));
                }
            };

            column_defs.push(format!(
                "{} {}",
                field_name.clone(),
                column_types[&field_name].to_sqlite_type()
            ));
            valid_columns.push(field_name);
        }

        if valid_columns.is_empty() {
            return Err(TGVError::IOError(format!(
                "{} table has no supported columns.",
                table_name
            )));
        }

        // Drop existing table if it exists, then create fresh
        sqlx::query(&format!("DROP TABLE IF EXISTS {}", table_name))
            .execute(sqlite_pool)
            .await?;

        sqlx::query(&format!(
            "CREATE TABLE {} ({})",
            table_name,
            column_defs.join(", ")
        ))
        .execute(sqlite_pool)
        .await?;

        // Transfer data
        let select_columns = valid_columns.join(", ");
        let query_sql = format!("SELECT {} FROM {}", select_columns, table_name);
        let rows = sqlx::query(&query_sql).fetch_all(mysql_pool).await?;

        if rows.is_empty() {
            println!("{} table is empty, skipping data transfer", table_name);
            return Ok(self);
        }

        // Insert data
        let placeholders = vec!["?"; valid_columns.len()].join(", ");
        let insert_sql = format!(
            "INSERT OR REPLACE INTO {} ({}) VALUES ({})",
            table_name, select_columns, placeholders
        );

        for row in &rows {
            let mut query = sqlx::query(&insert_sql);
            for col_name in &valid_columns {
                // Bind values based on SQLite type
                match column_types[col_name] {
                    UCSCColumnType::UnsignedInt => {
                        let value: u64 = row.try_get(col_name.as_str())?;
                        query = query.bind(value as i64);
                    }
                    UCSCColumnType::Int => {
                        let value: i64 = row.try_get(col_name.as_str())?;
                        query = query.bind(value);
                    }
                    UCSCColumnType::Float => {
                        let value: f64 = row.try_get(col_name.as_str())?;
                        query = query.bind(value);
                    }
                    UCSCColumnType::Blob => {
                        let value: Vec<u8> = row.try_get(col_name.as_str())?;
                        query = query.bind(value);
                    }
                    UCSCColumnType::String => {
                        let value: String = row.try_get(col_name.as_str())?;
                        query = query.bind(value);
                    }
                }
            }

            query.execute(sqlite_pool).await?;
        }

        println!(
            "Transferred {} table ({} columns, {} rows)",
            table_name,
            valid_columns.len(),
            rows.len()
        );
        Ok(self)
    }

    async fn transfer_gene_tracks(
        &self,
        mysql_pool: &MySqlPool,
        sqlite_pool: &SqlitePool,
    ) -> Result<&Self, TGVError> {
        // Get list of available gene tracks
        let table_rows = sqlx::query("SHOW TABLES").fetch_all(mysql_pool).await?;

        let available_tracks: Vec<String> = table_rows
            .into_iter()
            .map(|row| row.try_get::<String, usize>(0))
            .collect::<Result<Vec<String>, sqlx::Error>>()?;

        let preferred_track = get_preferred_track_name_from_vec(&available_tracks)?;

        if let Some(track_name) = preferred_track {
            self.transfer_table(mysql_pool, sqlite_pool, &track_name)
                .await?;
        } else {
            println!("No preferred gene track found");
        }

        Ok(self)
    }

    async fn download_genomes(&self, sqlite_pool: &SqlitePool) -> Result<(), TGVError> {
        println!("Downloading genome files...");

        // Query SQLite for unique fileName values from chromInfo table
        let rows = sqlx::query(
            "SELECT DISTINCT fileName FROM chromInfo WHERE fileName IS NOT NULL AND fileName != ''",
        )
        .fetch_all(sqlite_pool)
        .await?;

        if rows.is_empty() {
            println!("No genome files found in chromInfo table");
            return Ok(());
        }

        // Create HTTP client
        let cache_dir = self.reference_cache_dir(&self.reference)?;

        for row in rows {
            let file_name: String = row.try_get("fileName")?;
            let download_url = format!("http://hgdownload.soe.ucsc.edu/{}", file_name);

            // Extract just the filename part from the full path
            let actual_filename = std::path::Path::new(&file_name)
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or(&file_name);
            let file_path = cache_dir.join(actual_filename);

            self.download_file(&download_url, &file_path.to_str().unwrap())
                .await?;
        }

        println!("Genome file download completed");
        Ok(())
    }

    async fn download_file(&self, url: &str, local_path: &str) -> Result<(), TGVError> {
        let client = reqwest::Client::new();

        // Skip if file already exists
        if Path::new(local_path).exists() {
            println!("Genome file {} already exists, skipping", local_path);
            return Ok(());
        }

        println!("Downloading file: {}", local_path);

        // Download the file
        match client.get(url).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    let content = response.bytes().await.map_err(|e| {
                        TGVError::IOError(format!("Failed to read response bytes: {}", e))
                    })?;

                    // Write file to disk
                    std::fs::write(local_path, content).map_err(|e| {
                        TGVError::IOError(format!("Failed to write file {}: {}", local_path, e))
                    })?;

                    println!("Downloaded: {}", local_path);
                } else {
                    println!(
                        "Failed to download {}: HTTP {}",
                        local_path,
                        response.status()
                    );
                }
            }
            Err(e) => {
                println!("Failed to download {}: {}", local_path, e);
            }
        }

        Ok(())
    }
}

/// Convert BigBed files to SQLite database
pub struct BigBedConverter {
    bigbed_path: String,
    reference: Reference,
}

impl BigBedConverter {
    pub fn new(bigbed_path: &str, reference: Reference) -> Result<Self, TGVError> {
        Ok(Self {
            bigbed_path: bigbed_path.to_string(),
            reference,
        })
    }

    pub async fn convert_with_track_name(
        &self,
        track_name: &str,
        sqlite_pool: &Pool<Sqlite>,
    ) -> Result<(), TGVError> {
        use bigtools::BigBedRead;

        // Ensure the BigBed file exists
        if !Path::new(&self.bigbed_path).exists() {
            return Err(TGVError::IOError(format!(
                "BigBed file not found: {}",
                self.bigbed_path
            )));
        }

        // Open BigBed file
        let mut bigbed_reader = BigBedRead::open_file(&self.bigbed_path)?;

        // Drop existing table if it exists
        sqlx::query(&format!("DROP TABLE IF EXISTS {}", track_name))
            .execute(sqlite_pool)
            .await?;

        // Create table with gene track schema
        sqlx::query(&format!(
            "CREATE TABLE {} (
                name TEXT,
                chrom TEXT,
                strand TEXT,
                txStart INTEGER,
                txEnd INTEGER,
                cdsStart INTEGER,
                cdsEnd INTEGER,
                name2 TEXT,
                exonStarts BLOB,
                exonEnds BLOB
            )",
            track_name
        ))
        .execute(sqlite_pool)
        .await?;

        let mut transaction = sqlite_pool.begin().await?;

        // Get all chromosomes from BigBed
        let mut record_count = 0;
        let chromosomes: Vec<(String, u32)> = bigbed_reader
            .chroms()
            .iter()
            .map(|chrom_info| (chrom_info.name.clone(), chrom_info.length))
            .collect();

        for (chromosome_name, chromosome_length) in chromosomes {
            for interval in bigbed_reader.get_interval(&chromosome_name, 0, chromosome_length)? {
                let interval = interval?;
                // Parse BED12 fields
                let fields: Vec<&str> = interval.rest.split('\t').collect();

                let name = fields[0].to_string();
                let score = fields[1]; // Not used but part of BED12
                let strand = fields[2].to_string();
                let thick_start: u32 = fields[3].parse().unwrap_or(interval.start);
                let thick_end: u32 = fields[4].parse().unwrap_or(interval.end);
                let item_rgb = fields[5]; // Not used
                let block_count: u32 = fields[6].parse().unwrap_or(0);
                let exon_starts_blob = fields.get(7).unwrap().as_bytes(); // TODO
                let exon_ends_blob = fields.get(8).unwrap().as_bytes(); // TODO

                // Insert into database
                sqlx::query(&format!(
                    "INSERT INTO {} (name, chrom, strand, txStart, txEnd, cdsStart, cdsEnd, name2, exonStarts, exonEnds) 
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    track_name
                ))
                .bind(&name)
                .bind(&chromosome_name)
                .bind(&strand)
                .bind(interval.start as i64)
                .bind(interval.end as i64)
                .bind(thick_start as i64)
                .bind(thick_end as i64)
                .bind(&name) // Use name as name2 if no alternative available
                .bind(&exon_starts_blob)
                .bind(&exon_ends_blob)
                .execute(&mut *transaction)
                .await?;

                record_count += 1;
            }
        }

        transaction.commit().await?;
        sqlite_pool.close().await;

        println!(
            "Successfully converted {} records from BigBed to SQLite table '{}'",
            record_count, track_name
        );

        Ok(())
    }

    fn convert_blocks_to_exons(
        &self,
        tx_start: u32,
        block_sizes_str: &str,
        block_starts_str: &str,
    ) -> Result<(Vec<u32>, Vec<u32>), TGVError> {
        if block_sizes_str.is_empty() || block_starts_str.is_empty() {
            // No block information, treat as single exon
            return Ok((vec![tx_start], vec![tx_start]));
        }

        let block_sizes: Vec<u32> = block_sizes_str
            .trim_end_matches(',')
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|s| s.parse().unwrap_or(0))
            .collect();

        let block_starts: Vec<u32> = block_starts_str
            .trim_end_matches(',')
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|s| s.parse().unwrap_or(0))
            .collect();

        if block_sizes.len() != block_starts.len() {
            return Err(TGVError::ValueError(
                "Block sizes and starts arrays have different lengths".to_string(),
            ));
        }

        let mut exon_starts = Vec::new();
        let mut exon_ends = Vec::new();

        for (i, &block_size) in block_sizes.iter().enumerate() {
            let exon_start = tx_start + block_starts[i];
            let exon_end = exon_start + block_size;
            exon_starts.push(exon_start);
            exon_ends.push(exon_end);
        }

        Ok((exon_starts, exon_ends))
    }

    fn coords_to_blob(&self, coords: &[u32]) -> Vec<u8> {
        let coords_str = coords
            .iter()
            .map(|c| c.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let coords_str = format!("{},", coords_str); // Add trailing comma to match UCSC format
        coords_str.into_bytes()
    }
}

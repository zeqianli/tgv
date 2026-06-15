use crate::{
    contig_header::{Contig, ContigHeader},
    cytoband::{Cytoband, CytobandSegment},
    error::TGVError,
    feature::{Gene, SubGeneFeature},
    intervals::GenomeInterval,
    intervals::Region,
    reference::Reference,
    track::Track,
    tracks::UcscHost,
    tracks::schema::*,
};
use async_trait::async_trait;
use sqlx::{Column, MySqlPool, Row, mysql::MySqlPoolOptions};
use std::collections::HashMap;
use std::sync::Arc;

const UCSC_HGCENTRAL_URL: &str = "mysql://genome@genome-mysql.soe.ucsc.edu/hgcentral";

#[derive(Debug)]
pub struct UcscDbTrackService {
    pool: Arc<MySqlPool>,

    cache: TrackCache,
}
use crate::tracks::{TRACK_PREFERENCES, TrackCache, TrackService};

impl UcscDbTrackService {
    // Initialize the database connections. Reference is needed to find the corresponding schema.
    pub async fn new(reference: &Reference, ucsc_host: &UcscHost) -> Result<Self, TGVError> {
        let mysql_url = UcscDbTrackService::get_mysql_url(reference, ucsc_host)?;
        log::info!(
            "Database connect: database=ucsc-mysql connection={} context=reference={} host={}",
            mysql_url,
            reference,
            ucsc_host.to_string()
        );
        let pool = MySqlPoolOptions::new()
            .max_connections(1)
            .connect(&mysql_url)
            .await?;

        Ok(Self {
            pool: Arc::new(pool),
            cache: TrackCache::default(),
        })
    }

    pub fn get_mysql_url(reference: &Reference, ucsc_host: &UcscHost) -> Result<String, TGVError> {
        match reference {
            Reference::Hg19 | Reference::Hg38 | Reference::UcscGenome(_) => Ok(format!(
                "mysql://genome@{}/{}",
                ucsc_host.url(),
                reference
            )),
            _ => Err(TGVError::ValueError(format!(
                "Unsupported reference: {}",
                reference
            ))),
        }
    }

    pub async fn list_assemblies(n: Option<usize>) -> Result<Vec<(String, String)>, TGVError> {
        log::info!(
            "Database connect: database=ucsc-mysql connection={} context=list assemblies",
            UCSC_HGCENTRAL_URL
        );
        let connection = MySqlPoolOptions::new()
            .max_connections(5)
            .connect(UCSC_HGCENTRAL_URL)
            .await?;

        let rows = if let Some(n) = n {
            let sql = "SELECT name, organism FROM dbDb LIMIT ?";
            log::info!(
                "Database query: database=ucsc-mysql sql=\"{}\" context=list assemblies limit={}",
                sql,
                n
            );
            sqlx::query(sql)
                .bind(n as i32)
                .fetch_all(&connection)
                .await?
        } else {
            let sql = "SELECT name, organism FROM dbDb";
            log::info!(
                "Database query: database=ucsc-mysql sql=\"{}\" context=list assemblies",
                sql
            );
            sqlx::query(sql).fetch_all(&connection).await?
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
        log::info!(
            "Database connect: database=ucsc-mysql connection={} context=list accessions",
            UCSC_HGCENTRAL_URL
        );
        let connection = MySqlPoolOptions::new()
            .max_connections(5)
            .connect(UCSC_HGCENTRAL_URL)
            .await?;

        let sql = "SELECT name, organism FROM dbDb ORDER BY organism, name LIMIT ? OFFSET ?";
        log::info!(
            "Database query: database=ucsc-mysql sql=\"{}\" context=list accessions limit={} offset={}",
            sql,
            n,
            offset
        );
        let rows = sqlx::query(sql)
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

    async fn get_preferred_track_name_with_cache(
        &mut self,
        reference: &Reference,
    ) -> Result<String, TGVError> {
        match self.cache.preferred_track_name.as_ref() {
            None => {
                let preferred_track = self.get_preferred_track_name(reference).await?;
                self.cache.set_preferred_track_name(preferred_track.clone());
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
        _reference: &Reference,
        contig_header: &ContigHeader,
    ) -> Result<HashMap<usize, Option<String>>, TGVError> {
        let sql =
            "SELECT chrom, fileName FROM chromInfo WHERE chrom NOT LIKE 'chr%\\_%' ESCAPE '\\'";
        log::info!(
            "Database query: database=ucsc-mysql sql=\"{}\" context=get contig 2bit file lookup",
            sql
        );
        let rows_with_alias = sqlx::query(sql).fetch_all(&*self.pool).await?;

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
            filename_hashmap.insert(contig_header.try_get_index_by_str(&chrom)?, basename);
        }

        Ok(filename_hashmap)
    }
}

#[async_trait]
impl TrackService for UcscDbTrackService {
    async fn close(&mut self) -> Result<(), TGVError> {
        self.pool.close().await;
        Ok(())
    }

    async fn get_all_contigs(&mut self, _reference: &Reference) -> Result<Vec<Contig>, TGVError> {
        // Some references have chromAlias table, some don't.
        let sql = "SELECT
                chromInfo.chrom as chrom,
                chromInfo.size as size,
                GROUP_CONCAT(chromAlias.alias SEPARATOR ',') as aliases
            FROM chromInfo
            LEFT JOIN chromAlias ON chromAlias.chrom = chromInfo.chrom
            WHERE chromInfo.chrom NOT LIKE 'chr%\\_%'
            GROUP BY chromInfo.chrom
            ORDER BY chromInfo.chrom
            ";
        log::info!(
            "Database query: database=ucsc-mysql sql=\"{}\" context=get all contigs with aliases",
            sql
        );
        let contigs: Vec<ContigRow> = match sqlx::query_as(sql).fetch_all(&*self.pool).await {
            Ok(contigs) => contigs,
            Err(error) => {
                log::warn!("Falling back to contig query without aliases: {error}");
                let fallback_sql = "SELECT
                    chromInfo.chrom as chrom,
                    chromInfo.size as size
                FROM chromInfo
                WHERE chromInfo.chrom NOT LIKE 'chr%\\_%'
                ORDER BY chromInfo.chrom";
                log::info!(
                    "Database query: database=ucsc-mysql sql=\"{}\" context=get all contigs without aliases",
                    fallback_sql
                );
                sqlx::query_as(fallback_sql).fetch_all(&*self.pool).await?
            }
        };

        let mut contigs = contigs
            .into_iter()
            .map(|row| row.to_contig())
            .collect::<Result<Vec<Contig>, TGVError>>()?;

        contigs.sort_by(|a, b| {
            if a.name.starts_with("chr") || b.name.starts_with("chr") {
                Contig::contigs_compare(a, b)
            } else {
                b.length.cmp(&a.length) // Sort by length in descending order
            }
        });

        return Ok(contigs);
    }

    async fn get_cytoband(
        &mut self,
        reference: &Reference,
        contig_index: usize,

        contig_header: &ContigHeader,
    ) -> Result<Option<Cytoband>, TGVError> {
        let contig_name = match contig_header.try_get(contig_index)?.get_track_name() {
            Some(contig_name) => contig_name,
            None => return Ok(None),
        };
        let sql =
            "SELECT chrom, chromStart, chromEnd, name, gieStain FROM cytoBandIdeo WHERE chrom = ?";
        log::info!(
            "Database query: database=ucsc-mysql sql=\"{}\" context=get cytoband reference={} contig={}",
            sql,
            reference,
            contig_name
        );
        let cytoband_segment_rows: Vec<CytobandSegmentRow> = sqlx::query_as(sql)
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
        &mut self,
        reference: &Reference,
    ) -> Result<Option<String>, TGVError> {
        match reference {
            // Speed up for human genomes
            Reference::Hg19 | Reference::Hg38 => return Ok(Some("ncbiRefSeqSelect".to_string())),
            _ => {}
        }

        let sql = "SHOW TABLES";
        log::info!(
            "Database query: database=ucsc-mysql sql=\"{}\" context=get preferred track reference={}",
            sql,
            reference
        );
        let gene_track_rows = sqlx::query(sql).fetch_all(&*self.pool).await?;

        let available_gene_tracks: Vec<String> = gene_track_rows
            .into_iter()
            .map(|row| row.try_get::<String, usize>(0))
            .collect::<Result<Vec<String>, sqlx::Error>>()?;
        for pref in TRACK_PREFERENCES {
            if available_gene_tracks.contains(&pref.to_string()) {
                return Ok(Some(pref.to_string()));
            }
        }

        Ok(None)
    }

    async fn query_genes_overlapping(
        &mut self,
        reference: &Reference,
        region: &Region,

        contig_header: &ContigHeader,
    ) -> Result<Vec<Gene>, TGVError> {
        let contig_name = match contig_header
            .try_get(region.contig_index())?
            .get_track_name()
        {
            Some(contig_name) => contig_name,
            None => return Ok(Vec::new()), // Contig doesn't have track data
        };
        let track_name = self.get_preferred_track_name_with_cache(reference).await?;
        let sql = format!(
            "SELECT * FROM {}
             WHERE chrom = ? AND (txStart <= ?) AND (txEnd >= ?)",
            track_name
        );
        log::info!(
            "Database query: database=ucsc-mysql sql=\"{}\" context=query overlapping genes reference={} track={} contig={} contig_index={} start={} end={}",
            sql,
            reference,
            track_name,
            contig_name,
            region.contig_index(),
            region.start(),
            region.end()
        );
        let gene_rows: Vec<UcscGeneRow> = sqlx::query_as(sql.as_str())
            .bind(contig_name)
            .bind(u64::try_from(region.end()).unwrap()) // end is 1-based inclusive, UCSC is 0-based exclusive
            .bind(u64::try_from(region.start().saturating_sub(1)).unwrap()) // start is 1-based inclusive, UCSC is 0-based inclusive
            .fetch_all(&*self.pool)
            .await?;

        gene_rows
            .into_iter()
            .map(|row| row.to_gene(contig_header))
            .collect::<Result<Vec<Gene>, TGVError>>()
    }

    async fn query_gene_covering(
        &mut self,
        reference: &Reference,
        contig_index: usize,
        coord: u64,
        contig_header: &ContigHeader,
    ) -> Result<Option<Gene>, TGVError> {
        let contig_name = match contig_header.try_get(contig_index)?.get_track_name() {
            Some(contig_name) => contig_name,
            None => {
                return Err(TGVError::StateError(format!(
                    "Contig {} (index = {}, aliases = {}) does not have track data.",
                    contig_header.contigs[contig_index].name,
                    contig_index,
                    contig_header.contigs[contig_index].aliases.join(",")
                )));
            }
        };
        let track_name = self.get_preferred_track_name_with_cache(reference).await?;
        let sql = format!(
            "SELECT *
             FROM {}
             WHERE chrom = ? AND txStart <= ? AND txEnd >= ?",
            track_name,
        );
        log::info!(
            "Database query: database=ucsc-mysql sql=\"{}\" context=query gene covering reference={} track={} contig={} contig_index={} coord={}",
            sql,
            reference,
            track_name,
            contig_name,
            contig_index,
            coord
        );
        let gene_row: Option<UcscGeneRow> = sqlx::query_as(sql.as_str())
            .bind(contig_name)
            .bind(u32::try_from(coord.saturating_sub(1)).unwrap()) // coord is 1-based inclusive, UCSC is 0-based inclusive
            .bind(u32::try_from(coord).unwrap()) // coord is 1-based inclusive, UCSC is 0-based exclusive
            .fetch_optional(&*self.pool)
            .await?;

        gene_row.map(|row| row.to_gene(contig_header)).transpose()
    }

    async fn query_gene_name(
        &mut self,
        reference: &Reference,
        gene_name: &str,

        contig_header: &ContigHeader,
    ) -> Result<Gene, TGVError> {
        let track_name = self.get_preferred_track_name_with_cache(reference).await?;
        let sql = format!(
            "SELECT *
            FROM {}
            WHERE name2 = ?",
            track_name
        );
        log::info!(
            "Database query: database=ucsc-mysql sql=\"{}\" context=query gene by name reference={} track={} gene={}",
            sql,
            reference,
            track_name,
            gene_name
        );
        let gene_row: Option<UcscGeneRow> = sqlx::query_as(sql.as_str())
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
        &mut self,
        reference: &Reference,
        contig_index: usize,
        coord: u64,
        k: usize,

        contig_header: &ContigHeader,
    ) -> Result<Gene, TGVError> {
        let contig_name = match contig_header.try_get(contig_index)?.get_track_name() {
            Some(contig_name) => contig_name,
            None => {
                return Err(TGVError::StateError(format!(
                    "Contig {} (index = {}, aliases = {}) does not have track data.",
                    contig_header.contigs[contig_index].name,
                    contig_index,
                    contig_header.contigs[contig_index].aliases.join(",")
                )));
            }
        };

        if k == 0 {
            return Err(TGVError::ValueError("k cannot be 0".to_string()));
        }

        let track_name = self.get_preferred_track_name_with_cache(reference).await?;
        let sql = format!(
            "SELECT *
             FROM {}
             WHERE chrom = ? AND txEnd >= ?
             ORDER BY txEnd ASC LIMIT ?",
            track_name,
        );
        log::info!(
            "Database query: database=ucsc-mysql sql=\"{}\" context=query k genes after reference={} track={} contig={} contig_index={} coord={} k={}",
            sql,
            reference,
            track_name,
            contig_name,
            contig_index,
            coord,
            k
        );
        let gene_rows: Vec<UcscGeneRow> = sqlx::query_as(sql.as_str())
            .bind(contig_name)
            .bind(u32::try_from(coord).unwrap()) // coord is 1-based inclusive, UCSC is 0-based exclusive
            .bind(u32::try_from(k + 1).unwrap())
            .fetch_all(&*self.pool)
            .await?;

        Track::from_gene_rows(gene_rows, contig_index, contig_header)?
            .get_saturating_k_genes_after(coord, k)
            .cloned()
            .ok_or(TGVError::IOError("No genes found".to_string()))
    }

    async fn query_k_genes_before(
        &mut self,
        reference: &Reference,
        contig_index: usize,
        coord: u64,
        k: usize,
        contig_header: &ContigHeader,
    ) -> Result<Gene, TGVError> {
        let contig_name = match contig_header.try_get(contig_index)?.get_track_name() {
            Some(contig_name) => contig_name,
            None => {
                return Err(TGVError::StateError(format!(
                    "Contig {} (index = {}, aliases = {}) does not have track data.",
                    contig_header.contigs[contig_index].name,
                    contig_index,
                    contig_header.contigs[contig_index].aliases.join(",")
                )));
            }
        };
        if k == 0 {
            return Err(TGVError::ValueError("k cannot be 0".to_string()));
        }

        let track_name = self.get_preferred_track_name_with_cache(reference).await?;
        let sql = format!(
            "SELECT *
             FROM {}
             WHERE chrom = ? AND txStart <= ?
             ORDER BY txStart DESC LIMIT ?",
            track_name,
        );
        log::info!(
            "Database query: database=ucsc-mysql sql=\"{}\" context=query k genes before reference={} track={} contig={} contig_index={} coord={} k={}",
            sql,
            reference,
            track_name,
            contig_name,
            contig_index,
            coord,
            k
        );
        let gene_rows: Vec<UcscGeneRow> = sqlx::query_as(sql.as_str())
            .bind(contig_name)
            .bind(u32::try_from(coord.saturating_sub(1)).unwrap()) // coord is 1-based inclusive, UCSC is 0-based inclusive
            .bind(u32::try_from(k + 1).unwrap())
            .fetch_all(&*self.pool)
            .await?;

        Track::from_gene_rows(gene_rows, contig_index, contig_header)?
            .get_saturating_k_genes_before(coord, k)
            .cloned()
            .ok_or(TGVError::IOError("No genes found".to_string()))
    }

    async fn query_k_exons_after(
        &mut self,
        reference: &Reference,
        contig_index: usize,
        coord: u64,
        k: usize,

        contig_header: &ContigHeader,
    ) -> Result<SubGeneFeature, TGVError> {
        let contig_name = match contig_header.try_get(contig_index)?.get_track_name() {
            Some(contig_name) => contig_name,
            None => {
                return Err(TGVError::StateError(format!(
                    "Contig {} (index = {}, aliases = {}) does not have track data.",
                    contig_header.contigs[contig_index].name,
                    contig_index,
                    contig_header.contigs[contig_index].aliases.join(",")
                )));
            }
        };
        if k == 0 {
            return Err(TGVError::ValueError("k cannot be 0".to_string()));
        }

        let track_name = self.get_preferred_track_name_with_cache(reference).await?;
        let sql = format!(
            "SELECT *
             FROM {}
             WHERE chrom = ? AND txEnd >= ?
             ORDER BY txEnd ASC LIMIT ?",
            track_name,
        );
        log::info!(
            "Database query: database=ucsc-mysql sql=\"{}\" context=query k exons after reference={} track={} contig={} contig_index={} coord={} k={}",
            sql,
            reference,
            track_name,
            contig_name,
            contig_index,
            coord,
            k
        );
        let gene_rows: Vec<UcscGeneRow> = sqlx::query_as(sql.as_str())
            .bind(contig_name)
            .bind(u32::try_from(coord).unwrap()) // coord is 1-based inclusive, UCSC is 0-based exclusive
            .bind(u32::try_from(k + 1).unwrap())
            .fetch_all(&*self.pool)
            .await?;

        Track::from_gene_rows(gene_rows, contig_index, contig_header)?
            .get_saturating_k_exons_after(coord, k)
            .ok_or(TGVError::IOError("No exons found".to_string()))
    }

    async fn query_k_exons_before(
        &mut self,
        reference: &Reference,
        contig_index: usize,
        coord: u64,
        k: usize,
        contig_header: &ContigHeader,
    ) -> Result<SubGeneFeature, TGVError> {
        let contig_name = match contig_header.try_get(contig_index)?.get_track_name() {
            Some(contig_name) => contig_name,
            None => {
                return Err(TGVError::StateError(format!(
                    "Contig {} (index = {}, aliases = {}) does not have track data.",
                    contig_header.contigs[contig_index].name,
                    contig_index,
                    contig_header.contigs[contig_index].aliases.join(",")
                )));
            }
        };
        if k == 0 {
            return Err(TGVError::ValueError("k cannot be 0".to_string()));
        }

        let track_name = self.get_preferred_track_name_with_cache(reference).await?;
        let sql = format!(
            "SELECT *
             FROM {}
             WHERE chrom = ? AND txStart <= ?
             ORDER BY txStart DESC LIMIT ?",
            track_name,
        );
        log::info!(
            "Database query: database=ucsc-mysql sql=\"{}\" context=query k exons before reference={} track={} contig={} contig_index={} coord={} k={}",
            sql,
            reference,
            track_name,
            contig_name,
            contig_index,
            coord,
            k
        );
        let gene_rows: Vec<UcscGeneRow> = sqlx::query_as(sql.as_str())
            .bind(contig_name)
            .bind(u32::try_from(coord.saturating_sub(1)).unwrap()) // coord is 1-based inclusive, UCSC is 0-based inclusive
            .bind(u32::try_from(k + 1).unwrap())
            .fetch_all(&*self.pool)
            .await?;

        Track::from_gene_rows(gene_rows, contig_index, contig_header)?
            .get_saturating_k_exons_before(coord, k)
            .ok_or(TGVError::IOError("No exons found".to_string()))
    }
}

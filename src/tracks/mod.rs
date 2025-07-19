mod downloader;
mod local_db;
mod ucsc_api;
mod ucsc_db;

use crate::{
    contig::Contig,
    cytoband::Cytoband,
    error::TGVError,
    feature::{Gene, SubGeneFeature},
    reference::Reference,
    region::Region,
    track::Track,
    traits::GenomeInterval,
};
use async_trait::async_trait;
use sqlx::{Column, Row};
use std::collections::HashMap;

pub use downloader::UCSCDownloader;
pub use local_db::LocalDbTrackService;
pub use ucsc_api::UcscApiTrackService;
pub use ucsc_db::UcscDbTrackService;

/// Default track ordering when rendering the gene track.
const TRACK_PREFERENCES: [&str; 5] = [
    "ncbiRefSeqSelect",
    "ncbiRefSeqCurated",
    "ncbiRefSeq",
    "ncbiGene",
    "refGenes",
];

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

// --- Enum Wrapper ---

/// Enum to hold different TrackService implementations
#[derive(Debug)]
pub enum TrackServiceEnum {
    Api(UcscApiTrackService),
    Db(UcscDbTrackService),
    LocalDb(LocalDbTrackService),
}

impl TrackServiceEnum {
    /// Return a map of: contig name -> 2bit file basename, if available.
    /// If not available, the value is None.    
    pub async fn get_contig_2bit_file_lookup(
        &self,
        reference: &Reference,
    ) -> Result<HashMap<String, Option<String>>, TGVError> {
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

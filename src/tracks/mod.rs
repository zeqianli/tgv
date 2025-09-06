mod downloader;
mod local_db;
pub mod schema;
mod ucsc_api;
mod ucsc_db;

use crate::{
    contig_header::{Contig, ContigHeader},
    cytoband::Cytoband,
    error::TGVError,
    feature::{Gene, SubGeneFeature},
    intervals::GenomeInterval,
    reference::Reference,
    intervals::Region,
    track::Track,
};
use async_trait::async_trait;
use chrono::Local;
use std::collections::{HashMap, HashSet};

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
    pub tracks: HashMap<usize, Track<Gene>>,

    /// Contig index -> whether the track has been quried
    contig_queried: HashSet<usize>,

    /// Gene name -> index in tracks.
    /// If the gene name is not found, the value is None.
    gene_name_lookup: HashMap<String, usize>,

    gene_name_quried: HashSet<String>,

    /// Prefered track name.
    /// None: Not initialized.
    /// Some(None): Queried but not found.
    /// Some(Some(name)): Queried and found.
    pub preferred_track_name: Option<Option<String>>,

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
            tracks: HashMap::new(),
            contig_queried: HashSet::new(),
            gene_name_lookup: HashMap::new(),
            gene_name_quried: HashSet::new(),
            preferred_track_name: None,
            hub_url: None,
        }
    }

    pub fn contig_quried(&self, contig_index: &usize) -> bool {
        self.contig_queried.contains(contig_index)
    }

    pub fn gene_quried(&self, gene_name: &str) -> bool {
        self.gene_name_quried.contains(gene_name)
    }

    // pub fn includes_gene(&self, gene_name: &str) -> bool {
    //     self.gene_by_name.contains_key(gene_name)
    // }

    /// Note that this returns None both when the gene is not queried,
    ///    and returns Some(None) when the gene is queried but the gene data is not found.
    pub fn get_gene(&self, gene_name: &str) -> Option<&Gene> {
        match self.gene_name_lookup.get(gene_name) {
            None => None,
            Some(index) => match self.tracks.get(index) {
                None => None,
                Some(track) => track.gene_by_name(gene_name),
            },
        }
    }

    pub fn add_track(&mut self, contig_index: usize, track: Track<Gene>) {
        for (i, gene) in track.genes().iter().enumerate() {
            self.gene_name_lookup.insert(gene.name.clone(), i);
        }
        self.tracks.insert(contig_index, track);
        self.contig_queried.insert(contig_index);
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

    // Query contigs data given a reference.
    async fn get_all_contigs(
        &self,
        reference: &Reference,
        cache: &mut TrackCache,
    ) -> Result<Vec<Contig>, TGVError>;

    // Return the cytoband data given a reference and a contig.
    async fn get_cytoband(
        &self,
        reference: &Reference,
        contig_index: usize,
        cache: &mut TrackCache,
        contig_header: &ContigHeader,
    ) -> Result<Option<Cytoband>, TGVError>;

    /// Return a Track<Gene> that covers a region.
    async fn query_gene_track(
        &self,
        reference: &Reference,
        region: &Region,
        cache: &mut TrackCache,
        contig_header: &ContigHeader,
    ) -> Result<Track<Gene>, TGVError> {
        let genes = self
            .query_genes_overlapping(reference, region, cache, contig_header)
            .await?;
        Track::from_genes(genes, region.contig_index)
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
        contig_header: &ContigHeader,
    ) -> Result<Vec<Gene>, TGVError>;

    /// Return the Gene covering a contig:coordinate.
    async fn query_gene_covering(
        &self,
        reference: &Reference,
        contig_index: usize,
        coord: usize,
        cache: &mut TrackCache,
        contig_header: &ContigHeader,
    ) -> Result<Option<Gene>, TGVError>;

    async fn query_gene_name(
        &self,
        reference: &Reference,
        gene_name: &String,
        cache: &mut TrackCache,
        contig_header: &ContigHeader,
    ) -> Result<Gene, TGVError>;

    /// Return the k-th gene after a contig:coordinate.
    async fn query_k_genes_after(
        &self,
        reference: &Reference,
        contig_index: usize,
        coord: usize,
        k: usize,
        cache: &mut TrackCache,
        contig_header: &ContigHeader,
    ) -> Result<Gene, TGVError>;

    /// Return the k-th gene before a contig:coordinate.
    async fn query_k_genes_before(
        &self,
        reference: &Reference,
        contig_index: usize,
        coord: usize,
        k: usize,
        cache: &mut TrackCache,
        contig_header: &ContigHeader,
    ) -> Result<Gene, TGVError>;

    /// Return the k-th exon after a contig:coordinate.
    async fn query_k_exons_after(
        &self,
        reference: &Reference,
        contig_index: usize,
        coord: usize,
        k: usize,
        cache: &mut TrackCache,
        contig_header: &ContigHeader,
    ) -> Result<SubGeneFeature, TGVError>;

    /// Return the k-th exon before a contig:coordinate.
    async fn query_k_exons_before(
        &self,
        reference: &Reference,
        contig_index: usize,
        coord: usize,
        k: usize,
        cache: &mut TrackCache,
        contig_header: &ContigHeader,
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
        contig_header: &ContigHeader,
    ) -> Result<HashMap<usize, Option<String>>, TGVError> {
        match self {
            TrackServiceEnum::Api(_) => Err(TGVError::IOError(
                "get_contig_2bit_file_lookup is not supported for UcscApiTrackService".to_string(),
            )),
            TrackServiceEnum::Db(service) => {
                service
                    .get_contig_2bit_file_lookup(reference, contig_header)
                    .await
            }
            TrackServiceEnum::LocalDb(service) => {
                service
                    .get_contig_2bit_file_lookup(reference, contig_header)
                    .await
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
    ) -> Result<Vec<Contig>, TGVError> {
        match self {
            TrackServiceEnum::Api(service) => service.get_all_contigs(reference, cache).await,
            TrackServiceEnum::Db(service) => service.get_all_contigs(reference, cache).await,
            TrackServiceEnum::LocalDb(service) => service.get_all_contigs(reference, cache).await,
        }
    }

    async fn get_cytoband(
        &self,
        reference: &Reference,
        contig_index: usize,
        cache: &mut TrackCache,
        contig_header: &ContigHeader,
    ) -> Result<Option<Cytoband>, TGVError> {
        match self {
            TrackServiceEnum::Api(service) => {
                service
                    .get_cytoband(reference, contig_index, cache, contig_header)
                    .await
            }
            TrackServiceEnum::Db(service) => {
                service
                    .get_cytoband(reference, contig_index, cache, contig_header)
                    .await
            }
            TrackServiceEnum::LocalDb(service) => {
                service
                    .get_cytoband(reference, contig_index, cache, contig_header)
                    .await
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
        contig_header: &ContigHeader,
    ) -> Result<Vec<Gene>, TGVError> {
        match self {
            TrackServiceEnum::Api(service) => {
                service
                    .query_genes_overlapping(reference, region, cache, contig_header)
                    .await
            }
            TrackServiceEnum::Db(service) => {
                service
                    .query_genes_overlapping(reference, region, cache, contig_header)
                    .await
            }
            TrackServiceEnum::LocalDb(service) => {
                service
                    .query_genes_overlapping(reference, region, cache, contig_header)
                    .await
            }
        }
    }

    async fn query_gene_covering(
        &self,
        reference: &Reference,
        contig_index: usize,
        coord: usize,
        cache: &mut TrackCache,
        contig_header: &ContigHeader,
    ) -> Result<Option<Gene>, TGVError> {
        match self {
            TrackServiceEnum::Api(service) => {
                service
                    .query_gene_covering(reference, contig_index, coord, cache, contig_header)
                    .await
            }
            TrackServiceEnum::Db(service) => {
                service
                    .query_gene_covering(reference, contig_index, coord, cache, contig_header)
                    .await
            }
            TrackServiceEnum::LocalDb(service) => {
                service
                    .query_gene_covering(reference, contig_index, coord, cache, contig_header)
                    .await
            }
        }
    }

    async fn query_gene_name(
        &self,
        reference: &Reference,
        gene_name: &String,
        cache: &mut TrackCache,
        contig_header: &ContigHeader,
    ) -> Result<Gene, TGVError> {
        match self {
            TrackServiceEnum::Api(service) => {
                service
                    .query_gene_name(reference, gene_name, cache, contig_header)
                    .await
            }
            TrackServiceEnum::Db(service) => {
                service
                    .query_gene_name(reference, gene_name, cache, contig_header)
                    .await
            }
            TrackServiceEnum::LocalDb(service) => {
                service
                    .query_gene_name(reference, gene_name, cache, contig_header)
                    .await
            }
        }
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
        match self {
            TrackServiceEnum::Api(service) => {
                service
                    .query_k_genes_after(reference, contig_index, coord, k, cache, contig_header)
                    .await
            }
            TrackServiceEnum::Db(service) => {
                service
                    .query_k_genes_after(reference, contig_index, coord, k, cache, contig_header)
                    .await
            }
            TrackServiceEnum::LocalDb(service) => {
                service
                    .query_k_genes_after(reference, contig_index, coord, k, cache, contig_header)
                    .await
            }
        }
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
        match self {
            TrackServiceEnum::Api(service) => {
                service
                    .query_k_genes_before(reference, contig_index, coord, k, cache, contig_header)
                    .await
            }
            TrackServiceEnum::Db(service) => {
                service
                    .query_k_genes_before(reference, contig_index, coord, k, cache, contig_header)
                    .await
            }
            TrackServiceEnum::LocalDb(service) => {
                service
                    .query_k_genes_before(reference, contig_index, coord, k, cache, contig_header)
                    .await
            }
        }
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
        match self {
            TrackServiceEnum::Api(service) => {
                service
                    .query_k_exons_after(reference, contig_index, coord, k, cache, contig_header)
                    .await
            }
            TrackServiceEnum::Db(service) => {
                service
                    .query_k_exons_after(reference, contig_index, coord, k, cache, contig_header)
                    .await
            }
            TrackServiceEnum::LocalDb(service) => {
                service
                    .query_k_exons_after(reference, contig_index, coord, k, cache, contig_header)
                    .await
            }
        }
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
        match self {
            TrackServiceEnum::Api(service) => {
                service
                    .query_k_exons_before(reference, contig_index, coord, k, cache, contig_header)
                    .await
            }
            TrackServiceEnum::Db(service) => {
                service
                    .query_k_exons_before(reference, contig_index, coord, k, cache, contig_header)
                    .await
            }
            TrackServiceEnum::LocalDb(service) => {
                service
                    .query_k_exons_before(reference, contig_index, coord, k, cache, contig_header)
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
        contig_header: &ContigHeader,
    ) -> Result<Track<Gene>, TGVError> {
        match self {
            TrackServiceEnum::Api(service) => {
                service
                    .query_gene_track(reference, region, cache, contig_header)
                    .await
            }
            TrackServiceEnum::Db(service) => {
                service
                    .query_gene_track(reference, region, cache, contig_header)
                    .await
            }
            TrackServiceEnum::LocalDb(service) => {
                service
                    .query_gene_track(reference, region, cache, contig_header)
                    .await
            }
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum UcscHost {
    Us,
    Eu,
}

impl UcscHost {
    pub fn url(&self) -> String {
        match self {
            UcscHost::Us => "genome-mysql.soe.ucsc.edu".to_string(),
            UcscHost::Eu => "genome-euro-mysql.soe.ucsc.edu".to_string(),
        }
    }

    /// Choose the host based on the local timezone.
    pub fn auto() -> Self {
        let offset = Local::now().offset().local_minus_utc() / 3600;
        if (-12..=0).contains(&offset) {
            UcscHost::Us
        } else {
            UcscHost::Eu
        }
    }
}

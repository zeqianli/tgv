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
    intervals::{GenomeInterval, Region},
    reference::Reference,
    settings::{self, BackendType, Settings},
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
#[derive(Debug, Default)]
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
}

impl TrackCache {
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
    async fn close(&mut self) -> Result<(), TGVError>;

    // Query contigs data given a reference.
    async fn get_all_contigs(&mut self, reference: &Reference) -> Result<Vec<Contig>, TGVError>;

    // Return the cytoband data given a reference and a contig.
    async fn get_cytoband(
        &mut self,
        reference: &Reference,
        contig_index: usize,
        contig_header: &ContigHeader,
    ) -> Result<Option<Cytoband>, TGVError>;

    /// Return a Track<Gene> that covers a region.
    async fn query_gene_track(
        &mut self,
        reference: &Reference,
        region: &Region,
        contig_header: &ContigHeader,
    ) -> Result<Track<Gene>, TGVError> {
        let genes = self
            .query_genes_overlapping(reference, region, contig_header)
            .await?;
        Track::from_genes(genes, region.contig_index)
    }

    /// Given a reference, return the prefered track name.
    async fn get_preferred_track_name(
        &mut self,
        reference: &Reference,
    ) -> Result<Option<String>, TGVError>;

    /// Return a list of genes that overlap with a region.
    async fn query_genes_overlapping(
        &mut self,
        reference: &Reference,
        region: &Region,
        contig_header: &ContigHeader,
    ) -> Result<Vec<Gene>, TGVError>;

    /// Return the Gene covering a contig:coordinate.
    async fn query_gene_covering(
        &mut self,
        reference: &Reference,
        contig_index: usize,
        coord: usize,
        contig_header: &ContigHeader,
    ) -> Result<Option<Gene>, TGVError>;

    async fn query_gene_name(
        &mut self,
        reference: &Reference,
        gene_name: &String,
        contig_header: &ContigHeader,
    ) -> Result<Gene, TGVError>;

    /// Return the k-th gene after a contig:coordinate.
    async fn query_k_genes_after(
        &mut self,
        reference: &Reference,
        contig_index: usize,
        coord: usize,
        k: usize,
        contig_header: &ContigHeader,
    ) -> Result<Gene, TGVError>;

    /// Return the k-th gene before a contig:coordinate.
    async fn query_k_genes_before(
        &mut self,
        reference: &Reference,
        contig_index: usize,
        coord: usize,
        k: usize,
        contig_header: &ContigHeader,
    ) -> Result<Gene, TGVError>;

    /// Return the k-th exon after a contig:coordinate.
    async fn query_k_exons_after(
        &mut self,
        reference: &Reference,
        contig_index: usize,
        coord: usize,
        k: usize,
        contig_header: &ContigHeader,
    ) -> Result<SubGeneFeature, TGVError>;

    /// Return the k-th exon before a contig:coordinate.
    async fn query_k_exons_before(
        &mut self,
        reference: &Reference,
        contig_index: usize,
        coord: usize,
        k: usize,
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
    pub async fn new(settings: &Settings) -> Result<Option<Self>, TGVError> {
        match (&settings.backend, &settings.reference) {
            (_, Reference::NoReference) | (_, Reference::BYOIndexedFasta(_)) => Ok(None),
            (BackendType::Ucsc, Reference::UcscAccession(_)) => {
                Ok(Some(Self::Api(UcscApiTrackService::new()?)))
            }
            (BackendType::Ucsc, _) => Ok(Some(Self::Db(
                UcscDbTrackService::new(&settings.reference, &settings.ucsc_host).await?,
            ))),
            (BackendType::Local, _) => Ok(Some(TrackServiceEnum::LocalDb(
                LocalDbTrackService::new(&settings.reference, &settings.cache_dir).await?,
            ))),
            (BackendType::Default, reference) => {
                // If the local cache is available, use the local cache.
                // Otherwise, use the UCSC DB / API.
                match LocalDbTrackService::new(&settings.reference, &settings.cache_dir).await {
                    Ok(ts) => Ok(Some(TrackServiceEnum::LocalDb(ts))),
                    Err(TGVError::IOError(e)) => match reference {
                        Reference::UcscAccession(_) => {
                            Ok(Some(TrackServiceEnum::Api(UcscApiTrackService::new()?)))
                        }
                        _ => Ok(Some(TrackServiceEnum::Db(
                            UcscDbTrackService::new(&settings.reference, &settings.ucsc_host)
                                .await?,
                        ))),
                    },

                    Err(e) => return Err(e),
                }
            }

            _ => {
                return Err(TGVError::ValueError(format!(
                    "Failed to initialize TrackService for reference {}",
                    settings.reference.to_string()
                )));
            }
        }
    }
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
    async fn close(&mut self) -> Result<(), TGVError> {
        match self {
            TrackServiceEnum::Api(service) => service.close().await,
            TrackServiceEnum::Db(service) => service.close().await,
            TrackServiceEnum::LocalDb(service) => service.close().await,
        }
    }

    async fn get_all_contigs(&mut self, reference: &Reference) -> Result<Vec<Contig>, TGVError> {
        match self {
            TrackServiceEnum::Api(service) => service.get_all_contigs(reference).await,
            TrackServiceEnum::Db(service) => service.get_all_contigs(reference).await,
            TrackServiceEnum::LocalDb(service) => service.get_all_contigs(reference).await,
        }
    }

    async fn get_cytoband(
        &mut self,
        reference: &Reference,
        contig_index: usize,
        contig_header: &ContigHeader,
    ) -> Result<Option<Cytoband>, TGVError> {
        match self {
            TrackServiceEnum::Api(service) => {
                service
                    .get_cytoband(reference, contig_index, contig_header)
                    .await
            }
            TrackServiceEnum::Db(service) => {
                service
                    .get_cytoband(reference, contig_index, contig_header)
                    .await
            }
            TrackServiceEnum::LocalDb(service) => {
                service
                    .get_cytoband(reference, contig_index, contig_header)
                    .await
            }
        }
    }

    async fn get_preferred_track_name(
        &mut self,
        reference: &Reference,
    ) -> Result<Option<String>, TGVError> {
        match self {
            TrackServiceEnum::Api(service) => service.get_preferred_track_name(reference).await,
            TrackServiceEnum::Db(service) => service.get_preferred_track_name(reference).await,
            TrackServiceEnum::LocalDb(service) => service.get_preferred_track_name(reference).await,
        }
    }

    async fn query_genes_overlapping(
        &mut self,
        reference: &Reference,
        region: &Region,
        contig_header: &ContigHeader,
    ) -> Result<Vec<Gene>, TGVError> {
        match self {
            TrackServiceEnum::Api(service) => {
                service
                    .query_genes_overlapping(reference, region, contig_header)
                    .await
            }
            TrackServiceEnum::Db(service) => {
                service
                    .query_genes_overlapping(reference, region, contig_header)
                    .await
            }
            TrackServiceEnum::LocalDb(service) => {
                service
                    .query_genes_overlapping(reference, region, contig_header)
                    .await
            }
        }
    }

    async fn query_gene_covering(
        &mut self,
        reference: &Reference,
        contig_index: usize,
        coord: usize,
        contig_header: &ContigHeader,
    ) -> Result<Option<Gene>, TGVError> {
        match self {
            TrackServiceEnum::Api(service) => {
                service
                    .query_gene_covering(reference, contig_index, coord, contig_header)
                    .await
            }
            TrackServiceEnum::Db(service) => {
                service
                    .query_gene_covering(reference, contig_index, coord, contig_header)
                    .await
            }
            TrackServiceEnum::LocalDb(service) => {
                service
                    .query_gene_covering(reference, contig_index, coord, contig_header)
                    .await
            }
        }
    }

    async fn query_gene_name(
        &mut self,
        reference: &Reference,
        gene_name: &String,
        contig_header: &ContigHeader,
    ) -> Result<Gene, TGVError> {
        match self {
            TrackServiceEnum::Api(service) => {
                service
                    .query_gene_name(reference, gene_name, contig_header)
                    .await
            }
            TrackServiceEnum::Db(service) => {
                service
                    .query_gene_name(reference, gene_name, contig_header)
                    .await
            }
            TrackServiceEnum::LocalDb(service) => {
                service
                    .query_gene_name(reference, gene_name, contig_header)
                    .await
            }
        }
    }

    async fn query_k_genes_after(
        &mut self,
        reference: &Reference,
        contig_index: usize,
        coord: usize,
        k: usize,
        contig_header: &ContigHeader,
    ) -> Result<Gene, TGVError> {
        match self {
            TrackServiceEnum::Api(service) => {
                service
                    .query_k_genes_after(reference, contig_index, coord, k, contig_header)
                    .await
            }
            TrackServiceEnum::Db(service) => {
                service
                    .query_k_genes_after(reference, contig_index, coord, k, contig_header)
                    .await
            }
            TrackServiceEnum::LocalDb(service) => {
                service
                    .query_k_genes_after(reference, contig_index, coord, k, contig_header)
                    .await
            }
        }
    }

    async fn query_k_genes_before(
        &mut self,
        reference: &Reference,
        contig_index: usize,
        coord: usize,
        k: usize,
        contig_header: &ContigHeader,
    ) -> Result<Gene, TGVError> {
        match self {
            TrackServiceEnum::Api(service) => {
                service
                    .query_k_genes_before(reference, contig_index, coord, k, contig_header)
                    .await
            }
            TrackServiceEnum::Db(service) => {
                service
                    .query_k_genes_before(reference, contig_index, coord, k, contig_header)
                    .await
            }
            TrackServiceEnum::LocalDb(service) => {
                service
                    .query_k_genes_before(reference, contig_index, coord, k, contig_header)
                    .await
            }
        }
    }

    async fn query_k_exons_after(
        &mut self,
        reference: &Reference,
        contig_index: usize,
        coord: usize,
        k: usize,
        contig_header: &ContigHeader,
    ) -> Result<SubGeneFeature, TGVError> {
        match self {
            TrackServiceEnum::Api(service) => {
                service
                    .query_k_exons_after(reference, contig_index, coord, k, contig_header)
                    .await
            }
            TrackServiceEnum::Db(service) => {
                service
                    .query_k_exons_after(reference, contig_index, coord, k, contig_header)
                    .await
            }
            TrackServiceEnum::LocalDb(service) => {
                service
                    .query_k_exons_after(reference, contig_index, coord, k, contig_header)
                    .await
            }
        }
    }

    async fn query_k_exons_before(
        &mut self,
        reference: &Reference,
        contig_index: usize,
        coord: usize,
        k: usize,
        contig_header: &ContigHeader,
    ) -> Result<SubGeneFeature, TGVError> {
        match self {
            TrackServiceEnum::Api(service) => {
                service
                    .query_k_exons_before(reference, contig_index, coord, k, contig_header)
                    .await
            }
            TrackServiceEnum::Db(service) => {
                service
                    .query_k_exons_before(reference, contig_index, coord, k, contig_header)
                    .await
            }
            TrackServiceEnum::LocalDb(service) => {
                service
                    .query_k_exons_before(reference, contig_index, coord, k, contig_header)
                    .await
            }
        }
    }
    // Default helper methods delegate
    async fn query_gene_track(
        &mut self,
        reference: &Reference,
        region: &Region,
        contig_header: &ContigHeader,
    ) -> Result<Track<Gene>, TGVError> {
        match self {
            TrackServiceEnum::Api(service) => {
                service
                    .query_gene_track(reference, region, contig_header)
                    .await
            }
            TrackServiceEnum::Db(service) => {
                service
                    .query_gene_track(reference, region, contig_header)
                    .await
            }
            TrackServiceEnum::LocalDb(service) => {
                service
                    .query_gene_track(reference, region, contig_header)
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

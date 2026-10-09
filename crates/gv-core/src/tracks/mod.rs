mod downloader;
mod local_db;
pub mod schema;
mod ucsc_api;
mod ucsc_db;

use crate::{
    contig_header::{Contig, ContigHeader},
    cytoband::CytobandTable,
    error::TGVError,
    gene::GeneTable,
    intervals::Region,
    reference::Reference,
    settings::{BackendType, Settings},
};
use async_trait::async_trait;
use chrono::Local;
use polars::prelude::DataFrame;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub use downloader::{UCSCDownloadSource, UCSCDownloader};
pub use local_db::LocalDbTrackService;
pub use ucsc_api::UcscApiTrackService;
pub use ucsc_db::UcscDbTrackService;

/// Default track ordering when rendering the gene track.
const TRACK_PREFERENCES: [&str; 5] = [
    "ncbiRefSeqSelect",
    "ncbiRefSeqCurated",
    "ncbiRefSeq",
    "ncbiGene",
    "refGene",
];

/// Holds cache for track service queries.
/// Can be returned or pass into queries.
#[derive(Debug, Default)]
pub struct TrackCache {
    /// Cached gene tables keyed by contig index.
    pub tracks: HashMap<usize, GeneTable>,

    /// Contig index -> whether the track has been quried
    contig_queried: HashSet<usize>,

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

    pub fn get_gene(&self, gene_name: &str) -> Result<DataFrame, TGVError> {
        let mut contigs = self.tracks.keys().copied().collect::<Vec<_>>();
        contigs.sort_unstable();
        for contig in contigs {
            let rows = self.tracks[&contig].gene_by_name(gene_name)?;
            if rows.height() > 0 {
                return Ok(rows);
            }
        }
        Ok(GeneTable::default().data)
    }

    pub fn add_track(&mut self, contig_index: usize, track: GeneTable) {
        self.tracks.insert(contig_index, track);
        self.contig_queried.insert(contig_index);
    }
}

#[async_trait]
pub trait TrackService {
    // Basics

    /// Close the track service.
    async fn close(&mut self) -> Result<(), TGVError>;

    // Query contigs data given a reference.
    async fn get_all_contigs(&mut self, reference: &Reference) -> Result<Vec<Contig>, TGVError>;

    /// Returns the reference's cytobands for every contig in the header.
    async fn query_cytobands(
        &mut self,
        reference: &Reference,
        contig_header: &ContigHeader,
    ) -> Result<CytobandTable, TGVError>;

    /// Return a GeneTable that covers a region.
    async fn query_gene_track(
        &mut self,
        reference: &Reference,
        region: &Region,
        contig_header: &ContigHeader,
    ) -> Result<GeneTable, TGVError> {
        let genes = self
            .query_genes_overlapping(reference, region, contig_header)
            .await?;
        GeneTable::from_data(genes, region.contig_index(), (region.start(), region.end()))
    }

    /// Given a reference, return the prefered track name.
    async fn get_preferred_track_name(
        &mut self,
        reference: &Reference,
    ) -> Result<Option<String>, TGVError>;

    /// Return gene rows that overlap with a region.
    async fn query_genes_overlapping(
        &mut self,
        reference: &Reference,
        region: &Region,
        contig_header: &ContigHeader,
    ) -> Result<DataFrame, TGVError>;

    async fn query_gene_name(
        &mut self,
        reference: &Reference,
        gene_name: &str,
        contig_header: &ContigHeader,
    ) -> Result<DataFrame, TGVError>;

    /// Return the k-th gene after a contig:coordinate.
    async fn query_k_genes_after(
        &mut self,
        reference: &Reference,
        contig_index: usize,
        coord: u64,
        k: usize,
        contig_header: &ContigHeader,
    ) -> Result<DataFrame, TGVError>;

    /// Return the k-th gene before a contig:coordinate.
    async fn query_k_genes_before(
        &mut self,
        reference: &Reference,
        contig_index: usize,
        coord: u64,
        k: usize,
        contig_header: &ContigHeader,
    ) -> Result<DataFrame, TGVError>;

    /// Return the k-th exon after a contig:coordinate.
    async fn query_k_exons_after(
        &mut self,
        reference: &Reference,
        contig_index: usize,
        coord: u64,
        k: usize,
        contig_header: &ContigHeader,
    ) -> Result<DataFrame, TGVError>;

    /// Return the k-th exon before a contig:coordinate.
    async fn query_k_exons_before(
        &mut self,
        reference: &Reference,
        contig_index: usize,
        coord: u64,
        k: usize,
        contig_header: &ContigHeader,
    ) -> Result<DataFrame, TGVError>;
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
            (_, Reference::NoReference)
            | (_, Reference::BYOIndexedFasta(_))
            | (_, Reference::BYOTwoBit(_)) => Ok(None),
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
                    Err(TGVError::IOError(_e)) => match reference {
                        Reference::UcscAccession(_) => {
                            Ok(Some(TrackServiceEnum::Api(UcscApiTrackService::new()?)))
                        }
                        _ => Ok(Some(TrackServiceEnum::Db(
                            UcscDbTrackService::new(&settings.reference, &settings.ucsc_host)
                                .await?,
                        ))),
                    },

                    Err(e) => Err(e),
                }
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

    async fn query_cytobands(
        &mut self,
        reference: &Reference,
        contig_header: &ContigHeader,
    ) -> Result<CytobandTable, TGVError> {
        match self {
            TrackServiceEnum::Api(service) => {
                service.query_cytobands(reference, contig_header).await
            }
            TrackServiceEnum::Db(service) => {
                service.query_cytobands(reference, contig_header).await
            }
            TrackServiceEnum::LocalDb(service) => {
                service.query_cytobands(reference, contig_header).await
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
    ) -> Result<DataFrame, TGVError> {
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

    async fn query_gene_name(
        &mut self,
        reference: &Reference,
        gene_name: &str,
        contig_header: &ContigHeader,
    ) -> Result<DataFrame, TGVError> {
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
        coord: u64,
        k: usize,
        contig_header: &ContigHeader,
    ) -> Result<DataFrame, TGVError> {
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
        coord: u64,
        k: usize,
        contig_header: &ContigHeader,
    ) -> Result<DataFrame, TGVError> {
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
        coord: u64,
        k: usize,
        contig_header: &ContigHeader,
    ) -> Result<DataFrame, TGVError> {
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
        coord: u64,
        k: usize,
        contig_header: &ContigHeader,
    ) -> Result<DataFrame, TGVError> {
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
    ) -> Result<GeneTable, TGVError> {
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

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
#[derive(Default)]
pub enum UcscHost {
    #[default]
    Us,
    Eu,
}

impl From<UcscHost> for String {
    fn from(h: UcscHost) -> Self {
        h.to_string()
    }
}

impl TryFrom<String> for UcscHost {
    type Error = TGVError;
    fn try_from(s: String) -> Result<Self, TGVError> {
        s.parse()
    }
}

impl std::str::FromStr for UcscHost {
    type Err = TGVError;

    /// Parse `"us"`, `"eu"`, or `"auto"` (resolved via timezone detection).
    fn from_str(s: &str) -> Result<Self, TGVError> {
        match s {
            "us" => Ok(Self::Us),
            "eu" => Ok(Self::Eu),
            "auto" => Ok(Self::auto()),
            _ => Err(TGVError::ParsingError(format!(
                "Invalid ucsc_host `{s}`. Expected \"us\", \"eu\", or \"auto\"."
            ))),
        }
    }
}

impl std::string::ToString for UcscHost {
    fn to_string(&self) -> String {
        match self {
            UcscHost::Us => "us".to_string(),
            UcscHost::Eu => "eu".to_string(),
        }
    }
}

impl UcscHost {
    pub fn url(&self) -> String {
        match self {
            UcscHost::Us => "genome-mysql.gi.ucsc.edu".to_string(),
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

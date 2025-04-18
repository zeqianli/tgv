mod ucscapi;
mod ucscdb;
use crate::{
    error::TGVError,
    models::{
        contig::Contig,
        region::Region,
        track::{
            feature::{Gene, SubGeneFeature},
            track::Track,
        },
    },
};

pub use ucscapi::UcscApiTrackService;
pub use ucscdb::UcscDbTrackService;

pub trait TrackService {
    async fn query_gene_track(&self, region: &Region) -> Result<Track<Gene>, TGVError> {
        let genes = self.query_genes_between(region).await?;
        Track::from_genes(genes, region.contig.clone())
    }

    async fn close(&self) -> Result<(), TGVError>;

    async fn query_genes_between(&self, region: &Region) -> Result<Vec<Gene>, TGVError>;

    async fn query_gene_covering(
        &self,
        contig: &Contig,
        coord: usize,
    ) -> Result<Option<Gene>, TGVError>;

    async fn query_gene_name(&self, gene_id: &String) -> Result<Gene, TGVError>;

    async fn query_k_genes_after(
        &self,
        contig: &Contig,
        coord: usize,
        k: usize,
    ) -> Result<Gene, TGVError>;

    async fn query_k_genes_before(
        &self,
        contig: &Contig,
        coord: usize,
        k: usize,
    ) -> Result<Gene, TGVError>;

    async fn query_k_exons_after(
        &self,
        contig: &Contig,
        coord: usize,
        k: usize,
    ) -> Result<SubGeneFeature, TGVError>;

    async fn query_k_exons_before(
        &self,
        contig: &Contig,
        coord: usize,
        k: usize,
    ) -> Result<SubGeneFeature, TGVError>;
}

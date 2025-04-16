mod ucscapi;
mod ucscdb;
use crate::{
    error::TGVError,
    models::{
        contig::Contig,
        region::Region,
        track::{Feature, Gene, Track},
    },
};

pub trait TrackService {
    async fn query_feature_track(&self, region: &Region) -> Result<Track, TGVError>;

    async fn query_genes_between(
        &self,
        contig: &Contig,
        start: usize,
        end: usize,
    ) -> Result<Vec<Gene>, TGVError>;

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
    ) -> Result<Feature, TGVError>;

    async fn query_k_exons_before(
        &self,
        contig: &Contig,
        coord: usize,
        k: usize,
    ) -> Result<Feature, TGVError>;
}

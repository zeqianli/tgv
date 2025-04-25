mod ucscapi;
mod ucscdb;
use crate::{
    error::TGVError,
    models::{
        contig::Contig,
        cytoband::Cytoband,
        reference::Reference,
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
    // Basics

    /// Close the track service.
    async fn close(&self) -> Result<(), TGVError>;

    // Contigs and cytobands

    // Return all contigs given a reference.
    async fn get_all_contigs(
        &self,
        reference: &Reference,
    ) -> Result<Vec<(Contig, usize)>, TGVError>;

    // Return the cytoband data given a reference and a contig.
    async fn get_cytoband(
        &self,
        reference: &Reference,
        contig: &Contig,
    ) -> Result<Option<Cytoband>, TGVError>;

    // Genes and tracks

    /// Return a Track<Gene> that covers a region.
    async fn query_gene_track(&self, region: &Region) -> Result<Track<Gene>, TGVError> {
        let genes = self.query_genes_overlapping(region).await?;
        Track::from_genes(genes, region.contig.clone())
    }

    /// Given a reference, return the prefered track name.
    async fn get_prefered_track_name(&self, reference: &Reference) -> Result<String, TGVError>;

    /// Return a list of genes that overlap with a region.
    async fn query_genes_overlapping(&self, region: &Region) -> Result<Vec<Gene>, TGVError>;

    /// Return the Gene covering a contig:coordinate.
    async fn query_gene_covering(
        &self,
        contig: &Contig,
        coord: usize,
    ) -> Result<Option<Gene>, TGVError>;

    async fn query_gene_name(&self, gene_id: &String) -> Result<Gene, TGVError>;

    /// Return the k-th gene after a contig:coordinate.
    async fn query_k_genes_after(
        &self,
        contig: &Contig,
        coord: usize,
        k: usize,
    ) -> Result<Gene, TGVError>;

    /// Return the k-th gene before a contig:coordinate.
    async fn query_k_genes_before(
        &self,
        contig: &Contig,
        coord: usize,
        k: usize,
    ) -> Result<Gene, TGVError>;

    /// Return the k-th exon after a contig:coordinate.
    async fn query_k_exons_after(
        &self,
        contig: &Contig,
        coord: usize,
        k: usize,
    ) -> Result<SubGeneFeature, TGVError>;

    /// Return the k-th exon before a contig:coordinate.
    async fn query_k_exons_before(
        &self,
        contig: &Contig,
        coord: usize,
        k: usize,
    ) -> Result<SubGeneFeature, TGVError>;
}

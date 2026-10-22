//! Downloads and prepares the ferro-hgvs reference data for `tgv download`.

use ferro_hgvs::prepare::{PrepareConfig, prepare_references};
use ferro_hgvs::reference::multi_fasta::MultiFastaProvider;

use crate::assembly::{HgvsBuild, HgvsDataDir};
use crate::error::HgvsError;

/// Downloads the RefSeq transcripts, the cdot transcript models, and the genome for the data
/// directory's build. Files that already exist are kept, so an interrupted download resumes.
///
/// This blocks for a long time and reports progress on standard error, so call it from the CLI
/// in a blocking task.
pub fn prepare(data_dir: &HgvsDataDir) -> Result<(), HgvsError> {
    let grch38 = data_dir.build == HgvsBuild::Grch38;
    let config = PrepareConfig {
        output_dir: data_dir.path.clone(),
        download_transcripts: true,
        download_proteins: false,
        download_genome: grch38,
        download_genome_grch37: !grch38,
        // RefSeqGene (NG_) and LRG references only matter for descriptions on those accessions,
        // which tgv's chromosome coordinates never need.
        download_refseqgene: false,
        download_lrg: false,
        download_cdot: grch38,
        download_cdot_grch37: !grch38,
        download_ensembl: false,
        skip_existing: true,
        genome: data_dir.build.ferro_name().to_string(),
        ..PrepareConfig::default()
    };
    prepare_references(&config)?;

    // The first load builds the on-disk cdot cache, which takes a while. Pay for it here, so the
    // viewer starts quickly.
    eprintln!("Building the HGVS transcript cache...");
    MultiFastaProvider::from_manifest(data_dir.manifest())?;
    Ok(())
}

use std::path::PathBuf;

use ferro_hgvs::FerroError;
use gv_core::prelude::*;
use thiserror::Error;

/// Errors from HGVS search, annotation, and reference preparation.
#[derive(Debug, Error)]
pub enum HgvsError {
    #[error("HGVS data for {reference} is not prepared in {}. Run `tgv download {reference}` to enable HGVS.", dir.display())]
    NotPrepared { reference: String, dir: PathBuf },

    #[error("HGVS is only available for hg38 and hg19, not {reference}.")]
    UnsupportedReference { reference: String },

    #[error("Invalid HGVS search {input}: {reason}")]
    InvalidQuery { input: String, reason: String },

    #[error("{gene} has no RefSeq Select transcript.")]
    UnknownGene { gene: String },

    #[error("Transcript {transcript} is not in the HGVS data.")]
    UnknownTranscript { transcript: String },

    #[error("{position} is outside transcript {transcript}.")]
    PositionOutsideTranscript {
        transcript: String,
        position: String,
    },

    #[error("Transcript {transcript} is non-coding, so it has no {axis}. positions.")]
    NonCodingTranscript { transcript: String, axis: char },

    #[error("Contig {contig} has no RefSeq accession on this reference.")]
    UnmappedContig { contig: String },

    #[error("{0}")]
    Ferro(#[from] FerroError),

    #[error("HGVS is unavailable: {reason}")]
    Unavailable { reason: String },

    #[error("The HGVS worker stopped.")]
    WorkerStopped,
}

impl From<HgvsError> for TGVError {
    fn from(error: HgvsError) -> Self {
        TGVError::Hgvs(Box::new(error))
    }
}

use thiserror::Error;

#[derive(Debug, Error)]
#[allow(clippy::enum_variant_names)]
pub enum TGVError {
    #[error("CLI error: {0}")]
    CliError(String),

    #[error("Ucsc API IO error: {0}")]
    UcscApiIOError(#[from] reqwest::Error),

    #[error("UCSC DB IO error: {0}")]
    UcscDbIOError(#[from] sqlx::Error),

    #[error("JSON serialization error: {0}")]
    JsonSerializationError(#[from] serde_json::Error),

    #[error("Hts file parsing error: {0}")]
    HtsFileParsingError(#[from] rust_htslib::errors::Error),

    #[error("File IO error: {0}")]
    FileIOError(#[from] std::io::Error),

    #[error("IO Error: {0}")]
    IOError(String),

    #[error("State error: {0}")]
    StateError(String),

    #[error("Parsing error: {0}")]
    ParsingError(String),

    #[error("Value error: {0}")]
    ValueError(String),
}

impl TGVError {
    pub fn is_same_type(&self, other: &TGVError) -> bool {
        matches!(self, other)
    }
}

// TODO: tracing

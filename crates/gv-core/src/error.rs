use bigtools::{BBIReadError, BigBedReadOpenError};
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
#[allow(clippy::enum_variant_names)]
pub enum TGVError {
    #[error("CLI error: {0}")]
    CliError(String),

    #[error("Ucsc API IO error: {0}")]
    UcscApiIOError(#[from] reqwest::Error),

    #[error("SQLx error: {0}")]
    SqlxError(#[from] sqlx::Error),

    #[error(
        "The UCSC MariaDB download and HTTPS fallback failed. MariaDB: {mysql}. HTTPS: {https}."
    )]
    UcscDownloadFallbackError {
        mysql: Box<TGVError>,
        https: Box<TGVError>,
    },

    #[error("Failed to {operation} from {url}: {source}")]
    UcscHttpRequestError {
        operation: &'static str,
        url: String,
        #[source]
        source: reqwest::Error,
    },

    #[error("Failed to read the UCSC HTTPS dump for table {table} from {url}: {source}")]
    UcscTableReadError {
        table: String,
        url: String,
        #[source]
        source: std::io::Error,
    },

    #[error("Failed to {operation} {path}: {source}")]
    FileOperationError {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("JSON serialization error: {0}")]
    JsonSerializationError(#[from] serde_json::Error),

    #[error("File IO error: {0}")]
    FileIOError(#[from] std::io::Error),

    #[error("Logging error: {0}")]
    LoggingError(#[from] crate::logging::LoggingError),

    #[error("IO Error: {0}")]
    IOError(String),

    #[error("State error: {0}")]
    StateError(String),

    #[error("Parsing error: {0}")]
    ParsingError(String),

    #[error("Value error: {0}")]
    ValueError(String),

    #[error("Register error: {0}")]
    RegisterError(String),

    #[error("2bit file parsing error: {0}")]
    TwoBitFileParsingError(#[from] twobit::Error),

    #[error("BigBed file parsing error: {0}")]
    BigBedFileParsingError(#[from] BigBedReadOpenError),

    #[error("BigBed interal parsing error: {0}")]
    BigBedIntervalError(#[from] BBIReadError),

    #[error("UTF-8 decoding error: {0}")]
    Utf8DecodingError(#[from] std::string::FromUtf8Error),

    #[error("ParseInt error {0}")]
    ParseIntError(#[from] std::num::ParseIntError),

    #[error("an Interval must have a Range with a positive width")]
    InvalidRange,

    #[error("Noodles parse error")]
    NoodlesParseError(#[from] noodles::core::region::ParseError),

    #[error("OpenDAL error")]
    OpenDALError(#[from] opendal::Error),

    #[error("Alignment Parse error")]
    AlignmentParseError(String),

    #[error(
        "Cannot sort alignment by base at position {position}: the loaded alignment region is {loaded_left}-{loaded_right}."
    )]
    AlignmentSortPositionNotLoaded {
        position: u64,
        loaded_left: u64,
        loaded_right: u64,
    },
}

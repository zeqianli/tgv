use bigtools::{BBIReadError, BigBedReadOpenError};
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

    #[error("JSON serialization error: {0}")]
    JsonSerializationError(#[from] serde_json::Error),

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
}

use crate::keys::Key;
use polars::prelude::PolarsError;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Unknown command: {0}")]
    UnknownCommand(String),

    #[error("Invalid search: {0}")]
    InvalidSearch(String),

    #[error("Name the files to open: :{command} PATH…")]
    MissingPaths { command: String },

    #[error("Unmatched quote in: {0}")]
    UnmatchedQuote(String),

    #[error("Invalid {mode} mode input: {typed}")]
    InvalidKeys { mode: &'static str, typed: String },

    #[error("Invalid command mode input: {0:?}")]
    InvalidCommandKey(Key),

    #[error("Cannot interpret the region: {0}")]
    InvalidRegion(String),

    #[error("Both --offline and --online flags are used. Please use only one.")]
    ConflictingBackendFlags,

    #[error("The initial region cannot be a gene name {0} when no reference is provided.")]
    GeneWithoutReference(String),

    #[error("Input files and reference cannot both be none")]
    NoInput,

    #[error("Failed to parse session file: {0}")]
    ParseSession(#[from] toml::de::Error),

    #[error("Failed to serialize session: {0}")]
    SerializeSession(#[from] toml::ser::Error),

    #[error(
        "Unsupported session file version {version}. Expected a version between {min} and {max}."
    )]
    UnsupportedSessionVersion { version: u32, min: u32, max: u32 },

    #[error("Invalid locus \"{0}\": expected \"contig:position\" or a gene name")]
    InvalidLocus(String),

    #[error("CRAM track \"{path}\" requires a `reference` field in the session file")]
    CramWithoutReference { path: String },

    #[error("Invalid genome \"{genome}\" in the session file: {source}")]
    InvalidGenome {
        genome: String,
        #[source]
        source: gv_core::Error,
    },

    #[error("Zoom {direction} factor cannot be 0")]
    ZeroZoomFactor { direction: &'static str },

    #[error("Cannot split [{left}, {right}] into {n_bins} bins")]
    InvalidBins {
        left: u64,
        right: u64,
        n_bins: usize,
    },

    #[error("Unknown gene segment kind: {0}.")]
    UnknownSegmentKind(String),

    #[error(transparent)]
    Core(#[from] gv_core::Error),
}

impl From<PolarsError> for Error {
    fn from(error: PolarsError) -> Self {
        Self::Core(error.into())
    }
}

impl From<std::string::FromUtf8Error> for Error {
    fn from(error: std::string::FromUtf8Error) -> Self {
        Self::Core(error.into())
    }
}

impl From<Error> for gv_session::Error {
    fn from(error: Error) -> Self {
        match error {
            Error::Core(error) => Self::Core(error),
            other => Self::Viewer {
                message: other.to_string(),
            },
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Core(error.into())
    }
}

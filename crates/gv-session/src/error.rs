//! Errors returned by session commands.

use gv_core::error::TGVError;
use polars::prelude::PolarsError;
use thiserror::Error;

/// A failed session command.
#[derive(Debug, Error)]
pub enum SessionError {
    /// A request field is invalid for the current dataset.
    #[error("{message}")]
    InvalidInput {
        field: &'static str,
        message: String,
    },

    /// The command needs a loaded dataset.
    #[error("Load a dataset before {operation}.")]
    NoDataset { operation: &'static str },

    /// The command acts on a viewer, but no viewer displays this session.
    #[error("No viewer displays this session; open the dataset in TGV to show it.")]
    NoViewer,

    /// The host's dataset can't be replaced, for example because a viewer displays it.
    #[error("This session's dataset is fixed; open other files in a new TGV window.")]
    DatasetFixed,

    /// The connection to a viewer's session fails or closes.
    #[error("The connection to the TGV viewer fails: {0}")]
    Connection(String),

    /// An error that the session on the other end of a socket reports.
    #[error("{message}")]
    Remote {
        code: String,
        message: String,
        field: Option<String>,
    },

    /// The worker no longer accepts commands.
    #[error("The session worker is unavailable.")]
    Unavailable,

    /// The worker stopped before replying.
    #[error("The session worker stops before replying.")]
    Stopped,

    /// Reading or computing data fails.
    #[error(transparent)]
    Core(#[from] TGVError),
}

impl From<PolarsError> for SessionError {
    fn from(error: PolarsError) -> Self {
        Self::Core(error.into())
    }
}

impl SessionError {
    /// A stable code for clients, such as MCP tool errors and socket replies.
    pub fn code(&self) -> &str {
        match self {
            Self::InvalidInput { .. } => "invalid_input",
            Self::NoDataset { .. } => "no_dataset",
            Self::NoViewer => "no_viewer",
            Self::DatasetFixed => "dataset_fixed",
            Self::Connection(_) => "viewer_disconnected",
            Self::Remote { code, .. } => code,
            Self::Unavailable | Self::Stopped | Self::Core(_) => "internal_error",
        }
    }

    /// The request field that the error concerns, if any.
    pub fn field(&self) -> Option<&str> {
        match self {
            Self::InvalidInput { field, .. } => Some(field),
            Self::Remote { field, .. } => field.as_deref(),
            _ => None,
        }
    }
}

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

//! Errors returned by session commands.

use crate::schema::{ViewerInfo, ViewerKind};
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

    /// Several viewers are open, and the agent hasn't chosen one.
    #[error(
        "Several TGV viewers are open: {}. Choose one from the question's context, such as a file, a locus, or the TUI or the GUI. If it is still unclear, ask the user which one they mean. Then call attach with its pid.",
        describe_viewers(.0)
    )]
    ViewerAmbiguous(Vec<ViewerInfo>),

    /// No viewer runs with the process ID that `attach` names.
    #[error("No TGV viewer runs with pid {0}. Call list_viewers to see the open viewers.")]
    ViewerNotFound(u32),

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
            Self::ViewerAmbiguous(_) => "viewer_ambiguous",
            Self::ViewerNotFound(_) => "viewer_not_found",
            Self::Connection(_) => "viewer_disconnected",
            Self::Remote { code, .. } => code,
            Self::Unavailable | Self::Stopped | Self::Core(_) => "internal_error",
        }
    }

    /// The request field that the error concerns, if any.
    pub fn field(&self) -> Option<&str> {
        match self {
            Self::InvalidInput { field, .. } => Some(field),
            Self::ViewerNotFound(_) => Some("pid"),
            Self::Remote { field, .. } => field.as_deref(),
            _ => None,
        }
    }
}

/// The viewers for an error message, such as `pid 123 (GUI): hg38 at chr1:100 with a.bam`.
fn describe_viewers(viewers: &[ViewerInfo]) -> String {
    viewers
        .iter()
        .map(|viewer| {
            let kind = match viewer.kind {
                ViewerKind::Tui => "TUI",
                ViewerKind::Gui => "GUI",
            };
            let files = if viewer.files.is_empty() {
                "no files".to_string()
            } else {
                viewer.files.join(", ")
            };
            format!(
                "pid {} ({kind}): {} at {} with {files}",
                viewer.pid, viewer.reference, viewer.locus
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

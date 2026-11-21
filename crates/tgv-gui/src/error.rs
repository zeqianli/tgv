//! Errors from drawing the window and starting the app.

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Failed to draw: {0}")]
    Gpui(#[from] anyhow::Error),

    #[error("Paired alignment {index} is not ready when the window draws it")]
    PairedAlignmentNotReady { index: usize },

    #[error("Failed to resume session {}: {source}", path.display())]
    ResumeSession {
        path: PathBuf,
        #[source]
        source: gv_viewer::Error,
    },

    #[error(transparent)]
    Logging(#[from] gv_core::logging::LoggingError),

    #[error(transparent)]
    Viewer(#[from] gv_viewer::Error),

    #[error(transparent)]
    Core(#[from] gv_core::Error),
}

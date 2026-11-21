//! Errors from the terminal UI and the `tgv` command line.

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Failed to draw the terminal: {0}")]
    DrawTerminal(#[source] std::io::Error),

    #[error("Cannot scale a region of length 0")]
    EmptyScale,

    #[error("Paired alignment {index} is not ready when the terminal draws it")]
    PairedAlignmentNotReady { index: usize },

    #[error("Use the load_dataset MCP tool to load files when serving.")]
    McpWithFiles,

    #[error("Subcommands run without opening the viewer")]
    UnexpectedSubcommand,

    #[error("Failed to resume session {}: {source}", path.display())]
    ResumeSession {
        path: PathBuf,
        #[source]
        source: gv_viewer::Error,
    },

    #[error(transparent)]
    Logging(#[from] gv_core::logging::LoggingError),

    #[error(transparent)]
    Mcp(#[from] gv_mcp::Error),

    #[error(transparent)]
    Viewer(#[from] gv_viewer::Error),

    #[error(transparent)]
    Core(#[from] gv_core::Error),
}

impl From<std::string::FromUtf8Error> for Error {
    fn from(error: std::string::FromUtf8Error) -> Self {
        Self::Core(error.into())
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Core(error.into())
    }
}

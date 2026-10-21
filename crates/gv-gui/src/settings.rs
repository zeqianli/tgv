//! The `tgv-gui` command line: the options every viewer accepts.

use clap::Parser;
use gv_core::prelude::*;
pub use gv_viewer::settings::{Settings, ViewerArgs};

#[derive(Parser, Clone)]
#[command(author, version, about = "Explore genomes in a window, with TGV's keys.", long_about = None)]
pub struct Cli {
    #[command(flatten)]
    pub viewer: ViewerArgs,
}

/// Build [`Settings`] directly from CLI args, without loading any session file.
impl TryFrom<Cli> for Settings {
    type Error = TGVError;
    fn try_from(cli: Cli) -> Result<Self, TGVError> {
        Settings::try_from(&cli.viewer)
    }
}

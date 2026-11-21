use clap::Parser;
pub use gv_viewer::settings::{Settings, ViewerArgs};

#[derive(Parser, Clone)]
#[command(author, version, about = "Explore genomes in a window, with TGV's keys.", long_about = None)]
pub struct Cli {
    #[command(flatten)]
    pub viewer: ViewerArgs,
}

impl TryFrom<Cli> for Settings {
    type Error = gv_core::Error;
    fn try_from(cli: Cli) -> Result<Self, gv_core::Error> {
        Settings::try_from(&cli.viewer)
    }
}

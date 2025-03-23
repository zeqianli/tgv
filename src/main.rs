mod app;
mod error;
mod models;
mod rendering;
mod settings;
mod states;

use app::App;
use clap::Parser;
use error::TGVError;
use settings::{Cli, Settings};

#[tokio::main]
async fn main() -> Result<(), TGVError> {
    let cli = Cli::parse();
    let settings: Settings = Settings::new(cli).unwrap();

    let mut terminal = ratatui::init();

    let mut app = match App::new(settings).await {
        Ok(app) => app,
        Err(e) => {
            ratatui::restore();
            return Err(e);
        }
    };
    let app_result = app.run(&mut terminal).await;

    ratatui::restore();
    app_result
}

// TODO: check .bai file

mod app;
mod error;
mod helpers;
mod models;
mod rendering;
mod settings;
mod states;
mod traits;
use app::App;
use clap::Parser;
use error::TGVError;
use settings::{Cli, Settings};

#[tokio::main]
async fn main() -> Result<(), TGVError> {
    let cli = Cli::parse();
    let settings: Settings = Settings::new(cli).unwrap();

    let mut terminal = ratatui::init();

    // TODO: initialize UCSC connections here to ensure that they are properly closed in case of errors.

    let mut app = match App::new(settings).await {
        Ok(app) => app,
        Err(e) => {
            ratatui::restore();
            return Err(e);
        }
    };
    let app_result = app.run(&mut terminal).await;

    ratatui::restore();
    app.close().await?;
    app_result
}

// TODO: check .bai file

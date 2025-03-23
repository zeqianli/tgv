mod app;
mod models;
mod rendering;
mod settings;
mod states;

use app::App;
use clap::Parser;
use settings::{Cli, Settings};

#[tokio::main]
async fn main() -> Result<(), ()> {
    let cli = Cli::parse();
    let settings: Settings = Settings::new(cli).unwrap();

    let mut terminal = ratatui::init();
    let mut app = App::new(settings).await.unwrap();
    let app_result = app.run(&mut terminal).await;

    ratatui::restore();
    app_result
}

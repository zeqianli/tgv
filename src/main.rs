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
    let settings: Settings = Settings::new(cli, false).unwrap();

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
#[cfg(test)]
mod tests {
    use super::*;
    use insta::{assert_snapshot, Settings as InstaSettings};
    use ratatui::{backend::TestBackend, Terminal};
    use std::env;
    #[tokio::test]
    async fn test_non_human_bam() {
        let bam_path = env!("CARGO_MANIFEST_DIR").to_string() + "/tests/data/covid.sorted.bam";
        let args_string = format!("tgv {} --no-reference", bam_path);

        let cli = Cli::parse_from(shlex::split(&args_string).unwrap());
        let settings = Settings::new(cli, true).unwrap();

        let mut terminal = Terminal::new(TestBackend::new(50, 20)).unwrap();

        let mut app = App::new(settings).await.unwrap();
        app.run(&mut terminal).await.unwrap();

        assert_snapshot!(terminal.backend());
    }
}

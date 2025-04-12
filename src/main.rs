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

#[cfg(test)]
mod tests {
    use super::*;
    use insta::{assert_snapshot, Settings as InstaSettings};
    use ratatui::{backend::TestBackend, Terminal};
    use rstest::rstest;
    use std::env;

    /// Test that the app runs without panicking.
    /// Snapshots are saved in src/snapshots
    #[rstest]
    #[case(None, None)]
    #[case(None, Some("-r TP53"))]
    #[case(None, Some("-r TP53 -g hg19"))]
    #[case(Some("ncbi.sorted.bam"), Some("-r 22:33121120 -g hg19"))]
    #[case(Some("ncbi.sorted.bam"), Some("-r chr22:33121120 --no-reference"))]
    #[case(Some("covid.sorted.bam"), Some("--no-reference"))]
    #[case(Some("covid.sorted.bam"), Some("--no-reference -r MN908947.3:100"))]
    #[tokio::test]
    async fn integration_test(#[case] bam_path: Option<&str>, #[case] args: Option<&str>) {
        let snapshot_name = match (bam_path, args) {
            (Some(bam_path), Some(args)) => format!("{} {}", bam_path, args),
            (Some(bam_path), None) => format!("{} None", bam_path),
            (None, Some(args)) => format!("None {}", args),
            (None, None) => "None".to_string(),
        }
        .replace(" ", "_")
        .replace(":", "_")
        .replace(".", "_");

        let bam_path = match bam_path {
            Some(bam_path) => {
                Some(env!("CARGO_MANIFEST_DIR").to_string() + "/tests/data/" + bam_path)
            }
            None => None,
        };

        let args_string = match (bam_path, args) {
            (Some(bam_path), Some(args)) => format!("tgv {} {}", bam_path, args),
            (Some(bam_path), None) => format!("tgv {}", bam_path),
            (None, Some(args)) => format!("tgv {}", args),
            (None, None) => "tgv".to_string(),
        };

        let cli = Cli::parse_from(shlex::split(&args_string).unwrap());
        let settings = Settings::new(cli, true).unwrap();

        let mut terminal = Terminal::new(TestBackend::new(50, 20)).unwrap();

        let mut app = App::new(settings).await.unwrap();
        app.run(&mut terminal).await.unwrap();
        app.close().await.unwrap();

        assert_snapshot!(snapshot_name, terminal.backend());
    }
}

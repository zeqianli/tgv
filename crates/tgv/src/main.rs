mod app;
mod layout;
mod message;
mod mouse;
mod register;
mod rendering;
mod session;
mod settings;

use app::App;
use clap::Parser;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
};
use gv_core::error::TGVError;
use gv_core::reference::Reference;
use gv_core::tracks::{UCSCDownloader, UcscDbTrackService};
use session::SessionFile;
use settings::{Cli, Commands, Settings};
use std::io::stdout;
#[tokio::main]
async fn main() -> Result<(), TGVError> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Download {
            reference,
            cache_dir,
        }) => {
            let cache_dir = shellexpand::tilde(&cache_dir).to_string();
            let downloader = UCSCDownloader::new(reference.parse::<Reference>()?, &cache_dir)?;
            downloader.download().await?;
            return Ok(());
        }
        Some(Commands::List { more, all }) => {
            if more {
                let n = print_ucsc_assemblies().await?;
                println!("{} UCSC assemblies", n);
                println!("Browse a genome: tgv -g <genome> (e.g. tgv -g rn7)");
            } else {
                let n = print_common_genomes()?;
                println!("{} common genomes", n);
                println!("Browse a genome: tgv -g <genome> (e.g. tgv -g rat)");
            }
            return Ok(());
        }
        None => {}
    }

    // Load or create the default session, then apply CLI overrides on top.
    let session_path = SessionFile::default_path();
    let mut settings = if session_path.exists() {
        match SessionFile::from_path(&session_path).and_then(Settings::try_from) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Warning: failed to load session file: {e}. Using defaults.");
                Settings::default()
            }
        }
    } else {
        // First run: write a default session so future launches restore state.
        if let Err(e) = SessionFile::default().write_to_path(&session_path) {
            eprintln!("Warning: failed to write default session: {e}.");
        }
        Settings::default()
    };
    cli.apply_overrides(&mut settings)?;

    let mut terminal = ratatui::init();

    set_panic_hook();

    execute!(stdout(), EnableMouseCapture)?;

    // Gather resources before starting the app.
    let mut app = match App::new(settings).await {
        Ok(app) => app,
        Err(e) => {
            ratatui::restore();
            if let Err(err) = execute!(stdout(), DisableMouseCapture) {
                eprintln!("Error disabling mouse capture: {err}");
            }
            return Err(e);
        }
    };
    let app_result = app.run(&mut terminal).await;

    ratatui::restore();
    if let Err(err) = execute!(stdout(), DisableMouseCapture) {
        eprintln!("Error disabling mouse capture: {err}");
    }

    // Auto-save the default session on clean exit (skip in test mode).
    if !app.settings.test_mode && app_result.is_ok() {
        if let Err(e) = SessionFile::try_from(&app)
            .and_then(|s| s.write_to_path(&SessionFile::default_path()))
        {
            eprintln!("Warning: failed to save session: {e}.");
        }
    }

    app.close().await?;
    app_result
}

fn print_common_genomes() -> Result<usize, TGVError> {
    println!("{}", Reference::HG19);
    println!("{}", Reference::HG38);
    let genomes = Reference::get_common_genome_names()?;
    for (genome, name) in &genomes {
        println!("{} (UCSC assembly: {})", genome, name);
    }
    Ok(genomes.len() + 2)
}

async fn print_ucsc_assemblies() -> Result<usize, TGVError> {
    let assemblies = UcscDbTrackService::list_assemblies(None).await?;

    for (name, common_name) in &assemblies {
        println!("{} (Organism: {})", name, common_name);
    }
    Ok(assemblies.len())
}

/// Add to ratatui's panic hook: disable mouse capture.
fn set_panic_hook() {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        hook(info);
        if let Err(err) = execute!(stdout(), DisableMouseCapture) {
            eprintln!("Error disabling mouse capture: {err}");
        }
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_snapshot;
    use ratatui::{Terminal, backend::TestBackend};
    use rstest::rstest;
    use std::env;
    use std::path::Path;

    /// Test that the app runs without panicking.
    /// Snapshots are saved in src/snapshots
    #[rstest]
    #[case(None, Some("--online"))]
    #[case(Some("ncbi.sorted.bam"), Some("-r 22:33121120 -g hg19 --online"))]
    #[case(None, Some("-g GCF_028858775.2 -r NC_072398.2:76951800 --online"))]
    #[case(None, Some("-g wuhCor1 --offline --cache-dir tests/data/cache"))]
    #[case(None, Some("-g ecoli --offline --cache-dir tests/data/cache"))]
    #[case(
        Some("ncbi.sorted.bam"),
        Some("-r chr22:33121120 --no-reference --offline")
    )]
    #[case(
        Some("ncbi.sorted.bam"),
        Some(
            "-r chr22:33121120 tests/data/simple.vcf tests/data/simple.bed --no-reference --offline"
        )
    )]
    #[case(
        Some("covid.sorted.bam"),
        Some("-g covid --offline --cache-dir tests/data/cache")
    )]
    #[case(
        Some("covid.sorted.bam"),
        Some("--no-reference -r MN908947.3:100 --offline")
    )]
    #[case(Some("covid.sorted.bam"), Some("-g tests/data/covid.fa --offline"))]
    #[case(
        Some("covid.sorted.bam"),
        Some("-g tests/data/cache/wuhCor1/wuhCor1.2bit --offline")
    )]
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

        let bam_path = bam_path
            .map(|bam_path| env!("CARGO_MANIFEST_DIR").to_string() + "/tests/data/" + bam_path);

        let args_string = match (bam_path, args) {
            (Some(bam_path), Some(args)) => format!("tgv {} {}", bam_path, args),
            (Some(bam_path), None) => format!("tgv {}", bam_path),
            (None, Some(args)) => format!("tgv {}", args),
            (None, None) => "tgv".to_string(),
        };

        let cli = Cli::parse_from(shlex::split(&args_string).unwrap());
        let mut settings: Settings = cli.try_into().unwrap();
        settings.test_mode = true;

        let mut terminal = Terminal::new(TestBackend::new(50, 20)).unwrap();

        let mut app = App::new(settings).await.unwrap();
        app.run(&mut terminal).await.unwrap();
        app.close().await.unwrap();

        assert_snapshot!(snapshot_name, terminal.backend());
    }

    /// Test that downloading works.
    #[rstest]
    #[case("wuhCor1")]
    #[case("ecoli")]
    #[tokio::test]
    async fn download_integration_test(#[case] reference_str: &str) {
        let reference = reference_str.parse::<Reference>().unwrap();
        let temp_dir = tempfile::TempDir::new().unwrap();
        let temp_dir = temp_dir.path().to_str().unwrap();
        let downloader =
            UCSCDownloader::new(reference_str.parse::<Reference>().unwrap(), temp_dir).unwrap();

        downloader.download().await.unwrap();

        assert!(Path::new(&temp_dir).join(reference.to_string()).exists());
        assert!(
            Path::new(&temp_dir)
                .join(reference.to_string())
                .join("tracks.sqlite")
                .exists()
        );
    }
}

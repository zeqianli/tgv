mod alignment;
mod app;
mod bed;
mod contig_collection;
mod cytoband;
mod display_mode;
mod error;
mod feature;
mod helpers;
mod intervals;
mod message;
mod reference;
mod region;
mod register;
mod rendering;
mod repository;
mod sequence;
mod settings;
mod states;
mod strand;
mod track;
mod ucsc;
mod variant;
mod window;
use app::App;
use clap::Parser;
use error::TGVError;
mod tracks;
use crate::reference::Reference;
use crate::tracks::{UCSCDownloader, UcscDbTrackService};
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
};
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
            let downloader = UCSCDownloader::new(Reference::from_str(&reference)?, cache_dir)?;
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

    let settings: Settings = Settings::new(cli)?;

    let mut terminal = ratatui::init();
    execute!(stdout(), EnableMouseCapture)?;

    // Gather resources before starting the app.

    let mut app = match App::new(settings, &mut terminal).await {
        Ok(app) => app,
        Err(e) => {
            ratatui::restore();
            return Err(e);
        }
    };
    let app_result = app.run(&mut terminal).await;

    ratatui::restore();
    if let Err(err) = execute!(stdout(), DisableMouseCapture) {
        eprintln!("Error disabling mouse capture: {err}");
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

// async fn print_ucsc_accessions(n: usize, offset: usize) -> Result<(), TGVError> {
//     let accessions = UcscDbTrackService::list_accessions(n, 0).await?;

//     for (name, common_name) in accessions {
//         println!("{} (Organism: {})", name, common_name);
//     }
//     Ok(())
// }

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_snapshot;
    use ratatui::{backend::TestBackend, Terminal};
    use rstest::rstest;
    use std::env;

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
        Some("covid.sorted.bam"),
        Some("-g covid --offline --cache-dir tests/data/cache")
    )]
    #[case(
        Some("covid.sorted.bam"),
        Some("--no-reference -r MN908947.3:100 --offline")
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
        let settings = Settings::new(cli).unwrap().test_mode();

        let mut terminal = Terminal::new(TestBackend::new(50, 20)).unwrap();

        let mut app = App::new(settings, &mut terminal).await.unwrap();
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
        let reference = Reference::from_str(reference_str).unwrap();
        let temp_dir = tempfile::TempDir::new().unwrap();
        println!("temp_dir: {:?}", temp_dir.path());
        let downloader = UCSCDownloader::new(
            Reference::from_str(reference_str).unwrap(),
            temp_dir.path().to_str().unwrap().to_string(),
        )
        .unwrap();

        downloader.download().await.unwrap();

        assert!(temp_dir.path().join(reference.to_string()).exists());
        assert!(temp_dir
            .path()
            .join(reference.to_string())
            .join("tracks.sqlite")
            .exists());
    }
}

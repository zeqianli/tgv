use clap::Parser;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
};
use gv_core::error::TGVError;
use gv_core::reference::Reference;
use gv_core::tracks::{UCSCDownloader, UcscDbTrackService};
use std::io::stdout;
use tgv::{
    app::App,
    session::SessionFile,
    settings::{Cli, Commands, Settings},
};
#[tokio::main]
async fn main() -> Result<(), TGVError> {
    let cli = Cli::parse();

    match &cli.command {
        Some(Commands::Download {
            reference,
            cache_dir,
        }) => {
            let cache_dir = shellexpand::tilde(&cache_dir).to_string();
            let downloader = UCSCDownloader::new(reference.parse::<Reference>()?, &cache_dir)?;
            downloader.download().await?;
            return Ok(());
        }
        Some(Commands::List { more, all: _ }) => {
            if *more {
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

    // Load the requested session when provided. Otherwise, load or create the default session,
    // then apply CLI overrides on top.
    let session_path = cli.session_path();
    let mut settings = if session_path.exists() {
        match SessionFile::from_path(&session_path).and_then(Settings::try_from) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Warning: failed to load session file: {e}. Using defaults.");
                Settings::default()
            }
        }
    } else if cli.session.is_none() {
        // First run: write a default session so future launches restore state.
        if let Err(e) = SessionFile::default().write_to_path(&session_path) {
            eprintln!("Warning: failed to write default session: {e}.");
        }
        Settings::default()
    } else {
        Settings::default()
    };
    cli.apply_overrides(&mut settings)?;

    let mut terminal = ratatui::init();

    set_panic_hook();

    execute!(stdout(), EnableMouseCapture)?;

    // Gather resources before starting the app.
    let mut app = match App::new(settings, session_path).await {
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

    // Auto-save the active session on clean exit, and skip in test mode.
    if !app.settings.test_mode && app_result.is_ok() {
        if let Err(e) = SessionFile::try_from(&app).and_then(|s| s.write_to_path(&app.session_path))
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

use clap::Parser;
use crossterm::{
    event::{
        DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
};
use gv_core::logging::{init_file_logging_with_level, timestamped_log_file_name};
use gv_core::prelude::*;
use gv_core::tracks::{UCSCDownloader, UcscDbTrackService};
use gv_hgvs::{HgvsBuild, HgvsDataDir};
use gv_session::SessionSocket;
use gv_viewer::session::SessionFile;
use std::{io::stdout, path::PathBuf};
use tgv::{
    app::App,
    settings::{Cli, Commands, Settings},
};
#[tokio::main]
async fn main() -> Result<(), TGVError> {
    let cli = Cli::parse();
    let log_path = default_log_file_path();
    let log_level = if cli.debug_enabled() {
        log::LevelFilter::Trace
    } else {
        log::LevelFilter::Info
    };
    init_file_logging_with_level(&log_path, log_level)?;
    log::info!("Logging to {}", log_path.display());

    match &cli.command {
        Some(Commands::Mcp) => {
            return Ok(gv_mcp::serve(cli.mcp_settings()?).await?);
        }
        Some(Commands::Download {
            reference,
            cache_dir,
            source,
            no_hgvs,
        }) => {
            log::info!("Starting download for reference {reference}");
            let cache_dir = shellexpand::tilde(&cache_dir).to_string();
            let reference = reference.parse::<Reference>()?;
            let downloader = UCSCDownloader::new(reference.clone(), &cache_dir, (*source).into())?;
            downloader.download().await?;
            if !*no_hgvs {
                download_hgvs(&reference, &cache_dir).await?;
            }
            return Ok(());
        }
        Some(Commands::List { all }) => {
            log::info!("Listing reference genomes");
            if *all {
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

    // Only touch session files when the user explicitly resumes a session.
    let settings = match cli.resume_path() {
        Some(path) => {
            let mut settings = SessionFile::from_path(&path)
                .and_then(Settings::try_from)
                .map_err(|e| {
                    TGVError::CliError(format!("Failed to resume session {}: {e}", path.display()))
                })?;
            log::info!("Resumed session from {}", path.display());
            cli.apply_overrides(&mut settings)?;
            settings.session_path = Some(path);
            settings
        }
        None => Settings::try_from(cli)?,
    };
    log::info!(
        "Settings are ready: session={:?} reference={} tracks={} test_mode={}",
        settings.session_path,
        settings.core.reference,
        settings.files.len(),
        settings.test_mode,
    );

    let mut terminal = ratatui::init();

    set_panic_hook();

    execute!(stdout(), EnableMouseCapture, EnableBracketedPaste)?;
    // Insert mode paints leftward with Shift-Backspace, which terminals only tell apart from
    // Backspace with the kitty keyboard protocol.
    let keyboard_enhanced = matches!(
        crossterm::terminal::supports_keyboard_enhancement(),
        Ok(true)
    );
    if keyboard_enhanced {
        execute!(
            stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )?;
    }

    // Gather resources before starting the app.
    let mut app = match App::new(settings).await {
        Ok(app) => app,
        Err(e) => {
            log::error!("Failed to initialize the app: {e}");
            restore_keyboard(keyboard_enhanced);
            ratatui::restore();
            if let Err(err) = execute!(stdout(), DisableMouseCapture, DisableBracketedPaste) {
                log::error!("Error disabling mouse capture and bracketed paste: {err}");
                eprintln!("Error disabling mouse capture and bracketed paste: {err}");
            }
            return Err(e);
        }
    };
    app.viewer.warn_shift_backspace = !keyboard_enhanced;
    // Agents reach the viewer through `tgv mcp`; without the socket, the viewer still works.
    let socket = SessionSocket::bind(app.viewer.session.clone())
        .inspect_err(|e| log::warn!("Agents can't connect to this viewer: {e}"))
        .ok();
    let app_result = app.run(&mut terminal).await;
    drop(socket);

    restore_keyboard(keyboard_enhanced);
    ratatui::restore();
    if let Err(err) = execute!(stdout(), DisableMouseCapture, DisableBracketedPaste) {
        log::error!("Error disabling mouse capture and bracketed paste: {err}");
        eprintln!("Error disabling mouse capture and bracketed paste: {err}");
    }

    app.close().await?;
    match &app_result {
        Ok(()) => log::info!("The app exited successfully"),
        Err(e) => log::error!("The app exited with an error: {e}"),
    }
    app_result
}

/// Pops the keyboard protocol flags that `main` pushed.
/// Prepares the HGVS data for references that support it, and says so for the rest.
async fn download_hgvs(reference: &Reference, cache_dir: &str) -> Result<(), TGVError> {
    if HgvsBuild::from_reference(reference).is_none() {
        println!("HGVS data is only available for hg38 and hg19, so tgv skips it for {reference}.");
        return Ok(());
    }
    let data_dir = HgvsDataDir::new(reference, cache_dir)?;
    println!(
        "Downloading HGVS data to {} (about 6 GB). Skip it with --no-hgvs.",
        data_dir.path.display()
    );
    log::info!("Preparing HGVS data in {}", data_dir.path.display());
    let path = data_dir.path.clone();
    tokio::task::spawn_blocking(move || gv_hgvs::prepare::prepare(&data_dir))
        .await
        .map_err(|e| TGVError::IOError(format!("The HGVS download task failed: {e}")))??;
    println!("HGVS data is ready in {}.", path.display());
    Ok(())
}

fn restore_keyboard(enhanced: bool) {
    if enhanced && let Err(err) = execute!(stdout(), PopKeyboardEnhancementFlags) {
        log::error!("Error restoring the keyboard protocol: {err}");
    }
}

fn default_log_file_path() -> PathBuf {
    PathBuf::from(shellexpand::tilde("~/.tgv").as_ref()).join(timestamped_log_file_name())
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
        log::error!("The app panicked: {info}");
        hook(info);
        if let Err(err) = execute!(
            stdout(),
            PopKeyboardEnhancementFlags,
            DisableMouseCapture,
            DisableBracketedPaste
        ) {
            eprintln!("Error disabling mouse capture and bracketed paste: {err}");
        }
    }));
}

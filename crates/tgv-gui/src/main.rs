use clap::Parser;
use gpui::{
    AppContext, Application, Bounds, TitlebarOptions, WindowBounds, WindowOptions, px, size,
};
use gv_core::logging::{init_file_logging_with_level, timestamped_log_file_name};
use gv_core::prelude::*;
use gv_gui::{
    app::App,
    settings::{Cli, Settings},
    worker::WorkerHandle,
};
use gv_viewer::session::SessionFile;
use std::path::PathBuf;

fn main() -> Result<(), TGVError> {
    let cli = Cli::parse();
    let log_path = default_log_file_path();
    let log_level = if cli.viewer.debug {
        log::LevelFilter::Trace
    } else {
        log::LevelFilter::Info
    };
    init_file_logging_with_level(&log_path, log_level)?;
    log::info!("Logging to {}", log_path.display());

    // Only touch session files when the user explicitly resumes a session.
    let settings = match cli.viewer.resume_path() {
        Some(path) => {
            let mut settings = SessionFile::from_path(&path)
                .and_then(Settings::try_from)
                .map_err(|e| {
                    TGVError::CliError(format!("Failed to resume session {}: {e}", path.display()))
                })?;
            log::info!("Resumed session from {}", path.display());
            cli.viewer.apply_overrides(&mut settings)?;
            settings.session_path = Some(path);
            settings
        }
        None => Settings::try_from(cli)?,
    };
    log::info!(
        "Settings are ready: session={:?} reference={} tracks={}",
        settings.session_path,
        settings.core.reference,
        settings.files.len(),
    );

    // The worker opens the dataset while the window starts.
    let (worker, events) = WorkerHandle::spawn(settings.clone());
    Application::new().run(move |cx| {
        let bounds = Bounds::centered(None, size(px(1280.0), px(820.0)), cx);
        let window = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("TGV".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window, cx| {
                let app = cx.new(|cx| App::new(settings, worker, events, cx));
                window.focus(&app.read(cx).focus_handle);
                // Closing the window quits through the viewer, which refuses while tracks have
                // unsaved changes.
                let weak = app.downgrade();
                window.on_window_should_close(cx, move |_, cx| {
                    weak.update(cx, |app, cx| app.request_quit(cx))
                        .unwrap_or(true)
                });
                app
            },
        );
        match window {
            Ok(_) => cx.activate(true),
            Err(error) => {
                log::error!("Failed to open the window: {error}");
                cx.quit();
            }
        }
    });
    log::info!("The window closed");
    Ok(())
}

fn default_log_file_path() -> PathBuf {
    PathBuf::from(shellexpand::tilde("~/.tgv").as_ref()).join(timestamped_log_file_name())
}

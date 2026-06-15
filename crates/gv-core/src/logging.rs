//! File-backed logging for the `log` crate facade.

use chrono::{Local, SecondsFormat};
use log::{LevelFilter, Log, Metadata, Record};
use std::{
    fs::{File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LoggingError {
    #[error("failed to create the log directory {path}: {source}")]
    CreateDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("failed to open the log file {path}: {source}")]
    OpenFile {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("the global logger is already initialized")]
    AlreadyInitialized,
}

struct FileLogger {
    file: Mutex<File>,
    level: LevelFilter,
}

impl FileLogger {
    fn new(file: File, level: LevelFilter) -> Self {
        Self {
            file: Mutex::new(file),
            level,
        }
    }
}

impl Log for FileLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let timestamp = Local::now().to_rfc3339_opts(SecondsFormat::Millis, true);
        let target = record.module_path().unwrap_or_else(|| record.target());
        let line = record
            .line()
            .map(|line| format!(":{line}"))
            .unwrap_or_default();

        let mut file = match self.file.lock() {
            Ok(file) => file,
            Err(poisoned) => poisoned.into_inner(),
        };

        let _ = writeln!(
            file,
            "{timestamp} {:<5} {target}{line} - {}",
            record.level(),
            record.args()
        );
        let _ = file.flush();
    }

    fn flush(&self) {
        let mut file = match self.file.lock() {
            Ok(file) => file,
            Err(poisoned) => poisoned.into_inner(),
        };
        let _ = file.flush();
    }
}

pub fn init_file_logging(file_name: impl AsRef<Path>) -> Result<(), LoggingError> {
    init_file_logging_with_level(file_name, LevelFilter::Info)
}

pub fn timestamped_log_file_name() -> String {
    format!("{}.log", Local::now().format("%Y%m%d-%H%M%S-%3f"))
}

pub fn init_file_logging_with_level(
    file_name: impl AsRef<Path>,
    level: LevelFilter,
) -> Result<(), LoggingError> {
    let file_name = file_name.as_ref();
    if let Some(parent) = file_name
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent).map_err(|source| LoggingError::CreateDirectory {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(file_name)
        .map_err(|source| LoggingError::OpenFile {
            path: file_name.to_path_buf(),
            source,
        })?;

    log::set_boxed_logger(Box::new(FileLogger::new(file, level)))
        .map_err(|_| LoggingError::AlreadyInitialized)?;
    log::set_max_level(level);
    Ok(())
}

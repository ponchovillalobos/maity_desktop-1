//! File-based logging with automatic rotation
//!
//! Uses tracing-appender for automatic log file rotation.
//! Logs are stored in the app's data directory.

use std::path::PathBuf;
use std::sync::OnceLock;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

static LOG_DIRECTORY: OnceLock<PathBuf> = OnceLock::new();

/// Get the log directory path
pub fn get_log_directory() -> Option<PathBuf> {
    LOG_DIRECTORY.get().cloned()
}

/// Initialize file-based logging with rotation
///
/// Creates log files in the app's data directory with daily rotation.
/// Keeps logs for 7 days.
pub fn init_file_logging(app_name: &str) -> anyhow::Result<()> {
    // Get app data directory
    let log_dir = dirs::data_local_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not determine local data directory"))?
        .join(app_name)
        .join("logs");

    // Create log directory if it doesn't exist
    std::fs::create_dir_all(&log_dir)?;

    // Store the log directory for later use
    let _ = LOG_DIRECTORY.set(log_dir.clone());

    // Create a rolling file appender (daily rotation)
    let file_appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("maity")
        .filename_suffix("log")
        .max_log_files(7) // Keep 7 days of logs
        .build(&log_dir)?;

    // Create a non-blocking writer
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    // Keep the guard alive for the duration of the program
    // We leak it intentionally since logging should last the entire app lifetime
    std::mem::forget(_guard);

    // Create layers for console and file output
    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false) // No ANSI colors in file
        .with_target(true)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true);

    let console_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stdout)
        .with_ansi(true)
        .with_target(true);

    // Set up the subscriber with env filter
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(filter)
        .with(file_layer)
        .with(console_layer)
        .init();

    tracing::info!("File logging initialized at: {:?}", log_dir);

    Ok(())
}

/// Get list of log files in the log directory
pub fn list_log_files() -> anyhow::Result<Vec<PathBuf>> {
    let log_dir =
        get_log_directory().ok_or_else(|| anyhow::anyhow!("Log directory not initialized"))?;

    let mut files = Vec::new();
    for entry in std::fs::read_dir(&log_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && path.extension().map_or(false, |ext| ext == "log") {
            files.push(path);
        }
    }

    // Sort by modification time (newest first)
    files.sort_by(|a, b| {
        let a_time = a.metadata().and_then(|m| m.modified()).ok();
        let b_time = b.metadata().and_then(|m| m.modified()).ok();
        b_time.cmp(&a_time)
    });

    Ok(files)
}

/// Get total size of log files in bytes
pub fn get_logs_total_size() -> anyhow::Result<u64> {
    let files = list_log_files()?;
    let total: u64 = files
        .iter()
        .filter_map(|f| f.metadata().ok())
        .map(|m| m.len())
        .sum();
    Ok(total)
}

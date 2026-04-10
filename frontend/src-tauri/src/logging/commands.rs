//! Tauri commands for log management
//!
//! Provides commands to export logs as ZIP files and get log information.

use std::io::{Read, Write};
use std::path::PathBuf;
use tauri::{AppHandle, Manager, Runtime};
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use super::file_logger::{get_log_directory, get_logs_total_size, list_log_files};
use crate::database::repositories::recording_log::RecordingLogRepository;
use crate::state::AppState;

/// Information about the log files
#[derive(Debug, Clone, serde::Serialize)]
pub struct LogInfo {
    pub log_directory: Option<String>,
    pub total_size_bytes: u64,
    pub total_size_human: String,
    pub file_count: usize,
    pub files: Vec<LogFileInfo>,
}

/// Information about a single log file
#[derive(Debug, Clone, serde::Serialize)]
pub struct LogFileInfo {
    pub name: String,
    pub size_bytes: u64,
    pub size_human: String,
    pub modified: Option<String>,
}

/// Format bytes into human-readable string
fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} bytes", bytes)
    }
}

/// Get information about log files
#[tauri::command]
pub async fn get_log_info() -> Result<LogInfo, String> {
    let log_dir = get_log_directory();
    let files = list_log_files().map_err(|e| format!("Failed to list log files: {}", e))?;
    let total_size = get_logs_total_size().map_err(|e| format!("Failed to get log size: {}", e))?;

    let file_infos: Vec<LogFileInfo> = files
        .iter()
        .filter_map(|path| {
            let metadata = path.metadata().ok()?;
            let name = path.file_name()?.to_string_lossy().to_string();
            let size = metadata.len();
            let modified = metadata.modified().ok().map(|t| {
                let datetime: chrono::DateTime<chrono::Local> = t.into();
                datetime.format("%Y-%m-%d %H:%M:%S").to_string()
            });

            Some(LogFileInfo {
                name,
                size_bytes: size,
                size_human: format_bytes(size),
                modified,
            })
        })
        .collect();

    Ok(LogInfo {
        log_directory: log_dir.map(|p| p.to_string_lossy().to_string()),
        total_size_bytes: total_size,
        total_size_human: format_bytes(total_size),
        file_count: file_infos.len(),
        files: file_infos,
    })
}

/// Export logs as a ZIP file
///
/// Creates a ZIP file containing all log files and saves it to the specified path.
/// If no path is provided, saves to the user's Downloads folder.
#[tauri::command]
pub async fn export_logs<R: Runtime>(
    _app: AppHandle<R>,
    output_path: Option<String>,
) -> Result<String, String> {
    let files = list_log_files().map_err(|e| format!("Failed to list log files: {}", e))?;

    if files.is_empty() {
        return Err("No log files found to export".to_string());
    }

    // Determine output path
    let output_path = if let Some(path) = output_path {
        PathBuf::from(path)
    } else {
        // Default to Downloads folder
        let downloads = dirs::download_dir()
            .ok_or_else(|| "Could not determine Downloads folder".to_string())?;

        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        downloads.join(format!("maity_logs_{}.zip", timestamp))
    };

    // Create the ZIP file
    let file = std::fs::File::create(&output_path)
        .map_err(|e| format!("Failed to create ZIP file: {}", e))?;

    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .compression_level(Some(6));

    for log_file in &files {
        let file_name = log_file
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown.log".to_string());

        // Read the log file
        let mut contents = Vec::new();
        let mut file = std::fs::File::open(log_file)
            .map_err(|e| format!("Failed to open log file {}: {}", file_name, e))?;
        file.read_to_end(&mut contents)
            .map_err(|e| format!("Failed to read log file {}: {}", file_name, e))?;

        // Add to ZIP
        zip.start_file(&file_name, options)
            .map_err(|e| format!("Failed to add file to ZIP: {}", e))?;
        zip.write_all(&contents)
            .map_err(|e| format!("Failed to write to ZIP: {}", e))?;
    }

    // Add a system info file
    let system_info = generate_system_info();
    zip.start_file("system_info.txt", options)
        .map_err(|e| format!("Failed to add system info to ZIP: {}", e))?;
    zip.write_all(system_info.as_bytes())
        .map_err(|e| format!("Failed to write system info: {}", e))?;

    // Add recording lifecycle logs from SQLite
    if let Some(app_state) = _app.try_state::<AppState>() {
        let pool = app_state.db_manager.pool();
        match RecordingLogRepository::export_all_logs_json(pool).await {
            Ok(logs_json) => {
                if let Err(e) = zip.start_file("recording_lifecycle_logs.json", options) {
                    tracing::warn!("Failed to add recording logs to ZIP: {}", e);
                } else if let Err(e) = zip.write_all(logs_json.as_bytes()) {
                    tracing::warn!("Failed to write recording logs to ZIP: {}", e);
                } else {
                    tracing::info!("Added recording lifecycle logs to export ZIP");
                }
            }
            Err(e) => {
                tracing::warn!("Failed to export recording logs: {}", e);
            }
        }
    }

    zip.finish()
        .map_err(|e| format!("Failed to finish ZIP file: {}", e))?;

    tracing::info!("Logs exported to: {:?}", output_path);

    Ok(output_path.to_string_lossy().to_string())
}

/// Generate system information for debugging
fn generate_system_info() -> String {
    let mut info = String::new();

    info.push_str("=== Maity Desktop System Info ===\n\n");
    info.push_str(&format!(
        "Generated: {}\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
    ));
    info.push_str(&format!("App Version: {}\n", env!("CARGO_PKG_VERSION")));
    info.push_str(&format!("OS: {}\n", std::env::consts::OS));
    info.push_str(&format!("Architecture: {}\n", std::env::consts::ARCH));

    // System memory info
    let sys = sysinfo::System::new_all();
    info.push_str(&format!(
        "Total Memory: {} MB\n",
        sys.total_memory() / 1024 / 1024
    ));
    info.push_str(&format!(
        "Available Memory: {} MB\n",
        sys.available_memory() / 1024 / 1024
    ));
    info.push_str(&format!("CPU Count: {}\n", sys.cpus().len()));

    if let Some(name) = sysinfo::System::name() {
        info.push_str(&format!("System Name: {}\n", name));
    }
    if let Some(version) = sysinfo::System::os_version() {
        info.push_str(&format!("OS Version: {}\n", version));
    }

    info.push_str("\n=== End System Info ===\n");

    info
}

/// Open the log directory in the system file explorer
#[tauri::command]
pub async fn open_log_directory() -> Result<(), String> {
    let log_dir = get_log_directory().ok_or_else(|| "Log directory not initialized".to_string())?;

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&log_dir)
            .spawn()
            .map_err(|e| format!("Failed to open directory: {}", e))?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&log_dir)
            .spawn()
            .map_err(|e| format!("Failed to open directory: {}", e))?;
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&log_dir)
            .spawn()
            .map_err(|e| format!("Failed to open directory: {}", e))?;
    }

    Ok(())
}

/// Clear old log files (keeps only the most recent)
#[tauri::command]
pub async fn clear_old_logs(keep_count: Option<usize>) -> Result<usize, String> {
    let keep = keep_count.unwrap_or(2); // Keep last 2 by default
    let files = list_log_files().map_err(|e| format!("Failed to list log files: {}", e))?;

    let mut deleted = 0;
    for (i, file) in files.iter().enumerate() {
        if i >= keep {
            if let Err(e) = std::fs::remove_file(file) {
                tracing::warn!("Failed to delete log file {:?}: {}", file, e);
            } else {
                deleted += 1;
            }
        }
    }

    tracing::info!("Cleared {} old log files", deleted);
    Ok(deleted)
}

//! Logging module with file rotation and export capabilities
//!
//! Provides structured logging to files with automatic rotation,
//! and export functionality for support debugging.

pub mod commands;
pub mod file_logger;

pub use commands::*;
pub use file_logger::{get_log_directory, init_file_logging};

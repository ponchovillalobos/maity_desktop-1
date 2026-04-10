//! Meeting Detector Module
//!
//! Detects when meeting applications (Zoom, Teams, Google Meet) are running
//! and optionally prompts the user to start recording.

pub mod commands;
pub mod detector;
pub mod process_monitor;
pub mod settings;

pub use commands::*;
pub use detector::MeetingDetector;
pub use process_monitor::{DetectedMeeting, MeetingApp};
pub use settings::MeetingDetectorSettings;

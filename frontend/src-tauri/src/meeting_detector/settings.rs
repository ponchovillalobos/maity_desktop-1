//! Meeting Detector Settings
//!
//! Configuration for meeting detection and auto-recording behavior.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager, Runtime};

use super::MeetingApp;

/// Settings for the meeting detector
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingDetectorSettings {
    /// Whether meeting detection is enabled
    pub enabled: bool,

    /// Whether to auto-start recording when a meeting is detected
    pub auto_record: bool,

    /// Delay in seconds before auto-recording starts (gives user time to cancel)
    pub auto_record_delay_seconds: u32,

    /// Whether to show a notification when a meeting is detected
    pub show_detection_notification: bool,

    /// Which meeting apps to monitor
    pub monitored_apps: MonitoredApps,

    /// How often to check for meetings (in seconds)
    pub check_interval_seconds: u32,

    /// Whether to remember user's choice for each app
    pub remember_choices: bool,

    /// User's choices for each app (auto-record, ignore, always ask)
    pub app_choices: Vec<AppChoice>,
}

/// Which meeting apps to monitor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitoredApps {
    pub zoom: bool,
    pub microsoft_teams: bool,
    pub google_meet: bool,
    pub webex: bool,
    pub slack: bool,
    pub discord: bool,
    pub skype: bool,
}

/// User's choice for a specific app
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppChoice {
    pub app: MeetingApp,
    pub action: AppAction,
}

/// Action to take when a meeting app is detected
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AppAction {
    /// Always ask the user
    AlwaysAsk,
    /// Auto-start recording
    AutoRecord,
    /// Ignore this app
    Ignore,
}

impl Default for MeetingDetectorSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_record: false,
            auto_record_delay_seconds: 5,
            show_detection_notification: true,
            monitored_apps: MonitoredApps::default(),
            check_interval_seconds: 5,
            remember_choices: true,
            app_choices: Vec::new(),
        }
    }
}

impl Default for MonitoredApps {
    fn default() -> Self {
        Self {
            zoom: true,
            microsoft_teams: true,
            google_meet: true,
            webex: true,
            slack: false,   // Slack huddles are less common
            discord: false, // Discord is often not for work meetings
            skype: true,
        }
    }
}

impl MonitoredApps {
    /// Check if a specific app is being monitored
    pub fn is_monitored(&self, app: &MeetingApp) -> bool {
        match app {
            MeetingApp::Zoom => self.zoom,
            MeetingApp::MicrosoftTeams => self.microsoft_teams,
            MeetingApp::GoogleMeet => self.google_meet,
            MeetingApp::Webex => self.webex,
            MeetingApp::Slack => self.slack,
            MeetingApp::Discord => self.discord,
            MeetingApp::Skype => self.skype,
            MeetingApp::Unknown(_) => false,
        }
    }
}

impl MeetingDetectorSettings {
    /// Get the action for a specific app
    pub fn get_app_action(&self, app: &MeetingApp) -> AppAction {
        // First check if there's a specific choice for this app
        for choice in &self.app_choices {
            if &choice.app == app {
                return choice.action.clone();
            }
        }

        // Default behavior based on auto_record setting
        if self.auto_record {
            AppAction::AutoRecord
        } else {
            AppAction::AlwaysAsk
        }
    }

    /// Set the action for a specific app
    pub fn set_app_action(&mut self, app: MeetingApp, action: AppAction) {
        // Remove existing choice for this app
        self.app_choices.retain(|c| c.app != app);

        // Add new choice
        self.app_choices.push(AppChoice { app, action });
    }
}

/// Settings file path helper
fn get_settings_path<R: Runtime>(app_handle: &AppHandle<R>) -> Result<PathBuf> {
    let config_dir = app_handle
        .path()
        .app_config_dir()
        .map_err(|e| anyhow::anyhow!("Failed to get config directory: {}", e))?;

    std::fs::create_dir_all(&config_dir)?;
    Ok(config_dir.join("meeting_detector_settings.json"))
}

/// Load settings from disk
pub async fn load_settings<R: Runtime>(
    app_handle: &AppHandle<R>,
) -> Result<MeetingDetectorSettings> {
    let path = get_settings_path(app_handle)?;

    if !path.exists() {
        return Ok(MeetingDetectorSettings::default());
    }

    let content = tokio::fs::read_to_string(&path).await?;
    let settings: MeetingDetectorSettings = serde_json::from_str(&content)?;
    Ok(settings)
}

/// Save settings to disk
pub async fn save_settings<R: Runtime>(
    app_handle: &AppHandle<R>,
    settings: &MeetingDetectorSettings,
) -> Result<()> {
    let path = get_settings_path(app_handle)?;
    let content = serde_json::to_string_pretty(settings)?;
    tokio::fs::write(&path, content).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_settings() {
        let settings = MeetingDetectorSettings::default();
        assert!(settings.enabled);
        assert!(!settings.auto_record);
        assert!(settings.show_detection_notification);
        assert_eq!(settings.check_interval_seconds, 5);
    }

    #[test]
    fn test_monitored_apps_default() {
        let apps = MonitoredApps::default();
        assert!(apps.zoom);
        assert!(apps.microsoft_teams);
        assert!(apps.google_meet);
        assert!(!apps.discord); // Discord off by default
    }

    #[test]
    fn test_app_action() {
        let mut settings = MeetingDetectorSettings::default();
        settings.set_app_action(MeetingApp::Zoom, AppAction::AutoRecord);

        assert_eq!(
            settings.get_app_action(&MeetingApp::Zoom),
            AppAction::AutoRecord
        );
        assert_eq!(
            settings.get_app_action(&MeetingApp::MicrosoftTeams),
            AppAction::AlwaysAsk
        );
    }
}

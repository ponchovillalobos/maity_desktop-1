//! Meeting Detector Tauri Commands
//!
//! Exposes meeting detection functionality to the frontend.

use anyhow::Result;
use log::info;
use std::sync::Arc;
use tauri::{AppHandle, Runtime, State, Wry};
use tokio::sync::RwLock;

use super::detector::{MeetingDetector, UserResponseAction};
use super::process_monitor::DetectedMeeting;
use super::settings::{AppAction, MeetingDetectorSettings};
use super::MeetingApp;

/// Shared state for the meeting detector
pub type MeetingDetectorState = Arc<RwLock<MeetingDetector>>;

/// Initialize the meeting detector (called during app setup)
pub async fn initialize_meeting_detector<R: Runtime>(
    app_handle: AppHandle<R>,
) -> Result<MeetingDetector> {
    info!("Initializing meeting detector...");
    let mut detector = MeetingDetector::new();
    detector.initialize(&app_handle).await?;
    info!("Meeting detector initialized successfully");
    Ok(detector)
}

/// Get meeting detector settings
#[tauri::command]
pub async fn get_meeting_detector_settings(
    state: State<'_, MeetingDetectorState>,
) -> Result<MeetingDetectorSettings, String> {
    info!("Getting meeting detector settings");
    let detector = state.read().await;
    Ok(detector.get_settings().await)
}

/// Update meeting detector settings
#[tauri::command]
pub async fn set_meeting_detector_settings(
    app: AppHandle<Wry>,
    settings: MeetingDetectorSettings,
    state: State<'_, MeetingDetectorState>,
) -> Result<(), String> {
    info!("Updating meeting detector settings");
    let detector = state.read().await;
    detector
        .update_settings(&app, settings)
        .await
        .map_err(|e| format!("Failed to update settings: {}", e))
}

/// Start the meeting detector
#[tauri::command]
pub async fn start_meeting_detector(
    app: AppHandle<Wry>,
    state: State<'_, MeetingDetectorState>,
) -> Result<(), String> {
    info!("Starting meeting detector");
    let mut detector = state.write().await;
    detector
        .start(app)
        .await
        .map_err(|e| format!("Failed to start detector: {}", e))
}

/// Stop the meeting detector
#[tauri::command]
pub async fn stop_meeting_detector(state: State<'_, MeetingDetectorState>) -> Result<(), String> {
    info!("Stopping meeting detector");
    let mut detector = state.write().await;
    detector.stop().await;
    Ok(())
}

/// Check if meeting detector is running
#[tauri::command]
pub async fn is_meeting_detector_running(
    state: State<'_, MeetingDetectorState>,
) -> Result<bool, String> {
    let detector = state.read().await;
    Ok(detector.is_running().await)
}

/// Get currently active meetings
#[tauri::command]
pub async fn get_active_meetings(
    state: State<'_, MeetingDetectorState>,
) -> Result<Vec<DetectedMeeting>, String> {
    let detector = state.read().await;
    Ok(detector.get_active_meetings().await)
}

/// Trigger a manual check for meetings
#[tauri::command]
pub async fn check_for_meetings_now(
    state: State<'_, MeetingDetectorState>,
) -> Result<Vec<DetectedMeeting>, String> {
    info!("Manual meeting check triggered");
    let detector = state.read().await;
    detector
        .check_now()
        .await
        .map_err(|e| format!("Failed to check for meetings: {}", e))?;
    Ok(detector.get_active_meetings().await)
}

/// User response to a meeting detection
#[tauri::command]
pub async fn respond_to_meeting_detection(
    pid: u32,
    action: String,
    meeting_name: Option<String>,
    state: State<'_, MeetingDetectorState>,
) -> Result<(), String> {
    info!(
        "User response to meeting detection: pid={}, action={}",
        pid, action
    );

    let response_action = match action.as_str() {
        "start_recording" => UserResponseAction::StartRecording {
            meeting_name: meeting_name.unwrap_or_else(|| "Meeting".to_string()),
        },
        "ignore" => UserResponseAction::Ignore,
        "ignore_always" => UserResponseAction::IgnoreAlways,
        "auto_record_always" => UserResponseAction::AutoRecordAlways,
        _ => return Err(format!("Unknown action: {}", action)),
    };

    let detector = state.read().await;
    detector
        .send_command(super::detector::DetectorCommand::UserResponse {
            pid,
            action: response_action,
        })
        .await
        .map_err(|e| format!("Failed to send response: {}", e))
}

/// Set the action for a specific app
#[tauri::command]
pub async fn set_meeting_app_action(
    app: AppHandle<Wry>,
    meeting_app: String,
    action: String,
    state: State<'_, MeetingDetectorState>,
) -> Result<(), String> {
    info!("Setting app action: {} -> {}", meeting_app, action);

    let app_enum = match meeting_app.to_lowercase().as_str() {
        "zoom" => MeetingApp::Zoom,
        "teams" | "microsoft_teams" => MeetingApp::MicrosoftTeams,
        "meet" | "google_meet" => MeetingApp::GoogleMeet,
        "webex" => MeetingApp::Webex,
        "slack" => MeetingApp::Slack,
        "discord" => MeetingApp::Discord,
        "skype" => MeetingApp::Skype,
        other => MeetingApp::Unknown(other.to_string()),
    };

    let action_enum = match action.to_lowercase().as_str() {
        "always_ask" | "ask" => AppAction::AlwaysAsk,
        "auto_record" | "auto" => AppAction::AutoRecord,
        "ignore" => AppAction::Ignore,
        _ => return Err(format!("Unknown action: {}", action)),
    };

    let detector = state.read().await;
    let mut settings = detector.get_settings().await;
    settings.set_app_action(app_enum, action_enum);
    detector
        .update_settings(&app, settings)
        .await
        .map_err(|e| format!("Failed to update settings: {}", e))
}

/// Enable or disable monitoring for a specific app
#[tauri::command]
pub async fn set_meeting_app_monitoring(
    app_handle: AppHandle<Wry>,
    meeting_app: String,
    enabled: bool,
    state: State<'_, MeetingDetectorState>,
) -> Result<(), String> {
    info!("Setting app monitoring: {} -> {}", meeting_app, enabled);

    let detector = state.read().await;
    let mut settings = detector.get_settings().await;

    match meeting_app.to_lowercase().as_str() {
        "zoom" => settings.monitored_apps.zoom = enabled,
        "teams" | "microsoft_teams" => settings.monitored_apps.microsoft_teams = enabled,
        "meet" | "google_meet" => settings.monitored_apps.google_meet = enabled,
        "webex" => settings.monitored_apps.webex = enabled,
        "slack" => settings.monitored_apps.slack = enabled,
        "discord" => settings.monitored_apps.discord = enabled,
        "skype" => settings.monitored_apps.skype = enabled,
        _ => return Err(format!("Unknown app: {}", meeting_app)),
    }

    detector
        .update_settings(&app_handle, settings)
        .await
        .map_err(|e| format!("Failed to update settings: {}", e))
}

/// Enable or disable the meeting detector entirely
#[tauri::command]
pub async fn set_meeting_detector_enabled(
    app_handle: AppHandle<Wry>,
    enabled: bool,
    state: State<'_, MeetingDetectorState>,
) -> Result<(), String> {
    info!("Setting meeting detector enabled: {}", enabled);

    let detector = state.read().await;
    let mut settings = detector.get_settings().await;
    settings.enabled = enabled;

    detector
        .update_settings(&app_handle, settings)
        .await
        .map_err(|e| format!("Failed to update settings: {}", e))
}

/// Enable or disable auto-recording
#[tauri::command]
pub async fn set_meeting_auto_record(
    app_handle: AppHandle<Wry>,
    enabled: bool,
    state: State<'_, MeetingDetectorState>,
) -> Result<(), String> {
    info!("Setting auto-record: {}", enabled);

    let detector = state.read().await;
    let mut settings = detector.get_settings().await;
    settings.auto_record = enabled;

    detector
        .update_settings(&app_handle, settings)
        .await
        .map_err(|e| format!("Failed to update settings: {}", e))
}

/// Get list of all monitored apps with their status
#[tauri::command]
pub async fn get_monitored_apps_status(
    state: State<'_, MeetingDetectorState>,
) -> Result<Vec<MonitoredAppStatus>, String> {
    let detector = state.read().await;
    let settings = detector.get_settings().await;

    let apps = vec![
        MonitoredAppStatus {
            id: "zoom".to_string(),
            name: "Zoom".to_string(),
            enabled: settings.monitored_apps.zoom,
            action: settings.get_app_action(&MeetingApp::Zoom).into(),
        },
        MonitoredAppStatus {
            id: "microsoft_teams".to_string(),
            name: "Microsoft Teams".to_string(),
            enabled: settings.monitored_apps.microsoft_teams,
            action: settings.get_app_action(&MeetingApp::MicrosoftTeams).into(),
        },
        MonitoredAppStatus {
            id: "google_meet".to_string(),
            name: "Google Meet".to_string(),
            enabled: settings.monitored_apps.google_meet,
            action: settings.get_app_action(&MeetingApp::GoogleMeet).into(),
        },
        MonitoredAppStatus {
            id: "webex".to_string(),
            name: "Webex".to_string(),
            enabled: settings.monitored_apps.webex,
            action: settings.get_app_action(&MeetingApp::Webex).into(),
        },
        MonitoredAppStatus {
            id: "slack".to_string(),
            name: "Slack Huddle".to_string(),
            enabled: settings.monitored_apps.slack,
            action: settings.get_app_action(&MeetingApp::Slack).into(),
        },
        MonitoredAppStatus {
            id: "discord".to_string(),
            name: "Discord".to_string(),
            enabled: settings.monitored_apps.discord,
            action: settings.get_app_action(&MeetingApp::Discord).into(),
        },
        MonitoredAppStatus {
            id: "skype".to_string(),
            name: "Skype".to_string(),
            enabled: settings.monitored_apps.skype,
            action: settings.get_app_action(&MeetingApp::Skype).into(),
        },
    ];

    Ok(apps)
}

/// Status of a monitored app for the frontend
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MonitoredAppStatus {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub action: String, // "always_ask", "auto_record", "ignore"
}

impl From<AppAction> for String {
    fn from(action: AppAction) -> Self {
        match action {
            AppAction::AlwaysAsk => "always_ask".to_string(),
            AppAction::AutoRecord => "auto_record".to_string(),
            AppAction::Ignore => "ignore".to_string(),
        }
    }
}

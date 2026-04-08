//! Meeting Detector
//!
//! Main detector that monitors for meetings and triggers notifications/recording.

use anyhow::Result;
use log::{debug, error, info};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Runtime};
use tokio::sync::{mpsc, RwLock};
use tokio::time::{interval, Duration};

use super::process_monitor::{DetectedMeeting, ProcessMonitor};
use super::settings::{load_settings, save_settings, AppAction, MeetingDetectorSettings};

/// Events emitted by the meeting detector
#[derive(Debug, Clone, serde::Serialize)]
pub struct MeetingDetectedEvent {
    pub meeting: DetectedMeeting,
    pub action: String, // "ask", "auto_record", "ignored"
}

/// Commands that can be sent to the detector
#[derive(Debug)]
pub enum DetectorCommand {
    /// Start monitoring for meetings
    Start,
    /// Stop monitoring
    Stop,
    /// Update settings
    UpdateSettings(MeetingDetectorSettings),
    /// User response to a detection
    UserResponse {
        pid: u32,
        action: UserResponseAction,
    },
    /// Check for meetings now (manual trigger)
    CheckNow,
}

/// User's response to a meeting detection
#[derive(Debug, Clone)]
pub enum UserResponseAction {
    /// Start recording with the given meeting name
    StartRecording { meeting_name: String },
    /// Ignore this meeting
    Ignore,
    /// Ignore and remember for this app
    IgnoreAlways,
    /// Auto-record and remember for this app
    AutoRecordAlways,
}

/// The meeting detector service
pub struct MeetingDetector {
    settings: Arc<RwLock<MeetingDetectorSettings>>,
    process_monitor: Arc<RwLock<ProcessMonitor>>,
    is_running: Arc<RwLock<bool>>,
    command_tx: Option<mpsc::Sender<DetectorCommand>>,
}

impl MeetingDetector {
    /// Create a new meeting detector
    pub fn new() -> Self {
        Self {
            settings: Arc::new(RwLock::new(MeetingDetectorSettings::default())),
            process_monitor: Arc::new(RwLock::new(ProcessMonitor::new())),
            is_running: Arc::new(RwLock::new(false)),
            command_tx: None,
        }
    }

    /// Initialize the detector with saved settings
    pub async fn initialize<R: Runtime>(&mut self, app_handle: &AppHandle<R>) -> Result<()> {
        let settings = load_settings(app_handle).await.unwrap_or_default();
        *self.settings.write().await = settings;
        info!("Meeting detector initialized");
        Ok(())
    }

    /// Start the background monitoring task
    pub async fn start<R: Runtime + 'static>(&mut self, app_handle: AppHandle<R>) -> Result<()> {
        let (tx, rx) = mpsc::channel::<DetectorCommand>(32);
        self.command_tx = Some(tx);

        let settings = self.settings.clone();
        let process_monitor = self.process_monitor.clone();
        let is_running = self.is_running.clone();

        *is_running.write().await = true;

        // Spawn the background monitoring task
        tokio::spawn(async move {
            run_detector_loop(app_handle, settings, process_monitor, is_running, rx).await;
        });

        info!("Meeting detector started");
        Ok(())
    }

    /// Stop the monitoring task
    pub async fn stop(&mut self) {
        *self.is_running.write().await = false;
        if let Some(tx) = &self.command_tx {
            let _ = tx.send(DetectorCommand::Stop).await;
        }
        self.command_tx = None;
        info!("Meeting detector stopped");
    }

    /// Send a command to the detector
    pub async fn send_command(&self, cmd: DetectorCommand) -> Result<()> {
        if let Some(tx) = &self.command_tx {
            tx.send(cmd)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to send command: {}", e))?;
        }
        Ok(())
    }

    /// Get current settings
    pub async fn get_settings(&self) -> MeetingDetectorSettings {
        self.settings.read().await.clone()
    }

    /// Update settings
    pub async fn update_settings<R: Runtime>(
        &self,
        app_handle: &AppHandle<R>,
        settings: MeetingDetectorSettings,
    ) -> Result<()> {
        save_settings(app_handle, &settings).await?;
        *self.settings.write().await = settings.clone();
        if let Some(tx) = &self.command_tx {
            let _ = tx.send(DetectorCommand::UpdateSettings(settings)).await;
        }
        Ok(())
    }

    /// Check if detector is running
    pub async fn is_running(&self) -> bool {
        *self.is_running.read().await
    }

    /// Get currently active meetings
    pub async fn get_active_meetings(&self) -> Vec<DetectedMeeting> {
        self.process_monitor.write().await.get_active_meetings()
    }

    /// Check for meetings now (manual trigger)
    pub async fn check_now(&self) -> Result<()> {
        if let Some(tx) = &self.command_tx {
            tx.send(DetectorCommand::CheckNow)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to send check command: {}", e))?;
        }
        Ok(())
    }
}

impl Default for MeetingDetector {
    fn default() -> Self {
        Self::new()
    }
}

/// The main detector loop
async fn run_detector_loop<R: Runtime>(
    app_handle: AppHandle<R>,
    settings: Arc<RwLock<MeetingDetectorSettings>>,
    process_monitor: Arc<RwLock<ProcessMonitor>>,
    is_running: Arc<RwLock<bool>>,
    mut command_rx: mpsc::Receiver<DetectorCommand>,
) {
    let mut check_interval = {
        let s = settings.read().await;
        interval(Duration::from_secs(s.check_interval_seconds as u64))
    };

    info!("Meeting detector loop started");

    loop {
        tokio::select! {
            // Check for meetings on interval
            _ = check_interval.tick() => {
                if !*is_running.read().await {
                    break;
                }

                let current_settings = settings.read().await.clone();
                if !current_settings.enabled {
                    continue;
                }

                // Detect new meetings
                let detected = process_monitor.write().await.detect_meetings();

                for meeting in detected {
                    // Check if this app is being monitored
                    if !current_settings.monitored_apps.is_monitored(&meeting.app) {
                        debug!("Ignoring {} - not monitored", meeting.app.display_name());
                        continue;
                    }

                    // Determine action based on settings
                    let action = current_settings.get_app_action(&meeting.app);

                    match action {
                        AppAction::AutoRecord => {
                            info!("Auto-recording meeting: {}", meeting.suggested_name);
                            emit_meeting_detected(&app_handle, &meeting, "auto_record");
                        }
                        AppAction::Ignore => {
                            debug!("Ignoring {} - user preference", meeting.app.display_name());
                        }
                        AppAction::AlwaysAsk => {
                            info!("Meeting detected, asking user: {}", meeting.suggested_name);
                            emit_meeting_detected(&app_handle, &meeting, "ask");
                        }
                    }
                }
            }

            // Handle commands
            Some(cmd) = command_rx.recv() => {
                match cmd {
                    DetectorCommand::Stop => {
                        info!("Detector received stop command");
                        break;
                    }
                    DetectorCommand::Start => {
                        // Already running
                    }
                    DetectorCommand::UpdateSettings(new_settings) => {
                        // Update check interval if changed
                        let old_interval = settings.read().await.check_interval_seconds;
                        if new_settings.check_interval_seconds != old_interval {
                            check_interval = interval(Duration::from_secs(
                                new_settings.check_interval_seconds as u64
                            ));
                        }
                        *settings.write().await = new_settings;
                        info!("Detector settings updated");
                    }
                    DetectorCommand::UserResponse { pid, action } => {
                        handle_user_response(
                            &app_handle,
                            &settings,
                            &process_monitor,
                            pid,
                            action,
                        ).await;
                    }
                    DetectorCommand::CheckNow => {
                        // Force an immediate check
                        check_interval.reset();
                    }
                }
            }
        }
    }

    info!("Meeting detector loop ended");
}

/// Emit a meeting detected event to the frontend
fn emit_meeting_detected<R: Runtime>(
    app_handle: &AppHandle<R>,
    meeting: &DetectedMeeting,
    action: &str,
) {
    let event = MeetingDetectedEvent {
        meeting: meeting.clone(),
        action: action.to_string(),
    };

    if let Err(e) = app_handle.emit("meeting-detected", &event) {
        error!("Failed to emit meeting-detected event: {}", e);
    }
}

/// Handle user's response to a meeting detection
async fn handle_user_response<R: Runtime>(
    app_handle: &AppHandle<R>,
    _settings: &Arc<RwLock<MeetingDetectorSettings>>,
    process_monitor: &Arc<RwLock<ProcessMonitor>>,
    pid: u32,
    action: UserResponseAction,
) {
    match action {
        UserResponseAction::StartRecording { meeting_name } => {
            info!("User chose to start recording: {}", meeting_name);
            // Emit event to start recording
            if let Err(e) = app_handle.emit("start-recording-from-detector", &meeting_name) {
                error!("Failed to emit start-recording event: {}", e);
            }
        }
        UserResponseAction::Ignore => {
            info!("User chose to ignore meeting");
            process_monitor.write().await.ignore_pid(pid);
        }
        UserResponseAction::IgnoreAlways => {
            info!("User chose to always ignore this app");
            // Would need the app info to set this properly
            process_monitor.write().await.ignore_pid(pid);
        }
        UserResponseAction::AutoRecordAlways => {
            info!("User chose to always auto-record this app");
            // Would need the app info to set this properly
        }
    }
}

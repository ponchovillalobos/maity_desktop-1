// audio/recording_commands.rs
//
// Slim Tauri command layer for recording functionality.
// Delegates to recording_lifecycle and recording_helpers for actual implementation.

use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::sync::atomic::Ordering;

use super::{DeviceEvent, DeviceMonitorType};

// Re-export TranscriptUpdate for backward compatibility
pub use super::transcription::TranscriptUpdate;

// Re-export lifecycle functions that are used by lib.rs and mod.rs
pub use super::recording_lifecycle::{
    is_recording_active as is_recording_active_fn, pause_recording, resume_recording,
    start_recording, start_recording_with_devices, start_recording_with_devices_and_meeting,
    start_recording_with_meeting_name, stop_recording,
};

use super::recording_lifecycle::{IS_RECORDING, RECORDING_MANAGER};

// ============================================================================
// PUBLIC TYPES
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct RecordingArgs {
    pub save_path: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct TranscriptionStatus {
    pub chunks_in_queue: usize,
    pub is_processing: bool,
    pub last_activity_ms: u64,
}

// ============================================================================
// QUERY COMMANDS (Status, State, Metadata)
// ============================================================================

/// Check if recording is active
pub async fn is_recording() -> bool {
    IS_RECORDING.load(Ordering::SeqCst)
}

/// Get recording statistics
pub async fn get_transcription_status() -> TranscriptionStatus {
    TranscriptionStatus {
        chunks_in_queue: 0,
        is_processing: IS_RECORDING.load(Ordering::SeqCst),
        last_activity_ms: 0,
    }
}

/// Check if recording is currently paused
#[tauri::command]
pub async fn is_recording_paused() -> bool {
    match RECORDING_MANAGER.lock() {
        Ok(manager_guard) => {
            if let Some(manager) = manager_guard.as_ref() {
                manager.is_paused()
            } else {
                false
            }
        }
        Err(_) => false,
    }
}

/// Get detailed recording state
#[tauri::command]
pub async fn get_recording_state() -> serde_json::Value {
    let is_recording = IS_RECORDING.load(Ordering::SeqCst);
    let manager_guard = match RECORDING_MANAGER.lock() {
        Ok(guard) => guard,
        Err(_) => {
            return serde_json::json!({
                "is_recording": is_recording,
                "is_paused": false,
                "is_active": false,
                "recording_duration": null,
                "active_duration": null,
                "total_pause_duration": 0.0,
                "current_pause_duration": null,
                "error": "Lock poisoned"
            });
        }
    };

    if let Some(manager) = manager_guard.as_ref() {
        serde_json::json!({
            "is_recording": is_recording,
            "is_paused": manager.is_paused(),
            "is_active": manager.is_active(),
            "recording_duration": manager.get_recording_duration(),
            "active_duration": manager.get_active_recording_duration(),
            "total_pause_duration": manager.get_total_pause_duration(),
            "current_pause_duration": manager.get_current_pause_duration()
        })
    } else {
        serde_json::json!({
            "is_recording": is_recording,
            "is_paused": false,
            "is_active": false,
            "recording_duration": null,
            "active_duration": null,
            "total_pause_duration": 0.0,
            "current_pause_duration": null
        })
    }
}

/// Get the meeting folder path for the current recording
#[tauri::command]
pub async fn get_meeting_folder_path() -> Result<Option<String>, String> {
    let manager_guard = RECORDING_MANAGER
        .lock()
        .map_err(|e| format!("Recording manager lock poisoned: {}", e))?;
    if let Some(manager) = manager_guard.as_ref() {
        Ok(manager
            .get_meeting_folder()
            .map(|p| p.to_string_lossy().to_string()))
    } else {
        Ok(None)
    }
}

/// Get accumulated transcript segments from current recording session
/// Used for syncing frontend state after page reload during active recording
#[tauri::command]
pub async fn get_transcript_history(
) -> Result<Vec<crate::audio::recording_saver::TranscriptSegment>, String> {
    let manager_guard = RECORDING_MANAGER
        .lock()
        .map_err(|e| format!("Recording manager lock poisoned: {}", e))?;

    if let Some(manager) = manager_guard.as_ref() {
        Ok(manager.get_transcript_segments())
    } else {
        Ok(Vec::new())
    }
}

/// Get meeting name from current recording session
/// Used for syncing frontend state after page reload during active recording
#[tauri::command]
pub async fn get_recording_meeting_name() -> Result<Option<String>, String> {
    let manager_guard = RECORDING_MANAGER
        .lock()
        .map_err(|e| format!("Recording manager lock poisoned: {}", e))?;

    if let Some(manager) = manager_guard.as_ref() {
        Ok(manager.get_meeting_name())
    } else {
        Ok(None)
    }
}

// ============================================================================
// DEVICE MONITORING COMMANDS (AirPods/Bluetooth disconnect/reconnect support)
// ============================================================================

/// Response structure for device events
#[derive(Debug, Serialize, Clone)]
#[serde(tag = "type")]
pub enum DeviceEventResponse {
    DeviceDisconnected {
        device_name: String,
        device_type: String,
    },
    DeviceReconnected {
        device_name: String,
        device_type: String,
    },
    DeviceListChanged,
}

impl From<DeviceEvent> for DeviceEventResponse {
    fn from(event: DeviceEvent) -> Self {
        match event {
            DeviceEvent::DeviceDisconnected {
                device_name,
                device_type,
            } => DeviceEventResponse::DeviceDisconnected {
                device_name,
                device_type: format!("{:?}", device_type),
            },
            DeviceEvent::DeviceReconnected {
                device_name,
                device_type,
            } => DeviceEventResponse::DeviceReconnected {
                device_name,
                device_type: format!("{:?}", device_type),
            },
            DeviceEvent::DeviceListChanged => DeviceEventResponse::DeviceListChanged,
        }
    }
}

/// Reconnection status information
#[derive(Debug, Serialize, Clone)]
pub struct ReconnectionStatus {
    pub is_reconnecting: bool,
    pub disconnected_device: Option<DisconnectedDeviceInfo>,
}

/// Information about a disconnected device
#[derive(Debug, Serialize, Clone)]
pub struct DisconnectedDeviceInfo {
    pub name: String,
    pub device_type: String,
}

/// Poll for audio device events (disconnect/reconnect)
/// Should be called periodically (every 1-2 seconds) by frontend during recording
#[tauri::command]
pub async fn poll_audio_device_events() -> Result<Option<DeviceEventResponse>, String> {
    let mut manager_guard = RECORDING_MANAGER
        .lock()
        .map_err(|e| format!("Recording manager lock poisoned: {}", e))?;

    if let Some(manager) = manager_guard.as_mut() {
        if let Some(event) = manager.poll_device_events() {
            info!("📱 Device event polled: {:?}", event);
            Ok(Some(event.into()))
        } else {
            Ok(None)
        }
    } else {
        Ok(None)
    }
}

/// Get current reconnection status
#[tauri::command]
pub async fn get_reconnection_status() -> Result<ReconnectionStatus, String> {
    let manager_guard = RECORDING_MANAGER
        .lock()
        .map_err(|e| format!("Recording manager lock poisoned: {}", e))?;

    if let Some(manager) = manager_guard.as_ref() {
        let state = manager.get_state();
        let disconnected_device = state
            .get_disconnected_device()
            .map(|(device, device_type)| DisconnectedDeviceInfo {
                name: device.name.clone(),
                device_type: format!("{:?}", device_type),
            });

        Ok(ReconnectionStatus {
            is_reconnecting: manager.is_reconnecting(),
            disconnected_device,
        })
    } else {
        Ok(ReconnectionStatus {
            is_reconnecting: false,
            disconnected_device: None,
        })
    }
}

/// Get information about the active audio output device
/// Used to warn users about Bluetooth playback issues
#[tauri::command]
pub async fn get_active_audio_output() -> Result<super::playback_monitor::AudioOutputInfo, String> {
    super::playback_monitor::get_active_audio_output()
        .await
        .map_err(|e| format!("Failed to get audio output info: {}", e))
}

/// Switch to a different audio device during active recording.
/// Used by the inline device selector in the recording UI.
#[tauri::command]
pub async fn switch_audio_device(device_name: String, device_type: String) -> Result<bool, String> {
    let monitor_type = match device_type.as_str() {
        "Microphone" => DeviceMonitorType::Microphone,
        "SystemAudio" => DeviceMonitorType::SystemAudio,
        _ => return Err(format!("Invalid device type: {}", device_type)),
    };

    // Take the manager out of the mutex so we can call async methods without
    // holding the MutexGuard across await points (MutexGuard is not Send).
    let mut manager = {
        let mut guard = RECORDING_MANAGER
            .lock()
            .map_err(|e| format!("Recording manager lock poisoned: {}", e))?;
        guard
            .take()
            .ok_or_else(|| "Recording not active".to_string())?
    };

    let result = manager
        .switch_audio_device(&device_name, monitor_type)
        .await;

    // Put the manager back
    {
        let mut guard = RECORDING_MANAGER
            .lock()
            .map_err(|e| format!("Recording manager lock poisoned: {}", e))?;
        *guard = Some(manager);
    }

    match result {
        Ok(success) => {
            if success {
                info!("✅ Device switch successful");
            } else {
                warn!("❌ Device switch failed - device not found");
            }
            Ok(success)
        }
        Err(e) => {
            error!("Device switch error: {}", e);
            Err(e.to_string())
        }
    }
}

/// Manually trigger device reconnection attempt
/// Useful for UI "Retry" button
#[tauri::command]
pub async fn attempt_device_reconnect(
    device_name: String,
    device_type: String,
) -> Result<bool, String> {
    let monitor_type = match device_type.as_str() {
        "Microphone" => DeviceMonitorType::Microphone,
        "SystemAudio" => DeviceMonitorType::SystemAudio,
        _ => return Err(format!("Invalid device type: {}", device_type)),
    };

    // Take the manager out of the mutex so we can call async methods without
    // holding the MutexGuard across await points (MutexGuard is not Send).
    let mut manager = {
        let mut guard = RECORDING_MANAGER
            .lock()
            .map_err(|e| format!("Recording manager lock poisoned: {}", e))?;
        guard
            .take()
            .ok_or_else(|| "Recording not active".to_string())?
    };

    let result = manager
        .attempt_device_reconnect(&device_name, monitor_type)
        .await;

    // Put the manager back
    {
        let mut guard = RECORDING_MANAGER
            .lock()
            .map_err(|e| format!("Recording manager lock poisoned: {}", e))?;
        *guard = Some(manager);
    }

    match result {
        Ok(success) => {
            if success {
                info!("✅ Manual reconnection successful");
            } else {
                warn!("❌ Manual reconnection failed - device not available");
            }
            Ok(success)
        }
        Err(e) => {
            error!("Manual reconnection error: {}", e);
            Err(e.to_string())
        }
    }
}

// ============================================================================
// TRANSCRIPTION CONTROL COMMANDS
// ============================================================================

/// Cancel any pending transcription chunks during shutdown.
/// Frontend can call this to skip remaining chunks and finish faster.
#[tauri::command]
pub async fn cancel_pending_transcription() -> Result<(), String> {
    info!("cancel_pending_transcription called by frontend");
    super::transcription::worker::request_cancellation();
    Ok(())
}

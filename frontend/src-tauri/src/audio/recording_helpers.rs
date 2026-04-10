// audio/recording_helpers.rs
//
// Shared helper functions for recording lifecycle operations.
// Extracted from recording_commands.rs to reduce duplication.

use log::{error, info, warn};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Runtime};

use super::{default_input_device, default_output_device, parse_audio_device, RecordingManager};

use super::transcription::{self, reset_speech_detected_flag, TranscriptUpdate};

use super::recording_lifecycle::{
    set_recording_flag, RECORDING_MANAGER, TRANSCRIPTION_TASK, TRANSCRIPT_LISTENER_ID,
};

/// Result of device resolution for recording
pub struct ResolvedDevices {
    pub microphone: Option<Arc<super::devices::AudioDevice>>,
    pub system_audio: Option<Arc<super::devices::AudioDevice>>,
}

/// Resolve microphone device from preference name or fallback to default
pub fn resolve_microphone_from_preference(
    preferred_name: Option<String>,
) -> Result<Option<Arc<super::devices::AudioDevice>>, String> {
    match preferred_name {
        Some(pref_name) => {
            info!("🎤 Attempting to use preferred microphone: '{}'", pref_name);
            match parse_audio_device(&pref_name) {
                Ok(device) => {
                    info!("✅ Using preferred microphone: '{}'", device.name);
                    Ok(Some(Arc::new(device)))
                }
                Err(e) => {
                    warn!(
                        "⚠️ Preferred microphone '{}' not available: {}",
                        pref_name, e
                    );
                    warn!("   Falling back to system default microphone...");
                    match default_input_device() {
                        Ok(device) => {
                            info!("✅ Using default microphone: '{}'", device.name);
                            Ok(Some(Arc::new(device)))
                        }
                        Err(default_err) => {
                            error!(
                                "❌ No microphone available (preferred and default both failed)"
                            );
                            Err(format!(
                                "No microphone device available. Preferred device '{}' not found, and default microphone unavailable: {}",
                                pref_name, default_err
                            ))
                        }
                    }
                }
            }
        }
        None => {
            info!("🎤 No microphone preference set, using system default");
            match default_input_device() {
                Ok(device) => {
                    info!("✅ Using default microphone: '{}'", device.name);
                    Ok(Some(Arc::new(device)))
                }
                Err(e) => {
                    error!("❌ No default microphone available");
                    Err(format!("No microphone device available: {}", e))
                }
            }
        }
    }
}

/// Resolve system audio device from preference name or fallback to default
/// System audio is optional - returns None if unavailable
pub fn resolve_system_audio_from_preference(
    preferred_name: Option<String>,
) -> Option<Arc<super::devices::AudioDevice>> {
    match preferred_name {
        Some(pref_name) => {
            info!(
                "🔊 Attempting to use preferred system audio: '{}'",
                pref_name
            );
            match parse_audio_device(&pref_name) {
                Ok(device) => {
                    info!("✅ Using preferred system audio: '{}'", device.name);
                    Some(Arc::new(device))
                }
                Err(e) => {
                    warn!(
                        "⚠️ Preferred system audio '{}' not available: {}",
                        pref_name, e
                    );
                    warn!("   Falling back to system default...");
                    match default_output_device() {
                        Ok(device) => {
                            info!("✅ Using default system audio: '{}'", device.name);
                            Some(Arc::new(device))
                        }
                        Err(default_err) => {
                            warn!("⚠️ No system audio available (preferred and default both failed): {}", default_err);
                            warn!("   Recording will continue with microphone only");
                            None
                        }
                    }
                }
            }
        }
        None => {
            info!("🔊 No system audio preference set, using system default");
            match default_output_device() {
                Ok(device) => {
                    info!("✅ Using default system audio: '{}'", device.name);
                    Some(Arc::new(device))
                }
                Err(e) => {
                    warn!("⚠️ No default system audio available: {}", e);
                    warn!("   Recording will continue with microphone only");
                    None
                }
            }
        }
    }
}

/// Parse explicit device names into device handles
pub fn parse_explicit_devices(
    mic_device_name: &Option<String>,
    system_device_name: &Option<String>,
) -> Result<ResolvedDevices, String> {
    let microphone = if let Some(ref name) = mic_device_name {
        Some(Arc::new(parse_audio_device(name).map_err(|e| {
            format!("Invalid microphone device '{}': {}", name, e)
        })?))
    } else {
        None
    };

    let system_audio = if let Some(ref name) = system_device_name {
        Some(Arc::new(parse_audio_device(name).map_err(|e| {
            format!("Invalid system device '{}': {}", name, e)
        })?))
    } else {
        None
    };

    Ok(ResolvedDevices {
        microphone,
        system_audio,
    })
}

/// Initialize recording manager, start recording, store global state, and register event listeners.
/// This is the shared core logic used by both start_recording variants.
pub async fn initialize_recording<R: Runtime>(
    app: &AppHandle<R>,
    microphone_device: Option<Arc<super::devices::AudioDevice>>,
    system_device: Option<Arc<super::devices::AudioDevice>>,
    meeting_name: Option<String>,
    auto_save: bool,
) -> Result<(), String> {
    // Create new recording manager
    let mut manager = RecordingManager::new();

    // Generate effective meeting name
    let effective_meeting_name = meeting_name.unwrap_or_else(|| {
        let now = chrono::Local::now();
        format!("Reunión {}", now.format("%Y-%m-%d_%H-%M-%S"))
    });
    manager.set_meeting_name(Some(effective_meeting_name));

    // Set up error callback
    let app_for_error = app.clone();
    manager.set_error_callback(move |error| {
        let _ = app_for_error.emit("recording-error", error.user_message());
    });

    // Start recording with resolved devices
    let transcription_receiver = manager
        .start_recording(microphone_device, system_device, auto_save)
        .await
        .map_err(|e| format!("Failed to start recording: {}", e))?;

    // Get a clone of the recording state for the level emission task
    let recording_state = manager.get_state().clone();

    // Store the manager globally to keep it alive
    {
        let mut global_manager = RECORDING_MANAGER
            .lock()
            .map_err(|e| format!("Recording manager lock poisoned: {}", e))?;
        *global_manager = Some(manager);
    }

    // Set recording flag and reset speech detection flag
    info!("🔍 Setting IS_RECORDING to true and resetting SPEECH_DETECTED_EMITTED");
    set_recording_flag(true);
    reset_speech_detected_flag();

    // Spawn audio level emission task — polls RecordingState atomics every 100ms
    {
        let app_for_levels = app.clone();
        let state_for_levels = recording_state.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(100));
            loop {
                interval.tick().await;
                if !state_for_levels.is_recording() {
                    break;
                }
                let (mic_rms, mic_peak, sys_rms, sys_peak) = state_for_levels.get_audio_levels();
                let _ = app_for_levels.emit(
                    "recording-audio-levels",
                    serde_json::json!({
                        "micRms": mic_rms,
                        "micPeak": mic_peak,
                        "sysRms": sys_rms,
                        "sysPeak": sys_peak,
                    }),
                );
            }
        });
    }

    // Start optimized parallel transcription task and store handle
    let task_handle = transcription::start_transcription_task(app.clone(), transcription_receiver);
    {
        let mut global_task = TRANSCRIPTION_TASK
            .lock()
            .map_err(|e| format!("Transcription task lock poisoned: {}", e))?;
        *global_task = Some(task_handle);
    }

    // Register transcript-update event listener for history persistence
    register_transcript_listener(app);

    Ok(())
}

/// Register the transcript-update event listener that saves segments to the recording manager.
/// Stores the listener ID for cleanup during stop_recording.
fn register_transcript_listener<R: Runtime>(app: &AppHandle<R>) {
    use tauri::Listener;
    let listener_id = app.listen("transcript-update", move |event: tauri::Event| {
        if let Ok(update) = serde_json::from_str::<TranscriptUpdate>(event.payload()) {
            let segment = crate::audio::recording_saver::TranscriptSegment {
                id: format!("seg_{}", update.sequence_id),
                text: update.text.clone(),
                audio_start_time: update.audio_start_time,
                audio_end_time: update.audio_end_time,
                duration: update.duration,
                display_time: update.timestamp.clone(),
                confidence: update.confidence,
                sequence_id: update.sequence_id,
                source_type: update.source_type.clone(),
            };

            if let Ok(manager_guard) = RECORDING_MANAGER.lock() {
                if let Some(manager) = manager_guard.as_ref() {
                    manager.add_transcript_segment(segment);
                }
            }
        }
    });
    match TRANSCRIPT_LISTENER_ID.lock() {
        Ok(mut global_listener) => {
            *global_listener = Some(listener_id);
            info!("✅ Transcript-update event listener registered for history persistence");
        }
        Err(e) => {
            warn!(
                "⚠️ Failed to store transcript listener ID (lock poisoned): {}",
                e
            );
        }
    }
}

/// Validate that transcription models are ready before starting recording.
/// Emits an error event to the frontend if validation fails.
pub async fn validate_transcription_ready<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    info!("🔍 Validating transcription model availability before starting recording...");
    if let Err(validation_error) = transcription::validate_transcription_model_ready(app).await {
        error!("Model validation failed: {}", validation_error);

        let _ = app.emit("transcription-error", serde_json::json!({
            "error": validation_error,
            "userMessage": "Recording cannot start: Transcription model is still downloading. Please wait for the download to complete.",
            "actionable": false
        }));

        return Err(validation_error);
    }
    info!("✅ Transcription model validation passed");
    Ok(())
}

/// Helper function to classify device type from device name (privacy-safe)
pub fn classify_device_type(device_name: &str) -> &'static str {
    let name_lower = device_name.to_lowercase();
    if name_lower.contains("bluetooth")
        || name_lower.contains("airpods")
        || name_lower.contains("beats")
        || name_lower.contains("headphones")
        || name_lower.contains("bt ")
        || name_lower.contains("wireless")
    {
        "Bluetooth"
    } else {
        "Wired"
    }
}

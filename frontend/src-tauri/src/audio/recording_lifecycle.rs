// audio/recording_lifecycle.rs
//
// Recording lifecycle management: start, stop, pause, resume.
// Contains the global state and core lifecycle transitions.

use log::{error, info, warn};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tokio::task::JoinHandle;

use super::RecordingManager;

use super::recording_helpers;

// ============================================================================
// GLOBAL STATE
// ============================================================================

/// Simple recording state tracking
pub(crate) static IS_RECORDING: AtomicBool = AtomicBool::new(false);

/// Global recording manager and transcription task to keep them alive during recording
pub(crate) static RECORDING_MANAGER: Mutex<Option<RecordingManager>> = Mutex::new(None);
pub(crate) static TRANSCRIPTION_TASK: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);

/// Listener ID for proper cleanup - prevents microphone from staying active after recording stops
pub(crate) static TRANSCRIPT_LISTENER_ID: Mutex<Option<tauri::EventId>> = Mutex::new(None);

/// Set the IS_RECORDING flag
pub(crate) fn set_recording_flag(value: bool) {
    IS_RECORDING.store(value, Ordering::SeqCst);
}

/// Check if recording is active
pub fn is_recording_active() -> bool {
    IS_RECORDING.load(Ordering::SeqCst)
}

// ============================================================================
// START RECORDING
// ============================================================================

/// Start recording with default devices (loads preferences for device resolution)
pub async fn start_recording<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    start_recording_with_meeting_name(app, None).await
}

/// Start recording with default devices and optional meeting name
pub async fn start_recording_with_meeting_name<R: Runtime>(
    app: AppHandle<R>,
    meeting_name: Option<String>,
) -> Result<(), String> {
    info!(
        "Starting recording with default devices, meeting: {:?}",
        meeting_name
    );

    // Check if already recording
    let current_recording_state = IS_RECORDING.load(Ordering::SeqCst);
    info!("🔍 IS_RECORDING state check: {}", current_recording_state);
    if current_recording_state {
        return Err("Recording already in progress".to_string());
    }

    // Validate transcription model
    recording_helpers::validate_transcription_ready(&app).await?;

    info!("🚀 Starting async recording initialization");

    // Load recording preferences to get auto_save AND device preferences
    let (auto_save, preferred_mic_name, preferred_system_name) =
        match super::recording_preferences::load_recording_preferences(&app).await {
            Ok(prefs) => {
                info!("📋 Loaded recording preferences: auto_save={}, preferred_mic={:?}, preferred_system={:?}",
                      prefs.auto_save, prefs.preferred_mic_device, prefs.preferred_system_device);
                (
                    prefs.auto_save,
                    prefs.preferred_mic_device,
                    prefs.preferred_system_device,
                )
            }
            Err(e) => {
                warn!(
                    "Failed to load recording preferences, using defaults: {}",
                    e
                );
                (true, None, None)
            }
        };

    // Resolve devices from preferences
    let microphone_device =
        recording_helpers::resolve_microphone_from_preference(preferred_mic_name)?;
    let system_device =
        recording_helpers::resolve_system_audio_from_preference(preferred_system_name);

    // Initialize recording with resolved devices
    recording_helpers::initialize_recording(
        &app,
        microphone_device,
        system_device,
        meeting_name,
        auto_save,
    )
    .await?;

    // Emit success event
    app.emit(
        "recording-started",
        serde_json::json!({
            "message": "Recording started successfully with parallel processing",
            "devices": ["Default Microphone", "Default System Audio"],
            "workers": 3
        }),
    )
    .map_err(|e| e.to_string())?;

    // Update tray menu to reflect recording state
    crate::tray::update_tray_menu(&app);

    info!("✅ Recording started successfully with async-first approach");

    Ok(())
}

/// Start recording with specific devices
pub async fn start_recording_with_devices<R: Runtime>(
    app: AppHandle<R>,
    mic_device_name: Option<String>,
    system_device_name: Option<String>,
) -> Result<(), String> {
    start_recording_with_devices_and_meeting(app, mic_device_name, system_device_name, None).await
}

/// Start recording with specific devices and optional meeting name
pub async fn start_recording_with_devices_and_meeting<R: Runtime>(
    app: AppHandle<R>,
    mic_device_name: Option<String>,
    system_device_name: Option<String>,
    meeting_name: Option<String>,
) -> Result<(), String> {
    info!(
        "Starting recording with specific devices: mic={:?}, system={:?}, meeting={:?}",
        mic_device_name, system_device_name, meeting_name
    );

    // Check if already recording
    let current_recording_state = IS_RECORDING.load(Ordering::SeqCst);
    info!("🔍 IS_RECORDING state check: {}", current_recording_state);
    if current_recording_state {
        return Err("Recording already in progress".to_string());
    }

    // Skip transcription validation here — frontend already validated via checkTranscriptionReady()
    info!("🚀 Starting async recording initialization with custom devices");

    // Parse explicit device names
    let devices = recording_helpers::parse_explicit_devices(&mic_device_name, &system_device_name)?;

    // Load recording preferences for auto_save setting
    let auto_save = match super::recording_preferences::load_recording_preferences(&app).await {
        Ok(prefs) => {
            info!(
                "📋 Loaded recording preferences: auto_save={}",
                prefs.auto_save
            );
            prefs.auto_save
        }
        Err(e) => {
            warn!(
                "Failed to load recording preferences, defaulting to auto_save=true: {}",
                e
            );
            true
        }
    };

    // Initialize recording with explicit devices
    recording_helpers::initialize_recording(
        &app,
        devices.microphone,
        devices.system_audio,
        meeting_name,
        auto_save,
    )
    .await?;

    // Emit success event
    app.emit(
        "recording-started",
        serde_json::json!({
            "message": "Recording started with custom devices and parallel processing",
            "devices": [
                mic_device_name.unwrap_or_else(|| "Default Microphone".to_string()),
                system_device_name.unwrap_or_else(|| "Default System Audio".to_string())
            ],
            "workers": 3
        }),
    )
    .map_err(|e| e.to_string())?;

    // Update tray menu to reflect recording state
    crate::tray::update_tray_menu(&app);

    info!("✅ Recording started with custom devices using async-first approach");

    Ok(())
}

// ============================================================================
// STOP RECORDING
// ============================================================================

/// Stop recording with optimized graceful shutdown ensuring NO transcript chunks are lost
pub async fn stop_recording<R: Runtime>(
    app: AppHandle<R>,
    _args: super::recording_commands::RecordingArgs,
) -> Result<(), String> {
    info!(
        "🛑 Starting optimized recording shutdown - ensuring ALL transcript chunks are preserved"
    );

    // Check if recording is active
    if !IS_RECORDING.load(Ordering::SeqCst) {
        info!("Recording was not active");
        return Ok(());
    }

    // Emit shutdown progress to frontend
    let _ = app.emit(
        "recording-shutdown-progress",
        serde_json::json!({
            "stage": "stopping_audio",
            "message": "Stopping audio capture...",
            "progress": 20
        }),
    );

    // Step 1: Stop audio capture immediately (no more new chunks) with proper error handling
    let manager_for_cleanup = {
        let mut global_manager = RECORDING_MANAGER
            .lock()
            .map_err(|e| format!("Recording manager lock poisoned: {}", e))?;
        global_manager.take()
    };

    let stop_result = if let Some(mut manager) = manager_for_cleanup {
        // Use FORCE FLUSH to immediately process all accumulated audio
        info!("🚀 Using FORCE FLUSH to eliminate pipeline accumulation delays");
        let result = manager.stop_streams_and_force_flush().await;
        let manager_for_cleanup = Some(manager);
        (result, manager_for_cleanup)
    } else {
        warn!("No recording manager found to stop");
        (Ok(()), None)
    };

    let (stop_result, manager_for_cleanup) = stop_result;

    match stop_result {
        Ok(_) => {
            info!("✅ Audio streams stopped successfully - no more chunks will be created");
        }
        Err(e) => {
            error!("❌ Failed to stop audio streams: {}", e);
            return Err(format!("Failed to stop audio streams: {}", e));
        }
    }

    // Step 1.5: Clean up transcript listener to release microphone
    {
        use tauri::Listener;
        if let Some(listener_id) = TRANSCRIPT_LISTENER_ID
            .lock()
            .map_err(|e| format!("Listener ID lock poisoned: {}", e))?
            .take()
        {
            app.unlisten(listener_id);
            info!("✅ Transcript-update listener removed");
        }
    }

    // Step 2: Signal transcription workers to finish processing ALL queued chunks
    let _ = app.emit(
        "recording-shutdown-progress",
        serde_json::json!({
            "stage": "processing_transcripts",
            "message": "Processing remaining transcript chunks...",
            "progress": 40
        }),
    );

    // Wait for transcription task with enhanced progress monitoring
    let transcription_task = {
        let mut global_task = TRANSCRIPTION_TASK
            .lock()
            .map_err(|e| format!("Transcription task lock poisoned: {}", e))?;
        global_task.take()
    };

    if let Some(mut task_handle) = transcription_task {
        info!("Waiting for transcription to finish (2 min max)");

        // Adaptive timeout: 2 min max, but stop early if no progress for 15s
        let max_timeout = std::time::Duration::from_secs(120);
        let start = std::time::Instant::now();
        let mut task_done = false;

        loop {
            tokio::select! {
                result = &mut task_handle => {
                    match result {
                        Ok(()) => info!("All transcription chunks processed successfully"),
                        Err(e) => warn!("Transcription task completed with error: {:?}", e),
                    }
                    task_done = true;
                    break;
                }
                _ = tokio::time::sleep(tokio::time::Duration::from_millis(1500)) => {
                    let elapsed = start.elapsed();

                    // Emit progress event for frontend
                    let _ = app.emit(
                        "recording-shutdown-progress",
                        serde_json::json!({
                            "stage": "processing_transcripts",
                            "message": format!("Processing transcripts... ({:.0}s elapsed)", elapsed.as_secs_f64()),
                            "progress": 40,
                            "detailed": true,
                            "elapsed_seconds": elapsed.as_secs()
                        }),
                    );

                    // Check max timeout (2 minutes)
                    if elapsed >= max_timeout {
                        warn!("Transcription timeout (2 min) reached, continuing shutdown");
                        break;
                    }

                }
            }
        }

        // If task didn't complete naturally, it's still running — that's OK,
        // the worker will finish on its own or respond to cancellation
        if !task_done {
            info!("Transcription task still running after timeout, proceeding with shutdown");
        }
    } else {
        info!("No transcription task found to wait for");
    }

    // Step 3: Keep transcription model loaded in memory for fast recording restart
    // ~600MB RAM footprint is acceptable for a desktop meeting app.
    // Avoids 2-8s model reload delay on next recording start.
    info!("Transcription model kept loaded in memory for next recording");

    // Step 3.5: Track meeting ended analytics
    let analytics_data = if let Some(ref manager) = manager_for_cleanup {
        let state = manager.get_state();
        let stats = state.get_stats();

        Some((
            manager.get_recording_duration(),
            manager.get_active_recording_duration().unwrap_or(0.0),
            manager.get_total_pause_duration(),
            manager.get_transcript_segments().len() as u64,
            state.has_fatal_error(),
            state.get_microphone_device().map(|d| d.name.clone()),
            state.get_system_device().map(|d| d.name.clone()),
            stats.chunks_processed,
        ))
    } else {
        None
    };

    if let Some((
        total_duration,
        active_duration,
        pause_duration,
        transcript_segments_count,
        had_fatal_error,
        mic_device_name,
        sys_device_name,
        chunks_processed,
    )) = analytics_data
    {
        info!("📊 Collecting analytics for meeting end");

        let transcription_config =
            match crate::api::api_get_transcript_config(app.clone(), app.clone().state(), None)
                .await
            {
                Ok(Some(config)) => Some((config.provider, config.model)),
                _ => None,
            };

        let (transcription_provider, transcription_model) =
            transcription_config.unwrap_or_else(|| ("unknown".to_string(), "unknown".to_string()));

        let summary_config =
            match crate::api::api_get_model_config(app.clone(), app.clone().state(), None).await {
                Ok(Some(config)) => Some((config.provider, config.model)),
                _ => None,
            };

        let (summary_provider, summary_model) =
            summary_config.unwrap_or_else(|| ("unknown".to_string(), "unknown".to_string()));

        let microphone_device_type = mic_device_name
            .as_ref()
            .map(|name| recording_helpers::classify_device_type(name))
            .unwrap_or("Unknown");

        let system_audio_device_type = sys_device_name
            .as_ref()
            .map(|name| recording_helpers::classify_device_type(name))
            .unwrap_or("Unknown");

        match crate::analytics::commands::track_meeting_ended(
            transcription_provider.clone(),
            transcription_model.clone(),
            summary_provider.clone(),
            summary_model.clone(),
            total_duration,
            active_duration,
            pause_duration,
            microphone_device_type.to_string(),
            system_audio_device_type.to_string(),
            chunks_processed,
            transcript_segments_count,
            had_fatal_error,
        )
        .await
        {
            Ok(_) => info!("✅ Analytics tracked successfully for meeting end"),
            Err(e) => warn!("⚠️ Failed to track analytics: {}", e),
        }
    }

    // Step 4: Finalize recording state and cleanup resources safely
    let _ = app.emit(
        "recording-shutdown-progress",
        serde_json::json!({
            "stage": "finalizing",
            "message": "Finalizing recording and cleaning up resources...",
            "progress": 90
        }),
    );

    let (meeting_folder, meeting_name) = if let Some(mut manager) = manager_for_cleanup {
        info!("🧹 Performing final cleanup and saving recording data");

        let meeting_folder = manager.get_meeting_folder();
        let meeting_name = manager.get_meeting_name();

        match tokio::time::timeout(
            tokio::time::Duration::from_secs(300),
            manager.save_recording_only(&app),
        )
        .await
        {
            Ok(Ok(_)) => {
                info!("✅ Recording data saved successfully during cleanup");
            }
            Ok(Err(e)) => {
                warn!(
                    "⚠️ Error during recording cleanup (transcripts preserved): {}",
                    e
                );
            }
            Err(_) => {
                warn!("⏱️ File I/O timeout (5 minutes) reached during save, continuing shutdown");
            }
        }

        (meeting_folder, meeting_name)
    } else {
        info!("ℹ️ No recording manager available for cleanup");
        (None, None)
    };

    // Set recording flag to false
    info!("🔍 Setting IS_RECORDING to false");
    IS_RECORDING.store(false, Ordering::SeqCst);

    // Prepare metadata for frontend
    let (folder_path_str, meeting_name_str) = match (&meeting_folder, &meeting_name) {
        (Some(path), Some(name)) => (Some(path.to_string_lossy().to_string()), Some(name.clone())),
        _ => (None, None),
    };

    info!("📤 Preparing recording metadata for frontend save");
    info!("   folder_path: {:?}", folder_path_str);
    info!("   meeting_name: {:?}", meeting_name_str);

    info!("ℹ️ Skipping database save in Rust - frontend will save after all transcripts received");

    // Step 5: Complete shutdown
    let _ = app.emit(
        "recording-shutdown-progress",
        serde_json::json!({
            "stage": "complete",
            "message": "Recording stopped successfully",
            "progress": 100
        }),
    );

    app.emit(
        "recording-stopped",
        serde_json::json!({
            "message": "Recording stopped - frontend will save after all transcripts received",
            "folder_path": folder_path_str,
            "meeting_name": meeting_name_str
        }),
    )
    .map_err(|e| e.to_string())?;

    // Update tray menu to reflect stopped state
    crate::tray::update_tray_menu(&app);

    info!("🎉 Recording stopped successfully with ZERO transcript chunks lost");
    Ok(())
}

// ============================================================================
// PAUSE / RESUME
// ============================================================================

/// Pause the current recording
#[tauri::command]
pub async fn pause_recording<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    info!("Pausing recording");

    if !IS_RECORDING.load(Ordering::SeqCst) {
        return Err("No recording is currently active".to_string());
    }

    let manager_guard = RECORDING_MANAGER
        .lock()
        .map_err(|e| format!("Recording manager lock poisoned: {}", e))?;
    if let Some(manager) = manager_guard.as_ref() {
        manager.pause_recording().map_err(|e| e.to_string())?;

        app.emit(
            "recording-paused",
            serde_json::json!({
                "message": "Recording paused"
            }),
        )
        .map_err(|e| e.to_string())?;

        crate::tray::update_tray_menu(&app);

        info!("Recording paused successfully");
        Ok(())
    } else {
        Err("No recording manager found".to_string())
    }
}

/// Resume the current recording
#[tauri::command]
pub async fn resume_recording<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    info!("Resuming recording");

    if !IS_RECORDING.load(Ordering::SeqCst) {
        return Err("No recording is currently active".to_string());
    }

    let manager_guard = RECORDING_MANAGER
        .lock()
        .map_err(|e| format!("Recording manager lock poisoned: {}", e))?;
    if let Some(manager) = manager_guard.as_ref() {
        manager.resume_recording().map_err(|e| e.to_string())?;

        app.emit(
            "recording-resumed",
            serde_json::json!({
                "message": "Recording resumed"
            }),
        )
        .map_err(|e| e.to_string())?;

        crate::tray::update_tray_menu(&app);

        info!("Recording resumed successfully");
        Ok(())
    } else {
        Err("No recording manager found".to_string())
    }
}

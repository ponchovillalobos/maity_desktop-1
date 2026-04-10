// audio/transcription/worker.rs
//
// Parallel transcription worker pool and chunk processing logic.
// Includes ChunkAccumulator for batching small VAD segments before Parakeet/Whisper.

use super::engine::TranscriptionEngine;
use super::provider::{TranscriptionError, TranscriptionProvider};
use crate::audio::recording_state::DeviceType;
use crate::audio::AudioChunk;
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Runtime};

// Sequence counter for transcript updates (pub for use by streaming providers like Deepgram)
pub static SEQUENCE_COUNTER: AtomicU64 = AtomicU64::new(0);

// Speech detection flag - reset per recording session (pub for use by streaming providers)
pub static SPEECH_DETECTED_EMITTED: AtomicBool = AtomicBool::new(false);

/// Global cancellation flag for pending transcription during shutdown.
/// When set to true, workers stop processing and the task completes early.
static CANCEL_PENDING: AtomicBool = AtomicBool::new(false);

/// Check if cancellation has been requested
pub fn is_cancellation_requested() -> bool {
    CANCEL_PENDING.load(Ordering::SeqCst)
}

/// Request cancellation of pending transcription chunks
pub fn request_cancellation() {
    CANCEL_PENDING.store(true, Ordering::SeqCst);
    info!("Transcription cancellation requested");
}

/// Reset the speech detected flag for a new recording session
pub fn reset_speech_detected_flag() {
    SPEECH_DETECTED_EMITTED.store(false, Ordering::SeqCst);
    info!(
        "🔍 SPEECH_DETECTED_EMITTED reset to: {}",
        SPEECH_DETECTED_EMITTED.load(Ordering::SeqCst)
    );
}

/// Reset session-scoped counters for a new recording session
pub fn reset_session_counters() {
    SEQUENCE_COUNTER.store(0, Ordering::SeqCst);
    CANCEL_PENDING.store(false, Ordering::SeqCst);
    reset_speech_detected_flag();
    info!(
        "Session counters reset: SEQUENCE_COUNTER=0, SPEECH_DETECTED=false, CANCEL_PENDING=false"
    );
}

/// Accumulates small VAD segments into larger chunks before sending to transcription engine.
/// Reduces per-chunk inference overhead: 1 call with 3s audio is faster than 15 calls of 200ms.
/// Separate accumulators per device type (mic/system) preserve speaker attribution.
struct ChunkAccumulator {
    buffer: Vec<f32>,
    sample_rate: u32,
    /// Timestamp of the first sample in the buffer (recording-relative seconds)
    first_timestamp: f64,
    /// Device type of accumulated audio (mic or system)
    device_type: DeviceType,
    /// Running chunk_id counter for accumulated chunks
    next_chunk_id: u64,
    /// Minimum duration in seconds before flushing (default: varies by hardware tier)
    min_duration_secs: f64,
    /// Maximum duration in seconds before forcing a flush
    max_duration_secs: f64,
    /// Last time audio was added (for flush timeout)
    last_add_time: std::time::Instant,
    /// Flush timeout: if no new audio arrives within this duration, flush what we have
    flush_timeout: std::time::Duration,
}

impl ChunkAccumulator {
    fn new(min_duration: f64, max_duration: f64, flush_timeout_ms: u64) -> Self {
        Self {
            buffer: Vec::new(),
            sample_rate: 16000,
            first_timestamp: 0.0,
            device_type: DeviceType::Mixed,
            next_chunk_id: 0,
            min_duration_secs: min_duration,
            max_duration_secs: max_duration,
            last_add_time: std::time::Instant::now(),
            flush_timeout: std::time::Duration::from_millis(flush_timeout_ms),
        }
    }

    /// Add a chunk to the accumulator. Returns a merged AudioChunk if ready to flush.
    fn add(&mut self, chunk: AudioChunk) -> Option<AudioChunk> {
        if self.buffer.is_empty() {
            self.first_timestamp = chunk.timestamp;
            self.device_type = chunk.device_type;
            self.sample_rate = chunk.sample_rate;
        }

        self.buffer.extend_from_slice(&chunk.data);
        self.last_add_time = std::time::Instant::now();

        let duration = self.buffer.len() as f64 / self.sample_rate as f64;

        if duration >= self.max_duration_secs || duration >= self.min_duration_secs {
            return self.flush();
        }

        None
    }

    /// Check if buffer should be flushed due to timeout (no new audio for flush_timeout).
    fn check_timeout(&mut self) -> Option<AudioChunk> {
        if !self.buffer.is_empty() && self.last_add_time.elapsed() >= self.flush_timeout {
            let duration = self.buffer.len() as f64 / self.sample_rate as f64;
            if duration >= 0.5 {
                return self.flush();
            }
        }
        None
    }

    /// Force flush remaining buffer (e.g., when recording stops).
    fn flush(&mut self) -> Option<AudioChunk> {
        if self.buffer.is_empty() {
            return None;
        }

        let chunk_id = self.next_chunk_id;
        self.next_chunk_id += 1;

        let flushed = AudioChunk {
            data: std::mem::take(&mut self.buffer),
            sample_rate: self.sample_rate,
            timestamp: self.first_timestamp,
            chunk_id,
            device_type: self.device_type,
        };

        Some(flushed)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TranscriptUpdate {
    pub text: String,
    pub timestamp: String, // Wall-clock time for reference (e.g., "14:30:05")
    pub source: String,
    pub sequence_id: u64,
    pub chunk_start_time: f64, // Legacy field, kept for compatibility
    pub is_partial: bool,
    pub confidence: f32,
    // NEW: Recording-relative timestamps for playback sync
    pub audio_start_time: f64, // Seconds from recording start (e.g., 125.3)
    pub audio_end_time: f64,   // Seconds from recording start (e.g., 128.6)
    pub duration: f64,         // Segment duration in seconds (e.g., 3.3)
    // NEW: Source type for speaker identification (user=mic, interlocutor=system)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_type: Option<String>,
}

// NOTE: get_transcript_history and get_recording_meeting_name functions
// have been moved to recording_commands.rs where they have access to RECORDING_MANAGER

/// Optimized parallel transcription task ensuring ZERO chunk loss
pub fn start_transcription_task<R: Runtime>(
    app: AppHandle<R>,
    transcription_receiver: tokio::sync::mpsc::UnboundedReceiver<AudioChunk>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        info!("🚀 Starting optimized parallel transcription task - guaranteeing zero chunk loss");

        // Reset session-scoped counters (sequence IDs, speech detection flag)
        reset_session_counters();

        // Initialize transcription engine (Whisper or Parakeet based on config)
        println!("🚀 [WORKER] Inicializando transcription engine...");
        let transcription_engine = match super::engine::get_or_init_transcription_engine(&app).await
        {
            Ok(engine) => {
                println!(
                    "✅ [WORKER] Transcription engine inicializado: {}",
                    engine.provider_name()
                );
                engine
            }
            Err(e) => {
                println!(
                    "❌ [WORKER] Error inicializando transcription engine: {}",
                    e
                );
                error!("Failed to initialize transcription engine: {}", e);
                let _ = app.emit("transcription-error", serde_json::json!({
                    "error": e,
                    "userMessage": "Recording failed: Unable to initialize speech recognition. Please check your model settings.",
                    "actionable": true
                }));
                return;
            }
        };

        // Create parallel workers for faster processing while preserving ALL chunks
        const NUM_WORKERS: usize = 1; // Serial processing ensures transcripts emit in chronological order

        // FIX: Bounded channel con backpressure - evita memory leak en conversaciones muy largas
        // 2000 chunks = ~2 minutos de audio en cola máximo (a 60ms por chunk)
        // Si se llena, el sender esperará (backpressure) en vez de perder datos
        let (work_sender, work_receiver) = tokio::sync::mpsc::channel::<AudioChunk>(2000);
        let work_receiver = Arc::new(tokio::sync::Mutex::new(work_receiver));

        // Track completion: AtomicU64 for chunks queued, completed, and dropped
        let chunks_queued = Arc::new(AtomicU64::new(0));
        let chunks_completed = Arc::new(AtomicU64::new(0));
        let chunks_dropped = Arc::new(AtomicU64::new(0)); // FIX: Track dropped chunks for debugging
        let input_finished = Arc::new(AtomicBool::new(false));

        info!(
            "📊 Starting {} transcription worker{} (serial mode for ordered emission)",
            NUM_WORKERS,
            if NUM_WORKERS == 1 { "" } else { "s" }
        );

        // Check if this is a streaming provider (Deepgram persistent WS)
        let is_streaming = transcription_engine.is_streaming_provider();
        if is_streaming {
            info!("Streaming provider detected - reader task handles event emission");
        }

        // Spawn worker tasks
        let mut worker_handles = Vec::new();
        for worker_id in 0..NUM_WORKERS {
            let engine_clone = match &transcription_engine {
                TranscriptionEngine::Whisper(e) => TranscriptionEngine::Whisper(e.clone()),
                TranscriptionEngine::Parakeet(e) => TranscriptionEngine::Parakeet(e.clone()),
                TranscriptionEngine::Moonshine(e) => TranscriptionEngine::Moonshine(e.clone()),
                TranscriptionEngine::Deepgram { mic, sys } => TranscriptionEngine::Deepgram {
                    mic: mic.clone(),
                    sys: sys.clone(),
                },
                TranscriptionEngine::Provider(p) => TranscriptionEngine::Provider(p.clone()),
            };
            let app_clone = app.clone();
            let work_receiver_clone = work_receiver.clone();
            let chunks_completed_clone = chunks_completed.clone();
            let input_finished_clone = input_finished.clone();
            let chunks_queued_clone = chunks_queued.clone();
            let chunks_dropped_clone = chunks_dropped.clone();

            let worker_handle = tokio::spawn(async move {
                info!("👷 Worker {} started", worker_id);

                // PRE-VALIDATE model state to avoid repeated async calls per chunk
                let initial_model_loaded = engine_clone.is_model_loaded().await;
                let current_model = engine_clone
                    .get_current_model()
                    .await
                    .unwrap_or_else(|| "unknown".to_string());

                let engine_name = engine_clone.provider_name();

                if initial_model_loaded {
                    info!(
                        "✅ Worker {} pre-validation: {} model '{}' is loaded and ready",
                        worker_id, engine_name, current_model
                    );
                } else {
                    warn!(
                        "⚠️ Worker {} pre-validation: {} model not loaded - chunks may be skipped",
                        worker_id, engine_name
                    );
                }

                loop {
                    // Check cancellation before processing next chunk
                    if CANCEL_PENDING.load(Ordering::SeqCst) {
                        let mut cancelled_count: u64 = 0;
                        {
                            let mut receiver = work_receiver_clone.lock().await;
                            while receiver.try_recv().is_ok() {
                                cancelled_count += 1;
                            }
                        }
                        if cancelled_count > 0 {
                            chunks_dropped_clone.fetch_add(cancelled_count, Ordering::SeqCst);
                            chunks_completed_clone.fetch_add(cancelled_count, Ordering::SeqCst);
                            warn!(
                                "Worker {}: cancelled {} pending chunks",
                                worker_id, cancelled_count
                            );
                        }
                        info!("Worker {} stopping due to cancellation", worker_id);
                        break;
                    }

                    // Try to get a chunk to process
                    let chunk = {
                        let mut receiver = work_receiver_clone.lock().await;
                        receiver.recv().await
                    };

                    match chunk {
                        Some(chunk) => {
                            // PERFORMANCE OPTIMIZATION: Reduce logging in hot path
                            // Only log every 10th chunk per worker to reduce I/O overhead
                            let should_log_this_chunk = chunk.chunk_id % 10 == 0;

                            if should_log_this_chunk {
                                info!(
                                    "👷 Worker {} processing chunk {} with {} samples",
                                    worker_id,
                                    chunk.chunk_id,
                                    chunk.data.len()
                                );
                            }

                            // Check if model is still loaded before processing
                            if !engine_clone.is_model_loaded().await {
                                warn!("⚠️ Worker {}: Model unloaded, but continuing to preserve chunk {}", worker_id, chunk.chunk_id);
                                // Still count as completed even if we can't process
                                chunks_completed_clone.fetch_add(1, Ordering::SeqCst);
                                continue;
                            }

                            let chunk_timestamp = chunk.timestamp;
                            let chunk_duration = chunk.data.len() as f64 / chunk.sample_rate as f64;
                            // Capture device_type before chunk is moved (for speaker identification and routing)
                            let chunk_device_type = chunk.device_type.clone();
                            let chunk_source_type = match chunk_device_type {
                                crate::audio::recording_state::DeviceType::Microphone => {
                                    Some("user".to_string())
                                }
                                crate::audio::recording_state::DeviceType::System => {
                                    Some("interlocutor".to_string())
                                }
                                crate::audio::recording_state::DeviceType::Mixed => None, // Mixed audio should not be transcribed
                            };

                            // Transcribe with provider-agnostic approach
                            match transcribe_chunk_with_provider(
                                &engine_clone,
                                chunk,
                                &chunk_device_type,
                                &app_clone,
                            )
                            .await
                            {
                                Ok((transcript, confidence_opt, is_partial)) => {
                                    // Provider-aware confidence threshold
                                    let confidence_threshold = match &engine_clone {
                                        TranscriptionEngine::Whisper(_)
                                        | TranscriptionEngine::Provider(_) => 0.3,
                                        TranscriptionEngine::Deepgram { .. } => 0.3,
                                        TranscriptionEngine::Parakeet(_)
                                        | TranscriptionEngine::Moonshine(_) => 0.0, // Parakeet/Moonshine have no confidence, accept all
                                    };

                                    let confidence_str = match confidence_opt {
                                        Some(c) => format!("{:.2}", c),
                                        None => "N/A".to_string(),
                                    };

                                    info!("🔍 Worker {} transcription result: text='{}', confidence={}, partial={}, threshold={:.2}",
                                          worker_id, transcript, confidence_str, is_partial, confidence_threshold);

                                    // Check confidence threshold (or accept if no confidence provided)
                                    let meets_threshold =
                                        confidence_opt.map_or(true, |c| c >= confidence_threshold);

                                    if !transcript.trim().is_empty() && meets_threshold {
                                        // PERFORMANCE: Only log transcription results, not every processing step
                                        info!("✅ Worker {} transcribed: {} (confidence: {}, partial: {})",
                                              worker_id, transcript, confidence_str, is_partial);

                                        // Emit speech-detected event for frontend UX (only on first detection per session)
                                        // This is lightweight and provides better user feedback
                                        let current_flag =
                                            SPEECH_DETECTED_EMITTED.load(Ordering::SeqCst);
                                        info!("🔍 Checking speech-detected flag: current={}, will_emit={}", current_flag, !current_flag);

                                        if !current_flag {
                                            SPEECH_DETECTED_EMITTED.store(true, Ordering::SeqCst);
                                            match app_clone.emit("speech-detected", serde_json::json!({
                                                "message": "Speech activity detected"
                                            })) {
                                                Ok(_) => info!("🎤 ✅ First speech detected - successfully emitted speech-detected event"),
                                                Err(e) => error!("🎤 ❌ Failed to emit speech-detected event: {}", e),
                                            }
                                        } else {
                                            info!("🔍 Speech already detected in this session, not re-emitting");
                                        }

                                        // Generate sequence ID and calculate timestamps FIRST
                                        let sequence_id =
                                            SEQUENCE_COUNTER.fetch_add(1, Ordering::SeqCst);
                                        let audio_start_time = chunk_timestamp; // Already in seconds from recording start
                                        let audio_end_time = chunk_timestamp + chunk_duration;

                                        // Save structured transcript segment to recording manager (only final results)
                                        // Save ALL segments (partial and final) to ensure complete JSON
                                        // Create structured segment with full timestamp data
                                        // NOTE: This is now handled via the transcript-update event emission below
                                        // The recording_commands module listens to these events and saves them
                                        // This decouples the transcription worker from direct RECORDING_MANAGER access

                                        // Emit transcript update with NEW recording-relative timestamps

                                        let update = TranscriptUpdate {
                                            text: transcript,
                                            timestamp: format_current_timestamp(), // Wall-clock for reference
                                            source: "Audio".to_string(),
                                            sequence_id,
                                            chunk_start_time: chunk_timestamp, // Legacy compatibility
                                            is_partial,
                                            confidence: confidence_opt.unwrap_or(0.85), // Default for providers without confidence
                                            // NEW: Recording-relative timestamps for sync
                                            audio_start_time,
                                            audio_end_time,
                                            duration: chunk_duration,
                                            // NEW: Speaker identification (user=mic, interlocutor=system)
                                            source_type: chunk_source_type.clone(),
                                        };

                                        println!("📤 [WORKER] Emitiendo transcript-update: '{}' (seq: {}, partial: {}, confidence: {:.2})",
                                                 update.text, update.sequence_id, update.is_partial, update.confidence);

                                        match app_clone.emit("transcript-update", &update) {
                                            Ok(_) => {
                                                println!("✅ [WORKER] Evento transcript-update emitido correctamente");
                                            }
                                            Err(e) => {
                                                println!("❌ [WORKER] Error emitiendo transcript-update: {}", e);
                                                error!(
                                                    "Worker {}: Failed to emit transcript update: {}",
                                                    worker_id, e
                                                );
                                            }
                                        }
                                        // PERFORMANCE: Removed verbose logging of every emission
                                    } else if !transcript.trim().is_empty() && should_log_this_chunk
                                    {
                                        // PERFORMANCE: Only log low-confidence results occasionally
                                        if let Some(c) = confidence_opt {
                                            info!("Worker {} low-confidence transcription (confidence: {:.2}), skipping", worker_id, c);
                                        }
                                    }
                                }
                                Err(e) => {
                                    // Improved error handling with specific cases
                                    match e {
                                        TranscriptionError::AudioTooShort { .. } => {
                                            // Skip silently, this is expected for very short chunks
                                            info!("Worker {}: {}", worker_id, e);
                                            chunks_completed_clone.fetch_add(1, Ordering::SeqCst);
                                            continue;
                                        }
                                        TranscriptionError::ModelNotLoaded => {
                                            warn!(
                                                "Worker {}: Model unloaded during transcription",
                                                worker_id
                                            );
                                            chunks_completed_clone.fetch_add(1, Ordering::SeqCst);
                                            continue;
                                        }
                                        _ => {
                                            warn!(
                                                "Worker {}: Transcription failed: {}",
                                                worker_id, e
                                            );
                                            let _ = app_clone
                                                .emit("transcription-warning", e.to_string());
                                        }
                                    }
                                }
                            }

                            // Mark chunk as completed
                            let completed =
                                chunks_completed_clone.fetch_add(1, Ordering::SeqCst) + 1;
                            let queued = chunks_queued_clone.load(Ordering::SeqCst);

                            // PERFORMANCE: Only log progress every 5th chunk to reduce I/O overhead
                            if completed % 5 == 0 || should_log_this_chunk {
                                info!(
                                    "Worker {}: Progress {}/{} chunks ({:.1}%)",
                                    worker_id,
                                    completed,
                                    queued,
                                    (completed as f64 / queued.max(1) as f64 * 100.0)
                                );
                            }

                            // Emit progress event for frontend
                            let progress_percentage = if queued > 0 {
                                (completed as f64 / queued as f64 * 100.0) as u32
                            } else {
                                100
                            };

                            let _ = app_clone.emit("transcription-progress", serde_json::json!({
                                "worker_id": worker_id,
                                "chunks_completed": completed,
                                "chunks_queued": queued,
                                "progress_percentage": progress_percentage,
                                "message": format!("Worker {} processing... ({}/{})", worker_id, completed, queued)
                            }));
                        }
                        None => {
                            // No more chunks available
                            if input_finished_clone.load(Ordering::SeqCst) {
                                // Double-check that all queued chunks are actually completed
                                let final_queued = chunks_queued_clone.load(Ordering::SeqCst);
                                let final_completed = chunks_completed_clone.load(Ordering::SeqCst);

                                if final_completed >= final_queued {
                                    info!(
                                        "👷 Worker {} finishing - all {}/{} chunks processed",
                                        worker_id, final_completed, final_queued
                                    );
                                    break;
                                } else {
                                    warn!("👷 Worker {} detected potential chunk loss: {}/{} completed, waiting...", worker_id, final_completed, final_queued);
                                    // AGGRESSIVE POLLING: Reduced from 50ms to 5ms for faster chunk detection during shutdown
                                    tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
                                }
                            } else {
                                // AGGRESSIVE POLLING: Reduced from 10ms to 1ms for faster response during shutdown
                                tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
                            }
                        }
                    }
                }

                info!("👷 Worker {} completed", worker_id);
            });

            worker_handles.push(worker_handle);
        }

        // Main dispatcher: accumulate small VAD chunks before sending to workers.
        // Reduces transcription engine invocations (each has fixed inference overhead)
        // and improves throughput (processing 3s of audio in one call is faster than
        // processing 15 separate 200ms segments).
        let mut receiver = transcription_receiver;
        let chunks_dropped_dispatcher = chunks_dropped.clone();

        // Separate accumulators per device type (mic and system audio transcribe independently)
        // QW-4 (audit 2026-04-08): antes los tiers altos esperaban MÁS (1500ms flush
        // en Ultra) que los bajos — exactamente lo contrario de la intuición esperada.
        // El usuario con hardware potente se sentía MÁS laggy que el que tiene una laptop
        // débil. Ahora todos los tiers usan (0.3s, 4.0s, 400ms): emit-sooner, techo
        // razonable, flush rápido. Reduces latencia percibida ~700-1100ms.
        let hw_profile = crate::audio::HardwareProfile::detect();
        let (min_dur, max_dur, flush_timeout) = (0.3_f64, 4.0_f64, 400_u64);
        info!(
            "[WORKER] Adaptive ChunkAccumulator: min={:.1}s, max={:.1}s, flush={}ms (tier: {:?})",
            min_dur, max_dur, flush_timeout, hw_profile.performance_tier
        );
        let mut mic_accumulator = ChunkAccumulator::new(min_dur, max_dur, flush_timeout);
        let mut sys_accumulator = ChunkAccumulator::new(min_dur, max_dur, flush_timeout);

        // Lag metrics tracking
        let mut last_lag_emit = std::time::Instant::now();
        let lag_emit_interval = std::time::Duration::from_secs(3);
        let backpressure_threshold: u64 = 1500; // 75% of 2000 capacity

        /// Helper: send an accumulated chunk to the worker channel with adaptive backpressure
        async fn dispatch_accumulated(
            accumulated: AudioChunk,
            work_sender: &tokio::sync::mpsc::Sender<AudioChunk>,
            work_receiver: &Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<AudioChunk>>>,
            chunks_queued: &AtomicU64,
            chunks_dropped: &AtomicU64,
        ) -> bool {
            let duration = accumulated.data.len() as f64 / accumulated.sample_rate as f64;
            let queued = chunks_queued.fetch_add(1, Ordering::SeqCst) + 1;
            info!(
                "📥 Dispatching accumulated chunk {} ({:.1}s, {:?}) to workers (total queued: {})",
                accumulated.chunk_id, duration, accumulated.device_type, queued
            );

            // ADAPTIVE BACKPRESSURE: drop OLDEST chunks when full (not newest)
            match work_sender.try_send(accumulated) {
                Ok(()) => true,
                Err(tokio::sync::mpsc::error::TrySendError::Full(chunk)) => {
                    // Queue full: drain OLDEST chunks to make room for newest audio
                    let mut drained: u64 = 0;
                    {
                        let mut work_rx = work_receiver.lock().await;
                        for _ in 0..200 {
                            match work_rx.try_recv() {
                                Ok(_old_chunk) => {
                                    drained += 1;
                                    chunks_dropped.fetch_add(1, Ordering::SeqCst);
                                }
                                Err(_) => break,
                            }
                        }
                    }
                    if drained > 0 {
                        warn!(
                            "Backpressure: dropped {} oldest chunks to keep transcription current (chunk {})",
                            drained, chunk.chunk_id
                        );
                    }
                    match work_sender.try_send(chunk) {
                        Ok(()) => true,
                        Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                            warn!("Accumulated chunk dropped - queue still full after drain");
                            chunks_dropped.fetch_add(1, Ordering::SeqCst);
                            true
                        }
                        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                            error!("Channel closed - workers terminated unexpectedly");
                            false
                        }
                    }
                }
                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                    error!("Channel closed - workers terminated unexpectedly");
                    false
                }
            }
        }

        // Streaming providers (Deepgram) bypass accumulation — they send raw audio over WebSocket
        let use_accumulator = !is_streaming;

        loop {
            // Use a short timeout to periodically check for flush timeouts
            // (200ms for responsive silence detection in accumulators)
            match tokio::time::timeout(tokio::time::Duration::from_millis(200), receiver.recv())
                .await
            {
                Ok(Some(chunk)) => {
                    if use_accumulator {
                        // Route chunk to appropriate accumulator based on device type
                        let accumulated = match chunk.device_type {
                            DeviceType::Microphone => mic_accumulator.add(chunk),
                            DeviceType::System => sys_accumulator.add(chunk),
                            DeviceType::Mixed => Some(chunk), // Mixed: send directly
                        };

                        if let Some(acc_chunk) = accumulated {
                            if !dispatch_accumulated(
                                acc_chunk,
                                &work_sender,
                                &work_receiver,
                                &chunks_queued,
                                &chunks_dropped_dispatcher,
                            )
                            .await
                            {
                                break;
                            }
                        }
                    } else {
                        // Streaming provider: dispatch directly without accumulation
                        let queued = chunks_queued.fetch_add(1, Ordering::SeqCst) + 1;
                        info!(
                            "📥 Dispatching chunk {} to streaming worker (total queued: {})",
                            chunk.chunk_id, queued
                        );
                        match work_sender.try_send(chunk) {
                            Ok(()) => {}
                            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                                warn!("Streaming chunk dropped - queue full");
                                chunks_dropped_dispatcher.fetch_add(1, Ordering::SeqCst);
                            }
                            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                                error!("Channel closed - workers terminated unexpectedly");
                                break;
                            }
                        }
                    }
                }
                Ok(None) => {
                    // Channel closed - flush remaining accumulator buffers and exit
                    info!("📭 Input channel closed, flushing accumulators...");
                    if let Some(remaining) = mic_accumulator.flush() {
                        dispatch_accumulated(
                            remaining,
                            &work_sender,
                            &work_receiver,
                            &chunks_queued,
                            &chunks_dropped_dispatcher,
                        )
                        .await;
                    }
                    if let Some(remaining) = sys_accumulator.flush() {
                        dispatch_accumulated(
                            remaining,
                            &work_sender,
                            &work_receiver,
                            &chunks_queued,
                            &chunks_dropped_dispatcher,
                        )
                        .await;
                    }
                    break;
                }
                Err(_) => {
                    // Timeout — check if accumulators need flushing due to silence
                    if use_accumulator {
                        if let Some(timeout_chunk) = mic_accumulator.check_timeout() {
                            dispatch_accumulated(
                                timeout_chunk,
                                &work_sender,
                                &work_receiver,
                                &chunks_queued,
                                &chunks_dropped_dispatcher,
                            )
                            .await;
                        }
                        if let Some(timeout_chunk) = sys_accumulator.check_timeout() {
                            dispatch_accumulated(
                                timeout_chunk,
                                &work_sender,
                                &work_receiver,
                                &chunks_queued,
                                &chunks_dropped_dispatcher,
                            )
                            .await;
                        }
                    }
                }
            }

            // Lag metrics to frontend every 3 seconds
            let current_queued = chunks_queued.load(Ordering::Relaxed);
            let current_completed = chunks_completed.load(Ordering::Relaxed);
            let current_dropped = chunks_dropped.load(Ordering::Relaxed);
            let pending = current_queued.saturating_sub(current_completed);

            if pending > backpressure_threshold && pending % 100 == 0 {
                warn!("Transcription queue depth high: {} pending chunks", pending);
            }

            if last_lag_emit.elapsed() >= lag_emit_interval {
                // With accumulation, each queued chunk is ~min_dur seconds of audio
                let lag_seconds = pending as f64 * min_dur;
                let chunks_per_second = if current_completed > 0 {
                    current_completed as f64 / last_lag_emit.elapsed().as_secs_f64().max(1.0)
                } else {
                    0.0
                };
                let _ = app.emit(
                    "transcription-lag-update",
                    serde_json::json!({
                        "queue_depth": pending,
                        "lag_seconds": lag_seconds,
                        "chunks_per_second": chunks_per_second,
                        "chunks_received": current_queued,
                        "chunks_processed": current_completed,
                        "chunks_dropped": current_dropped
                    }),
                );
                last_lag_emit = std::time::Instant::now();
            }
        }

        // Signal that input is finished
        input_finished.store(true, Ordering::SeqCst);
        drop(work_sender); // Close the channel to signal workers

        let total_chunks_queued = chunks_queued.load(Ordering::SeqCst);
        info!("📭 Input finished with {} total chunks queued. Waiting for all {} workers to complete...",
              total_chunks_queued, NUM_WORKERS);

        // Emit transcription-finishing event so frontend can show progress UI
        let completed_now = chunks_completed.load(Ordering::SeqCst);
        let remaining = total_chunks_queued.saturating_sub(completed_now);
        let estimated_seconds = remaining as f64 * 0.5; // ~500ms per accumulated chunk
        let _ = app.emit(
            "transcription-finishing",
            serde_json::json!({
                "total_remaining": remaining,
                "processed": 0,
                "estimated_seconds": estimated_seconds,
                "total_chunks": total_chunks_queued,
                "chunks_completed": completed_now
            }),
        );

        // Emit final chunk count to frontend
        let _ = app.emit("transcription-queue-complete", serde_json::json!({
            "total_chunks": total_chunks_queued,
            "message": format!("{} chunks queued for processing - waiting for completion", total_chunks_queued)
        }));

        // Wait for all workers to complete
        for (worker_id, handle) in worker_handles.into_iter().enumerate() {
            match handle.await {
                Ok(()) => info!("Worker {} completed successfully", worker_id),
                Err(e) => {
                    error!("Worker {} panicked: {:?}", worker_id, e);
                    // RUST-002: report audio worker panics to Sentry with worker context
                    // (the global panic hook also fires, but this adds worker_id + task context)
                    if e.is_panic() {
                        sentry::capture_message(
                            &format!("Audio transcription worker {} panicked: {:?}", worker_id, e),
                            sentry::Level::Fatal,
                        );
                    }
                }
            }
        }

        // Close persistent stream for streaming providers (e.g., Deepgram)
        // This sends CloseStream and waits for the reader task to process remaining responses
        if is_streaming {
            info!("Closing persistent stream after all workers completed");
            transcription_engine.close_stream().await;
        }

        // Final verification with retry logic to catch any stragglers
        let mut verification_attempts = 0;
        const MAX_VERIFICATION_ATTEMPTS: u32 = 10;

        loop {
            let final_queued = chunks_queued.load(Ordering::SeqCst);
            let final_completed = chunks_completed.load(Ordering::SeqCst);
            let final_dropped = chunks_dropped.load(Ordering::SeqCst);

            if final_queued == final_completed + final_dropped {
                if final_dropped == 0 {
                    info!(
                        "🎉 ALL {} chunks processed successfully - ZERO chunks lost!",
                        final_completed
                    );
                } else {
                    warn!(
                        "⚠️ {} chunks processed, {} dropped due to backpressure",
                        final_completed, final_dropped
                    );
                }

                // FIX: Emit transcription summary with loss metrics for debugging
                let loss_percentage = if final_queued > 0 {
                    (final_dropped as f64 / final_queued as f64) * 100.0
                } else {
                    0.0
                };

                let _ = app.emit(
                    "transcription-summary",
                    serde_json::json!({
                        "chunks_queued": final_queued,
                        "chunks_completed": final_completed,
                        "chunks_dropped": final_dropped,
                        "loss_percentage": loss_percentage,
                        "status": if final_dropped == 0 { "success" } else { "partial_loss" }
                    }),
                );

                break;
            } else if verification_attempts < MAX_VERIFICATION_ATTEMPTS {
                verification_attempts += 1;
                warn!("⚠️ Chunk count mismatch (attempt {}): {} queued, {} completed, {} dropped - waiting for stragglers...",
                     verification_attempts, final_queued, final_completed, final_dropped);

                // Wait a bit for any remaining chunks to be processed
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            } else {
                let processing_loss = final_queued.saturating_sub(final_completed + final_dropped);
                error!(
                    "❌ CRITICAL: After {} attempts, chunk loss detected: {} queued, {} completed, {} dropped, {} lost in processing",
                    MAX_VERIFICATION_ATTEMPTS, final_queued, final_completed, final_dropped, processing_loss
                );

                // Emit critical error event with full metrics
                let _ = app.emit(
                    "transcript-chunk-loss-detected",
                    serde_json::json!({
                        "chunks_queued": final_queued,
                        "chunks_completed": final_completed,
                        "chunks_dropped": final_dropped,
                        "chunks_lost_in_processing": processing_loss,
                        "total_loss": final_dropped + processing_loss,
                        "message": "Some transcript chunks may have been lost during shutdown"
                    }),
                );
                break;
            }
        }

        // Emit transcription-complete event so frontend knows processing is done
        let final_completed = chunks_completed.load(Ordering::SeqCst);
        let final_dropped = chunks_dropped.load(Ordering::SeqCst);
        let was_cancelled = CANCEL_PENDING.load(Ordering::SeqCst);
        let _ = app.emit(
            "transcription-complete",
            serde_json::json!({
                "chunks_completed": final_completed,
                "chunks_dropped": final_dropped,
                "total_chunks": total_chunks_queued,
                "was_cancelled": was_cancelled
            }),
        );

        info!(
            "Parallel transcription task completed - all workers finished, ready for model unload"
        );
    })
}

/// Transcribe audio chunk using the appropriate provider (Whisper, Parakeet, or trait-based)
/// Returns: (text, confidence Option, is_partial)
async fn transcribe_chunk_with_provider<R: Runtime>(
    engine: &TranscriptionEngine,
    chunk: AudioChunk,
    device_type: &crate::audio::recording_state::DeviceType,
    app: &AppHandle<R>,
) -> std::result::Result<(String, Option<f32>, bool), TranscriptionError> {
    // Convert to 16kHz mono for transcription
    let transcription_data = if chunk.sample_rate != 16000 {
        crate::audio::audio_processing::resample_audio(&chunk.data, chunk.sample_rate, 16000)
    } else {
        chunk.data
    };

    // Skip VAD processing here since the pipeline already extracted speech using VAD
    let speech_samples = transcription_data;

    // Check for empty samples - improved error handling
    if speech_samples.is_empty() {
        warn!(
            "Audio chunk {} is empty, skipping transcription",
            chunk.chunk_id
        );
        return Err(TranscriptionError::AudioTooShort {
            samples: 0,
            minimum: 1600, // 100ms at 16kHz
        });
    }

    // Calculate energy for logging/monitoring only
    let energy: f32 =
        speech_samples.iter().map(|&x| x * x).sum::<f32>() / speech_samples.len() as f32;
    info!(
        "Processing speech audio chunk {} with {} samples (energy: {:.6})",
        chunk.chunk_id,
        speech_samples.len(),
        energy
    );

    // Transcribe using the appropriate engine (with improved error handling)
    match engine {
        TranscriptionEngine::Whisper(whisper_engine) => {
            // Get language preference from global state
            let language = crate::get_language_preference_internal();

            match whisper_engine
                .transcribe_audio_with_confidence(speech_samples, language)
                .await
            {
                Ok((text, confidence, is_partial)) => {
                    let cleaned_text = text.trim().to_string();
                    if cleaned_text.is_empty() {
                        return Ok((String::new(), Some(confidence), is_partial));
                    }

                    info!(
                        "Whisper transcription complete for chunk {}: '{}' (confidence: {:.2}, partial: {})",
                        chunk.chunk_id, cleaned_text, confidence, is_partial
                    );

                    Ok((cleaned_text, Some(confidence), is_partial))
                }
                Err(e) => {
                    error!(
                        "Whisper transcription failed for chunk {}: {}",
                        chunk.chunk_id, e
                    );

                    let transcription_error = TranscriptionError::EngineFailed(e.to_string());
                    let _ = app.emit(
                        "transcription-error",
                        &serde_json::json!({
                            "error": transcription_error.to_string(),
                            "userMessage": format!("Transcription failed: {}", transcription_error),
                            "actionable": false
                        }),
                    );

                    Err(transcription_error)
                }
            }
        }
        TranscriptionEngine::Parakeet(parakeet_engine) => {
            match parakeet_engine.transcribe_audio(speech_samples).await {
                Ok(text) => {
                    let cleaned_text = text.trim().to_string();
                    if cleaned_text.is_empty() {
                        return Ok((String::new(), None, false));
                    }

                    info!(
                        "Parakeet transcription complete for chunk {}: '{}'",
                        chunk.chunk_id, cleaned_text
                    );

                    // Parakeet doesn't provide confidence or partial results
                    Ok((cleaned_text, None, false))
                }
                Err(e) => {
                    error!(
                        "Parakeet transcription failed for chunk {}: {}",
                        chunk.chunk_id, e
                    );

                    let transcription_error = TranscriptionError::EngineFailed(e.to_string());
                    let _ = app.emit(
                        "transcription-error",
                        &serde_json::json!({
                            "error": transcription_error.to_string(),
                            "userMessage": format!("Transcription failed: {}", transcription_error),
                            "actionable": false
                        }),
                    );

                    Err(transcription_error)
                }
            }
        }
        TranscriptionEngine::Moonshine(moonshine_engine) => {
            match moonshine_engine.transcribe_audio(speech_samples).await {
                Ok(text) => {
                    let cleaned_text = text.trim().to_string();
                    if cleaned_text.is_empty() {
                        return Ok((String::new(), None, false));
                    }

                    info!(
                        "Moonshine transcription complete for chunk {}: '{}'",
                        chunk.chunk_id, cleaned_text
                    );

                    // Moonshine doesn't provide confidence or partial results
                    Ok((cleaned_text, None, false))
                }
                Err(e) => {
                    error!(
                        "Moonshine transcription failed for chunk {}: {}",
                        chunk.chunk_id, e
                    );

                    let transcription_error = TranscriptionError::EngineFailed(e.to_string());
                    let _ = app.emit(
                        "transcription-error",
                        &serde_json::json!({
                            "error": transcription_error.to_string(),
                            "userMessage": format!("Transcription failed: {}", transcription_error),
                            "actionable": false
                        }),
                    );

                    Err(transcription_error)
                }
            }
        }
        TranscriptionEngine::Deepgram { mic, sys } => {
            // Deepgram dual persistent streaming: route audio to correct instance by device_type
            // The reader task handles transcript emission directly
            let dg = match device_type {
                crate::audio::recording_state::DeviceType::Microphone => mic,
                crate::audio::recording_state::DeviceType::System => sys,
                crate::audio::recording_state::DeviceType::Mixed => {
                    warn!("Unexpected Mixed device_type in Deepgram transcription, skipping");
                    return Ok((String::new(), None, false));
                }
            };
            let language = crate::get_language_preference_internal();

            match dg.transcribe(speech_samples, language).await {
                Ok(result) => {
                    // Result is always empty text for streaming mode
                    // Reader task emits transcript-update events directly
                    Ok((result.text, result.confidence, result.is_partial))
                }
                Err(e) => {
                    let err_msg = e.to_string();
                    // Emit WARNING instead of ERROR — individual Deepgram chunk failures are
                    // recoverable (reconnection in transcribe()). Emitting transcription-error
                    // would cause RecordingControls to call onRecordingStop(false) and kill the
                    // entire recording session, which is too aggressive for a single chunk fail.
                    warn!(
                        "Deepgram chunk {} send failed ({:?}): {}",
                        chunk.chunk_id, device_type, err_msg
                    );
                    let _ = app.emit("transcription-warning", &err_msg);
                    Err(e)
                }
            }
        }
        TranscriptionEngine::Provider(provider) => {
            // Trait-based provider (clean, unified interface)
            let language = crate::get_language_preference_internal();
            println!(
                "[WORKER] Using provider: {} (language: {:?}, {} samples)",
                provider.provider_name(),
                language,
                speech_samples.len()
            );

            match provider.transcribe(speech_samples, language).await {
                Ok(result) => {
                    let cleaned_text = result.text.trim().to_string();
                    if cleaned_text.is_empty() {
                        return Ok((String::new(), result.confidence, result.is_partial));
                    }

                    let confidence_str = match result.confidence {
                        Some(c) => format!("confidence: {:.2}", c),
                        None => "no confidence".to_string(),
                    };

                    info!(
                        "{} transcription complete for chunk {}: '{}' ({}, partial: {})",
                        provider.provider_name(),
                        chunk.chunk_id,
                        cleaned_text,
                        confidence_str,
                        result.is_partial
                    );

                    Ok((cleaned_text, result.confidence, result.is_partial))
                }
                Err(e) => {
                    error!(
                        "{} transcription failed for chunk {}: {}",
                        provider.provider_name(),
                        chunk.chunk_id,
                        e
                    );

                    let _ = app.emit(
                        "transcription-error",
                        &serde_json::json!({
                            "error": e.to_string(),
                            "userMessage": format!("Transcription failed: {}", e),
                            "actionable": false
                        }),
                    );

                    Err(e)
                }
            }
        }
    }
}

/// Format current timestamp (wall-clock local time)
fn format_current_timestamp() -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}

/// Format recording-relative time as [MM:SS]
#[allow(dead_code)]
fn format_recording_time(seconds: f64) -> String {
    let total_seconds = seconds.floor() as u64;
    let minutes = total_seconds / 60;
    let secs = total_seconds % 60;

    format!("[{:02}:{:02}]", minutes, secs)
}

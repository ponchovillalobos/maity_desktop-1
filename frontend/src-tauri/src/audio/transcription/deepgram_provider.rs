// audio/transcription/deepgram_provider.rs
//
// Deepgram Realtime transcription provider using persistent WebSocket streaming.
// Maintains a single WebSocket connection for the entire recording session,
// sending audio chunks as binary messages and receiving transcription results
// via a background reader task.

use super::deepgram_commands::get_cached_proxy_config;
use super::provider::{TranscriptionError, TranscriptionProvider, TranscriptResult};
use super::worker::{TranscriptUpdate, SEQUENCE_COUNTER};
use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use futures::stream::SplitSink;
use log::{debug, error, info, warn};
use serde::Deserialize;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{http::Request, Message},
    MaybeTlsStream, WebSocketStream,
};

type WsStream = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

// ============================================================================
// DEEPGRAM API RESPONSE TYPES
// ============================================================================

#[derive(Debug, Deserialize)]
struct DeepgramResponse {
    #[serde(rename = "type")]
    #[allow(dead_code)]
    response_type: Option<String>,
    channel: Option<DeepgramChannel>,
    is_final: Option<bool>,
    #[allow(dead_code)]
    speech_final: Option<bool>,
    /// Offset in seconds from the start of the audio stream
    start: Option<f64>,
    /// Duration of this transcript segment in seconds
    duration: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct DeepgramChannel {
    alternatives: Vec<DeepgramAlternative>,
}

#[derive(Debug, Deserialize)]
struct DeepgramAlternative {
    transcript: String,
    confidence: f32,
}

// ============================================================================
// DEEPGRAM PROVIDER CONFIGURATION
// ============================================================================

#[derive(Debug, Clone)]
pub struct DeepgramConfig {
    /// Base URL of the Cloudflare Worker proxy (e.g., "wss://maity-deepgram-proxy.jagv-1390.workers.dev")
    pub proxy_base_url: Option<String>,
    /// JWT token for authenticating with the proxy (short-lived, ~5 min TTL)
    pub jwt: Option<String>,
    pub model: String,
    pub language: String,
    pub encoding: String,
    pub sample_rate: u32,
    pub channels: u8,
    pub punctuate: bool,
    pub interim_results: bool,
}

impl Default for DeepgramConfig {
    fn default() -> Self {
        Self {
            proxy_base_url: None,
            jwt: None,
            model: "nova-3".to_string(),
            language: "es-419".to_string(), // Latin American Spanish by default
            encoding: "linear16".to_string(),
            sample_rate: 16000,
            channels: 1,
            punctuate: true,
            interim_results: true,
        }
    }
}

// ============================================================================
// DEEPGRAM REALTIME TRANSCRIBER (PERSISTENT STREAMING)
// ============================================================================

/// Maximum number of reconnection attempts before failing
const MAX_RECONNECT_ATTEMPTS: u32 = 3;

/// Delay between reconnection attempts (milliseconds)
const RECONNECT_DELAY_MS: u64 = 1000;

pub struct DeepgramRealtimeTranscriber {
    config: DeepgramConfig,
    is_connected: Arc<Mutex<bool>>,
    // Persistent streaming fields
    persistent_ws: Arc<Mutex<Option<SplitSink<WsStream, Message>>>>,
    reader_handle: Arc<Mutex<Option<JoinHandle<()>>>>,
    /// Event emitter function: called by reader task to emit transcript-update events
    event_emitter: Arc<Mutex<Option<Arc<dyn Fn(TranscriptUpdate) + Send + Sync>>>>,
    /// Accumulated text from interim results for the current utterance
    interim_text: Arc<Mutex<String>>,
    /// Fixed source label for this transcriber instance ("user" or "interlocutor").
    /// Set once at creation; the reader task uses this instead of per-chunk metadata.
    source_label: Arc<Mutex<Option<String>>>,
    /// Generation counter: incremented on each new WebSocket connection.
    /// Reader tasks check this to detect they've been superseded and should exit.
    connection_generation: Arc<AtomicU64>,
    /// Handle for the keep-alive task; aborted before spawning a new one on reconnect
    keepalive_handle: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl DeepgramRealtimeTranscriber {
    /// Create a new Deepgram transcriber using Cloudflare Worker proxy
    pub fn with_proxy(proxy_base_url: String, jwt: String) -> Self {
        let mut config = DeepgramConfig::default();
        config.proxy_base_url = Some(proxy_base_url);
        config.jwt = Some(jwt);
        Self::with_config(config)
    }

    /// Create with full configuration
    pub fn with_config(config: DeepgramConfig) -> Self {
        Self {
            config,
            is_connected: Arc::new(Mutex::new(false)),
            persistent_ws: Arc::new(Mutex::new(None)),
            reader_handle: Arc::new(Mutex::new(None)),
            event_emitter: Arc::new(Mutex::new(None)),
            interim_text: Arc::new(Mutex::new(String::new())),
            source_label: Arc::new(Mutex::new(None)),
            connection_generation: Arc::new(AtomicU64::new(0)),
            keepalive_handle: Arc::new(Mutex::new(None)),
        }
    }

    /// Set the fixed source label for this transcriber instance.
    /// "user" for mic audio, "interlocutor" for system audio.
    pub fn set_source_label(&mut self, label: String) {
        self.source_label = Arc::new(Mutex::new(Some(label)));
    }

    /// Set the transcription model (e.g., "nova-2", "nova-2-general")
    pub fn set_model(&mut self, model: String) {
        self.config.model = model;
    }

    /// Set the language (e.g., "es", "en", "multi")
    pub fn set_language(&mut self, language: String) {
        self.config.language = language;
    }

    /// Update the proxy JWT (for token refresh)
    pub fn set_proxy_jwt(&mut self, jwt: String) {
        self.config.jwt = Some(jwt);
    }

    /// Set the event emitter function for streaming mode.
    /// The reader task calls this function to emit transcript-update events to the frontend.
    pub async fn set_event_emitter<F>(&self, emitter: F)
    where
        F: Fn(TranscriptUpdate) + Send + Sync + 'static,
    {
        let mut guard = self.event_emitter.lock().await;
        *guard = Some(Arc::new(emitter));
    }

    /// Close the persistent WebSocket stream gracefully.
    /// Sends CloseStream message and waits for the reader task to finish.
    pub async fn close_persistent_stream(&self) {
        info!("Closing Deepgram persistent WebSocket stream");

        // Send CloseStream to Deepgram
        {
            let mut ws_guard = self.persistent_ws.lock().await;
            if let Some(ref mut ws) = *ws_guard {
                if let Err(e) = ws.send(Message::Text(r#"{"type": "CloseStream"}"#.to_string())).await {
                    warn!("Failed to send CloseStream to Deepgram: {}", e);
                }
                // Close the WebSocket
                if let Err(e) = ws.close().await {
                    warn!("Failed to close Deepgram WebSocket: {}", e);
                }
            }
            *ws_guard = None;
        }

        // Wait for reader task to finish (with timeout)
        let reader_handle = {
            let mut handle_guard = self.reader_handle.lock().await;
            handle_guard.take()
        };

        if let Some(handle) = reader_handle {
            match tokio::time::timeout(tokio::time::Duration::from_secs(10), handle).await {
                Ok(Ok(())) => info!("Deepgram reader task completed"),
                Ok(Err(e)) => error!("Deepgram reader task panicked: {:?}", e),
                Err(_) => warn!("Deepgram reader task timed out after 10s, aborting"),
            }
        }

        // Abort keep-alive task
        {
            let mut ka_guard = self.keepalive_handle.lock().await;
            if let Some(ka) = ka_guard.take() {
                ka.abort();
            }
        }

        *self.is_connected.lock().await = false;

        self.interim_text.lock().await.clear();

        info!("Deepgram persistent stream closed");
    }

    /// Check if proxy config is ready (both proxy_base_url and jwt are set)
    fn is_proxy_configured(&self) -> bool {
        self.config.proxy_base_url.is_some() && self.config.jwt.is_some()
    }

    /// Build the WebSocket URL targeting the Cloudflare Worker proxy.
    /// The JWT and Deepgram params are passed as query parameters.
    /// Checks the global proxy config cache for a fresher JWT (handles token refresh
    /// when the original JWT has expired after >5 min recording sessions).
    fn build_websocket_url(&self, language_override: Option<&str>) -> Option<String> {
        let proxy_base_url = self.config.proxy_base_url.as_ref()?;

        // Prefer a fresher JWT from the global cache (frontend may have refreshed it)
        let jwt = if let Some((_cached_url, cached_jwt)) = get_cached_proxy_config() {
            cached_jwt
        } else {
            // Fall back to the JWT stored at construction time
            self.config.jwt.as_ref()?.clone()
        };

        let language = language_override.unwrap_or(&self.config.language);

        // STT-003: cuando el usuario pide auto-detect, usar 'multi' (Deepgram nova-3)
        // en lugar de hardcoded 'es'. nova-3 soporta multi-idioma real con auto-switch
        // por segmento. Mantiene compat con valores legacy ('auto-translate', 'auto', 'detect').
        let language_value = match language {
            "auto-translate" | "auto" | "detect" | "multi" => {
                println!("[DEEPGRAM] STT-003: auto-detect requested, using language=multi (nova-3 multilingual)");
                "multi".to_string()
            }
            lang => {
                println!("[DEEPGRAM] Using language: {}", lang);
                lang.to_string()
            }
        };

        Some(format!(
            "{}?token={}&\
            model={}&\
            language={}&\
            encoding={}&\
            sample_rate={}&\
            channels={}&\
            punctuate={}&\
            interim_results={}&\
            endpointing=200&\
            vad_events=true",
            proxy_base_url,
            jwt,
            self.config.model,
            language_value,
            self.config.encoding,
            self.config.sample_rate,
            self.config.channels,
            self.config.punctuate,
            self.config.interim_results
        ))
    }

    /// Convert f32 audio samples to 16-bit PCM bytes
    fn convert_to_pcm16(audio: &[f32]) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(audio.len() * 2);
        for &sample in audio {
            let clamped = sample.clamp(-1.0, 1.0);
            let pcm_sample = (clamped * 32767.0) as i16;
            bytes.extend_from_slice(&pcm_sample.to_le_bytes());
        }
        bytes
    }

    /// Establish the persistent WebSocket connection and spawn the reader task.
    /// Called automatically on the first `transcribe()` call.
    async fn ensure_connected(&self, language: Option<&str>) -> Result<(), TranscriptionError> {
        // Check if already connected
        {
            let ws_guard = self.persistent_ws.lock().await;
            if ws_guard.is_some() {
                return Ok(());
            }
        }

        // Not connected - establish connection with retries
        let mut last_error = None;
        for attempt in 1..=MAX_RECONNECT_ATTEMPTS {
            match self.connect_websocket(language).await {
                Ok(()) => return Ok(()),
                Err(e) => {
                    let safe_msg = e.to_string();
                    if attempt < MAX_RECONNECT_ATTEMPTS {
                        warn!(
                            "Deepgram connection attempt {}/{} failed: {}. Retrying in {}ms...",
                            attempt, MAX_RECONNECT_ATTEMPTS, safe_msg, RECONNECT_DELAY_MS
                        );
                        tokio::time::sleep(tokio::time::Duration::from_millis(RECONNECT_DELAY_MS)).await;
                    } else {
                        error!(
                            "Deepgram connection failed after {} attempts: {}",
                            MAX_RECONNECT_ATTEMPTS, safe_msg
                        );
                    }
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            TranscriptionError::EngineFailed("Unknown connection error".to_string())
        }))
    }

    /// Internal: create WebSocket connection and spawn reader task
    async fn connect_websocket(&self, language: Option<&str>) -> Result<(), TranscriptionError> {
        let label = self.source_label.lock().await.clone()
            .unwrap_or_else(|| "unknown".to_string())
            .to_uppercase();

        let url = self.build_websocket_url(language).ok_or_else(|| {
            TranscriptionError::EngineFailed(
                "Deepgram proxy not configured (no proxy_base_url or jwt)".to_string(),
            )
        })?;
        debug!("Deepgram proxy WebSocket URL: {}", url.split('?').next().unwrap_or(&url));

        // Extract host from proxy URL for the Host header
        let host = url::Url::parse(&url)
            .ok()
            .and_then(|u| u.host_str().map(|h| h.to_string()))
            .unwrap_or_else(|| "localhost".to_string());

        let request = Request::builder()
            .uri(&url)
            .header("Host", &host)
            .header("Upgrade", "websocket")
            .header("Connection", "Upgrade")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", generate_websocket_key())
            .body(())
            .map_err(|e| TranscriptionError::EngineFailed(format!("Failed to build request: {}", e)))?;

        println!("[DEEPGRAM-{}] Connecting persistent WebSocket...", label);
        let (ws_stream, _response) = connect_async(request)
            .await
            .map_err(|e| {
                println!("[DEEPGRAM-{}] WebSocket connection error: {}", label, e);
                TranscriptionError::EngineFailed(format!("WebSocket connection failed: {}", e))
            })?;

        println!("[DEEPGRAM-{}] Persistent WebSocket connected successfully", label);
        info!("[DEEPGRAM-{}] Connected to Deepgram persistent WebSocket", label);

        let (write, read) = ws_stream.split();

        // Store the write half
        *self.persistent_ws.lock().await = Some(write);
        *self.is_connected.lock().await = true;

        // Abort any existing reader task before spawning a new one
        {
            let mut handle_guard = self.reader_handle.lock().await;
            if let Some(old_handle) = handle_guard.take() {
                info!("[DEEPGRAM-{}] Aborting previous reader task", label);
                old_handle.abort();
            }
        }

        // Increment connection generation so stale readers will exit
        let my_generation = self.connection_generation.fetch_add(1, Ordering::SeqCst) + 1;
        info!("[DEEPGRAM-{}] New connection generation: {}", label, my_generation);

        // Spawn the reader task
        let event_emitter = self.event_emitter.clone();
        let is_connected = self.is_connected.clone();
        let interim_text = self.interim_text.clone();
        let source_label = self.source_label.clone();
        let connection_generation = self.connection_generation.clone();

        let reader_handle = tokio::spawn(async move {
            Self::reader_task(read, event_emitter, is_connected, interim_text, source_label, connection_generation, my_generation).await;
        });

        *self.reader_handle.lock().await = Some(reader_handle);

        // Abort any existing keep-alive task before spawning a new one
        {
            let mut ka_guard = self.keepalive_handle.lock().await;
            if let Some(old_ka) = ka_guard.take() {
                info!("[DEEPGRAM-{}] Aborting previous keep-alive task", label);
                old_ka.abort();
            }
        }

        // Spawn keep-alive task (Deepgram closes idle connections after ~10s of inactivity)
        let ws_for_keepalive = self.persistent_ws.clone();
        let is_connected_keepalive = self.is_connected.clone();
        let ka_handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(8));
            loop {
                interval.tick().await;
                let connected = *is_connected_keepalive.lock().await;
                if !connected {
                    break;
                }
                let mut ws_guard = ws_for_keepalive.lock().await;
                if let Some(ref mut ws) = *ws_guard {
                    let msg = Message::Text(r#"{"type":"KeepAlive"}"#.to_string());
                    if ws.send(msg).await.is_err() {
                        break;
                    }
                } else {
                    break;
                }
            }
        });

        *self.keepalive_handle.lock().await = Some(ka_handle);

        Ok(())
    }

    /// Background reader task that receives Deepgram responses and emits transcript events.
    async fn reader_task(
        mut read: futures::stream::SplitStream<WsStream>,
        event_emitter: Arc<Mutex<Option<Arc<dyn Fn(TranscriptUpdate) + Send + Sync>>>>,
        is_connected: Arc<Mutex<bool>>,
        interim_text: Arc<Mutex<String>>,
        source_label: Arc<Mutex<Option<String>>>,
        connection_generation: Arc<AtomicU64>,
        my_generation: u64,
    ) {
        let label = source_label.lock().await.clone()
            .unwrap_or_else(|| "unknown".to_string())
            .to_uppercase();
        info!("[DEEPGRAM-{}] Reader task started (generation {})", label, my_generation);

        while let Some(msg) = read.next().await {
            // Check if this reader has been superseded by a newer connection
            if connection_generation.load(Ordering::SeqCst) != my_generation {
                info!("[DEEPGRAM-{}] Reader task generation {} is stale (current: {}), exiting",
                    label, my_generation, connection_generation.load(Ordering::SeqCst));
                break;
            }
            match msg {
                Ok(Message::Text(text)) => {
                    // Parse the Deepgram response
                    let response: DeepgramResponse = match serde_json::from_str(&text) {
                        Ok(r) => r,
                        Err(e) => {
                            debug!("Failed to parse Deepgram response: {} (text: {})", e, &text[..text.len().min(200)]);
                            continue;
                        }
                    };

                    // Extract transcript from response
                    let channel = match response.channel {
                        Some(ch) => ch,
                        None => continue,
                    };

                    let alt = match channel.alternatives.first() {
                        Some(a) => a,
                        None => continue,
                    };

                    let is_final = response.is_final.unwrap_or(false);
                    // speech_final indicates a natural pause in speech (end of utterance).
                    // Treating it as final reduces latency by ~200-500ms vs waiting for is_final.
                    let should_emit_final = is_final || response.speech_final.unwrap_or(false);

                    if alt.transcript.is_empty() {
                        continue;
                    }

                    // Use the fixed source_label for speaker attribution
                    // (each transcriber instance handles only one audio source)
                    let source_type = source_label.lock().await.clone();

                    if should_emit_final {
                        // Final result - emit transcript-update event
                        let transcript = alt.transcript.clone();
                        let confidence = alt.confidence;

                        // Clear interim text since we got the final version
                        *interim_text.lock().await = String::new();

                        // Use Deepgram's own start/duration from the response (more reliable
                        // than the FIFO chunk_info_queue which can get out of sync)
                        let dg_start = response.start.unwrap_or(0.0);
                        let dg_duration = response.duration.unwrap_or(0.0);
                        let audio_start_time = dg_start;
                        let audio_end_time = dg_start + dg_duration;
                        let duration = dg_duration;

                        let sequence_id = SEQUENCE_COUNTER.fetch_add(1, Ordering::SeqCst);

                        let update = TranscriptUpdate {
                            text: transcript.clone(),
                            timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                            source: "Audio".to_string(),
                            sequence_id,
                            chunk_start_time: audio_start_time,
                            is_partial: false,
                            confidence,
                            audio_start_time,
                            audio_end_time,
                            duration,
                            source_type: source_type.clone(),
                        };

                        println!(
                            "[DEEPGRAM-STREAM] Final transcript ({}): '{}' (seq: {}, confidence: {:.2})",
                            source_type.as_deref().unwrap_or("unknown"),
                            if transcript.len() > 80 { format!("{}...", &transcript[..80]) } else { transcript },
                            sequence_id,
                            confidence
                        );

                        // Emit via the stored emitter
                        let emitter_guard = event_emitter.lock().await;
                        if let Some(ref emitter) = *emitter_guard {
                            emitter(update);
                        } else {
                            warn!("Deepgram reader: no event emitter set, dropping transcript");
                        }
                    } else {
                        // Interim result - update interim text for display
                        *interim_text.lock().await = alt.transcript.clone();

                        // Also emit interim results so the UI can show live transcription
                        // Use Deepgram's response timestamps directly
                        let dg_start = response.start.unwrap_or(0.0);
                        let dg_duration = response.duration.unwrap_or(0.0);
                        let audio_start_time = dg_start;
                        let audio_end_time = dg_start + dg_duration;
                        let duration = dg_duration;

                        let sequence_id = SEQUENCE_COUNTER.fetch_add(1, Ordering::SeqCst);

                        let update = TranscriptUpdate {
                            text: alt.transcript.clone(),
                            timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                            source: "Audio".to_string(),
                            sequence_id,
                            chunk_start_time: audio_start_time,
                            is_partial: true,
                            confidence: alt.confidence,
                            audio_start_time,
                            audio_end_time,
                            duration,
                            source_type: source_type.clone(),
                        };

                        let emitter_guard = event_emitter.lock().await;
                        if let Some(ref emitter) = *emitter_guard {
                            emitter(update);
                        }
                    }
                }
                Ok(Message::Close(_)) => {
                    info!("[DEEPGRAM-{}] WebSocket closed by server", label);
                    break;
                }
                Err(e) => {
                    error!("[DEEPGRAM-{}] WebSocket error: {}", label, e);
                    break;
                }
                _ => {}
            }
        }

        *is_connected.lock().await = false;
        info!("[DEEPGRAM-{}] Reader task finished", label);
    }
}

#[async_trait]
impl TranscriptionProvider for DeepgramRealtimeTranscriber {
    async fn transcribe(
        &self,
        audio: Vec<f32>,
        language: Option<String>,
    ) -> Result<TranscriptResult, TranscriptionError> {
        // Validate audio length
        let minimum_samples = 1600; // 100ms at 16kHz
        if audio.len() < minimum_samples {
            return Err(TranscriptionError::AudioTooShort {
                samples: audio.len(),
                minimum: minimum_samples,
            });
        }

        // Ensure persistent connection is established
        self.ensure_connected(language.as_deref()).await?;

        // Convert audio to PCM16 and send over persistent WebSocket
        let audio_bytes = Self::convert_to_pcm16(&audio);

        // Try sending; if it fails, attempt ONE reconnect before giving up
        let send_result = {
            let mut ws_guard = self.persistent_ws.lock().await;
            match ws_guard.as_mut() {
                Some(ws) => ws.send(Message::Binary(audio_bytes.clone())).await,
                None => Err(tokio_tungstenite::tungstenite::Error::ConnectionClosed),
            }
        };

        if let Err(e) = send_result {
            let label = self.source_label.lock().await.clone().unwrap_or_else(|| "unknown".to_string());
            warn!("[DEEPGRAM-{}] Send failed, attempting reconnect: {}", label.to_uppercase(), e);

            // Abort old reader task to prevent duplicate emissions
            {
                let mut handle_guard = self.reader_handle.lock().await;
                if let Some(old_handle) = handle_guard.take() {
                    old_handle.abort();
                }
            }

            // Clear old connection state
            *self.persistent_ws.lock().await = None;
            *self.is_connected.lock().await = false;
            self.interim_text.lock().await.clear();

            // Reconnect (build_websocket_url will check for a fresher JWT from cache)
            self.ensure_connected(language.as_deref()).await?;

            // Retry send once after reconnect
            let mut ws_guard = self.persistent_ws.lock().await;
            match ws_guard.as_mut() {
                Some(ws) => {
                    ws.send(Message::Binary(audio_bytes)).await.map_err(|e| {
                        error!("[DEEPGRAM-{}] Send failed after reconnect: {}", label.to_uppercase(), e);
                        TranscriptionError::EngineFailed(format!("Reconnect send failed: {}", e))
                    })?;
                }
                None => {
                    return Err(TranscriptionError::EngineFailed(
                        "WebSocket unavailable after reconnect".to_string(),
                    ));
                }
            }
        }

        // Return empty result - the reader task handles response emission directly
        // The worker will see empty text and skip its own event emission
        Ok(TranscriptResult {
            text: String::new(),
            confidence: None,
            is_partial: false,
        })
    }

    async fn is_model_loaded(&self) -> bool {
        // Deepgram via proxy: "ready" if proxy config is set
        self.is_proxy_configured()
    }

    async fn get_current_model(&self) -> Option<String> {
        if self.is_proxy_configured() {
            Some(self.config.model.clone())
        } else {
            None
        }
    }

    fn provider_name(&self) -> &'static str {
        "Deepgram"
    }
}

// ============================================================================
// UTILITY FUNCTIONS
// ============================================================================

/// Generate a random WebSocket key
fn generate_websocket_key() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let key: [u8; 16] = rng.gen();
    base64_encode(&key)
}

/// Simple base64 encoding (avoiding external dependency)
fn base64_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();

    for chunk in data.chunks(3) {
        let mut buffer = [0u8; 4];
        buffer[0] = chunk[0] >> 2;
        buffer[1] = ((chunk[0] & 0x03) << 4) | (chunk.get(1).unwrap_or(&0) >> 4);
        buffer[2] = ((chunk.get(1).unwrap_or(&0) & 0x0f) << 2) | (chunk.get(2).unwrap_or(&0) >> 6);
        buffer[3] = chunk.get(2).unwrap_or(&0) & 0x3f;

        for (i, &idx) in buffer.iter().enumerate() {
            if i < chunk.len() + 1 {
                result.push(ALPHABET[idx as usize] as char);
            } else {
                result.push('=');
            }
        }
    }

    result
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = DeepgramConfig::default();
        assert_eq!(config.model, "nova-3");
        assert_eq!(config.language, "es-419");
        assert_eq!(config.sample_rate, 16000);
        assert!(config.proxy_base_url.is_none());
        assert!(config.jwt.is_none());
    }

    #[test]
    fn test_pcm_conversion() {
        // Test conversion of silence
        let silence = vec![0.0f32; 100];
        let pcm = DeepgramRealtimeTranscriber::convert_to_pcm16(&silence);
        assert_eq!(pcm.len(), 200); // 100 samples * 2 bytes

        // All values should be 0
        for byte in &pcm {
            assert_eq!(*byte, 0);
        }

        // Test conversion of max values
        let max_signal = vec![1.0f32, -1.0f32];
        let pcm = DeepgramRealtimeTranscriber::convert_to_pcm16(&max_signal);
        assert_eq!(pcm.len(), 4);

        // +1.0 should become 32767 (0x7FFF)
        assert_eq!(pcm[0], 0xFF);
        assert_eq!(pcm[1], 0x7F);

        // -1.0 should become -32767 (0x8001)
        let neg_value = i16::from_le_bytes([pcm[2], pcm[3]]);
        assert_eq!(neg_value, -32767);
    }

    #[test]
    fn test_websocket_url_building_with_proxy() {
        let transcriber = DeepgramRealtimeTranscriber::with_proxy(
            "wss://maity-deepgram-proxy.jagv-1390.workers.dev".to_string(),
            "test_jwt_token".to_string(),
        );
        let url = transcriber.build_websocket_url(None).unwrap();

        assert!(url.starts_with("wss://maity-deepgram-proxy.jagv-1390.workers.dev?"));
        assert!(url.contains("token=test_jwt_token"));
        assert!(url.contains("model=nova-3"));
        assert!(url.contains("language=es-419"));
        assert!(url.contains("sample_rate=16000"));
        assert!(url.contains("encoding=linear16"));
    }

    #[test]
    fn test_websocket_url_with_language_override() {
        let transcriber = DeepgramRealtimeTranscriber::with_proxy(
            "wss://proxy.example.com".to_string(),
            "jwt".to_string(),
        );
        let url = transcriber.build_websocket_url(Some("en")).unwrap();

        assert!(url.contains("language=en"));
    }

    #[test]
    fn test_websocket_url_none_without_config() {
        let transcriber = DeepgramRealtimeTranscriber::with_config(DeepgramConfig::default());
        let url = transcriber.build_websocket_url(None);
        assert!(url.is_none());
    }

    #[tokio::test]
    async fn test_model_loaded_without_proxy() {
        let transcriber = DeepgramRealtimeTranscriber::with_config(DeepgramConfig::default());
        assert!(!transcriber.is_model_loaded().await);
    }

    #[tokio::test]
    async fn test_model_loaded_with_proxy() {
        let transcriber = DeepgramRealtimeTranscriber::with_proxy(
            "wss://proxy.example.com".to_string(),
            "jwt".to_string(),
        );
        assert!(transcriber.is_model_loaded().await);
    }

    #[tokio::test]
    async fn test_provider_name() {
        let transcriber = DeepgramRealtimeTranscriber::with_proxy(
            "wss://proxy.example.com".to_string(),
            "jwt".to_string(),
        );
        assert_eq!(transcriber.provider_name(), "Deepgram");
    }

    #[tokio::test]
    async fn test_audio_too_short() {
        let transcriber = DeepgramRealtimeTranscriber::with_proxy(
            "wss://proxy.example.com".to_string(),
            "jwt".to_string(),
        );
        let short_audio = vec![0.0f32; 100]; // Only 100 samples (< 1600 minimum)

        let result = transcriber.transcribe(short_audio, None).await;
        assert!(matches!(result, Err(TranscriptionError::AudioTooShort { .. })));
    }

}

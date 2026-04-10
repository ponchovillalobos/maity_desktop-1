// audio/transcription/engine.rs
//
// TranscriptionEngine enum and model initialization/validation logic.

use super::deepgram_provider::DeepgramRealtimeTranscriber;
use super::provider::TranscriptionProvider;
use log::{error, info, warn};
use std::sync::Arc;
use tauri::{AppHandle, Manager, Runtime};

// ============================================================================
// TRANSCRIPTION ENGINE ENUM
// ============================================================================

// Transcription engine abstraction to support multiple providers
pub enum TranscriptionEngine {
    Whisper(Arc<crate::whisper_engine::WhisperEngine>), // Direct access (backward compat)
    Parakeet(Arc<crate::parakeet_engine::ParakeetEngine>), // Direct access (backward compat)
    Moonshine(Arc<crate::moonshine_engine::MoonshineEngine>), // Moonshine edge-optimized
    Deepgram {
        mic: Arc<DeepgramRealtimeTranscriber>,
        sys: Arc<DeepgramRealtimeTranscriber>,
    }, // Deepgram dual persistent streaming (one per audio source)
    Provider(Arc<dyn TranscriptionProvider>),           // Trait-based (preferred for new code)
}

impl TranscriptionEngine {
    /// Check if the engine has a model loaded
    pub async fn is_model_loaded(&self) -> bool {
        match self {
            Self::Whisper(engine) => engine.is_model_loaded().await,
            Self::Parakeet(engine) => engine.is_model_loaded().await,
            Self::Moonshine(engine) => engine.is_model_loaded().await,
            Self::Deepgram { mic, .. } => mic.is_model_loaded().await,
            Self::Provider(provider) => provider.is_model_loaded().await,
        }
    }

    /// Get the current model name
    pub async fn get_current_model(&self) -> Option<String> {
        match self {
            Self::Whisper(engine) => engine.get_current_model().await,
            Self::Parakeet(engine) => engine.get_current_model().await,
            Self::Moonshine(engine) => engine.get_current_model().await,
            Self::Deepgram { mic, .. } => mic.get_current_model().await,
            Self::Provider(provider) => provider.get_current_model().await,
        }
    }

    /// Get the provider name for logging
    pub fn provider_name(&self) -> &str {
        match self {
            Self::Whisper(_) => "Whisper (direct)",
            Self::Parakeet(_) => "Parakeet (direct)",
            Self::Moonshine(_) => "Moonshine (direct)",
            Self::Deepgram { .. } => "Deepgram (streaming)",
            Self::Provider(provider) => provider.provider_name(),
        }
    }

    /// Check if this engine uses persistent streaming (e.g., Deepgram).
    /// When true, the worker should not emit transcript-update events itself
    /// because the engine's reader task handles emission directly.
    pub fn is_streaming_provider(&self) -> bool {
        matches!(self, Self::Deepgram { .. })
    }

    /// Transcribe audio routed to the correct Deepgram instance by device_type.
    /// Only valid for Deepgram engines; returns error for other engine types.
    pub async fn transcribe_for_device(
        &self,
        device_type: &crate::audio::recording_state::DeviceType,
        audio: Vec<f32>,
        language: Option<String>,
    ) -> Result<super::provider::TranscriptResult, super::provider::TranscriptionError> {
        if let Self::Deepgram { mic, sys } = self {
            let dg = match device_type {
                crate::audio::recording_state::DeviceType::Microphone => mic,
                crate::audio::recording_state::DeviceType::System => sys,
                crate::audio::recording_state::DeviceType::Mixed => {
                    return Err(super::provider::TranscriptionError::EngineFailed(
                        "Mixed device_type should not reach Deepgram transcription".to_string(),
                    ));
                }
            };
            dg.transcribe(audio, language).await
        } else {
            Err(super::provider::TranscriptionError::EngineFailed(
                "transcribe_for_device called on non-Deepgram engine".to_string(),
            ))
        }
    }

    /// Close persistent stream for streaming providers (e.g., Deepgram).
    /// No-op for non-streaming engines.
    pub async fn close_stream(&self) {
        if let Self::Deepgram { mic, sys } = self {
            mic.close_persistent_stream().await;
            sys.close_persistent_stream().await;
        }
    }
}

// ============================================================================
// MODEL VALIDATION AND INITIALIZATION
// ============================================================================

/// Validate that transcription models (Whisper or Parakeet) are ready before starting recording
pub async fn validate_transcription_model_ready<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<(), String> {
    // Check transcript configuration to determine which engine to validate
    let config =
        match crate::api::api_get_transcript_config(app.clone(), app.clone().state(), None).await {
            Ok(Some(config)) => {
                info!(
                    "📝 Found transcript config - provider: {}, model: {}",
                    config.provider, config.model
                );
                config
            }
            Ok(None) => {
                info!("📝 No transcript config found, defaulting to parakeet");
                crate::api::TranscriptConfig {
                    provider: "parakeet".to_string(),
                    model: "parakeet-tdt-0.6b-v3-int8".to_string(),
                    api_key: None,
                    language: Some("es-419".to_string()),
                }
            }
            Err(e) => {
                warn!(
                    "⚠️ Failed to get transcript config: {}, defaulting to parakeet",
                    e
                );
                crate::api::TranscriptConfig {
                    provider: "parakeet".to_string(),
                    model: "parakeet-tdt-0.6b-v3-int8".to_string(),
                    api_key: None,
                    language: Some("es-419".to_string()),
                }
            }
        };

    // Validate based on provider
    match config.provider.as_str() {
        "localWhisper" => {
            info!("🔍 Validating Whisper model...");
            // Ensure whisper engine is initialized first
            if let Err(init_error) = crate::whisper_engine::commands::whisper_init().await {
                warn!("❌ Failed to initialize Whisper engine: {}", init_error);
                return Err(format!(
                    "Failed to initialize speech recognition: {}",
                    init_error
                ));
            }

            // Call the whisper validation command with config support
            match crate::whisper_engine::commands::whisper_validate_model_ready_with_config(app)
                .await
            {
                Ok(model_name) => {
                    info!(
                        "✅ Whisper model validation successful: {} is ready",
                        model_name
                    );
                    Ok(())
                }
                Err(e) => {
                    warn!("❌ Whisper model validation failed: {}", e);
                    Err(e)
                }
            }
        }
        "parakeet" => {
            info!("🔍 Validating Parakeet model...");
            // Ensure parakeet engine is initialized first
            if let Err(init_error) = crate::parakeet_engine::commands::parakeet_init().await {
                warn!("❌ Failed to initialize Parakeet engine: {}", init_error);
                return Err(format!(
                    "Failed to initialize Parakeet speech recognition: {}",
                    init_error
                ));
            }

            // Use the validation command that includes auto-discovery and loading
            // This matches the Whisper behavior for consistency
            match crate::parakeet_engine::commands::parakeet_validate_model_ready_with_config(app)
                .await
            {
                Ok(model_name) => {
                    info!(
                        "✅ Parakeet model validation successful: {} is ready",
                        model_name
                    );
                    Ok(())
                }
                Err(e) => {
                    warn!("❌ Parakeet model validation failed: {}", e);
                    Err(e)
                }
            }
        }
        "moonshine" => {
            info!("🌙 Validating Moonshine model...");
            // Ensure moonshine engine is initialized first
            if let Err(init_error) = crate::moonshine_engine::commands::moonshine_init().await {
                warn!("❌ Failed to initialize Moonshine engine: {}", init_error);
                return Err(format!(
                    "Failed to initialize Moonshine speech recognition: {}",
                    init_error
                ));
            }

            // Use the validation command that includes auto-discovery and loading
            match crate::moonshine_engine::commands::moonshine_validate_model_ready_with_config(app)
                .await
            {
                Ok(model_name) => {
                    info!(
                        "✅ Moonshine model validation successful: {} is ready",
                        model_name
                    );
                    Ok(())
                }
                Err(e) => {
                    warn!("❌ Moonshine model validation failed: {}", e);
                    Err(e)
                }
            }
        }
        "deepgram" => {
            info!("🔍 Validating Deepgram cloud provider...");

            // Check if we have a valid proxy config (obtained from Vercel API)
            if super::deepgram_commands::has_cached_proxy_config() {
                info!("✅ Deepgram proxy config disponible, transcripción en la nube lista");
                Ok(())
            } else {
                // No proxy config available - user needs to be authenticated
                warn!("⚠️ No hay configuración de proxy Deepgram disponible");
                warn!("   El frontend debe obtener la configuración del proxy antes de iniciar la grabación");
                Err(
                    "Configuración de Deepgram no disponible. Por favor asegúrate de estar autenticado con tu cuenta de Google.".to_string()
                )
            }
        }
        other => {
            warn!("❌ Unsupported transcription provider: {}", other);
            Err(format!(
                "El proveedor '{}' no es compatible. Por favor selecciona 'deepgram', 'localWhisper', 'parakeet', o 'moonshine'.",
                other
            ))
        }
    }
}

/// Get or initialize the appropriate transcription engine based on provider configuration
pub async fn get_or_init_transcription_engine<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<TranscriptionEngine, String> {
    // Get provider configuration from API
    let config =
        match crate::api::api_get_transcript_config(app.clone(), app.clone().state(), None).await {
            Ok(Some(config)) => {
                info!(
                    "📝 Transcript config - provider: {}, model: {}",
                    config.provider, config.model
                );
                config
            }
            Ok(None) => {
                info!("📝 No transcript config found, defaulting to parakeet");
                crate::api::TranscriptConfig {
                    provider: "parakeet".to_string(),
                    model: "parakeet-tdt-0.6b-v3-int8".to_string(),
                    api_key: None,
                    language: Some("es-419".to_string()),
                }
            }
            Err(e) => {
                warn!(
                    "⚠️ Failed to get transcript config: {}, defaulting to parakeet",
                    e
                );
                crate::api::TranscriptConfig {
                    provider: "parakeet".to_string(),
                    model: "parakeet-tdt-0.6b-v3-int8".to_string(),
                    api_key: None,
                    language: Some("es-419".to_string()),
                }
            }
        };

    // Initialize the appropriate engine based on provider
    match config.provider.as_str() {
        "parakeet" => {
            info!("🦜 Initializing Parakeet transcription engine");

            // Get Parakeet engine
            let engine = {
                let guard = crate::parakeet_engine::commands::PARAKEET_ENGINE
                    .lock()
                    .map_err(|e| format!("Parakeet engine mutex poisoned: {}", e))?;
                guard.as_ref().cloned()
            };

            match engine {
                Some(engine) => {
                    // Check if model is loaded
                    if engine.is_model_loaded().await {
                        let model_name = engine
                            .get_current_model()
                            .await
                            .unwrap_or_else(|| "unknown".to_string());
                        info!("✅ Parakeet model '{}' already loaded", model_name);
                        Ok(TranscriptionEngine::Parakeet(engine))
                    } else {
                        Err("Parakeet engine initialized but no model loaded. This should not happen after validation.".to_string())
                    }
                }
                None => Err(
                    "Parakeet engine not initialized. This should not happen after validation."
                        .to_string(),
                ),
            }
        }
        "moonshine" => {
            info!("🌙 Initializing Moonshine transcription engine");

            // Get Moonshine engine
            let engine = {
                let guard = crate::moonshine_engine::commands::MOONSHINE_ENGINE
                    .lock()
                    .map_err(|e| format!("Moonshine engine mutex poisoned: {}", e))?;
                guard.as_ref().cloned()
            };

            match engine {
                Some(engine) => {
                    // Check if model is loaded
                    if engine.is_model_loaded().await {
                        let model_name = engine
                            .get_current_model()
                            .await
                            .unwrap_or_else(|| "unknown".to_string());
                        info!("✅ Moonshine model '{}' already loaded", model_name);
                        Ok(TranscriptionEngine::Moonshine(engine))
                    } else {
                        Err("Moonshine engine initialized but no model loaded. This should not happen after validation.".to_string())
                    }
                }
                None => Err(
                    "Moonshine engine not initialized. This should not happen after validation."
                        .to_string(),
                ),
            }
        }
        "deepgram" => {
            info!("Initializing Deepgram cloud transcription engine (dual persistent streaming via proxy)");
            println!("[ENGINE] Initializing Deepgram dual persistent streaming engine via proxy (mic + sys)");

            // Get proxy config from cache (should have been set by frontend before starting recording)
            let proxy_config = super::deepgram_commands::get_cached_proxy_config();

            match proxy_config {
                Some((proxy_base_url, jwt)) => {
                    info!("Deepgram proxy config found");

                    // Apply model from config if specified, otherwise use nova-3
                    let model = if !config.model.is_empty() && config.model != "deepgram" {
                        config.model.clone()
                    } else {
                        "nova-3".to_string()
                    };

                    // Apply language from config, default to es-419 (Latin American Spanish)
                    let language = config
                        .language
                        .clone()
                        .filter(|l| !l.is_empty())
                        .unwrap_or_else(|| "es-419".to_string());

                    info!("Setting Deepgram model={}, language={}", model, language);

                    // Create TWO Deepgram instances: one for mic, one for system audio
                    let mut mic_dg = DeepgramRealtimeTranscriber::with_proxy(
                        proxy_base_url.clone(),
                        jwt.clone(),
                    );
                    mic_dg.set_source_label("user".to_string());
                    mic_dg.set_model(model.clone());
                    mic_dg.set_language(language.clone());

                    let mut sys_dg = DeepgramRealtimeTranscriber::with_proxy(proxy_base_url, jwt);
                    sys_dg.set_source_label("interlocutor".to_string());
                    sys_dg.set_model(model.clone());
                    sys_dg.set_language(language);

                    let mic_arc = Arc::new(mic_dg);
                    let sys_arc = Arc::new(sys_dg);

                    // Set up event emitters for both instances
                    let app_for_mic = app.clone();
                    mic_arc.set_event_emitter(move |update: super::worker::TranscriptUpdate| {
                        use tauri::Emitter;
                        let speech_flag = &super::worker::SPEECH_DETECTED_EMITTED;
                        if !speech_flag.load(std::sync::atomic::Ordering::SeqCst) {
                            speech_flag.store(true, std::sync::atomic::Ordering::SeqCst);
                            let _ = app_for_mic.emit("speech-detected", serde_json::json!({
                                "message": "Speech activity detected"
                            }));
                        }
                        match app_for_mic.emit("transcript-update", &update) {
                            Ok(_) => {
                                println!("[DEEPGRAM-MIC] transcript-update emitted: seq={}, partial={}, source={:?}",
                                    update.sequence_id, update.is_partial, update.source_type);
                            }
                            Err(e) => {
                                log::error!("Failed to emit transcript-update from Deepgram mic reader: {}", e);
                            }
                        }
                    }).await;

                    let app_for_sys = app.clone();
                    sys_arc.set_event_emitter(move |update: super::worker::TranscriptUpdate| {
                        use tauri::Emitter;
                        let speech_flag = &super::worker::SPEECH_DETECTED_EMITTED;
                        if !speech_flag.load(std::sync::atomic::Ordering::SeqCst) {
                            speech_flag.store(true, std::sync::atomic::Ordering::SeqCst);
                            let _ = app_for_sys.emit("speech-detected", serde_json::json!({
                                "message": "Speech activity detected"
                            }));
                        }
                        match app_for_sys.emit("transcript-update", &update) {
                            Ok(_) => {
                                println!("[DEEPGRAM-SYS] transcript-update emitted: seq={}, partial={}, source={:?}",
                                    update.sequence_id, update.is_partial, update.source_type);
                            }
                            Err(e) => {
                                log::error!("Failed to emit transcript-update from Deepgram sys reader: {}", e);
                            }
                        }
                    }).await;

                    info!("Deepgram dual streaming initialized: mic (user) + sys (interlocutor) with model: {}", model);
                    println!(
                        "[ENGINE] Deepgram dual streaming ready with model: {}",
                        model
                    );

                    Ok(TranscriptionEngine::Deepgram {
                        mic: mic_arc,
                        sys: sys_arc,
                    })
                }
                None => {
                    error!("No Deepgram proxy config available");
                    Err(
                        "Configuración de Deepgram no disponible. Por favor asegúrate de estar autenticado con tu cuenta de Google.".to_string()
                    )
                }
            }
        }
        "localWhisper" | _ => {
            info!("🎤 Initializing Whisper transcription engine");
            let whisper_engine = get_or_init_whisper(app).await?;
            Ok(TranscriptionEngine::Whisper(whisper_engine))
        }
    }
}

/// Initialize Parakeet as fallback when cloud provider fails
#[allow(dead_code)] // Reserved for Parakeet fallback functionality
async fn init_parakeet_fallback() -> Result<TranscriptionEngine, String> {
    info!("🦜 Falling back to Parakeet transcription engine");

    let engine = {
        let guard = crate::parakeet_engine::commands::PARAKEET_ENGINE
            .lock()
            .map_err(|e| format!("Parakeet engine mutex poisoned: {}", e))?;
        guard.as_ref().cloned()
    };

    match engine {
        Some(engine) => {
            if engine.is_model_loaded().await {
                let model_name = engine.get_current_model().await
                    .unwrap_or_else(|| "unknown".to_string());
                info!("✅ Parakeet fallback model '{}' loaded", model_name);
                Ok(TranscriptionEngine::Parakeet(engine))
            } else {
                Err("Parakeet engine initialized but no model loaded for fallback.".to_string())
            }
        }
        None => {
            Err("Parakeet engine not available for fallback. Please ensure a local transcription model is downloaded.".to_string())
        }
    }
}

/// Get or initialize transcription engine using API configuration
/// Returns Whisper engine if provider is localWhisper, otherwise returns error for non-Whisper providers
pub async fn get_or_init_whisper<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<Arc<crate::whisper_engine::WhisperEngine>, String> {
    // Check if engine already exists and has a model loaded
    let existing_engine = {
        let engine_guard = crate::whisper_engine::commands::WHISPER_ENGINE
            .lock()
            .map_err(|e| format!("Whisper engine mutex poisoned: {}", e))?;
        engine_guard.as_ref().cloned()
    };

    if let Some(engine) = existing_engine {
        // Check if a model is already loaded
        if engine.is_model_loaded().await {
            let current_model = engine
                .get_current_model()
                .await
                .unwrap_or_else(|| "unknown".to_string());

            // NEW: Check if loaded model matches saved config
            let configured_model =
                match crate::api::api_get_transcript_config(app.clone(), app.clone().state(), None)
                    .await
                {
                    Ok(Some(config)) => {
                        info!(
                            "📝 Saved transcript config - provider: {}, model: {}",
                            config.provider, config.model
                        );
                        if config.provider == "localWhisper" && !config.model.is_empty() {
                            Some(config.model)
                        } else {
                            None
                        }
                    }
                    Ok(None) => {
                        info!("📝 No transcript config found in database");
                        None
                    }
                    Err(e) => {
                        warn!("⚠️ Failed to get transcript config: {}", e);
                        None
                    }
                };

            // If loaded model matches config, reuse it
            if let Some(ref expected_model) = configured_model {
                if current_model == *expected_model {
                    info!(
                        "✅ Loaded model '{}' matches saved config, reusing",
                        current_model
                    );
                    return Ok(engine);
                } else {
                    info!(
                        "🔄 Loaded model '{}' doesn't match saved config '{}', reloading correct model...",
                        current_model, expected_model
                    );
                    // Unload the incorrect model
                    engine.unload_model().await;
                    info!("📉 Unloaded incorrect model '{}'", current_model);
                    // Continue to model loading logic below
                }
            } else {
                // No specific config saved, accept currently loaded model
                info!(
                    "✅ No specific model configured, using currently loaded model: '{}'",
                    current_model
                );
                return Ok(engine);
            }
        } else {
            info!("🔄 Whisper engine exists but no model loaded, will load model from config");
        }
    }

    // Initialize new engine if needed
    info!("Initializing Whisper engine");

    // First ensure the engine is initialized
    if let Err(e) = crate::whisper_engine::commands::whisper_init().await {
        return Err(format!("Failed to initialize Whisper engine: {}", e));
    }

    // Get the engine reference
    let engine = {
        let engine_guard = crate::whisper_engine::commands::WHISPER_ENGINE
            .lock()
            .map_err(|e| format!("Whisper engine mutex poisoned: {}", e))?;
        engine_guard
            .as_ref()
            .cloned()
            .ok_or("Failed to get initialized engine")?
    };

    // Get model configuration from API
    let model_to_load = match crate::api::api_get_transcript_config(
        app.clone(),
        app.clone().state(),
        None,
    )
    .await
    {
        Ok(Some(config)) => {
            info!(
                "Got transcript config from API - provider: {}, model: {}",
                config.provider, config.model
            );
            if config.provider == "localWhisper" {
                info!("Using model from API config: {}", config.model);
                config.model
            } else {
                // Non-Whisper provider (e.g., parakeet) - this function shouldn't be called
                return Err(format!(
                        "Cannot initialize Whisper engine: Config uses '{}' provider. This is a bug in the transcription task initialization.",
                        config.provider
                    ));
            }
        }
        Ok(None) => {
            info!("No transcript config found in API, falling back to 'small'");
            "small".to_string()
        }
        Err(e) => {
            warn!(
                "Failed to get transcript config from API: {}, falling back to 'small'",
                e
            );
            "small".to_string()
        }
    };

    info!("Selected model to load: {}", model_to_load);

    // Discover available models to check if the desired model is downloaded
    let models = engine
        .discover_models()
        .await
        .map_err(|e| format!("Failed to discover models: {}", e))?;

    info!("Discovered {} models", models.len());
    for model in &models {
        info!(
            "Model: {} - Status: {:?} - Path: {}",
            model.name,
            model.status,
            model.path.display()
        );
    }

    // Check if the desired model is available
    let model_info = models.iter().find(|model| model.name == model_to_load);

    if model_info.is_none() {
        info!(
            "Model '{}' not found in discovered models. Available models: {:?}",
            model_to_load,
            models.iter().map(|m| &m.name).collect::<Vec<_>>()
        );
    }

    match model_info {
        Some(model) => {
            match model.status {
                crate::whisper_engine::ModelStatus::Available => {
                    info!("Loading model: {}", model_to_load);
                    engine
                        .load_model(&model_to_load)
                        .await
                        .map_err(|e| format!("Failed to load model '{}': {}", model_to_load, e))?;
                    info!("✅ Model '{}' loaded successfully", model_to_load);
                }
                crate::whisper_engine::ModelStatus::Missing => {
                    return Err(format!(
                        "Model '{}' is not downloaded. Please download it first from the settings.",
                        model_to_load
                    ));
                }
                crate::whisper_engine::ModelStatus::Downloading { progress } => {
                    return Err(format!("Model '{}' is currently downloading ({}%). Please wait for it to complete.", model_to_load, progress));
                }
                crate::whisper_engine::ModelStatus::Error(ref err) => {
                    return Err(format!("Model '{}' has an error: {}. Please check the model or try downloading it again.", model_to_load, err));
                }
                crate::whisper_engine::ModelStatus::Corrupted { .. } => {
                    return Err(format!("Model '{}' is corrupted. Please delete it and download again from the settings.", model_to_load));
                }
            }
        }
        None => {
            // Check if we have any available models and try to load the first one
            let available_models: Vec<_> = models
                .iter()
                .filter(|m| matches!(m.status, crate::whisper_engine::ModelStatus::Available))
                .collect();

            if let Some(fallback_model) = available_models.first() {
                warn!(
                    "Model '{}' not found, falling back to available model: '{}'",
                    model_to_load, fallback_model.name
                );
                engine.load_model(&fallback_model.name).await.map_err(|e| {
                    format!(
                        "Failed to load fallback model '{}': {}",
                        fallback_model.name, e
                    )
                })?;
                info!(
                    "✅ Fallback model '{}' loaded successfully",
                    fallback_model.name
                );
            } else {
                return Err(format!("Model '{}' is not supported and no other models are available. Please download a model from the settings.", model_to_load));
            }
        }
    }

    Ok(engine)
}

// audio/transcription/mod.rs
//
// Transcription module: Provider abstraction, engine management, and worker pool.

pub mod deepgram_commands; // Tauri commands for Deepgram cloud proxy tokens
pub mod deepgram_provider; // Deepgram cloud transcription
pub mod engine;
pub mod parakeet_provider;
pub mod provider;
pub mod whisper_provider;
pub mod worker;

// Re-export commonly used types
pub use deepgram_commands::{
    clear_deepgram_proxy_config, get_cached_proxy_config, get_deepgram_proxy_config,
    has_cached_proxy_config, has_valid_deepgram_proxy_config, set_deepgram_proxy_config,
};
pub use deepgram_provider::{DeepgramConfig, DeepgramRealtimeTranscriber};
pub use engine::{
    get_or_init_transcription_engine, get_or_init_whisper, validate_transcription_model_ready,
    TranscriptionEngine,
};
pub use parakeet_provider::ParakeetProvider;
pub use provider::{TranscriptResult, TranscriptionError, TranscriptionProvider};
pub use whisper_provider::WhisperProvider;
pub use worker::{reset_speech_detected_flag, start_transcription_task, TranscriptUpdate};

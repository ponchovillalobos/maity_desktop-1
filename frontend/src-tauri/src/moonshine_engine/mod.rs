//! Moonshine (UsefulSensors) speech recognition engine module.
//!
//! This module provides a high-performance alternative to Whisper for speech-to-text transcription.
//! Moonshine is optimized for edge devices and offers 5-15x faster processing than Whisper.
//!
//! # Features
//!
//! - **Edge-Optimized**: Designed for edge devices with efficient inference
//! - **ONNX Runtime**: Cross-platform support via ONNX
//! - **Spanish Support**: moonshine-base-es model for Spanish transcription
//! - **Unified API**: Compatible interface with Whisper and Parakeet engines
//!
//! # Module Structure
//!
//! - `moonshine_engine`: Main engine implementation
//! - `model`: ONNX model wrapper and inference logic
//! - `commands`: Tauri command interface for frontend integration

pub mod commands;
pub mod model;
pub mod moonshine_engine;

pub use commands::*;
pub use model::{MoonshineError, MoonshineModel};
pub use moonshine_engine::{
    DownloadProgress, ModelInfo, ModelStatus, MoonshineEngine, MoonshineEngineError,
};

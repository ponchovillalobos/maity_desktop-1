use crate::moonshine_engine::model::MoonshineModel;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::fs;
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::sync::RwLock;
use tokio::time::timeout;

/// Model status for Moonshine models
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ModelStatus {
    Available,
    Missing,
    Downloading {
        progress: u8,
    },
    Error(String),
    Corrupted {
        file_size: u64,
        expected_min_size: u64,
    },
}

/// Detailed download progress info (MB-based with speed)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadProgress {
    /// Bytes downloaded so far
    pub downloaded_bytes: u64,
    /// Total file size in bytes
    pub total_bytes: u64,
    /// Downloaded in MB (for display)
    pub downloaded_mb: f64,
    /// Total size in MB (for display)
    pub total_mb: f64,
    /// Download speed in MB/s
    pub speed_mbps: f64,
    /// Percentage complete (0-100)
    pub percent: u8,
}

impl DownloadProgress {
    pub fn new(downloaded: u64, total: u64, speed_mbps: f64) -> Self {
        let percent = if total > 0 {
            ((downloaded as f64 / total as f64) * 100.0).min(100.0) as u8
        } else {
            0
        };
        Self {
            downloaded_bytes: downloaded,
            total_bytes: total,
            downloaded_mb: downloaded as f64 / (1024.0 * 1024.0),
            total_mb: total as f64 / (1024.0 * 1024.0),
            speed_mbps,
            percent,
        }
    }
}

/// Information about a Moonshine model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub name: String,
    pub path: PathBuf,
    pub size_mb: u32,
    pub speed: String, // Performance description
    pub status: ModelStatus,
    pub description: String,
    pub language: String, // Supported language (e.g., "es" for Spanish)
}

#[derive(Debug)]
pub enum MoonshineEngineError {
    ModelNotLoaded,
    ModelNotFound(String),
    TranscriptionFailed(String),
    DownloadFailed(String),
    IoError(std::io::Error),
    Other(String),
}

impl std::fmt::Display for MoonshineEngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MoonshineEngineError::ModelNotLoaded => write!(f, "No Moonshine model loaded"),
            MoonshineEngineError::ModelNotFound(name) => write!(f, "Model '{}' not found", name),
            MoonshineEngineError::TranscriptionFailed(err) => {
                write!(f, "Transcription failed: {}", err)
            }
            MoonshineEngineError::DownloadFailed(err) => write!(f, "Download failed: {}", err),
            MoonshineEngineError::IoError(err) => write!(f, "IO error: {}", err),
            MoonshineEngineError::Other(err) => write!(f, "Error: {}", err),
        }
    }
}

impl std::error::Error for MoonshineEngineError {}

impl From<std::io::Error> for MoonshineEngineError {
    fn from(err: std::io::Error) -> Self {
        MoonshineEngineError::IoError(err)
    }
}

pub struct MoonshineEngine {
    models_dir: PathBuf,
    current_model: Arc<RwLock<Option<MoonshineModel>>>,
    current_model_name: Arc<RwLock<Option<String>>>,
    pub(crate) available_models: Arc<RwLock<HashMap<String, ModelInfo>>>,
    cancel_download_flag: Arc<RwLock<Option<String>>>, // Model name being cancelled
    // Active downloads tracking to prevent concurrent downloads
    pub(crate) active_downloads: Arc<RwLock<HashSet<String>>>, // Set of models currently being downloaded
}

impl MoonshineEngine {
    /// Create a new Moonshine engine with optional custom models directory
    pub fn new_with_models_dir(models_dir: Option<PathBuf>) -> Result<Self> {
        let models_dir = if let Some(dir) = models_dir {
            dir.join("moonshine") // Moonshine models in subdirectory
        } else {
            // Fallback to default location
            let current_dir = std::env::current_dir()
                .map_err(|e| anyhow!("Failed to get current directory: {}", e))?;

            if cfg!(debug_assertions) {
                // Development mode
                current_dir.join("models").join("moonshine")
            } else {
                // Production mode
                dirs::data_dir()
                    .or_else(|| dirs::home_dir())
                    .ok_or_else(|| anyhow!("Could not find system data directory"))?
                    .join("Maity")
                    .join("models")
                    .join("moonshine")
            }
        };

        log::info!(
            "MoonshineEngine using models directory: {}",
            models_dir.display()
        );

        // Create directory if it doesn't exist
        if !models_dir.exists() {
            std::fs::create_dir_all(&models_dir)?;
        }

        Ok(Self {
            models_dir,
            current_model: Arc::new(RwLock::new(None)),
            current_model_name: Arc::new(RwLock::new(None)),
            available_models: Arc::new(RwLock::new(HashMap::new())),
            cancel_download_flag: Arc::new(RwLock::new(None)),
            active_downloads: Arc::new(RwLock::new(HashSet::new())),
        })
    }

    /// Discover available Moonshine models
    pub async fn discover_models(&self) -> Result<Vec<ModelInfo>> {
        let models_dir = &self.models_dir;
        let mut models = Vec::new();

        // Moonshine model configurations
        // moonshine-base is an English-only model for ultra-fast transcription
        let model_configs = [(
            "moonshine-base",
            250,
            "Ultra Fast",
            "en",
            "Ultra-fast English model for real-time transcription",
        )];

        // Get active downloads to override status
        let active_downloads = self.active_downloads.read().await;

        for (name, size_mb, speed, language, description) in model_configs {
            let model_path = models_dir.join(name);

            // Check if model is currently downloading
            let status = if active_downloads.contains(name) {
                // If downloading, preserve that status regardless of file system
                ModelStatus::Downloading { progress: 0 }
            } else if model_path.exists() {
                // Check for required ONNX files
                // Using separate decoder models instead of merged to avoid MatMul errors
                let required_files = vec![
                    "encoder_model.onnx",
                    "decoder_model.onnx",
                    "decoder_with_past_model.onnx",
                    "tokenizer.json",
                ];

                let all_files_exist = required_files
                    .iter()
                    .all(|file| model_path.join(file).exists());

                if all_files_exist {
                    // Validate model by checking file sizes
                    match self.validate_model_directory(&model_path).await {
                        Ok(_) => ModelStatus::Available,
                        Err(_) => {
                            log::warn!("Model directory {} appears corrupted", name);
                            // Calculate total size of existing files
                            let mut total_size = 0u64;
                            for file in required_files {
                                if let Ok(metadata) = std::fs::metadata(model_path.join(file)) {
                                    total_size += metadata.len();
                                }
                            }
                            ModelStatus::Corrupted {
                                file_size: total_size,
                                expected_min_size: (size_mb as u64) * 1024 * 1024,
                            }
                        }
                    }
                } else {
                    ModelStatus::Missing
                }
            } else {
                ModelStatus::Missing
            };

            let model_info = ModelInfo {
                name: name.to_string(),
                path: model_path,
                size_mb: size_mb as u32,
                speed: speed.to_string(),
                status,
                description: description.to_string(),
                language: language.to_string(),
            };

            models.push(model_info);
        }

        // Update internal cache
        let mut available_models = self.available_models.write().await;
        available_models.clear();
        for model in &models {
            available_models.insert(model.name.clone(), model.clone());
        }

        Ok(models)
    }

    /// Validate model directory by checking if all required files exist AND have valid sizes
    async fn validate_model_directory(&self, model_dir: &PathBuf) -> Result<()> {
        // Check if tokenizer.json exists and is readable
        let tokenizer_path = model_dir.join("tokenizer.json");
        if !tokenizer_path.exists() {
            return Err(anyhow!("tokenizer.json not found"));
        }

        // Check encoder and decoder models
        if !model_dir.join("encoder_model.onnx").exists() {
            return Err(anyhow!("encoder_model.onnx not found"));
        }
        if !model_dir.join("decoder_model.onnx").exists() {
            return Err(anyhow!("decoder_model.onnx not found"));
        }
        if !model_dir.join("decoder_with_past_model.onnx").exists() {
            return Err(anyhow!("decoder_with_past_model.onnx not found"));
        }

        // Define minimum file sizes (80% of expected to allow some variance)
        // moonshine-base ONNX files: encoder ~81MB, decoder ~158MB, decoder_with_past ~147MB, tokenizer ~4KB
        let expected_sizes: Vec<(&str, u64)> = vec![
            ("encoder_model.onnx", 65_000_000),            // ~81 MB, min 65 MB
            ("decoder_model.onnx", 130_000_000),           // ~158 MB, min 130 MB
            ("decoder_with_past_model.onnx", 120_000_000), // ~147 MB, min 120 MB
            ("tokenizer.json", 1_000),                     // ~4 KB, min 1 KB
        ];

        // Validate each file exists AND has sufficient size
        for (filename, min_size) in expected_sizes {
            let file_path = model_dir.join(filename);
            if !file_path.exists() {
                return Err(anyhow!("{} not found", filename));
            }

            match std::fs::metadata(&file_path) {
                Ok(metadata) => {
                    let actual_size = metadata.len();
                    if actual_size < min_size {
                        return Err(anyhow!(
                            "{} is incomplete: {} bytes (expected at least {} bytes)",
                            filename,
                            actual_size,
                            min_size
                        ));
                    }
                }
                Err(e) => {
                    return Err(anyhow!("Failed to read {} metadata: {}", filename, e));
                }
            }
        }

        Ok(())
    }

    /// Clean incomplete model directory before download
    async fn clean_incomplete_model_directory(&self, model_dir: &PathBuf) -> Result<()> {
        if !model_dir.exists() {
            return Ok(()); // Nothing to clean
        }

        // Validate the directory
        match self.validate_model_directory(model_dir).await {
            Ok(_) => {
                log::info!("Model directory is valid, no cleanup needed");
                return Ok(());
            }
            Err(validation_error) => {
                log::warn!(
                    "Model directory exists but is invalid: {}. Cleaning up...",
                    validation_error
                );

                // List and remove all files in the directory
                let mut entries = fs::read_dir(model_dir)
                    .await
                    .map_err(|e| anyhow!("Failed to read model directory: {}", e))?;

                let mut removed_count = 0;
                while let Some(entry) = entries
                    .next_entry()
                    .await
                    .map_err(|e| anyhow!("Failed to read directory entry: {}", e))?
                {
                    let path = entry.path();
                    if path.is_file() {
                        match fs::remove_file(&path).await {
                            Ok(_) => {
                                log::info!("Removed incomplete file: {:?}", path.file_name());
                                removed_count += 1;
                            }
                            Err(e) => {
                                log::warn!("Failed to remove file {:?}: {}", path, e);
                            }
                        }
                    }
                }

                log::info!(
                    "Cleaned {} incomplete files from model directory",
                    removed_count
                );
                Ok(())
            }
        }
    }

    /// Load a Moonshine model
    pub async fn load_model(&self, model_name: &str) -> Result<()> {
        let models = self.available_models.read().await;
        let model_info = models
            .get(model_name)
            .ok_or_else(|| anyhow!("Model {} not found", model_name))?;

        match model_info.status {
            ModelStatus::Available => {
                // Check if this model is already loaded
                if let Some(current_model) = self.current_model_name.read().await.as_ref() {
                    if current_model == model_name {
                        log::info!(
                            "Moonshine model {} is already loaded, skipping reload",
                            model_name
                        );
                        return Ok(());
                    }

                    // Unload current model before loading new one
                    log::info!(
                        "Unloading current Moonshine model '{}' before loading '{}'",
                        current_model,
                        model_name
                    );
                    self.unload_model().await;
                }

                log::info!("Loading Moonshine model: {}", model_name);

                // Load model
                let model = MoonshineModel::new(&model_info.path)
                    .map_err(|e| anyhow!("Failed to load Moonshine model {}: {}", model_name, e))?;

                // Update current model and model name
                *self.current_model.write().await = Some(model);
                *self.current_model_name.write().await = Some(model_name.to_string());

                log::info!("Successfully loaded Moonshine model: {}", model_name);
                Ok(())
            }
            ModelStatus::Missing => {
                Err(anyhow!("Moonshine model {} is not downloaded", model_name))
            }
            ModelStatus::Downloading { .. } => Err(anyhow!(
                "Moonshine model {} is currently downloading",
                model_name
            )),
            ModelStatus::Error(ref err) => {
                Err(anyhow!("Moonshine model {} has error: {}", model_name, err))
            }
            ModelStatus::Corrupted { .. } => Err(anyhow!(
                "Moonshine model {} is corrupted and cannot be loaded",
                model_name
            )),
        }
    }

    /// Unload the current model
    pub async fn unload_model(&self) -> bool {
        let mut model_guard = self.current_model.write().await;
        let unloaded = model_guard.take().is_some();
        if unloaded {
            log::info!("Moonshine model unloaded");
        }

        let mut model_name_guard = self.current_model_name.write().await;
        model_name_guard.take();

        unloaded
    }

    /// Get the currently loaded model name
    pub async fn get_current_model(&self) -> Option<String> {
        self.current_model_name.read().await.clone()
    }

    /// Check if a model is loaded
    pub async fn is_model_loaded(&self) -> bool {
        self.current_model.read().await.is_some()
    }

    /// Transcribe audio samples using the loaded Moonshine model
    pub async fn transcribe_audio(&self, audio_data: Vec<f32>) -> Result<String> {
        let mut model_guard = self.current_model.write().await;
        let model = model_guard
            .as_mut()
            .ok_or_else(|| anyhow!("No Moonshine model loaded. Please load a model first."))?;

        let duration_seconds = audio_data.len() as f64 / 16000.0; // Assuming 16kHz
        log::debug!(
            "Moonshine transcribing {} samples ({:.1}s duration)",
            audio_data.len(),
            duration_seconds
        );

        // Transcribe using Moonshine model
        let result = model
            .transcribe(audio_data)
            .map_err(|e| anyhow!("Moonshine transcription failed: {}", e))?;

        log::debug!("Moonshine transcription result: '{}'", result);

        Ok(result)
    }

    /// Get the models directory path
    pub async fn get_models_directory(&self) -> PathBuf {
        self.models_dir.clone()
    }

    /// Delete a corrupted model
    pub async fn delete_model(&self, model_name: &str) -> Result<String> {
        log::info!("Attempting to delete Moonshine model: {}", model_name);

        // Get model info to find the directory path
        let model_info = {
            let models = self.available_models.read().await;
            models.get(model_name).cloned()
        };

        let model_info =
            model_info.ok_or_else(|| anyhow!("Moonshine model '{}' not found", model_name))?;

        log::info!(
            "Moonshine model '{}' has status: {:?}",
            model_name,
            model_info.status
        );

        // Allow deletion of corrupted or available models
        match &model_info.status {
            ModelStatus::Corrupted { .. } | ModelStatus::Available => {
                // Delete the entire model directory
                if model_info.path.exists() {
                    fs::remove_dir_all(&model_info.path).await
                        .map_err(|e| anyhow!("Failed to delete directory '{}': {}", model_info.path.display(), e))?;
                    log::info!("Successfully deleted Moonshine model directory: {}", model_info.path.display());
                } else {
                    log::warn!("Directory '{}' does not exist, nothing to delete", model_info.path.display());
                }

                // Update model status to Missing
                {
                    let mut models = self.available_models.write().await;
                    if let Some(model) = models.get_mut(model_name) {
                        model.status = ModelStatus::Missing;
                    }
                }

                Ok(format!("Successfully deleted Moonshine model '{}'", model_name))
            }
            _ => {
                Err(anyhow!(
                    "Can only delete corrupted or available Moonshine models. Model '{}' has status: {:?}",
                    model_name,
                    model_info.status
                ))
            }
        }
    }

    /// Download a Moonshine model from HuggingFace (backward-compatible wrapper)
    pub async fn download_model(
        &self,
        model_name: &str,
        progress_callback: Option<Box<dyn Fn(u8) + Send>>,
    ) -> Result<()> {
        // Wrap simple callback to use detailed version
        let detailed_callback: Option<Box<dyn Fn(DownloadProgress) + Send>> = progress_callback
            .map(|cb| {
                Box::new(move |p: DownloadProgress| cb(p.percent))
                    as Box<dyn Fn(DownloadProgress) + Send>
            });
        self.download_model_detailed(model_name, detailed_callback)
            .await
    }

    /// Download a Moonshine model with detailed progress (MB/speed/resume support)
    pub async fn download_model_detailed(
        &self,
        model_name: &str,
        progress_callback: Option<Box<dyn Fn(DownloadProgress) + Send>>,
    ) -> Result<()> {
        log::info!("Starting download for Moonshine model: {}", model_name);

        // Check if download is already in progress for this model
        {
            let active = self.active_downloads.read().await;
            if active.contains(model_name) {
                log::warn!(
                    "Download already in progress for Moonshine model: {}",
                    model_name
                );
                return Err(anyhow!(
                    "Download already in progress for model: {}",
                    model_name
                ));
            }
        }

        // Add to active downloads
        {
            let mut active = self.active_downloads.write().await;
            active.insert(model_name.to_string());
        }

        // Clear any previous cancellation flag for this model
        {
            let mut cancel_flag = self.cancel_download_flag.write().await;
            *cancel_flag = None;
        }

        // Get model info
        let model_info = {
            let models = self.available_models.read().await;
            match models.get(model_name).cloned() {
                Some(info) => info,
                None => {
                    // Remove from active downloads on error
                    let mut active = self.active_downloads.write().await;
                    active.remove(model_name);
                    return Err(anyhow!("Model {} not found", model_name));
                }
            }
        };

        // Update model status to downloading
        {
            let mut models = self.available_models.write().await;
            if let Some(model) = models.get_mut(model_name) {
                model.status = ModelStatus::Downloading { progress: 0 };
            }
        }

        // HuggingFace base URL for Moonshine models
        // moonshine-base ONNX is at: https://huggingface.co/onnx-community/moonshine-base-ONNX
        let base_url =
            "https://huggingface.co/onnx-community/moonshine-base-ONNX/resolve/main/onnx";

        // Files to download
        // Using separate decoder models instead of merged to avoid MatMul errors
        let files_to_download = vec![
            "encoder_model.onnx",
            "decoder_model.onnx",
            "decoder_with_past_model.onnx",
        ];
        // tokenizer.json is at a different path
        let tokenizer_url =
            "https://huggingface.co/onnx-community/moonshine-base-ONNX/resolve/main/tokenizer.json";

        // Create model directory
        let model_dir = &model_info.path;
        if !model_dir.exists() {
            if let Err(e) = fs::create_dir_all(model_dir).await {
                // Remove from active downloads on error
                let mut active = self.active_downloads.write().await;
                active.remove(model_name);
                return Err(anyhow!("Failed to create model directory: {}", e));
            }
        }

        // Clean up incomplete downloads before starting
        log::info!("Checking for incomplete model files to clean up...");
        if let Err(e) = self.clean_incomplete_model_directory(model_dir).await {
            log::warn!("Failed to clean incomplete model directory: {}", e);
        }

        // Optimized HTTP client for large file downloads
        let client = reqwest::Client::builder()
            .tcp_nodelay(true)
            .pool_max_idle_per_host(1)
            .timeout(Duration::from_secs(3600))
            .connect_timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| anyhow!("Failed to create HTTP client: {}", e))?;

        // Approximate file sizes for progress calculation
        // moonshine-base ONNX files: encoder ~81MB, decoder ~158MB, decoder_with_past ~147MB, tokenizer ~4KB
        let file_sizes: std::collections::HashMap<&str, u64> = [
            ("encoder_model.onnx", 81_000_000u64),
            ("decoder_model.onnx", 158_000_000u64),
            ("decoder_with_past_model.onnx", 147_000_000u64),
            ("tokenizer.json", 4_000u64),
        ]
        .iter()
        .cloned()
        .collect();

        let total_size_bytes: u64 = file_sizes.values().sum();
        let mut total_downloaded: u64 = 0;

        // Timing for speed calculation
        let download_start_time = Instant::now();
        let mut last_report_time = Instant::now();
        let mut bytes_since_last_report: u64 = 0;
        let mut last_reported_progress: u8 = 0;

        // Download ONNX files
        let all_files: Vec<(&str, String)> = files_to_download
            .iter()
            .map(|f| (*f, format!("{}/{}", base_url, f)))
            .chain(std::iter::once((
                "tokenizer.json",
                tokenizer_url.to_string(),
            )))
            .collect();

        for (filename, file_url) in &all_files {
            let file_path = model_dir.join(filename);

            // Check for cancellation
            {
                let cancel_flag = self.cancel_download_flag.read().await;
                if cancel_flag.as_ref() == Some(&model_name.to_string()) {
                    log::info!("Download cancelled for {}", model_name);
                    let mut active = self.active_downloads.write().await;
                    active.remove(model_name);
                    return Err(anyhow!("Download cancelled by user"));
                }
            }

            log::info!("Downloading: {}", filename);

            let response = client
                .get(file_url)
                .send()
                .await
                .map_err(|e| anyhow!("Failed to start download for {}: {}", filename, e))?;

            if !response.status().is_success() {
                let mut active = self.active_downloads.write().await;
                active.remove(model_name);
                return Err(anyhow!(
                    "Download failed for {} with status: {}",
                    filename,
                    response.status()
                ));
            }

            let file_total_size = response
                .content_length()
                .unwrap_or(*file_sizes.get(*filename).unwrap_or(&0));

            let file = fs::File::create(&file_path)
                .await
                .map_err(|e| anyhow!("Failed to create file {}: {}", filename, e))?;

            let mut writer = BufWriter::with_capacity(8 * 1024 * 1024, file);

            use futures_util::StreamExt;
            let mut stream = response.bytes_stream();
            let mut file_downloaded = 0u64;

            loop {
                // Check for cancellation
                {
                    let cancel_flag = self.cancel_download_flag.read().await;
                    if cancel_flag.as_ref() == Some(&model_name.to_string()) {
                        log::info!("Download cancelled for {}", model_name);
                        let _ = writer.flush().await;
                        drop(writer);
                        let mut active = self.active_downloads.write().await;
                        active.remove(model_name);
                        return Err(anyhow!("Download cancelled by user"));
                    }
                }

                let next_result = timeout(Duration::from_secs(30), stream.next()).await;

                let chunk = match next_result {
                    Err(_) => {
                        log::warn!("Download timeout for {}", model_name);
                        let _ = writer.flush().await;
                        let mut active = self.active_downloads.write().await;
                        active.remove(model_name);
                        return Err(anyhow!(
                            "Download timeout - No data received for 30 seconds"
                        ));
                    }
                    Ok(None) => break,
                    Ok(Some(chunk_result)) => match chunk_result {
                        Ok(c) => c,
                        Err(e) => {
                            log::error!("Download error for {}: {:?}", model_name, e);
                            let _ = writer.flush().await;
                            let mut active = self.active_downloads.write().await;
                            active.remove(model_name);
                            return Err(anyhow!("Download error: {}", e));
                        }
                    },
                };

                if let Err(e) = writer.write_all(&chunk).await {
                    let mut active = self.active_downloads.write().await;
                    active.remove(model_name);
                    return Err(anyhow!("Failed to write chunk to file: {}", e));
                }

                let chunk_len = chunk.len() as u64;
                file_downloaded += chunk_len;
                total_downloaded += chunk_len;
                bytes_since_last_report += chunk_len;

                let overall_progress = if total_size_bytes > 0 {
                    ((total_downloaded as f64 / total_size_bytes as f64) * 100.0).min(99.0) as u8
                } else {
                    0
                };

                let elapsed_since_report = last_report_time.elapsed();
                let progress_changed = overall_progress > last_reported_progress;
                let time_threshold = elapsed_since_report >= Duration::from_millis(500);

                if progress_changed || time_threshold {
                    let speed_mbps = if elapsed_since_report.as_secs_f64() >= 0.1 {
                        (bytes_since_last_report as f64 / (1024.0 * 1024.0))
                            / elapsed_since_report.as_secs_f64()
                    } else {
                        let total_elapsed = download_start_time.elapsed().as_secs_f64();
                        if total_elapsed > 0.0 {
                            (total_downloaded as f64 / (1024.0 * 1024.0)) / total_elapsed
                        } else {
                            0.0
                        }
                    };

                    last_reported_progress = overall_progress;
                    last_report_time = Instant::now();
                    bytes_since_last_report = 0;

                    let progress =
                        DownloadProgress::new(total_downloaded, total_size_bytes, speed_mbps);
                    if let Some(ref callback) = progress_callback {
                        callback(progress);
                    }

                    // Update model status
                    {
                        let mut models = self.available_models.write().await;
                        if let Some(model) = models.get_mut(model_name) {
                            model.status = ModelStatus::Downloading {
                                progress: overall_progress,
                            };
                        }
                    }
                }
            }

            if let Err(e) = writer.flush().await {
                let mut active = self.active_downloads.write().await;
                active.remove(model_name);
                return Err(anyhow!("Failed to flush file {}: {}", filename, e));
            }

            log::info!(
                "Completed download: {} ({:.2} MB)",
                filename,
                file_downloaded as f64 / 1_048_576.0
            );
        }

        // Report 100% progress
        let total_elapsed = download_start_time.elapsed().as_secs_f64();
        let final_speed = if total_elapsed > 0.0 {
            (total_downloaded as f64 / (1024.0 * 1024.0)) / total_elapsed
        } else {
            0.0
        };
        let final_progress = DownloadProgress::new(total_size_bytes, total_size_bytes, final_speed);
        if let Some(ref callback) = progress_callback {
            callback(final_progress);
        }

        // Update model status to available
        {
            let mut models = self.available_models.write().await;
            if let Some(model) = models.get_mut(model_name) {
                model.status = ModelStatus::Available;
                model.path = model_dir.clone();
            }
        }

        // Remove from active downloads on completion
        {
            let mut active = self.active_downloads.write().await;
            active.remove(model_name);
        }

        // Clear cancellation flag on successful completion
        {
            let mut cancel_flag = self.cancel_download_flag.write().await;
            if cancel_flag.as_ref() == Some(&model_name.to_string()) {
                *cancel_flag = None;
            }
        }

        log::info!("Download completed for Moonshine model: {}", model_name);
        Ok(())
    }

    /// Cancel an ongoing model download
    pub async fn cancel_download(&self, model_name: &str) -> Result<()> {
        log::info!("Cancelling download for Moonshine model: {}", model_name);

        // Set cancellation flag to interrupt the download loop
        {
            let mut cancel_flag = self.cancel_download_flag.write().await;
            *cancel_flag = Some(model_name.to_string());
        }

        // Remove from active downloads
        {
            let mut active = self.active_downloads.write().await;
            active.remove(model_name);
        }

        // Update model status to Missing
        {
            let mut models = self.available_models.write().await;
            if let Some(model) = models.get_mut(model_name) {
                model.status = ModelStatus::Missing;
            }
        }

        // Clean up partially downloaded files
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let model_path = self.models_dir.join(model_name);
        if model_path.exists() {
            if let Err(e) = fs::remove_dir_all(&model_path).await {
                log::warn!("Failed to clean up cancelled download directory: {}", e);
            } else {
                log::info!(
                    "Cleaned up cancelled download directory: {}",
                    model_path.display()
                );
            }
        }

        Ok(())
    }
}

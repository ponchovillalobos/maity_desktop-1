use ndarray::{Array2, ArrayD};
use ort::execution_providers::CPUExecutionProvider;
use ort::inputs;
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::{DynValue, Tensor, TensorRef};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

const MAX_TOKENS: usize = 1024;
const EOS_TOKEN_ID: i64 = 2; // End of sequence token
const BOS_TOKEN_ID: i64 = 1; // Beginning of sequence token

// Moonshine-base decoder configuration (from config.json)
const DECODER_NUM_LAYERS: usize = 8;

#[derive(thiserror::Error, Debug)]
pub enum MoonshineError {
    #[error("ORT error: {0}")]
    Ort(#[from] ort::Error),
    #[error("I/O error")]
    Io(#[from] std::io::Error),
    #[error("ndarray shape error")]
    Shape(#[from] ndarray::ShapeError),
    #[error("Tokenizer error: {0}")]
    Tokenizer(String),
    #[error("Model input not found: {0}")]
    InputNotFound(String),
    #[error("Model output not found: {0}")]
    OutputNotFound(String),
    #[error("JSON parse error: {0}")]
    JsonParse(String),
}

/// Simple tokenizer that parses HuggingFace tokenizer.json format
/// This avoids the C++ runtime conflicts caused by the tokenizers crate on Windows
pub struct SimpleTokenizer {
    /// Map from token ID to token string
    id_to_token: HashMap<u32, String>,
    /// Map from token string to token ID
    token_to_id: HashMap<String, u32>,
}

impl SimpleTokenizer {
    /// Load tokenizer from HuggingFace tokenizer.json format
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, MoonshineError> {
        let content = fs::read_to_string(path.as_ref()).map_err(|e| {
            MoonshineError::Tokenizer(format!("Failed to read tokenizer file: {}", e))
        })?;

        let json: serde_json::Value = serde_json::from_str(&content).map_err(|e| {
            MoonshineError::JsonParse(format!("Failed to parse tokenizer JSON: {}", e))
        })?;

        let mut id_to_token = HashMap::new();
        let mut token_to_id = HashMap::new();

        // Parse the model.vocab section which contains the vocabulary
        // HuggingFace tokenizer.json format has vocab in model.vocab as {"token": id, ...}
        if let Some(model) = json.get("model") {
            if let Some(vocab) = model.get("vocab") {
                if let Some(vocab_obj) = vocab.as_object() {
                    for (token, id_value) in vocab_obj {
                        if let Some(id) = id_value.as_u64() {
                            let id = id as u32;
                            id_to_token.insert(id, token.clone());
                            token_to_id.insert(token.clone(), id);
                        }
                    }
                }
            }
        }

        // Also check for added_tokens which may contain special tokens
        if let Some(added_tokens) = json.get("added_tokens") {
            if let Some(tokens_array) = added_tokens.as_array() {
                for token_obj in tokens_array {
                    if let (Some(id), Some(content)) = (
                        token_obj.get("id").and_then(|v| v.as_u64()),
                        token_obj.get("content").and_then(|v| v.as_str()),
                    ) {
                        let id = id as u32;
                        id_to_token.insert(id, content.to_string());
                        token_to_id.insert(content.to_string(), id);
                    }
                }
            }
        }

        if id_to_token.is_empty() {
            return Err(MoonshineError::Tokenizer(
                "No vocabulary found in tokenizer.json".to_string(),
            ));
        }

        log::info!(
            "Loaded Moonshine tokenizer with {} tokens",
            id_to_token.len()
        );

        Ok(Self {
            id_to_token,
            token_to_id,
        })
    }

    /// Decode token IDs to text
    pub fn decode(
        &self,
        token_ids: &[u32],
        skip_special_tokens: bool,
    ) -> Result<String, MoonshineError> {
        let mut result = String::new();

        for &id in token_ids {
            if let Some(token) = self.id_to_token.get(&id) {
                // Skip special tokens if requested
                if skip_special_tokens {
                    // Common special tokens to skip
                    if token == "<s>"
                        || token == "</s>"
                        || token == "<pad>"
                        || token == "<unk>"
                        || token == "[CLS]"
                        || token == "[SEP]"
                        || token == "[PAD]"
                        || token == "[UNK]"
                        || token == "<|endoftext|>"
                    {
                        continue;
                    }
                }

                // Handle byte-level BPE tokens (like "Ġ" for space)
                let decoded_token = token
                    .replace("Ġ", " ") // GPT-style space marker
                    .replace("▁", " ") // SentencePiece space marker
                    .replace("Ċ", "\n"); // GPT-style newline marker

                result.push_str(&decoded_token);
            } else {
                // Unknown token - skip or add placeholder
                log::trace!("Unknown token ID: {}", id);
            }
        }

        Ok(result)
    }

    /// Get vocabulary size
    #[allow(dead_code)]
    pub fn vocab_size(&self) -> usize {
        self.id_to_token.len()
    }
}

pub struct MoonshineModel {
    encoder: Session,
    /// Decoder for the first step (without KV cache)
    decoder_first: Session,
    /// Decoder for subsequent steps (with KV cache)
    decoder_with_past: Session,
    tokenizer: SimpleTokenizer,
}

impl Drop for MoonshineModel {
    fn drop(&mut self) {
        log::debug!("Dropping MoonshineModel");
    }
}

impl MoonshineModel {
    pub fn new<P: AsRef<Path>>(model_dir: P) -> Result<Self, MoonshineError> {
        let encoder = Self::init_session(&model_dir, "encoder_model")?;
        // Using separate decoder models instead of merged to avoid MatMul errors
        // decoder_model.onnx: for the first step (no past_key_values input)
        // decoder_with_past_model.onnx: for subsequent steps (with past_key_values input)
        let decoder_first = Self::init_session(&model_dir, "decoder_model")?;
        let decoder_with_past = Self::init_session(&model_dir, "decoder_with_past_model")?;
        let tokenizer = Self::load_tokenizer(&model_dir)?;

        log::info!(
            "Loaded Moonshine model from {}",
            model_dir.as_ref().display()
        );

        Ok(Self {
            encoder,
            decoder_first,
            decoder_with_past,
            tokenizer,
        })
    }

    /// Extract tensor data to a properly aligned ArrayD.
    ///
    /// Uses downcast to `Tensor<f32>`, then `data_ptr()` and `shape()` to get raw pointer
    /// and dimensions, then copies data using unaligned reads. This avoids the alignment
    /// panics that occur with `try_extract_array()` and `try_extract_tensor()`, which
    /// internally use `slice::from_raw_parts` that requires aligned memory.
    ///
    /// ORT may return memory that is not aligned according to Rust's requirements
    /// (typically 4 bytes for f32), causing panics in `slice::from_raw_parts` at
    /// `ort-2.0.0-rc.10\src\value\impl_tensor\extract.rs:158`.
    ///
    /// By using `data_ptr()` + `read_unaligned()`, we safely handle misaligned pointers.
    fn extract_to_aligned_array(value: &DynValue) -> Result<ArrayD<f32>, MoonshineError> {
        // Downcast DynValue to Tensor<f32> to access data_ptr() method
        // This doesn't copy data or create slices, just validates the type
        let tensor: ort::value::TensorRef<'_, f32> = value.downcast_ref()?;

        // Get shape from tensor - returns &Shape (Vec<i64>), no slice creation
        let shape = tensor.shape();

        // Get raw data pointer from tensor - returns *const c_void, no slice creation
        let ptr = tensor.data_ptr() as *const f32;

        // Calculate total number of elements from shape dimensions
        let total_elements: usize = shape.iter().map(|&d| d as usize).product();

        // Convert ORT Shape (Vec<i64>) to ndarray IxDyn (Vec<usize>)
        let shape_vec: Vec<usize> = shape.iter().map(|&d| d as usize).collect();
        let ix_dyn = ndarray::IxDyn(&shape_vec);

        // Copy data element by element using unaligned reads
        // This is the key: read_unaligned handles misaligned pointers safely
        let mut data = Vec::with_capacity(total_elements);
        for i in 0..total_elements {
            // SAFETY: i is in bounds [0, total_elements), read_unaligned handles misalignment
            let val = unsafe { std::ptr::read_unaligned(ptr.add(i)) };
            data.push(val);
        }

        // Create array from copied data - now in properly aligned Rust memory
        let array = ArrayD::from_shape_vec(ix_dyn, data)?;

        Ok(array)
    }

    fn init_session<P: AsRef<Path>>(
        model_dir: P,
        model_name: &str,
    ) -> Result<Session, MoonshineError> {
        let providers = vec![CPUExecutionProvider::default().build()];

        let model_filename = format!("{}.onnx", model_name);
        log::info!("Loading Moonshine model from {}...", model_filename);

        let session = Session::builder()?
            .with_optimization_level(GraphOptimizationLevel::Level3)?
            .with_execution_providers(providers)?
            .with_parallel_execution(true)?
            .commit_from_file(model_dir.as_ref().join(&model_filename))?;

        // Log all inputs and outputs for debugging
        let input_names: Vec<&str> = session.inputs.iter().map(|i| i.name.as_str()).collect();
        let output_names: Vec<&str> = session.outputs.iter().map(|o| o.name.as_str()).collect();

        log::info!(
            "Moonshine '{}' loaded: inputs={:?}, outputs={:?}",
            model_filename,
            input_names,
            output_names
        );

        for input in &session.inputs {
            log::info!("  Input '{}': type={:?}", input.name, input.input_type);
        }

        for output in &session.outputs {
            log::info!("  Output '{}': type={:?}", output.name, output.output_type);
        }

        // Validate expected inputs based on model type
        if model_name == "encoder_model" {
            // Encoder uses positional input (first input) so any single input name is valid
            if input_names.is_empty() {
                log::warn!(
                    "⚠️ Encoder model has no inputs. This will cause transcription to fail."
                );
            } else {
                log::info!(
                    "✅ Encoder model has {} input(s): {:?} (using positional input)",
                    input_names.len(),
                    input_names
                );
            }
        } else if model_name == "decoder_model" {
            // First-step decoder: no past_key_values inputs required
            let expected_decoder_inputs = ["input_ids", "encoder_hidden_states"];
            for expected in expected_decoder_inputs {
                if !input_names.contains(&expected) {
                    log::warn!(
                        "⚠️ Decoder (first step) model missing expected input '{}'. Available inputs: {:?}. \
                        This may cause transcription to fail.",
                        expected, input_names
                    );
                }
            }
        } else if model_name == "decoder_with_past_model" {
            // Subsequent decoder: requires past_key_values inputs
            let expected_decoder_inputs = ["input_ids", "encoder_hidden_states"];
            for expected in expected_decoder_inputs {
                if !input_names.contains(&expected) {
                    log::warn!(
                        "⚠️ Decoder (with past) model missing expected input '{}'. Available inputs: {:?}. \
                        This may cause transcription to fail.",
                        expected, input_names
                    );
                }
            }
        }

        Ok(session)
    }

    fn load_tokenizer<P: AsRef<Path>>(model_dir: P) -> Result<SimpleTokenizer, MoonshineError> {
        let tokenizer_path = model_dir.as_ref().join("tokenizer.json");
        let tokenizer = SimpleTokenizer::from_file(tokenizer_path)?;

        log::info!("Loaded Moonshine tokenizer");
        Ok(tokenizer)
    }

    /// Encode audio samples to hidden states
    fn encode(&mut self, audio_samples: &[f32]) -> Result<ArrayD<f32>, MoonshineError> {
        log::info!(
            "Running Moonshine encoder with {} samples ({:.2}s at 16kHz)",
            audio_samples.len(),
            audio_samples.len() as f64 / 16000.0
        );

        // Moonshine expects audio as [batch_size, sequence_length]
        let batch_size = 1;
        let seq_len = audio_samples.len();

        // Log audio statistics for debugging
        let (min_val, max_val, mean_val) = if !audio_samples.is_empty() {
            let min = audio_samples.iter().cloned().fold(f32::INFINITY, f32::min);
            let max = audio_samples
                .iter()
                .cloned()
                .fold(f32::NEG_INFINITY, f32::max);
            let sum: f32 = audio_samples.iter().sum();
            let mean = sum / audio_samples.len() as f32;
            (min, max, mean)
        } else {
            (0.0, 0.0, 0.0)
        };
        log::debug!(
            "Audio stats: min={:.4}, max={:.4}, mean={:.6}, shape=[{}, {}]",
            min_val,
            max_val,
            mean_val,
            batch_size,
            seq_len
        );

        // Log the encoder input name for debugging (different Moonshine exports use different names)
        if let Some(input_info) = self.encoder.inputs.first() {
            log::debug!(
                "Encoder input name: '{}', using positional input",
                input_info.name
            );
        }

        // Create input array
        let audio_array =
            Array2::from_shape_vec((batch_size, seq_len), audio_samples.to_vec())?.into_dyn();

        // Use positional input (first input) instead of named input to support
        // different Moonshine model exports that may use different input names
        // (e.g., "args_0", "audio", "input", etc.)
        let inputs = inputs![TensorRef::from_array_view(audio_array.view())?,];

        let outputs = self.encoder.run(inputs).map_err(|e| {
            log::error!(
                "Moonshine encoder ORT error: {:?} | Input shape: [{}, {}] | Audio range: [{:.4}, {:.4}]",
                e, batch_size, seq_len, min_val, max_val
            );
            e
        })?;

        // Get encoder output (hidden states)
        // Try different output names that Moonshine models might use
        // Use extract_to_aligned_array() to avoid alignment panics with ORT memory
        let hidden_states = if let Some(v) = outputs.get("last_hidden_state") {
            Self::extract_to_aligned_array(v)?
        } else if let Some(v) = outputs.get("hidden_states") {
            Self::extract_to_aligned_array(v)?
        } else {
            // Fallback to first output
            let first_output = outputs
                .values()
                .next()
                .ok_or_else(|| MoonshineError::OutputNotFound("hidden_states".to_string()))?;
            Self::extract_to_aligned_array(&first_output)?
        };

        log::trace!("Encoder output shape: {:?}", hidden_states.shape());

        Ok(hidden_states)
    }

    /// Decode hidden states to text tokens using separate decoder models.
    ///
    /// Uses two models instead of the merged decoder to avoid MatMul errors:
    /// - decoder_model.onnx: for the first step (without KV cache input)
    /// - decoder_with_past_model.onnx: for subsequent steps (with KV cache input)
    ///
    /// This is the approach recommended by HuggingFace for encoder-decoder models.
    fn decode(&mut self, encoder_output: &ArrayD<f32>) -> Result<Vec<i64>, MoonshineError> {
        log::info!(
            "Running Moonshine decoder with encoder_output shape: {:?}",
            encoder_output.shape()
        );

        let mut tokens: Vec<i64> = vec![BOS_TOKEN_ID];
        let batch_size = 1;

        // Cache will be populated after the first step
        let mut past_key_values: Option<HashMap<String, ArrayD<f32>>> = None;

        for step in 0..MAX_TOKENS {
            // For first step: use full input_ids and decoder_first model (no cache input)
            // For subsequent steps: use only the last token and decoder_with_past model (with cache)
            let (logits, new_cache) = if step == 0 {
                // First step: use decoder_first (decoder_model.onnx)
                let input_ids_data = tokens.clone();
                let token_seq_len = input_ids_data.len();
                let input_ids =
                    Array2::from_shape_vec((batch_size, token_seq_len), input_ids_data)?.into_dyn();

                self.run_decoder_first_step(&input_ids, encoder_output)?
            } else {
                // Subsequent steps: use decoder_with_past (decoder_with_past_model.onnx)
                let input_ids_data = vec![*tokens.last().unwrap()];
                let input_ids = Array2::from_shape_vec((batch_size, 1), input_ids_data)?.into_dyn();

                let cache = past_key_values.as_ref().ok_or_else(|| {
                    MoonshineError::InputNotFound("past_key_values not available".to_string())
                })?;

                self.run_decoder_with_past(&input_ids, encoder_output, cache, step)?
            };

            // Update cache for next step
            past_key_values = Some(new_cache);

            log::trace!("Decoder step {} logits shape: {:?}", step, logits.shape());

            // Get vocabulary size from logits shape
            let vocab_size = if logits.shape().len() >= 3 {
                logits.shape()[2]
            } else if logits.shape().len() == 2 {
                logits.shape()[1]
            } else {
                return Err(MoonshineError::OutputNotFound(
                    "Could not determine vocab size from logits".to_string(),
                ));
            };

            // Get the logits for the last token position
            let logits_slice = logits.as_slice().ok_or_else(|| {
                MoonshineError::Shape(ndarray::ShapeError::from_kind(
                    ndarray::ErrorKind::IncompatibleShape,
                ))
            })?;
            let last_token_start = logits_slice.len() - vocab_size;
            let last_logits = &logits_slice[last_token_start..];

            // Find argmax
            let next_token = last_logits
                .iter()
                .enumerate()
                .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(idx, _)| idx as i64)
                .unwrap_or(EOS_TOKEN_ID);

            log::trace!("Step {}: next token = {}", step, next_token);

            // Check for EOS token
            if next_token == EOS_TOKEN_ID {
                log::debug!("EOS token reached at step {}", step);
                break;
            }

            tokens.push(next_token);
        }

        // Remove BOS token from output
        if !tokens.is_empty() && tokens[0] == BOS_TOKEN_ID {
            tokens.remove(0);
        }

        Ok(tokens)
    }

    /// Run the first decoder step using decoder_model.onnx (without past_key_values input).
    /// Returns logits and the initial KV cache from present.* outputs.
    fn run_decoder_first_step(
        &mut self,
        input_ids: &ArrayD<i64>,
        encoder_output: &ArrayD<f32>,
    ) -> Result<(ArrayD<f32>, HashMap<String, ArrayD<f32>>), MoonshineError> {
        log::trace!("Running decoder first step (no cache input)");

        // Build inputs: only input_ids and encoder_hidden_states (no past_key_values)
        let mut inputs: Vec<(String, DynValue)> = Vec::with_capacity(2);

        let input_ids_tensor = Tensor::from_array(input_ids.clone())?.into_dyn();
        inputs.push(("input_ids".to_string(), input_ids_tensor));

        let encoder_tensor = Tensor::from_array(encoder_output.clone())?.into_dyn();
        inputs.push(("encoder_hidden_states".to_string(), encoder_tensor));

        // Run the first decoder
        let outputs = self.decoder_first.run(inputs)?;

        // Extract logits
        let logits = if let Some(v) = outputs.get("logits") {
            Self::extract_to_aligned_array(v)?
        } else {
            let first_output = outputs
                .values()
                .next()
                .ok_or_else(|| MoonshineError::OutputNotFound("logits".to_string()))?;
            Self::extract_to_aligned_array(&first_output)?
        };

        // Extract cache values from present.* outputs
        let mut new_cache: HashMap<String, ArrayD<f32>> = HashMap::new();
        for layer in 0..DECODER_NUM_LAYERS {
            for module in ["decoder", "encoder"] {
                for kv in ["key", "value"] {
                    let present_name = format!("present.{}.{}.{}", layer, module, kv);
                    let cache_name = format!("past_key_values.{}.{}.{}", layer, module, kv);

                    if let Some(present) = outputs.get(&present_name) {
                        let present_array = Self::extract_to_aligned_array(present)?;
                        new_cache.insert(cache_name, present_array);
                    } else {
                        log::warn!("First step decoder missing output: {}", present_name);
                    }
                }
            }
        }

        log::debug!(
            "First step: extracted {} cache tensors from decoder outputs",
            new_cache.len()
        );

        Ok((logits, new_cache))
    }

    /// Run subsequent decoder steps using decoder_with_past_model.onnx (with past_key_values input).
    /// Returns logits and the updated KV cache.
    fn run_decoder_with_past(
        &mut self,
        input_ids: &ArrayD<i64>,
        encoder_output: &ArrayD<f32>,
        past_key_values: &HashMap<String, ArrayD<f32>>,
        step: usize,
    ) -> Result<(ArrayD<f32>, HashMap<String, ArrayD<f32>>), MoonshineError> {
        log::trace!("Running decoder step {} with cache", step);

        // Build inputs: input_ids, encoder_hidden_states, and all 32 past_key_values
        let mut inputs: Vec<(String, DynValue)> = Vec::with_capacity(34);

        let input_ids_tensor = Tensor::from_array(input_ids.clone())?.into_dyn();
        inputs.push(("input_ids".to_string(), input_ids_tensor));

        let encoder_tensor = Tensor::from_array(encoder_output.clone())?.into_dyn();
        inputs.push(("encoder_hidden_states".to_string(), encoder_tensor));

        // Add all 32 past_key_values (8 layers × 2 modules × 2 kv)
        for layer in 0..DECODER_NUM_LAYERS {
            for module in ["decoder", "encoder"] {
                for kv in ["key", "value"] {
                    let name = format!("past_key_values.{}.{}.{}", layer, module, kv);
                    let cache = past_key_values
                        .get(&name)
                        .ok_or_else(|| MoonshineError::InputNotFound(name.clone()))?;
                    let cache_tensor = Tensor::from_array(cache.clone())?.into_dyn();
                    inputs.push((name, cache_tensor));
                }
            }
        }

        // Run the decoder with past
        let outputs = self.decoder_with_past.run(inputs)?;

        // Extract logits
        let logits = if let Some(v) = outputs.get("logits") {
            Self::extract_to_aligned_array(v)?
        } else {
            let first_output = outputs
                .values()
                .next()
                .ok_or_else(|| MoonshineError::OutputNotFound("logits".to_string()))?;
            Self::extract_to_aligned_array(&first_output)?
        };

        // Extract new cache values from present.* outputs
        let mut new_cache: HashMap<String, ArrayD<f32>> = HashMap::new();
        for layer in 0..DECODER_NUM_LAYERS {
            for module in ["decoder", "encoder"] {
                for kv in ["key", "value"] {
                    let present_name = format!("present.{}.{}.{}", layer, module, kv);
                    let cache_name = format!("past_key_values.{}.{}.{}", layer, module, kv);

                    if let Some(present) = outputs.get(&present_name) {
                        let present_array = Self::extract_to_aligned_array(present)?;
                        new_cache.insert(cache_name, present_array);
                    } else {
                        // If no new cache value, keep the old one
                        if let Some(old_cache) = past_key_values.get(&cache_name) {
                            new_cache.insert(cache_name, old_cache.clone());
                        }
                    }
                }
            }
        }

        Ok((logits, new_cache))
    }

    /// Decode token IDs to text
    fn tokens_to_text(&self, tokens: &[i64]) -> Result<String, MoonshineError> {
        // Convert i64 tokens to u32 for tokenizer
        let token_ids: Vec<u32> = tokens.iter().map(|&t| t as u32).collect();

        let text = self.tokenizer.decode(&token_ids, true)?;

        Ok(text.trim().to_string())
    }

    /// Transcribe audio samples to text
    pub fn transcribe(&mut self, audio_samples: Vec<f32>) -> Result<String, MoonshineError> {
        let samples_len = audio_samples.len();
        log::debug!(
            "Moonshine transcribing {} samples ({:.1}s duration)",
            samples_len,
            samples_len as f64 / 16000.0
        );

        // Encode audio to hidden states
        let encoder_output = self.encode(&audio_samples)?;

        // Decode hidden states to tokens
        let tokens = self.decode(&encoder_output)?;

        log::debug!("Decoded {} tokens", tokens.len());

        // Convert tokens to text
        let text = self.tokens_to_text(&tokens)?;

        log::debug!("Moonshine transcription: '{}'", text);

        Ok(text)
    }
}

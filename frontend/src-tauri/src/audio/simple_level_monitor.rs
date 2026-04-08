use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};
use log::{debug, error, info, warn};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use tauri::{AppHandle, Emitter, Runtime};

#[derive(Debug, Serialize, Clone)]
pub struct AudioLevelData {
    pub device_name: String,
    pub device_type: String, // "input" or "output"
    pub rms_level: f32,      // RMS level (0.0 to 1.0)
    pub peak_level: f32,     // Peak level (0.0 to 1.0)
    pub is_active: bool,     // Whether audio is being detected
}

#[derive(Debug, Serialize, Clone)]
pub struct AudioLevelUpdate {
    pub timestamp: u64,
    pub levels: Vec<AudioLevelData>,
}

// Global monitoring state using atomics (lock-free, same pattern as RecordingState)
static IS_MONITORING: AtomicBool = AtomicBool::new(false);
static MIC_RMS: AtomicU32 = AtomicU32::new(0);
static MIC_PEAK: AtomicU32 = AtomicU32::new(0);

/// Start real CPAL audio level monitoring for the specified input device.
/// Spawns an OS thread to own the CPAL stream (may not be Send on all platforms)
/// and a tokio task to emit level events every 100ms.
pub async fn start_monitoring<R: Runtime>(
    app_handle: AppHandle<R>,
    device_names: Vec<String>,
) -> Result<()> {
    info!(
        "Starting audio level monitoring for devices: {:?}",
        device_names
    );

    // Stop any existing monitoring and wait for cleanup
    IS_MONITORING.store(false, Ordering::SeqCst);
    tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

    // Reset levels
    MIC_RMS.store(0u32, Ordering::Relaxed);
    MIC_PEAK.store(0u32, Ordering::Relaxed);

    IS_MONITORING.store(true, Ordering::SeqCst);

    let mic_device_name = device_names.first().cloned().unwrap_or_default();

    // Spawn OS thread for CPAL (streams may not be Send on all platforms)
    std::thread::Builder::new()
        .name("audio-level-monitor".to_string())
        .spawn(move || {
            let host = cpal::default_host();

            // Find the requested device or fall back to default input
            let device = if mic_device_name.is_empty() {
                host.default_input_device()
            } else {
                host.input_devices()
                    .ok()
                    .and_then(|mut devices| {
                        devices.find(|d| d.name().map(|n| n == mic_device_name).unwrap_or(false))
                    })
                    .or_else(|| {
                        warn!(
                            "Device '{}' not found, falling back to default input",
                            mic_device_name
                        );
                        host.default_input_device()
                    })
            };

            let device = match device {
                Some(d) => d,
                None => {
                    error!("No input device available for monitoring");
                    IS_MONITORING.store(false, Ordering::SeqCst);
                    return;
                }
            };

            let device_name_actual = device.name().unwrap_or_else(|_| "Unknown".to_string());
            debug!("Monitoring input device: {}", device_name_actual);

            let config = match device.default_input_config() {
                Ok(c) => c,
                Err(e) => {
                    error!(
                        "Failed to get input config for '{}': {}",
                        device_name_actual, e
                    );
                    IS_MONITORING.store(false, Ordering::SeqCst);
                    return;
                }
            };

            let channels = config.channels();
            let sample_format = config.sample_format();
            let stream_config = StreamConfig {
                channels,
                sample_rate: config.sample_rate(),
                buffer_size: cpal::BufferSize::Default,
            };

            debug!(
                "Monitor stream config: {}Hz, {} ch, {:?}",
                config.sample_rate().0,
                channels,
                sample_format
            );

            // Build input stream based on sample format
            let stream = match sample_format {
                SampleFormat::F32 => {
                    let ch = channels;
                    device.build_input_stream(
                        &stream_config,
                        move |data: &[f32], _: &cpal::InputCallbackInfo| {
                            compute_and_store_levels(data, ch);
                        },
                        |err| error!("Audio monitor stream error: {}", err),
                        None,
                    )
                }
                SampleFormat::I16 => {
                    let ch = channels;
                    device.build_input_stream(
                        &stream_config,
                        move |data: &[i16], _: &cpal::InputCallbackInfo| {
                            // Convert i16 samples to f32 in-place
                            let f32_data: Vec<f32> =
                                data.iter().map(|&s| s as f32 / 32768.0).collect();
                            compute_and_store_levels(&f32_data, ch);
                        },
                        |err| error!("Audio monitor stream error: {}", err),
                        None,
                    )
                }
                SampleFormat::U16 => {
                    let ch = channels;
                    device.build_input_stream(
                        &stream_config,
                        move |data: &[u16], _: &cpal::InputCallbackInfo| {
                            let f32_data: Vec<f32> =
                                data.iter().map(|&s| (s as f32 / 32768.0) - 1.0).collect();
                            compute_and_store_levels(&f32_data, ch);
                        },
                        |err| error!("Audio monitor stream error: {}", err),
                        None,
                    )
                }
                _ => {
                    error!(
                        "Unsupported sample format for monitoring: {:?}",
                        sample_format
                    );
                    IS_MONITORING.store(false, Ordering::SeqCst);
                    return;
                }
            };

            let stream = match stream {
                Ok(s) => s,
                Err(e) => {
                    error!("Failed to build monitor stream: {}", e);
                    IS_MONITORING.store(false, Ordering::SeqCst);
                    return;
                }
            };

            if let Err(e) = stream.play() {
                error!("Failed to start monitor stream: {}", e);
                IS_MONITORING.store(false, Ordering::SeqCst);
                return;
            }

            info!("Audio monitor stream started for '{}'", device_name_actual);

            // Keep thread alive while monitoring — stream is dropped when we exit
            while IS_MONITORING.load(Ordering::SeqCst) {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }

            drop(stream);
            info!("Audio monitor stream stopped for '{}'", device_name_actual);
        })?;

    // Spawn tokio task to poll atomics and emit Tauri events
    let emit_device_name = device_names
        .first()
        .cloned()
        .unwrap_or_else(|| "Default".to_string());

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(100));

        while IS_MONITORING.load(Ordering::SeqCst) {
            interval.tick().await;

            let rms = f32::from_bits(MIC_RMS.load(Ordering::Relaxed));
            let peak = f32::from_bits(MIC_PEAK.load(Ordering::Relaxed));

            let update = AudioLevelUpdate {
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64,
                levels: vec![AudioLevelData {
                    device_name: emit_device_name.clone(),
                    device_type: "input".to_string(),
                    rms_level: rms,
                    peak_level: peak,
                    is_active: rms > 0.001,
                }],
            };

            if let Err(e) = app_handle.emit("audio-levels", &update) {
                error!("Failed to emit audio levels: {}", e);
                break;
            }
        }

        info!("Audio level emission task ended");
    });

    Ok(())
}

/// Compute RMS and peak from audio data and store in atomics
fn compute_and_store_levels(data: &[f32], channels: u16) {
    if data.is_empty() {
        return;
    }

    // Convert to mono by averaging channels
    let mono: Vec<f32> = if channels > 1 {
        data.chunks(channels as usize)
            .map(|frame| frame.iter().sum::<f32>() / channels as f32)
            .collect()
    } else {
        data.to_vec()
    };

    if mono.is_empty() {
        return;
    }

    let rms = (mono.iter().map(|x| x * x).sum::<f32>() / mono.len() as f32)
        .sqrt()
        .min(1.0);
    let peak = mono.iter().map(|x| x.abs()).fold(0.0f32, f32::max).min(1.0);

    MIC_RMS.store(rms.to_bits(), Ordering::Relaxed);
    MIC_PEAK.store(peak.to_bits(), Ordering::Relaxed);
}

/// Stop audio level monitoring
pub async fn stop_monitoring() -> Result<()> {
    info!("Stopping audio level monitoring");
    IS_MONITORING.store(false, Ordering::SeqCst);
    // Reset levels to zero
    MIC_RMS.store(0u32, Ordering::Relaxed);
    MIC_PEAK.store(0u32, Ordering::Relaxed);
    Ok(())
}

/// Check if currently monitoring
pub fn is_monitoring() -> bool {
    IS_MONITORING.load(Ordering::SeqCst)
}

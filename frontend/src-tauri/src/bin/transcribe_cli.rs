// transcribe_cli — Headless Whisper driver for Maity Desktop test harnesses.
// Reuses `app_lib::whisper_engine::WhisperEngine`. See docs/audit/TRANSCRIBE_CLI_SPEC.md.
// Exit codes: 0 ok | 1 audio | 2 model | 3 transcribe | 4 io

use std::fs::File;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use app_lib::whisper_engine::WhisperEngine;
use clap::Parser;
use serde_json::json;
use symphonia::core::audio::AudioBufferRef;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

const TARGET_SR: u32 = 16_000;

#[derive(Parser, Debug)]
#[command(name = "transcribe_cli", about = "Headless Whisper CLI for Maity Desktop")]
struct Args {
    /// Path to input audio (WAV recommended; any symphonia-supported format works)
    #[arg(long)]
    audio: PathBuf,

    /// Whisper model name (tiny, base, small, medium, large-v3, large-v3-turbo, ...)
    #[arg(long)]
    model: String,

    /// ISO language code passed to whisper (default: es)
    #[arg(long, default_value = "es")]
    language: String,

    /// Emit JSON instead of plain text
    #[arg(long, default_value_t = false)]
    json: bool,
}

/// Decode any symphonia-supported audio file to mono f32 @ 16 kHz.
fn load_audio_16k_mono(path: &PathBuf) -> Result<Vec<f32>, String> {
    let file = File::open(path).map_err(|e| format!("open: {e}"))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| format!("probe: {e}"))?;
    let mut format = probed.format;

    let track = format
        .default_track()
        .ok_or_else(|| "no default track".to_string())?;
    let track_id = track.id;
    let codec_params = track.codec_params.clone();
    let src_sr = codec_params.sample_rate.ok_or("unknown sample rate")?;
    let src_channels = codec_params
        .channels
        .map(|c| c.count())
        .unwrap_or(1)
        .max(1);

    let mut decoder = symphonia::default::get_codecs()
        .make(&codec_params, &DecoderOptions::default())
        .map_err(|e| format!("decoder: {e}"))?;

    let mut mono: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break
            }
            Err(SymphoniaError::ResetRequired) => break,
            Err(e) => return Err(format!("packet: {e}")),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = decoder.decode(&packet).map_err(|e| format!("decode: {e}"))?;
        append_mono(&decoded, &mut mono, src_channels);
    }

    // Resample to 16 kHz with simple linear interpolation (good enough for CLI tests).
    if src_sr == TARGET_SR {
        Ok(mono)
    } else {
        Ok(resample_linear(&mono, src_sr, TARGET_SR))
    }
}

fn append_mono(buf: &AudioBufferRef<'_>, out: &mut Vec<f32>, channels: usize) {
    // Copy via symphonia's generic conversion into an f32 sample buffer, then mixdown.
    use symphonia::core::audio::SampleBuffer;
    let spec = *buf.spec();
    let mut sb = SampleBuffer::<f32>::new(buf.capacity() as u64, spec);
    sb.copy_interleaved_ref(buf.clone());
    let samples = sb.samples();
    let frames = samples.len() / channels;
    for i in 0..frames {
        let mut acc = 0.0f32;
        for ch in 0..channels {
            acc += samples[i * channels + ch];
        }
        out.push(acc / channels as f32);
    }
}

fn resample_linear(input: &[f32], src_sr: u32, dst_sr: u32) -> Vec<f32> {
    if input.is_empty() || src_sr == dst_sr {
        return input.to_vec();
    }
    let ratio = dst_sr as f64 / src_sr as f64;
    let out_len = ((input.len() as f64) * ratio).round() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src_idx = i as f64 / ratio;
        let i0 = src_idx.floor() as usize;
        let i1 = (i0 + 1).min(input.len() - 1);
        let frac = (src_idx - i0 as f64) as f32;
        out.push(input[i0] * (1.0 - frac) + input[i1] * frac);
    }
    out
}

fn default_models_dir() -> Option<PathBuf> {
    // FIX: la app real usa dirs::data_dir() (AppData/Roaming en Windows)
    // vía Tauri's app_data_dir() que mapea a ~/AppData/Roaming/com.maity.ai/.
    // Antes teníamos data_local_dir() (AppData/Local) que es un path distinto
    // donde no están los modelos descargados. Usar Roaming para que el CLI
    // vea los mismos modelos que el app.
    dirs::data_dir().map(|p| p.join("com.maity.ai").join("models"))
}

fn run() -> Result<(), (u8, String)> {
    let args = Args::parse();

    if !args.audio.exists() {
        return Err((1, format!("audio file not found: {}", args.audio.display())));
    }

    let samples = load_audio_16k_mono(&args.audio).map_err(|e| (1, e))?;
    let audio_duration_sec = samples.len() as f64 / TARGET_SR as f64;

    let rt = tokio::runtime::Runtime::new().map_err(|e| (3, format!("runtime: {e}")))?;

    let result = rt.block_on(async {
        let engine = WhisperEngine::new_with_models_dir(default_models_dir())
            .map_err(|e| (2, format!("engine init: {e}")))?;
        engine
            .discover_models()
            .await
            .map_err(|e| (2, format!("discover_models: {e}")))?;
        engine
            .load_model(&args.model)
            .await
            .map_err(|e| (2, format!("load_model({}): {e}", args.model)))?;

        let started = Instant::now();
        let (text, confidence, _is_partial) = engine
            .transcribe_audio_with_confidence(samples, Some(args.language.clone()))
            .await
            .map_err(|e| (3, format!("transcribe: {e}")))?;
        let elapsed = started.elapsed().as_secs_f64();
        Ok::<_, (u8, String)>((text, confidence, elapsed))
    })?;

    let (text, _confidence, processing_time_sec) = result;
    let speed_factor = if processing_time_sec > 0.0 {
        audio_duration_sec / processing_time_sec
    } else {
        0.0
    };

    if args.json {
        let out = json!({
            "audio_path": args.audio.display().to_string(),
            "audio_duration_sec": audio_duration_sec,
            "processing_time_sec": processing_time_sec,
            "speed_factor": speed_factor,
            "model": args.model,
            "language": args.language,
            "text": text,
            "segments": [
                {"start": 0.0, "end": audio_duration_sec, "text": text}
            ]
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&out).map_err(|e| (4, format!("json: {e}")))?
        );
    } else {
        println!("{}", text);
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::from(0),
        Err((code, msg)) => {
            eprintln!("transcribe_cli error: {msg}");
            ExitCode::from(code)
        }
    }
}

# transcribe_cli — Spec

Headless Whisper transcription binary for Maity Desktop test harnesses. Drives
`app_lib::whisper_engine::WhisperEngine` directly so tests can measure model
quality, speed and repeatability **without launching the Tauri GUI**.

- Source: `frontend/src-tauri/src/bin/transcribe_cli.rs`
- Built by: `cargo build --manifest-path frontend/src-tauri/Cargo.toml --bin transcribe_cli`
- Artifact: `target/debug/transcribe_cli.exe` (Windows) / `target/debug/transcribe_cli`

## Usage

```
transcribe_cli --audio <path> --model <name> [--language <code>] [--json]
```

| Flag | Required | Default | Description |
|------|:--------:|---------|-------------|
| `--audio` | yes | — | Path to an audio file (any symphonia-supported format; WAV recommended). Resampled internally to 16 kHz mono f32. |
| `--model` | yes | — | Whisper model name registered in `WhisperEngine::discover_models()` (e.g. `tiny`, `base`, `small`, `medium`, `large-v3`, `large-v3-turbo`). The model must already be downloaded in the models directory. |
| `--language` | no | `es` | ISO language code passed to Whisper (`es`, `en`, `fr`, ...). Use `auto` for language detection, `auto-translate` to translate to English. |
| `--json` | no | `false` | Emit JSON to stdout instead of plain text. |

## Models directory

Defaults to `dirs::data_local_dir().join("Maity/models")`, matching where
production Maity Desktop stores whisper models:

- Windows: `%LOCALAPPDATA%\Maity\models`
- macOS:   `~/Library/Application Support/Maity/models`
- Linux:   `~/.local/share/Maity/models`

The engine calls `discover_models()` + `load_model(name)`. If the requested
model is not present on disk the binary exits with code `2` — downloads are
the responsibility of the main Tauri app.

## JSON output schema (`--json`)

```json
{
  "audio_path": "tests/fixtures/audio/A_Poncho_Mensaje.wav",
  "audio_duration_sec": 12.34,
  "processing_time_sec": 5.67,
  "speed_factor": 2.17,
  "model": "base",
  "language": "es",
  "text": "transcripción completa",
  "segments": [
    {"start": 0.0, "end": 12.34, "text": "transcripción completa"}
  ]
}
```

`speed_factor = audio_duration_sec / processing_time_sec` — how many seconds
of audio are processed per wall-clock second. Values >1 mean faster than
real time.

> **Note on `segments`.** `WhisperEngine::transcribe_audio_with_confidence`
> currently returns a single concatenated string without per-segment
> timestamps (whisper.cpp timestamps are deliberately disabled upstream to
> avoid the "single timestamp ending → skip chunk" issue). The CLI therefore
> emits a single synthetic segment spanning the full audio. When the engine
> is extended to expose per-segment data, this CLI should pass it through
> verbatim.

## Exit codes

| Code | Meaning |
|:----:|---------|
| 0 | Success |
| 1 | Audio file not found or failed to decode |
| 2 | Model not downloaded or failed to load |
| 3 | Transcription error |
| 4 | IO error writing output (JSON serialization, stdout) |

## Examples

Plain text:

```bash
./target/debug/transcribe_cli.exe \
  --audio tests/fixtures/audio/A_Poncho_Mensaje.wav \
  --model base \
  --language es
```

JSON piped into `jq`:

```bash
./target/debug/transcribe_cli.exe \
  --audio tests/fixtures/audio/A_Poncho_Mensaje.wav \
  --model large-v3-turbo \
  --language es \
  --json | jq '.speed_factor, .text'
```

Loop over fixtures and record metrics:

```bash
for f in tests/fixtures/audio/*.wav; do
  ./target/debug/transcribe_cli.exe --audio "$f" --model base --json \
    >> memory/build_logs/transcribe_cli_runs.jsonl
done
```

## Notes / gotchas

- The binary calls `WhisperEngine::new_with_models_dir(Some(default_models_dir()))`.
  The models directory is currently hard-wired; add a `--models-dir` flag if
  the harness needs an alternate location.
- Resampling uses a simple linear interpolator, sufficient for test harnesses.
  For production-grade sample-rate conversion use the pipeline resampler.
- The first invocation with a large model can take a while because whisper.cpp
  initialises GPU backends; subsequent invocations reuse on-disk caches.
- Whisper C-library logs are suppressed via `GGML_METAL_LOG_LEVEL=1` and
  `WHISPER_LOG_LEVEL=1`, set inside `WhisperEngine::new_with_models_dir`.
- `transcribe_audio_with_confidence` does not return per-segment data, so
  `speed_factor` is the main performance signal for benchmarking.

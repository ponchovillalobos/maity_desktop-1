# Cargo Dependencies — Maity Desktop

> Documentación completa del `frontend/src-tauri/Cargo.toml` tras el refactor RUST-011 (2026-04-08).
> Para nuevos contribuidores: este es el mapa de qué hace cada dependencia.

---

## TL;DR

- **75 dependencias runtime + 5 dev + 3 build** organizadas en 13 categorías
- **Cero duplicados** (RUST-011 eliminó 6 dependencias declaradas dos o tres veces)
- **Todas las deps git pinneadas a rev exacto** (RUST-003 supply-chain hardening)
- **`whisper-rs` declarado por OS** (intencional: features per-platform)
- **`rust-version = "1.80"`** (bumpeado desde 1.77 en RUST-011)
- **77/77 tests pasan CPU-only** sin GPU

---

## Estructura del archivo

```
[package]            # Metadata del crate
[lib]                # crate-types: staticlib, cdylib, rlib
[features]           # platform-default + opt-in GPU (cuda/vulkan/metal/...)
[build-dependencies] # tauri-build, reqwest, which
[dependencies]       # 13 categorías con encabezados claros
[target.cfg(macos)]  # whisper-rs metal+coreml + ScreenCaptureKit (objc, cidre, ...)
[target.cfg(windows)]# whisper-rs raw-api + WASAPI (windows crate)
[target.cfg(linux)]  # whisper-rs raw-api
[dev-dependencies]   # tempfile, criterion, infer, ...
[patch.crates-io]    # cpal + esaxx-rs pinneados por rev
```

---

## Categorías de dependencias runtime

### 1. Serialization & error handling
| Crate | Versión | Por qué |
|---|---|---|
| `serde` | 1.0 | (de)serialización de structs |
| `serde_json` | 1.0 | JSON específico |
| `anyhow` | 1.0 | error handling unificado en Result<_, anyhow::Error> |
| `thiserror` | 2.0.16 | derive Error para enums propios (Parakeet, audio errors) |
| `once_cell` | 1.17.1 | lazy statics inicializados a runtime |
| `lazy_static` | 1.4.0 | (legacy) statics con código de inicialización |
| `uuid` | 1.0 | identificadores únicos para meetings, tracks |
| `url` | 2.5.0 | parsing/construction de URLs (Deepgram, OpenRouter) |
| `chrono` | 0.4.31 | timestamps (timezone, ISO8601, formato local) |
| `bytes` | 1.9.0 | buffers eficientes (audio chunks) |
| `bytemuck` | 1.16.1 | cast seguro entre `[f32]` y `[u8]` para audio |

### 2. Logging & telemetry
| Crate | Versión | Por qué |
|---|---|---|
| `log` | 0.4 | facade clásica `log::info!`, todavía usada por crates externos |
| `env_logger` | 0.11 | implementación simple de `log` (legacy, RUST-007 propone migrar todo a tracing) |
| `tracing` | 0.1.40 | facade moderna con spans para perf-critical paths |
| `tracing-subscriber` | 0.3 | implementación de tracing (env-filter + JSON output) |
| `tracing-appender` | 0.2 | escritura asíncrona de logs a disco con rotación |
| `sentry` | 0.34 | crash reports (rustls-only, no openssl) |
| `posthog-rs` | 0.3.7 | analytics opt-in |
| `zip` | 2.1 | bundle de logs para export desde Settings → Diagnostics |

### 3. Async runtime
| Crate | Versión | Por qué |
|---|---|---|
| `tokio` | 1.32.0 | runtime async multi-thread + macros + sync + time + io-util + process + tracing |
| `tokio-util` | 0.7 | CancellationToken para abortar grabaciones |
| `async-trait` | 0.1 | abstracción sobre traits con métodos async (Provider trait) |
| `futures` | 0.3 | combinators stream/future genéricos |
| `futures-util` | 0.3 | utilities adicionales (StreamExt, SinkExt) |
| `futures-channel` | 0.3.31 | mpsc/oneshot para enviar audio chunks entre threads |

### 4. HTTP & WebSocket
| Crate | Versión | Por qué |
|---|---|---|
| `reqwest` | 0.11 | HTTP client (blocking + multipart + json + stream) |
| `tokio-tungstenite` | 0.21 | WebSocket cliente para Deepgram realtime API (con TLS nativo) |

### 5. Filesystem & paths
| Crate | Versión | Por qué |
|---|---|---|
| `dirs` | 5.0.1 | resolución cross-platform de $APPDATA, $HOME, etc. |
| `which` | 6.0.1 | localizar binario `ffmpeg` en PATH (audio/ffmpeg.rs) |

### 6. Database
| Crate | Versión | Por qué |
|---|---|---|
| `sqlx` | 0.8 | acceso a SQLite con runtime tokio + soporte chrono |

### 7. Audio capture & processing (núcleo del producto)
| Crate | Versión | Por qué |
|---|---|---|
| `cpal` | 0.15.3 | captura cross-platform de mic (parchado por rev a fork RustAudio) |
| `ebur128` | 0.1 | normalización de loudness EBU R128 (estándar broadcast) |
| `nnnoiseless` | 0.5 | reducción de ruido neural (RNNoise) |
| `silero_rs` | git rev `26a6460` | VAD (Voice Activity Detection) Silero |
| `symphonia` | 0.5.4 | decodificación AAC + MP4 (con SIMD) |
| `rubato` | 0.15.0 | sample-rate conversion (48kHz ↔ 16kHz para Whisper) |
| `ringbuf` | 0.4.8 | ring buffer lock-free para streaming entre cpal callback y main |
| `realfft` | 3.4.0 | FFT real para análisis espectral |
| `ndarray` | 0.16 | arrays N-dimensionales para procesamiento batch |
| `ffmpeg-sidecar` | git rev `33272cc` | wrapper sobre binario ffmpeg para encoding final (RUST-003 pinned) |

### 8. Speech recognition
| Crate | Versión | Por qué |
|---|---|---|
| `ort` | 2.0.0-rc.10 | ONNX Runtime para Parakeet (NVIDIA's STT model) |
| `esaxx-rs` | 0.1.10 | tokenizer base (parchado por rev en `[patch.crates-io]`) |
| `whisper-rs` | 0.13.2 | bindings whisper.cpp — **declarado por OS** con features distintas (ver abajo) |

**Nota:** Moonshine (UsefulSensors) usa un parser JSON custom (`src/moonshine_engine/`) para evitar conflictos de runtime C++ en Windows.

### 9. Concurrency primitives
| Crate | Versión | Por qué |
|---|---|---|
| `crossbeam` | 0.8.4 | channels lock-free + epoch GC |
| `dashmap` | 6.1.0 | HashMap concurrente para registro de tracks activos |

### 10. System monitoring
| Crate | Versión | Por qué |
|---|---|---|
| `sysinfo` | 0.32 | RAM disponible, CPU usage, # cores (PERF-003 — enforce RAM antes de cargar Whisper) |

### 11. Misc
| Crate | Versión | Por qué |
|---|---|---|
| `clap` | 4.3 | parser CLI para sub-binaries (whisper-server, etc.) |
| `rand` | 0.8.5 | RNG para WebSocket keys, jitter de retries |
| `regex` | 1.11.0 | parsing de outputs de ffmpeg, etc. |

### 12. Tauri 2.x core + plugins
| Crate | Versión | Por qué |
|---|---|---|
| `tauri` | 2.10.1 | core framework con `macos-private-api` + `protocol-asset` + `tray-icon` |
| `tauri-plugin-fs` | 2.4.0 | acceso filesystem (RESTRINGIDO post SEC-002) |
| `tauri-plugin-dialog` | 2.3.0 | file pickers nativos |
| `tauri-plugin-store` | 2.4.0 | persistencia de settings (clave-valor en JSON) |
| `tauri-plugin-notification` | 2.3.1 | notificaciones nativas del SO |
| `tauri-plugin-updater` | 2.10.0 | auto-updates con minisign |
| `tauri-plugin-process` | 2.3.0 | spawn/kill de procesos sidecar |
| `tauri-plugin-deep-link` | 2 | scheme `maity://` para callbacks OAuth/SSO (futuro SEC-008) |
| `tauri-plugin-single-instance` | 2 | evita múltiples instancias simultáneas |

---

## Dependencias por OS (target-specific)

### macOS (`cfg(target_os = "macos")`)
| Crate | Versión | Por qué |
|---|---|---|
| `whisper-rs` | 0.13.2 features `["raw-api", "metal", "coreml"]` | GPU Metal + CoreML auto-enabled |
| `objc` | 0.2.7 | bindings Objective-C |
| `core-graphics` | 0.23 | Quartz para captura de screen |
| `cidre` | git rev `a9587fa` features `["av"]` | bindings ScreenCaptureKit (necesario macOS 13+) |
| `dasp` | 0.11.0 | DSP utilities (audio sample manipulation) |
| `time` | 0.3 features `["formatting"]` | timestamps macOS-specific |
| `tauri-plugin-log` | 2.6.0 features `["colored"]` | logging con colores |

### Windows (`cfg(target_os = "windows")`)
| Crate | Versión | Por qué |
|---|---|---|
| `whisper-rs` | 0.13.2 features `["raw-api"]` | CPU-only por defecto; opt-in cuda/vulkan/openblas via features |
| `windows` | 0.58 con features `Win32_Media_Audio`, `Win32_System_Com`, `Win32_Foundation`, `Win32_Devices_Properties`, `Win32_System_Threading`, `Win32_Security`, `implement` | WASAPI loopback para capturar audio del sistema |

### Linux (`cfg(target_os = "linux")`)
| Crate | Versión | Por qué |
|---|---|---|
| `whisper-rs` | 0.13.2 features `["raw-api"]` | CPU-only por defecto; opt-in cuda/vulkan/hipblas via features |

---

## Dev dependencies

| Crate | Versión | Por qué |
|---|---|---|
| `tempfile` | 3.3.0 | directorios temporales para tests |
| `infer` | 0.15 | detección de tipo MIME para fixtures |
| `criterion` | 0.5.1 features `["async_tokio"]` | benchmarks (no se ejecutan en CI todavía — ver QA-005) |
| `memory-stats` | 1.0 | medición de uso de memoria en stress tests |
| `strsim` | 0.10.0 | similitud de strings para tests de transcripción |

---

## Patches (RUST-003)

```toml
[patch.crates-io]
cpal     = { git = "https://github.com/RustAudio/cpal", rev = "51c3b43" }
esaxx-rs = { git = "https://github.com/thewh1teagle/esaxx-rs.git", rev = "3c8ac57d245ab328f7c71953b7c116a8d1d5498f" }
```

**Por qué pinneados a rev:**
- `cpal`: usamos un fork de RustAudio con un fix de WASAPI loopback no mergeado upstream todavía
- `esaxx-rs`: usamos un branch con dynamic MSVC linking para compilar correcto en Windows

**Política**: actualizar estos rev periódicamente (al menos cada 6 meses) y siempre tras advisories de supply-chain.

---

## Feature flags (`[features]`)

```toml
default = ["platform-default"]
platform-default = []  # Auto-enables best backend per platform via target deps below
```

### Manual GPU acceleration (override defaults)
| Feature | Activa | Cuándo usar |
|---|---|---|
| `metal` | `whisper-rs/metal` | macOS Apple Metal (auto on macOS) |
| `coreml` | `whisper-rs/coreml` | macOS CoreML (auto on macOS) |
| `cuda` | `whisper-rs/cuda` | NVIDIA GPU (Windows/Linux) |
| `vulkan` | `whisper-rs/vulkan` | AMD/Intel GPU (Windows/Linux) |
| `hipblas` | `whisper-rs/hipblas` | AMD ROCm (Linux) |
| `openblas` | `whisper-rs/openblas` | CPU optimized (requiere BLAS_INCLUDE_DIRS) |
| `openmp` | `whisper-rs/openmp` | OpenMP parallelization |

### Comandos típicos

```bash
# Default per platform
cargo build --release

# Windows con CUDA
cargo build --release --features cuda

# Linux con AMD ROCm
cargo build --release --features hipblas

# Tests CPU-only (B2B-real, sin GPU)
cargo test --manifest-path frontend/src-tauri/Cargo.toml --lib
```

---

## Cómo añadir una nueva dependencia

1. **Decide la categoría** (1-12 arriba). Si no encaja, abre issue antes.
2. **Verifica que no exista ya** (incluyendo en target-specific blocks).
3. **Pin a versión exacta o caret**:
   - Crates de crates.io: `"1.2.3"` (caret implícito)
   - Crates git: **siempre** `rev = "<sha completo>"`, jamás `branch = "main"` (RUST-003)
4. **Comenta el "por qué"** al lado del Cargo.toml entry — uno debe entender el propósito sin abrir el código.
5. **Actualiza este documento** con la entrada en la categoría correspondiente.
6. **Run `cargo check --lib`** para validar antes de commit.

---

## Histórico de cambios mayores

| Fecha | Cambio | Tracking |
|---|---|---|
| 2026-04-08 | RUST-011: refactor completo, eliminados 6 duplicados, organizado por categorías | PR #18 |
| 2026-04-08 | RUST-003: ffmpeg-sidecar pinned a rev (era branch=main) | PR #16 |
| 2026-04-07 | RUST-010: cargo fmt --all aplicado a 123 archivos del workspace | PR #5 |
| 2026-04-07 | RUST-008: fix `mul_f32(2.0)` → `* 2` (precision float) | PR #8 |
| 2026-04-07 | QA-008/RUST-009: AudioChunk test fix (compile + assertion) | PR #6 |

---

## Próximos hallazgos relacionados (pending)

- **RUST-001** (critical, v3.0): refactor 126 `unwrap()` → `Result` propagation
- **RUST-002** (high, v2.0): `std::panic::set_hook` con report a Sentry
- **RUST-004** (medium, v2.0): bump `reqwest` 0.11 → 0.12 con rustls-tls
- **RUST-005** (medium, v2.0): refactor `Mutex<WsStream>` → `SplitSink` + mpsc en deepgram_provider
- **RUST-006** (medium, v2.0): separar capabilities Tauri por ventana
- **RUST-007** (low, v1.0): eliminar `env_logger`, unificar todo en `tracing`

---

*Última actualización: 2026-04-08 (RUST-011)*

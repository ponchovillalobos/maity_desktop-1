# CLAUDE.md

Este archivo proporciona orientacion a Claude Code al trabajar con este repositorio.

## Descripcion del Proyecto

**Meetily (Maity Desktop)** es un asistente de reuniones con IA enfocado en privacidad que captura, transcribe y resume reuniones localmente. Dos componentes principales:

1. **Frontend**: App de escritorio Tauri (Rust + Next.js + TypeScript)
2. **Backend**: Servidor FastAPI para persistencia y resumenes LLM (Python)

### Stack Tecnologico
- **App de Escritorio**: Tauri 2.x (Rust) + Next.js 14 + React 18
- **Procesamiento de Audio**: Rust (cpal, whisper-rs, mezcla de audio profesional)
- **Transcripcion**: Whisper.cpp (local, GPU) + Deepgram (nube, opcional)
- **Backend API**: FastAPI + SQLite (aiosqlite) — modulo DB en `backend/app/db/`
- **Integracion LLM**: Ollama (local), Claude, Groq, OpenRouter

## Sistema de Asamblea de Expertos + Auto-Mejora

Este fork incluye un sistema completo de Asamblea de Expertos donde 12 perspectivas auditan el proyecto y alimentan un ciclo de mejora continua. **Antes de proponer/aplicar cualquier cambio, consultar la asamblea via `GET http://127.0.0.1:8770/api/consult/{EXP-ID}`** y registrar los veredictos en el commit.

Documentación completa: **`docs/ASSEMBLY.md`**

### Prioridades del proyecto (en orden estricto)
1. **Cero pérdidas de datos** — ninguna grabación, transcript o summary debe perderse jamás
2. **Velocidad de transcripción post-sesión casi instantánea** — al colgar, el resumen final está disponible inmediatamente
3. Velocidad/latencia durante grabación en vivo
4. Resto

### Componentes
- **`scripts/assembly_data.json`** — 12 expertos / 84 hallazgos accionables
- **`scripts/portal.py`** — Portal FastAPI puerto 8770 con sidebar SPA, auto-refresh 8s, `/api/consult`, `/api/activity`, dashboard live de métricas
- **`.claude/skills/improve/SKILL.md`** + **`.claude/commands/improve-pr.md`** — Workflow 1 branch + 1 commit + 1 PR por hallazgo
- **`.claude/agents/{auditor,validator,janitor}/AGENT.md`** — Agentes especializados
- **`memory/`** — IMPROVEMENT_LOG / FAILED_ATTEMPTS / METRICS_HISTORY / ANALYSIS_STATE / build_logs/

### Lanzar el portal
```bash
python scripts/portal.py
# o detached en Windows:
powershell -Command "Start-Process python -ArgumentList 'scripts/portal.py' -WindowStyle Hidden"
# → http://127.0.0.1:8770
```

### Quality gates obligatorios antes de cualquier commit en `improve/*`
```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd frontend && npm run lint && npm run typecheck && npm test
```

## Skills (Slash Commands)

### `/improve` y `/improve-pr`
Ciclo de auto-mejora continua. `/improve-pr` aísla cada hallazgo en su propio branch + PR, consulta a la asamblea, ejecuta quality gates, y actualiza memoria. Ver `.claude/commands/improve-pr.md`.

### `/build [patch|minor|major]`
Build firmado de produccion con bump automatico de version semver. Lee signing keys de `frontend/.env`, actualiza la version en 3 archivos (`tauri.conf.json`, `package.json`, `Cargo.toml`), y ejecuta `pnpm run tauri:build` con las credenciales de firma. Definicion: `.claude/skills/build/SKILL.md`

## Comandos Esenciales de Desarrollo

### Frontend (App de Escritorio Tauri) — Ubicacion: `/frontend`

```bash
# Desarrollo en macOS
./clean_run.sh              # Build limpio y ejecutar con logging info
./clean_run.sh debug        # Ejecutar con logging debug

# Desarrollo en Windows
clean_run_windows.bat       # Build limpio y ejecutar

# Comandos Manuales
pnpm install                # Instalar dependencias
pnpm run dev                # Servidor dev Next.js (puerto 3118)
pnpm run tauri:dev          # Modo desarrollo completo Tauri
pnpm run tauri:build        # Build de produccion (release)
pnpm run tauri:build:debug  # Build debug (mas rapido, para verificar)

# Builds especificos por GPU
pnpm run tauri:dev:metal    # macOS Metal GPU
pnpm run tauri:dev:cuda     # NVIDIA CUDA
pnpm run tauri:dev:vulkan   # AMD/Intel Vulkan
pnpm run tauri:dev:cpu      # Solo CPU (sin GPU)
```

### Backend (Servidor FastAPI) — Ubicacion: `/backend`

```bash
# macOS
./build_whisper.sh small              # Compilar Whisper con modelo 'small'
./clean_start_backend.sh              # Iniciar servidor FastAPI (puerto 5167)

# Windows
build_whisper.cmd small               # Compilar Whisper con modelo
clean_start_backend.cmd               # Iniciar servidor

# Docker (Multiplataforma)
./run-docker.sh start --interactive   # macOS/Linux
.\run-docker.ps1 start -Interactive   # Windows
```

**Modelos Whisper**: `tiny`, `base`, `small`, `medium`, `large-v3`, `large-v3-turbo` (variantes `.en` disponibles)

### Endpoints
- **API Backend**: http://localhost:5167 (opcional, para persistencia y resumenes LLM)
- **Documentacion Backend**: http://localhost:5167/docs
- **Frontend Dev**: http://localhost:3118

## Arquitectura de Alto Nivel

```
┌─────────────────────────────────────────────────────────────────┐
│                Frontend (App de Escritorio Tauri)                │
│  ┌──────────────────┐  ┌─────────────────┐  ┌────────────────┐ │
│  │   UI Next.js     │  │  Backend Rust   │  │ Motor Whisper  │ │
│  │  (React/TS)      │<->│  (Audio + IPC)  │<->│  (STT Local)   │ │
│  └──────────────────┘  └─────────────────┘  └────────────────┘ │
└─────────┬──────────────────────────────────────────────────────┘
          │ HTTP/WebSocket (opcional)
          ↓
┌─────────────────────────────────────────────────────────────────┐
│              Backend (FastAPI + SQLite + LLM providers)          │
└─────────────────────────────────────────────────────────────────┘
```

### Pipeline de Procesamiento de Audio (Comprension Critica)

El sistema de audio tiene **tres rutas paralelas**:

```
Audio Crudo (Microfono + Sistema)
         ↓
    AudioPipelineManager (pipeline.rs)
    ┌────────┬──────────────┬──────────────────┐
    ↓        ↓              ↓                  ↓
Grabacion   Transcripcion  Transcripcion Nube
Stereo L/R  VAD local      Deepgram WebSocket
    ↓        ↓              ↓
RecordingSaver WhisperEngine DeepgramProvider
```

**Puntos Clave**:
- **Grabacion stereo**: Audio entrelazado (L=microfono/usuario, R=sistema/interlocutor) para separacion de hablantes
- **VAD dual-canal**: Procesadores VAD independientes para microfono (`mic_vad_processor`) y sistema (`sys_vad_processor`)
- **Atribucion de hablante**: `DeviceType` (Microphone/System) se captura ANTES de enviar al motor de transcripcion, mapeando `Microphone->"user"` y `System->"interlocutor"`
- **Ring Buffer de mezcla**: Acumula muestras hasta ventanas alineadas de 50ms; ducking RMS evita que audio del sistema ahogue al microfono

### Estructura del Modulo de Audio

```
audio/
├── devices/                    # Descubrimiento y configuracion de dispositivos
│   ├── discovery.rs           # list_audio_devices, trigger_audio_permission
│   ├── microphone.rs          # default_input_device
│   ├── speakers.rs            # default_output_device
│   ├── configuration.rs       # Tipos AudioDevice, parsing
│   └── platform/              # Implementaciones por plataforma
│       ├── windows.rs         # Logica WASAPI
│       ├── macos.rs           # Logica ScreenCaptureKit
│       └── linux.rs           # Logica ALSA/PulseAudio
├── capture/                   # Captura de streams de audio
│   ├── microphone.rs          # Stream de captura de microfono
│   ├── system.rs              # Stream de captura de audio del sistema
│   └── core_audio.rs          # Integracion ScreenCaptureKit macOS
├── transcription/             # Motor de transcripcion
│   ├── engine.rs              # Gestion de motores (Whisper + Parakeet)
│   ├── worker.rs              # Pool de workers de transcripcion
│   ├── deepgram_provider.rs   # Proveedor Deepgram (nube, WebSocket)
│   └── deepgram_commands.rs   # Comandos Tauri para proxy config
├── pipeline.rs                # Mezcla de audio, VAD y distribucion
├── recording_manager.rs       # Coordinacion de grabacion de alto nivel
├── recording_commands.rs      # Interfaz de comandos Tauri
├── recording_saver.rs         # Escritura de archivos de audio
├── incremental_saver.rs       # Guardado incremental con checkpoints (30s)
└── encode.rs                  # Codificacion FFmpeg (PCM -> AAC/MP4)
```

**Al trabajar en funcionalidades de audio**:
- Deteccion de dispositivos -> `devices/discovery.rs` o `devices/platform/{windows,macos,linux}.rs`
- Microfono/altavoces -> `devices/microphone.rs` o `devices/speakers.rs`
- Captura de audio -> `capture/microphone.rs` o `capture/system.rs`
- Mezcla/procesamiento -> `pipeline.rs`
- Flujo de grabacion -> `recording_manager.rs` + `recording_saver.rs` + `incremental_saver.rs`
- Transcripcion local -> `transcription/engine.rs` + `transcription/worker.rs`
- Transcripcion nube -> `transcription/deepgram_provider.rs`

### Comunicacion Rust <-> Frontend

Comandos via `invoke()` (Frontend->Rust), Eventos via `emit()`/`listen()` (Rust->Frontend). Todos los comandos registrados en `lib.rs`. La implementacion delega a modulos en `audio/recording_commands.rs`, `audio/transcription/deepgram_commands.rs`, etc.

**Patron de estado**: Comandos Tauri actualizan estado Rust -> Emiten eventos -> Listeners del frontend actualizan estado React -> El contexto se propaga a los componentes.

### Gestion de Modelos Whisper

**Ubicaciones de Almacenamiento**:
- **Desarrollo**: `frontend/models/`
- **Produccion (macOS)**: `~/Library/Application Support/com.maity.ai/models/`
- **Produccion (Windows)**: `%APPDATA%\com.maity.ai\models\`

Los modelos se cargan una vez y se cachean. Cambiar modelos requiere reinicio de la app o descarga/recarga manual. Auto-deteccion de GPU (Metal/CUDA/Vulkan) con fallback a CPU.

## Patrones Criticos de Desarrollo

### Seguridad de Hilos y Estado Compartido
- `Arc<RwLock<T>>` para estado compartido entre tareas async, `Arc<AtomicBool>` para flags simples
- Mutex con `.lock().map_err()`, **nunca** `.lock().unwrap()` — evita panics por envenenamiento de mutex
- Ver `recording_state.rs` para el patron de referencia

### Logging Consciente del Rendimiento
- `perf_debug!()`/`perf_trace!()` para logging en rutas criticas — costo cero en builds de release (definidos en `lib.rs`)
- `AudioMetricsBatcher` (batch_processor.rs) para agrupar metricas de audio
- `AudioBufferPool` (buffer_pool.rs) para pre-asignar buffers

### Rendimiento de Audio
- El filtrado VAD reduce la carga de Whisper en ~70% (solo procesa voz)
- El guardado incremental con checkpoints de 30s previene perdida de datos por crashes
- Features de Cargo para GPU: `--features cuda`, `--features vulkan`, `--features metal`

## Depuracion

```bash
# Habilitar logging verbose de audio
RUST_LOG=app_lib::audio=debug ./clean_run.sh                    # macOS
$env:RUST_LOG="debug"; ./clean_run_windows.bat                   # Windows

# DevTools
# macOS: Cmd+Shift+I  |  Windows: Ctrl+Shift+I
```

**ChunkLoadError Recovery** (modo desarrollo): Script inline en `layout.tsx` (strategy `beforeInteractive`) detecta `ChunkLoadError` y recarga automaticamente (max 3 intentos). Si persiste, reiniciar `pnpm run tauri:dev`. Componente backup: `ChunkErrorRecovery.tsx`.

**Metricas del Pipeline**: Tamanos de buffer, tasa VAD, chunks descartados, backpressure del canal de transcripcion — visibles en la consola de desarrollador durante grabacion.

## Plataformas y GPU

| Plataforma | Captura de Audio | GPU | Dependencias Clave |
|---|---|---|---|
| macOS 13+ | ScreenCaptureKit + BlackHole | Metal+CoreML (auto) | Permisos mic + screen recording |
| Windows | WASAPI loopback | CUDA (NVIDIA) o Vulkan (AMD/Intel) | VS Build Tools 2022, LLVM (`winget install LLVM.LLVM`), FFmpeg |
| Linux | ALSA/PulseAudio | CUDA o Vulkan | cmake, llvm, libomp |

**LLVM en Windows**: Requerido por `whisper-rs-sys` (bindgen necesita `libclang.dll`). Configurar `LIBCLANG_PATH=C:\Program Files\LLVM\bin`.

## Configuracion Multiplataforma

Tauri 2.x soporta configs por plataforma que se **mergean** con el base via JSON Merge Patch (RFC 7396):

```
frontend/src-tauri/
├── tauri.conf.json              # Config BASE compartida (todas las plataformas)
├── tauri.macos.conf.json        # Overrides para macOS (merge automatico)
├── entitlements.plist           # Entitlements para desarrollo/distribucion directa
├── entitlements-appstore.plist  # Entitlements para App Store (sandbox)
└── Info.plist                   # Permisos macOS (NUNCA eliminar las *UsageDescription)
```

### Reglas CRITICAS

1. **`tauri.conf.json` es la config BASE compartida** — NO modificar para una sola plataforma. Usar `tauri.{platform}.conf.json` para overrides.
2. **NUNCA cambiar el `identifier`** (`com.maity.ai`) — Rompe datos de usuarios existentes (SQLite, modelos, config) porque el OS almacena datos por identifier.
3. **NUNCA eliminar permisos de `Info.plist`** (`NSMicrophoneUsageDescription`, `NSScreenCaptureUsageDescription`, `NSAudioCaptureUsageDescription`) — macOS los requiere para mostrar el dialogo de permisos.
4. **NUNCA commitear artefactos de build** (`.pkg`, `.dmg`, `.msi`, `*-setup.exe`) — usar GitHub Releases.
5. **NUNCA eliminar la config `bundle.windows`** del `tauri.conf.json` base — contiene signing, idioma de instaladores, etc.
6. **El sistema `visible: false` + `app-ready`** en `lib.rs` y `layout.tsx` es intencional — evita pantalla negra al inicio. No eliminar.

### CI/CD (GitHub Actions)

Workflows en `.github/workflows/`: `build-windows.yml` (DigiCert HSM signing), `build-macos.yml` (Apple notarization), `build-linux.yml` (deb + AppImage), `release.yml`.

## Deepgram via Cloudflare Worker Proxy

La transcripcion en la nube usa Deepgram a traves de un Cloudflare Worker proxy. **La API key de Deepgram nunca llega al cliente**.

**Config por defecto**: Nova-3, idioma `es-419` (espanol latinoamericano). Persiste en tabla `transcript_settings` de SQLite.

**Modelos disponibles**: `nova-3` (recomendado), `nova-2`, `nova-2-phonecall`, `nova-2-meeting`
**Idiomas**: `es-419` (LATAM), `es` (Espana), `en`, `multi` (auto-deteccion)

| Archivo | Descripcion |
|---------|-------------|
| `frontend/src/lib/deepgram.ts` | Cliente TS para obtener proxy config de Vercel API |
| `frontend/src/hooks/useRecordingStart.ts` | Obtiene proxy config antes de iniciar grabacion |
| `frontend/src-tauri/src/audio/transcription/deepgram_commands.rs` | Comandos Tauri para proxy config en cache |
| `frontend/src-tauri/src/audio/transcription/deepgram_provider.rs` | Proveedor que conecta via proxy WebSocket |
| `frontend/src-tauri/src/audio/transcription/engine.rs` | Inicializacion del motor de transcripcion |

**Gotchas de seguridad**:
- JWT tiene TTL de 5 minutos, se valida solo al conectar el WebSocket
- Conexiones activas sobreviven mas alla del TTL (validacion solo al inicio)
- Ambas conexiones WS (mic + system) usan el mismo JWT simultaneamente
- Reconexion despues de expirar el JWT (>5 min) fallara gracefully
- Usuario debe estar autenticado con Supabase (login con Google)

## Sistema de Roles (Developer vs Usuario Regular)

- **Developers**: Emails con dominio `@asertio.mx` o `@maity.cloud` -> interfaz completa
- **Usuarios regulares**: Interfaz restringida (sin Gamificacion/Conversaciones en sidebar, settings limitados, transcripcion forzada a Deepgram nova-3 es-419)
- Archivos: `lib/roles.ts`, `hooks/useUserRole.ts`, `Sidebar/index.tsx`, `settings/page.tsx`, `ConfigContext.tsx`

## Restricciones Importantes

1. **Frecuencia de muestreo**: El pipeline espera 48kHz consistente. El remuestreo ocurre al momento de la captura.
2. **Audio por plataforma**: macOS requiere ScreenCaptureKit (13+) + permiso de screen recording. Windows WASAPI modo exclusivo puede conflictuar con otras apps.
3. **Grabacion stereo**: Se guarda como audio stereo entrelazado (L=mic, R=sistema). El `IncrementalAudioSaver` maneja checkpoints cada 30s con `channels=2`.
4. **Rutas de archivos**: Usar APIs de rutas de Tauri (`downloadDir`, etc.) para compatibilidad multiplataforma. Nunca hardcodear rutas.
5. **Permisos de audio**: macOS requiere tanto microfono COMO grabacion de pantalla para audio del sistema.

## Convenciones del Repositorio

- **Manejo de Errores**: Rust usa `anyhow::Result`, frontend usa try-catch con mensajes amigables
- **Nomenclatura audio**: Siempre "microphone" y "system" (no "input"/"output")
- **Ramas de Git**: `main` (releases), `fix/*`, `enhance/*`, `feat/*`
- **Commits**: Prefijos estandar (`feat:`, `fix:`, `docs:`, `refactor:`, `style:`, `test:`, `chore:`) con descripcion en espanol

---

## Protocolo Guardian - Modo Protegido

### 1. Respaldo Pre-Cambio (Solo Alto Riesgo)

Crear rama de backup **antes** de cambios de alto riesgo:
- Refactoring grande (>3 archivos o >200 lineas)
- Cambios en pipeline de audio (`pipeline.rs`, `recording_manager.rs`)
- Cambios en motor de transcripcion (`engine.rs`, `worker.rs`)
- Modificaciones a `lib.rs` o al sistema de comandos Tauri

```bash
git checkout -b backup/{fecha}-{descripcion-corta}
git checkout -    # Volver a la rama de trabajo
```

**NO se requiere backup para**: edits menores, correcciones puntuales, cambios de UI, actualizaciones de dependencias.

### 2. Protocolo de Compilacion (OBLIGATORIO — SIN EXCEPCIONES)

**REGLA ABSOLUTA**: Despues de CADA cambio de codigo, se DEBE ejecutar el build completo integrado de Tauri. NUNCA se debe entregar, hacer commit, ni reportar completado sin que el build haya pasado con exit code 0.

```bash
cd frontend && pnpm run tauri:build:debug     # OBLIGATORIO - Build integrado Tauri (debug)
```

Este comando ejecuta: `pnpm build` (Next.js) -> `cargo build` (Rust, debug) -> empaqueta frontend + backend en un ejecutable funcional.

**Criterio de exito**: Exit code 0. Si termina con exit code != 0, el build NO paso — corregir antes de entregar.

**Nota sobre firma local**: El script `tauri-auto.js` maneja la ausencia de `TAURI_SIGNING_PRIVATE_KEY` en desarrollo local. Si la compilacion es exitosa pero falta la clave de firma, el script reporta un warning y sale con code 0 (comportamiento esperado).

**PROHIBIDO**:
- Usar `cargo build` como build final (solo compila Rust, no integra frontend)
- Hacer commit sin build exitoso (exit code 0)
- Reportar tarea completada sin build exitoso

**Artefactos debug**: `target/debug/maity-desktop.exe`, `target/debug/bundle/msi/Maity_*.msi`, `target/debug/bundle/nsis/Maity_*-setup.exe`

**Build de produccion** (solo para releases): `cd frontend && pnpm run tauri:build`

### 3. Alerta de Cambios Peligrosos

Si el usuario solicita alguna de estas acciones, **advertir y proponer enfoque incremental**:
- Eliminar archivos completos del sistema de audio
- Reescribir modulos enteros desde cero
- Cambiar la arquitectura del pipeline de audio
- Modificar el formato de comunicacion Rust <-> Frontend

Formato: > **Cambio de alto riesgo detectado**: [descripcion]. Este cambio afecta [componentes]. Propongo un enfoque incremental: [pasos].

### 4. Formato de Commits

Prefijos estandar con descripcion en espanol: `feat:`, `fix:`, `docs:`, `refactor:`, `style:`, `test:`, `chore:`

Ejemplo: `feat: agregar grabacion stereo dual-canal (L=mic, R=sistema)`

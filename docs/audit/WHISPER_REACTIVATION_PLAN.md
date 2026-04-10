# Whisper Reactivation Plan (Spanish-first)

**Fecha:** 2026-04-08
**Objetivo:** Volver a activar Whisper como STT local por defecto (multilingüe, español), reemplazando a Parakeet (English-only).
**Resultado de auditoría:** El código de Whisper está **intacto y production-ready**. La desactivación es cosmética: solo 3 puntos en `lib.rs` y 1 en `engine.rs` gatean el arranque. No hacen falta fixes de ingeniería, solo flipping de defaults + un preload de startup (copia del patrón de Parakeet).

---

## 1. Modelos Whisper registrados

Fuente: `frontend/src-tauri/src/whisper_engine/whisper_engine.rs:234-333` (array `model_configs`)
URL base: `https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{name}.bin` (código en `whisper_engine.rs:1207-1221`)

| Modelo              | Archivo                       | Size MB | Multilingüe | Notas                                      |
|---------------------|-------------------------------|---------|-------------|--------------------------------------------|
| `tiny`              | ggml-tiny.bin                 | 39      | Sí          | Muy rápido, precisión baja                 |
| `base`              | ggml-base.bin                 | 142     | Sí          | Balance velocidad/precisión               |
| `small`             | ggml-small.bin                | 466     | **Sí**      | **Recomendado para español LATAM**        |
| `medium`            | ggml-medium.bin               | 1420    | Sí          | Profesional, lento                         |
| `large-v3-turbo`    | ggml-large-v3-turbo.bin       | 809     | Sí          | Mejor precisión con velocidad razonable   |
| `large-v3`          | ggml-large-v3.bin             | 2870    | Sí          | Máxima precisión, muy lento                |
| `tiny-q5_0`         | ggml-tiny-q5_0.bin            | 26      | Sí          | Cuantizado                                 |
| `base-q5_0`         | ggml-base-q5_0.bin            | 85      | Sí          | Cuantizado                                 |
| `small-q5_0`        | ggml-small-q5_0.bin           | 280     | Sí          | Cuantizado, buen trade-off                 |
| `medium-q5_0`       | ggml-medium-q5_0.bin          | 852     | Sí          | Cuantizado                                 |
| `large-v3-turbo-q5_0` | ggml-large-v3-turbo-q5_0.bin | 574     | Sí          | Cuantizado — **alternativa premium**       |
| `large-v3-q5_0`     | ggml-large-v3-q5_0.bin        | 1050    | Sí          | Cuantizado                                 |

**Importante:** NINGÚN modelo `.en` está registrado. Todos los modelos del array son multilingües de forma nativa (whisper.cpp/HF publica `.en` como variante aparte; aquí no se incluyó). Por lo tanto, **cualquiera transcribe español sin cambios de código**.

**Recomendación default:** `small` (466 MB, multilingüe, palabras/min aceptables en español, corre en CPU Windows 8GB sin drama). Alternativa premium opcional (descargable bajo demanda desde settings): `large-v3-turbo-q5_0` (574 MB, precisión casi large-v3 con 2-3× más velocidad).

---

## 2. Descarga, ubicación y flujo

- **Directorio:** `app_data_dir/models/` — seteado por `whisper_engine::commands::set_models_directory()` (`commands.rs:14-34`), llamado desde `lib.rs:515`. En Windows corresponde a `%APPDATA%\com.maity.ai\models\`.
- **Descarga on-demand:** `WhisperEngine::download_model(name, progress_cb)` (`whisper_engine.rs:1175`). Emite eventos de progreso; no se auto-descarga en startup actualmente — el usuario debe pulsarlo desde `WhisperModelManager.tsx`.
- **Validación/corrupción:** `discover_models()` (`whisper_engine.rs:230`) verifica tamaño ≥90% esperado + `validate_model_file()`. Marca `Missing`/`Corrupted`/`Available`.
- **Cache de context:** `Arc<RwLock<Option<WhisperContext>>>` — se carga una vez por modelo, se unloadea al cambiar (`load_model`, `whisper_engine.rs:423-519`).

---

## 3. API del engine (language + thread safety)

- `WhisperEngine::transcribe_audio_with_confidence(audio, language: Option<String>)` — `whisper_engine.rs:708`. Acepta `None`/`"auto"` (auto-detect), `"auto-translate"` (traduce a inglés) o código ISO (`"es"`, `"en"`, etc.). Se pasa a `params.set_language(...)` en línea 737.
- **Wiring actual del language:** `worker.rs:942` llama `crate::get_language_preference_internal()` que retorna el contenido de `LANGUAGE_PREFERENCE` — un `LazyLock<Mutex<String>>` inicializado a `"es"` en `lib.rs:65-66`. Perfecto para español por default.
- **Nota DB:** `transcript_settings.language` default `'es-419'` (migración `20260205000000_add_transcript_language.sql`) — pero ese campo lo consume Deepgram, no Whisper. Whisper toma el `LANGUAGE_PREFERENCE` global (un cambio futuro: hidratar `LANGUAGE_PREFERENCE` desde la DB al startup, pero no es bloqueante — "es" ya está correcto para whisper.cpp).
- **Thread-safety:** `WhisperEngine` se comparte como `Arc<WhisperEngine>` en múltiples tasks (worker.rs, engine.rs, commands.rs). El `WhisperContext` de `whisper-rs 0.13.2` es `Send+Sync`. Se puede compartir entre la task de preload y la task de recording sin cambios.

---

## 4. Features de compilación (GPU)

Fuente: `frontend/src-tauri/Cargo.toml:53-66, 181-207`

- `[features] default = ["platform-default"]` — **platform-default está vacío**, las features de whisper-rs se activan vía `[target.'cfg(...)'.dependencies]`.
- **macOS:** `whisper-rs` con `["raw-api", "metal", "coreml"]` — GPU activa automáticamente.
- **Windows:** `whisper-rs` con `["raw-api"]` únicamente — **CPU por defecto**. CUDA/Vulkan/OpenBLAS solo si el usuario compila con `--features cuda` / `--features vulkan`. Para el build actual de CI/producción Windows, Whisper corre en CPU.
- **Linux:** igual que Windows (CPU default, opt-in cuda/vulkan/hipblas).

**Implicación:** Activar Whisper por default en Windows implica que los usuarios con hardware modesto notarán Whisper `small` corriendo en CPU (~1× realtime o menos). Para usuarios con GPU NVIDIA habría que proveer un build `--features cuda`. Esto se amortigua con el modelo `small` (466 MB) o `base` (142 MB).

---

## 5. Estado actual del dispatch

- `engine.rs:147-174` — branch `"localWhisper"` funcional: invoca `whisper_init()` y `whisper_validate_model_ready_with_config()`, ambas existentes (`commands.rs:42, 226`).
- `engine.rs:465-469` — branch `_` fallback también inicializa Whisper vía `get_or_init_whisper(app)` (línea 504), que lee `transcript_config.model` y llama `engine.load_model(name)`.
- `engine.rs:624, 632` — **ya hace fallback a `"small"` si la DB no tiene modelo configurado**. Perfecto.
- Worker dispatch `worker.rs:940-980` — ya pasa `language` al engine. Sin cambios.

**Veredicto:** toda la ruta `localWhisper → validate → load → transcribe` está lista.

---

## 6. UI de selección en frontend

- `frontend/src/components/transcript/TranscriptSettings.tsx:27, 95, 189-191` — select con opción **"🏠 Whisper Local (Alta Precisión)"** (value=`localWhisper`) ya presente. El state default del select de modelo Whisper es `'small'` (línea 27). Si el usuario elige `localWhisper`, se guarda con modelo `small`.
- `frontend/src/components/models/WhisperModelManager.tsx` — ya existe, permite descargar/eliminar cualquiera de los 12 modelos.
- **No hacen falta cambios de UI** si aceptamos que el usuario pueda seguir eligiendo Parakeet manualmente. Si queremos esconder Parakeet del selector, borrar línea 189 de `TranscriptSettings.tsx`.

---

## 7. Plan de activación paso a paso

### Paso 1 — Cambiar default del provider en Rust (3 ubicaciones)

**Archivo:** `frontend/src-tauri/src/lib.rs`

- **Línea 532:** `Ok(None) => "parakeet".to_string(),` → `Ok(None) => "localWhisper".to_string(),`
- **Línea 535:** `"parakeet".to_string()` → `"localWhisper".to_string()` (dentro del `Err` arm)
- **Línea 540:** `"parakeet".to_string()` → `"localWhisper".to_string()` (cuando `AppState` no disponible)

**Archivo:** `frontend/src-tauri/src/audio/transcription/engine.rs`

- **Líneas 123-128:** el bloque `"📝 No transcript config found, defaulting to parakeet"` — cambiar el literal a `"localWhisper"` y el model a `"small"` (aplicar misma edición en **líneas 133-138**, **273-279**, **283-289** — son 4 lugares idénticos).

### Paso 2 — Cambiar default de modelo para Whisper

**Archivo:** `frontend/src-tauri/src/audio/transcription/engine.rs:624, 632`
Ya es `"small"`. No cambiar.

**Archivo:** `frontend/src/components/transcript/TranscriptSettings.tsx:27`
Ya es `'small'`. No cambiar. (Opcionalmente promover a `'large-v3-turbo-q5_0'` si queremos premium-first.)

### Paso 3 — Preload de Whisper en startup (reemplazar bloque Parakeet)

**Archivo:** `frontend/src-tauri/src/lib.rs:565-652`

Diff conceptual:

```rust
// ANTES (lib.rs:580-652 aprox): toda la sección "Whisper DESACTIVADO" + preload Parakeet
// DESPUÉS:

log::info!("PERF-005: Whisper local ACTIVO (STT multilingüe default)");

// Inicializar Whisper
if let Err(e) = whisper_engine::commands::whisper_init().await {
    log::error!("Failed to initialize Whisper engine: {}", e);
} else {
    // Leer modelo configurado o fallback a "small"
    let whisper_model_name = {
        let state = app_handle_for_config.try_state::<crate::state::AppState>();
        if let Some(app_state) = state {
            let pool = app_state.db_manager.pool();
            match crate::database::repositories::setting::SettingsRepository::get_transcript_config(pool).await {
                Ok(Some(cfg)) if cfg.provider == "localWhisper" => cfg.model.clone(),
                _ => "small".to_string(),
            }
        } else {
            "small".to_string()
        }
    };

    log::info!("PERF-005: preloading Whisper model '{}' at startup", whisper_model_name);
    let _ = app_handle_for_config.emit(
        "whisper-model-preload-started",
        serde_json::json!({ "modelName": &whisper_model_name }),
    );

    let engine_opt = {
        let guard = crate::whisper_engine::commands::WHISPER_ENGINE.lock().unwrap();
        guard.as_ref().cloned()
    };

    if let Some(engine) = engine_opt {
        // Discover first (igual que Parakeet)
        if let Err(e) = engine.discover_models().await {
            log::warn!("PERF-005: whisper discover_models failed: {}", e);
        }

        // Auto-download si falta el modelo default
        let models = engine.discover_models().await.unwrap_or_default();
        let needs_download = models.iter().any(|m| {
            m.name == whisper_model_name
                && matches!(m.status, crate::whisper_engine::ModelStatus::Missing)
        });

        if needs_download {
            log::info!("PERF-005: Whisper model '{}' missing, downloading…", whisper_model_name);
            let app_for_progress = app_handle_for_config.clone();
            let progress_cb: Box<dyn Fn(u8) + Send> = Box::new(move |pct| {
                let _ = app_for_progress.emit(
                    "whisper-model-download-progress",
                    serde_json::json!({ "progress": pct }),
                );
            });
            if let Err(e) = engine.download_model(&whisper_model_name, Some(progress_cb)).await {
                log::error!("PERF-005: Whisper auto-download failed: {}", e);
            } else {
                let _ = engine.discover_models().await;
            }
        }

        match engine.load_model(&whisper_model_name).await {
            Ok(_) => {
                log::info!("PERF-005: Whisper '{}' preloaded", whisper_model_name);
                let _ = app_handle_for_config.emit(
                    "whisper-model-preload-completed",
                    serde_json::json!({ "modelName": &whisper_model_name }),
                );
            }
            Err(e) => {
                log::warn!("PERF-005: Whisper preload failed: {}", e);
                let _ = app_handle_for_config.emit(
                    "whisper-model-preload-failed",
                    serde_json::json!({ "error": e.to_string(), "modelName": &whisper_model_name }),
                );
            }
        }
    }
}

// Parakeet: dejar init opcional SOLO si el usuario lo seleccionó manualmente
if transcript_provider == "parakeet" {
    log::info!("Parakeet selected by user, initializing fallback");
    let _ = parakeet_engine::commands::parakeet_init().await;
}
```

### Paso 4 — Auto-download en primer launch

Cubierto dentro del Paso 3 (bloque `needs_download`). El evento `whisper-model-download-progress` debe ser listenable desde el frontend — ya hay `DownloadProgressToast.tsx`, solo hay que suscribirlo a ese nuevo evento (agregar 6-8 líneas en ese componente).

### Paso 5 — Frontend UI (cambios mínimos opcionales)

- **`DownloadProgressToast.tsx`:** agregar listener del evento `whisper-model-download-progress` / `whisper-model-preload-started/completed/failed` (copiar la lógica del equivalente `parakeet-*`).
- **`useParakeetAutoDownload.ts`:** crear `useWhisperAutoDownload.ts` hermano, o simplemente confiar en el preload del backend.
- **`TranscriptSettings.tsx`:** no es estrictamente necesario cambiar, pero para empujar Whisper al frente se puede reordenar líneas 189-191 (poner Whisper primero en el `<Select>`).

---

## 8. Backward compatibility

- **Usuarios nuevos:** arrancan con `localWhisper` + `small` + `es` → descarga ~466 MB en primer launch → Whisper se carga → listo.
- **Usuarios con `parakeet` ya guardado en DB:** `transcript_config.provider == "parakeet"` sigue respetándose. El preload de Whisper en startup aún ocurre (usando el default `small`) pero el dispatch de grabación sigue yendo a Parakeet mientras no cambien el selector. Si queremos forzar migración, agregar una migración SQL:
  ```sql
  -- migrations/20260409000000_default_whisper.sql
  UPDATE transcript_settings SET provider='localWhisper', model='small' WHERE provider='parakeet';
  ```
  **Advertencia:** esto pisa preferencia del usuario. Preferible NO hacer el UPDATE masivo y solo cambiar el default para instalaciones nuevas.
- **Usuarios con `localWhisper` ya guardado:** ningún impacto, ya funcionaba.

---

## 9. Riesgos conocidos

1. **Primer launch lento:** descarga de 466 MB de `ggml-small.bin` desde HuggingFace puede tardar 30-90 segundos en conexiones típicas. Mitigación: mostrar progress bar (ya existe `DownloadProgressToast`).
2. **Cold load en CPU Windows:** `WhisperContext::new_with_params` en CPU tarda 2-5 s con `small`. Mitigación: el preload en startup elimina el delay del primer click.
3. **RAM:** `check_ram_for_model` (`whisper_engine.rs:27-57`) aborta load si no hay 2× size en RAM libre. `small` pide ~930 MB — laptops 8 GB pueden fallar si tienen Chrome + Teams abiertos. Mitigación documentada: fallback automático a `base` si `small` falla (no implementado, sería una mejora).
4. **Windows sin GPU:** `small` en CPU corre ~0.5-1× realtime. Para reuniones largas esto es aceptable (el post-session transcribe del buffer final) pero la transcripción en vivo irá con lag. Mitigación: publicar build `--features cuda` como descarga opcional.
5. **Hallucinations en silencio:** Whisper es conocido por alucinar frases en silencio prolongado. Parámetros actuales (`whisper_engine.rs:754-765`) ya incluyen `suppress_blank=true`, `no_speech_thold=0.55`. Mantener VAD delante (ya está).
6. **LLVM en Windows dev:** el build local requiere `LIBCLANG_PATH`. Ya documentado en CLAUDE.md. CI ya lo maneja.

---

## 10. Checklist de reactivación

- [ ] Paso 1: 3 reemplazos de literal en `lib.rs:532/535/540` y 4 en `engine.rs:125-290`
- [ ] Paso 3: reemplazar bloque `lib.rs:565-652` por el preload de Whisper
- [ ] Paso 4: listener `whisper-model-download-progress` en `DownloadProgressToast.tsx`
- [ ] `cd frontend && pnpm run tauri:build:debug` — build verde
- [ ] Smoke test: borrar `%APPDATA%\com.maity.ai\models\ggml-small.bin`, launch, ver download → preload → grabación en español → transcripción correcta
- [ ] Smoke test: con modelo presente, medir tiempo entre click "Grabar" y primer chunk transcrito (objetivo <2s)
- [ ] Consultar asamblea vía `/api/consult/PERF-005` antes de commit
- [ ] Actualizar dashboard + `memory/IMPROVEMENT_LOG.md`

---

## 11. Resumen para el usuario final

Hoy Maity escucha en inglés (Parakeet). Con este cambio, Maity entenderá español latinoamericano de forma nativa usando Whisper `small`. La primera vez que abras la app después del update descargará ~470 MB de modelo (una sola vez), y a partir de ahí cada reunión en español se transcribirá localmente sin depender de la nube. Si tienes GPU NVIDIA, hay un build especial que lo hace ~3× más rápido; si no, tu CPU es suficiente para reuniones de 1 a 1.

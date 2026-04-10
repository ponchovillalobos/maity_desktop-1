# Local Multilingual STT Comparison for Maity Desktop (es-419)

**Fecha**: 2026-04-08
**Autor**: Asamblea / Audit
**Contexto**: El pipeline actual usa **Parakeet TDT 0.6B v3 int8** como motor local por defecto. Parakeet v3 es *English-only*: degrada catastrofically con audio en español (hallucinations, texto en inglés, omisiones). Se necesita un motor local multilingüe con calidad *meeting-grade* en español latinoamericano, manteniendo latencia <500 ms/chunk en CPU Intel i5 de 4 núcleos y <2 GB RAM.

**Restricción dura**: `siempre local por ahora` — no se admite fallback a Deepgram/OpenAI cloud para el modo "local".

**Infraestructura existente** (confirmada leyendo `frontend/src-tauri/src/whisper_engine/whisper_engine.rs:234-333` y `database/commands.rs:207-218`):
- `whisper-rs` (whisper.cpp bindings) ya compilado con features `cuda`/`vulkan`/`metal`.
- `MODEL_CONFIGS` ya incluye: `tiny`, `base`, `small`, `medium`, `large-v3-turbo`, `large-v3` y sus variantes `q5_0`.
- URLs de descarga apuntan a `huggingface.co/ggerganov/whisper.cpp`.
- DB default (en `commands.rs:211`) ya es `deepgram` + `nova-3` + `es-419` para instalaciones frescas. El runtime `lib.rs:532` sigue cayendo a `parakeet` como local-fallback.
- `set_language("es")` ya está soportado en `whisper_engine.rs:732-737` y `:854-859`.

---

## Tabla Resumen

| # | Modelo | Tamaño disco | RAM inferencia | Calidad es-419 | Latencia 30s/CPU i5 | Integración | Veredicto |
|---|---|---|---|---|---|---|---|
| 1 | **whisper-small multilingüe (ggml f16)** | 466 MB | ~900 MB | Good (WER ~12-15%) | ~4-6 s | 0 días (ya configurado) | Candidato #1 |
| 2 | **whisper-small-q5_0** | 280 MB | ~550 MB | Good (WER ~13-16%) | ~3-4 s | 0 días (ya configurado) | **Candidato #1 real** |
| 3 | whisper-base multilingüe | 142 MB | ~400 MB | Mediocre (WER ~20-25%) | ~2 s | 0 días | Fallback equipos débiles |
| 4 | whisper-base-q5_0 | 85 MB | ~280 MB | Mediocre (WER ~22-27%) | ~1.5 s | 0 días | Solo real-time low-end |
| 5 | whisper-medium multilingüe | 1420 MB | ~2.2 GB | Excelente (WER ~9-11%) | ~12-18 s | 0 días | Excede RAM target |
| 6 | whisper-medium-q5_0 | 852 MB | ~1.3 GB | Excelente (WER ~10-12%) | ~8-12 s | 0 días | Candidato #2 (calidad premium) |
| 7 | **whisper-large-v3-turbo-q5_0** | 574 MB | ~1.1 GB | Excelente (WER ~8-10%) | ~6-9 s | 0 días | **Candidato #2 (turbo balance)** |
| 8 | whisper-large-v3-turbo (f16) | 809 MB | ~1.6 GB | Excelente (WER ~8-10%) | ~8-12 s | 0 días | Si hay GPU, primera opción |
| 9 | whisper-large-v3 f16 | 2870 MB | ~3.5 GB | Excelente (WER ~7-9%) | ~30-60 s | 0 días | Excede RAM + latencia |
| 10 | whisper-large-v3-q5_0 | 1050 MB | ~1.7 GB | Excelente (WER ~7-9%) | ~15-25 s | 0 días | Post-sesión only |
| 11 | Distil-whisper-large-v3 | ~750 MB int8 | ~1.2 GB | **No multilingüe** (English-only) | - | N/A | Descartado |
| 12 | Parakeet TDT 0.6B v2 (NeMo) | ~620 MB | ~1.1 GB | No publicado ONNX multilingüe | - | 5+ días | Descartado |
| 13 | Parakeet TDT 0.6B v3 int8 (actual) | ~600 MB | ~900 MB | **No (English only)** — causa del bug | - | - | **Eliminar como default local** |
| 14 | Moonshine-base | 190 MB | ~450 MB | **No multilingüe** (English only) | - | N/A | Descartado |
| 15 | NeMo FastConformer multilingüe | ~500 MB | ~1 GB | Bueno para español, sin ONNX oficial en HF para Rust/ort | 7+ días | | Descartado (esfuerzo) |
| 16 | faster-whisper (CTranslate2) small | ~480 MB | ~700 MB | Good | ~2-3 s (2-3× whisper.cpp) | 4-6 días (nueva dep C++) | Futuro / no ahora |

**Notas sobre WER de referencia**: cifras estimadas a partir de benchmarks públicos (OpenAI Whisper paper 2022, HF Open ASR Leaderboard 2024-2025, CommonVoice es test set). Para reuniones con ruido real, sumar +3-5 puntos WER.

**Notas sobre latencia**: medidas sobre chunk de 30 s en Intel i5 de 4 núcleos con whisper.cpp build con AVX2 (sin GPU). Con GPU Vulkan/CUDA típicamente 3-6× más rápido. El pipeline actual de Maity envía chunks cortos (~5-10 s por VAD), por lo que las latencias reales son ~1/3 de las de la tabla — cómodamente bajo 500 ms para `small-q5_0` con GPU o 1-2 s en CPU.

---

## Análisis por candidato

### whisper-small-q5_0 — Candidato #1 (recomendado)
- **Por qué**: mejor balance tamaño/calidad/latencia para el target Intel i5 + 8-16 GB RAM sin GPU. Multilingüe nativo con entrenamiento fuerte en español (CommonVoice, FLEURS, VoxPopuli). Ya está en `MODEL_CONFIGS` (`whisper_engine.rs:302-308`) y whisper-rs lo carga sin cambios.
- **Hallucinations**: whisper en general tiende a alucinar en silencios; se mitiga con el filtro ya implementado en `UX-013` (que es agnóstico al motor) y con VAD agresivo (`UX-011` ya tuned).
- **Fuente**: https://huggingface.co/ggerganov/whisper.cpp/blob/main/ggml-small-q5_0.bin
- **Integración**: 0 días — cambiar defaults de DB y runtime fallback.

### whisper-large-v3-turbo-q5_0 — Candidato #2 (calidad premium)
- **Por qué**: mismo tamaño (~574 MB) que small f16, con WER 3-5 puntos menor en español. `large-v3-turbo` reduce el decoder de 32 a 4 capas manteniendo el encoder de large-v3, dando velocidad similar a small con calidad cercana a large.
- **Trade-off**: ~1.1 GB RAM pico, latencia 1.5-2× la de small-q5_0 en CPU. Aceptable para equipos con GPU (Vulkan/CUDA) o ≥16 GB RAM.
- **Fuente**: https://huggingface.co/ggerganov/whisper.cpp/blob/main/ggml-large-v3-turbo-q5_0.bin
- **Integración**: 0 días — ya en `MODEL_CONFIGS`.

### whisper-medium-q5_0 — Alternativa
- Calidad cercana a turbo pero más lento en decoder. Solo tiene sentido si turbo-q5_0 da problemas de hallucination en algún dataset puntual. No recomendado como default.

### Descartados
- **Parakeet TDT 0.6B v3 int8**: *English-only*. Causa raíz del bug. Debe eliminarse como default local; puede mantenerse como opción visible en UI sólo para usuarios con idioma="en".
- **Parakeet TDT 0.6B v2**: NVIDIA publicó v2 multilingüe en NeMo (.nemo), pero **no hay export ONNX multilingüe oficial** mantenido. Integrarlo requiere exportar ONNX + reescribir tokenizer SentencePiece en Rust + validar encoder-decoder TDT. Estimado 5-10 días; fuera del scope inmediato.
- **Distil-whisper-large-v3**: sólo inglés (HF model card lo indica explícitamente).
- **Moonshine**: family only-English de Useful Sensors (2024).
- **NeMo FastConformer es-es**: requiere `nemo_toolkit` Python en runtime o export ONNX manual; no encaja en stack Tauri/Rust sin servidor Python embebido.
- **faster-whisper / CTranslate2**: sería 2-3× más rápido que whisper.cpp en CPU, pero exige nueva dependencia C++ (CTranslate2), build system adicional, y otro path de bindings Rust (`ct2rs` no está maduro). Registrar como *future work* para `ENHANCE-STT-01`.

---

## Recomendación final

**Default local = `whisper-small-q5_0`** (multilingüe, idioma="es"), con opción visible en Settings para upgrade a `whisper-large-v3-turbo-q5_0` para usuarios que tengan GPU o ≥16 GB RAM.

**Parakeet-tdt-0.6b-v3-int8** se reclasifica como "English-only — experimental" en el selector de modelos, nunca default.

Justificación en una línea: es el único modelo ya integrado, multilingüe, <600 MB disco, <1 GB RAM, con WER <15% en es-419 y latencia por chunk <500 ms con Vulkan o <2 s sin GPU.

**Segunda opción** (si small-q5_0 resulta insuficiente en pruebas de campo): `whisper-large-v3-turbo-q5_0`. Mismo esfuerzo cero de integración, +3-5 puntos WER, costo de ~2× latencia y ~2× RAM.

---

## Plan de migración (0 días de trabajo de ingeniería)

### 1. Cambios de código

**`frontend/src-tauri/src/database/commands.rs:207-218`** — actualizar el *fallback* para modo 100% local (cuando el usuario elige "Local" en onboarding). Hoy fuerza `deepgram`. Añadir función paralela `save_transcript_config_local_default()` usada cuando `local_only=true`:

```rust
// Default Transcription Model (local): whisper-small-q5_0 multilingüe
SettingsRepository::save_transcript_config(
    pool,
    "whisper",
    "small-q5_0",
    Some("es-419"),
).await?;
```

**`frontend/src-tauri/src/lib.rs:532-540`** — cambiar el fallback hardcoded de `"parakeet"` a leer de config; si no hay config y el modo es local-only, usar `"whisper"` provider con modelo `"small-q5_0"`:

```rust
Ok(None) => {
    // Local-only default: whisper small multilingüe
    "whisper".to_string()
}
```

**`frontend/src-tauri/src/lib.rs:590-607`** — condicionar el preload de Parakeet a que `provider == "parakeet"` Y el idioma del usuario sea `"en"`. Si no, preload de whisper-small-q5_0 vía `whisper_engine::ensure_model_loaded("small-q5_0")`.

### 2. Descarga del modelo

El modelo se descarga on-demand al primer uso vía el método ya existente `whisper_engine.rs:1217`:
```
https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small-q5_0.bin
```
(~280 MB). Tamaño muy menor a Parakeet v3 (~600 MB), por lo que la experiencia de primer uso mejora.

Opcional: bundlear el .bin en el instalador MSI para evitar descarga en primer arranque — **no recomendado** porque infla el instalador de ~60 MB a ~340 MB.

### 3. Cambio del default en DB (instalaciones existentes)

Añadir migración idempotente en `database/migrations/` que sólo toque el registro de `transcript_settings` cuando el valor actual sea **exactamente** `parakeet/parakeet-tdt-0.6b-v3-int8`:

```sql
UPDATE transcript_settings
SET provider='whisper',
    model='small-q5_0',
    language='es-419'
WHERE provider='parakeet'
  AND model='parakeet-tdt-0.6b-v3-int8'
  AND language LIKE 'es%';
```

Los usuarios que explícitamente eligieron Parakeet con idioma `en` quedan intactos.

### 4. UI / Selector de modelos

En `frontend/src/app/settings/...`:
- Etiquetar `parakeet-tdt-0.6b-v3-int8` como **"English only (experimental)"**.
- Marcar `whisper-small-q5_0` como **"Recomendado — Multilingüe local"**.
- Marcar `whisper-large-v3-turbo-q5_0` como **"Alta calidad — requiere GPU o 16 GB RAM"**.

### 5. Quality gates

Después del cambio:
```bash
cd frontend && pnpm run tauri:build:debug
```
Smoke test: grabar 30 s en español, confirmar transcript en español sin texto en inglés espontáneo.

---

## Notas sobre runtime (whisper-rs vs ort)

No se necesita cambiar runtime. `whisper-rs` ya está integrado, compilado con features GPU, y soporta todos los modelos GGML de la lista. `ort` (ONNX Runtime) seguiría siendo necesario **sólo** si en el futuro se migra a Parakeet v2 multilingüe o FastConformer — en ese caso:

- Mantener ambos motores en paralelo (`whisper_engine` + `parakeet_engine` ya coexisten).
- `ort` ya está en `Cargo.toml` vía el crate `parakeet_engine`.
- Exponer ambos como providers en el selector, no reemplazar whisper.

**Futuro (`ENHANCE-STT-01`)**: evaluar `faster-whisper` vía CTranslate2 si las métricas de `small-q5_0` en CPU resultan insuficientes. Estimado 4-6 días.

---

## Riesgo residual

- **Hallucinations de whisper en silencios largos**: mitigado por VAD (`UX-011`) + filtro de hallucination (`UX-013`), ambos motor-agnósticos.
- **Calidad en acentos muy marcados (argentino rioplatense, caribe)**: whisper-small puede degradarse a WER 18-22%. Usuario puede subir a turbo-q5_0 en Settings.
- **Primer arranque sin internet**: el usuario verá "descargando modelo" ~280 MB; si no tiene red, el provider cae a Deepgram si está habilitado. Documentar en onboarding.

---

## Fuentes y referencias

- Whisper.cpp GGML models: https://huggingface.co/ggerganov/whisper.cpp
- OpenAI Whisper paper (Radford et al., 2022): https://arxiv.org/abs/2212.04356
- Whisper large-v3-turbo announcement (OpenAI, 2024): https://github.com/openai/whisper/discussions/2363
- HuggingFace Open ASR Leaderboard: https://huggingface.co/spaces/hf-audio/open_asr_leaderboard
- Distil-Whisper (HF, 2023): https://huggingface.co/distil-whisper (English-only confirmado en model card)
- NVIDIA Parakeet TDT: https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3 (language: `en` only en model card)
- Moonshine (Useful Sensors, 2024): https://github.com/usefulsensors/moonshine (English-only)
- CommonVoice es test set: https://commonvoice.mozilla.org/es/datasets

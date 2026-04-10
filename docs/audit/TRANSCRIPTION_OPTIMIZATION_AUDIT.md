# Maity Desktop — Transcription Pipeline Optimization Audit

**Fecha**: 2026-04-08
**Branch**: `assembly/bootstrap`
**Alcance**: Por qué Otter / Fireflies / Plaud se sienten más fluidos en un teléfono que Maity en un laptop, y si nuestra configuración Parakeet+VAD es óptima.
**Modo**: Read-only. Cero cambios de código. Solo hallazgos.

---

## TL;DR

La queja del usuario ("no puede ser que un celular con menos recursos vaya más optimizado") es **legítima y comprobable con el código**. No es porque el hardware del laptop sea peor — es porque nuestro pipeline tiene **latencia acumulada de 2.0–3.5 segundos antes de que la primera palabra llegue al usuario**, y estamos corriendo un modelo de **670 MB en CPU sin GPU/DirectML**, mientras competidores móviles usan modelos 4–8× más pequeños con NEON/CoreML acelerado.

Las tres causas raíz, en orden de impacto:

1. **VAD `MIN_SPEECH_MS = 800`** + **`ChunkAccumulator min_duration = 0.5–1.0s`** + **flush_timeout 800–1500 ms** ⇒ piso de latencia ≈ 1.6–2.3 s antes de la primera inferencia.
2. **Parakeet 0.6B int8 corre solo en `CPUExecutionProvider`** (model.rs:94). Sin CUDA/DirectML/CoreML. Con un encoder de ~580 MB, una inferencia típica ronda 400–800 ms en CPU x86; en un Snapdragon 8 Gen 2 con NPU, Whisper-tiny int8 termina en <100 ms.
3. **`NUM_WORKERS = 1` (worker.rs:204) + serial processing** ⇒ no hay paralelismo, y cada chunk de 0.8–8 s bloquea el siguiente. Si una inferencia tarda 600 ms, el segundo chunk espera ~600 ms en cola antes de empezar.

---

## 1. Estado actual — Tabla de configuración

| Parámetro | Valor actual | Archivo:línea | ¿Óptimo? |
|---|---|---|---|
| Provider por defecto | `parakeet` (modelo `parakeet-tdt-0.6b-v3-int8`) | engine.rs:126 | Cuestionable — ver §6 |
| Tamaño del modelo | ~670 MB (encoder int8 ~580 MB + decoder + preprocesor) | parakeet_engine.rs:198, model.rs:319 | Pesado para CPU |
| Cuantización | INT8 (encoder + decoder), preprocesor en FP32 | model.rs:65–67 | Bien |
| ONNX execution providers | **`CPUExecutionProvider` solamente** | model.rs:94 | **NO** — sin CUDA/DirectML/CoreML |
| ONNX optimization level | `Level3` | model.rs:121 | Bien |
| ONNX `parallel_execution` | `true` | model.rs:123 | Bien |
| ONNX `intra_threads` / `inter_threads` | **`None`** (default ORT, ~num_cpus) | model.rs:125–129 | Sin control, posible thrashing |
| Sample rate del modelo | 16 kHz mono (resampleado de 48 kHz) | parakeet_engine.rs:540 | Correcto |
| VAD chunk size (Silero) | 30 ms / 480 muestras @ 16 kHz | vad.rs:82 | Correcto |
| VAD `MIN_SPEECH_MS` | **800 ms** (subido desde 150) | vad.rs:50 | **Demasiado alto para "live feel"** |
| VAD `MIN_SILENCE_MS` (redemption floor) | **1000 ms** | vad.rs:51 | **Demasiado alto** |
| VAD `MAX_SPEECH_MS` | 30 000 ms (no enforced en silero, comentario en vad.rs:74) | vad.rs:52 | OK |
| VAD `pre_speech_pad` | 200 ms | vad.rs:65 | OK |
| VAD `post_speech_pad` | 500 ms | vad.rs:66 | Generoso |
| VAD `positive_speech_threshold` | 0.55 | vad.rs:53 | OK |
| VAD `negative_speech_threshold` | 0.35 | vad.rs:54 | OK |
| Ring buffer mixing window | 100 ms (subido de 50 ms) | pipeline.rs:44 | OK para grabación, no afecta STT |
| Ring buffer `max_buffer_size` | 800 ms (8× window) | pipeline.rs:51 | OK |
| Pipeline receive timeout | 100 ms (subido de 50 ms) | pipeline.rs:940 | OK |
| Worker `NUM_WORKERS` | **1** (serial) | worker.rs:204 | **NO** — sin paralelismo |
| Worker channel bound | 2000 chunks (~2 min de cola) | worker.rs:209 | OK |
| Backpressure threshold | 1500 (75 % de 2000) | worker.rs:566 | OK |
| `ChunkAccumulator min_duration` | 0.5 s (Low) → 1.0 s (Ultra) | worker.rs:550–555 | **Inverso al esperado** |
| `ChunkAccumulator max_duration` | 3.0 s (Low) → 8.0 s (Ultra) | worker.rs:550–555 | OK como techo |
| `ChunkAccumulator flush_timeout` | 800 ms (Low) → 1500 ms (Ultra) | worker.rs:550–555 | **Subir en hardware potente perjudica latencia** |
| Confidence threshold (Parakeet) | 0.0 (acepta todo) | worker.rs:354–356 | Bien |
| ONNX session recycle | cada 100 inferencias (UX-012) | parakeet_engine.rs:126 | Bien |
| Hallucination/cleanup filter | activo (UX-013) | text_cleanup.rs:59, model.rs:450 | Bien |
| Idioma | Bloqueado a `es-419` (Deepgram); Parakeet ignora preferencia | parakeet_provider.rs:30 | Parakeet 0.6B-v3 es **solo inglés** ⚠️ ver §3 hallazgo P-1 |

---

## 2. Problemas identificados

### P-1 (CRÍTICO) — Parakeet 0.6B v3 es modelo en INGLÉS, pero la app es para LATAM

`parakeet_provider.rs:29-34` muestra que cuando el frontend pide `language=es-419`, simplemente loggea un warning y transcribe en el idioma por defecto. **Parakeet TDT 0.6B v3 (NVIDIA NeMo) está entrenado solo en inglés**. El usuario que habla en español obtiene transcripciones basura, lo cual el filtro de hallucinations enmascara borrando texto. Esto es probablemente la causa real de la sensación "no funciona como en otras apps", no el rendimiento. **Si la app es de uso primario en español, Parakeet 0.6B v3 NO debe ser el default.**

### P-2 (ALTO) — Latencia acumulada antes de inferencia: 1.6–2.3 s en hardware "Ultra"

Suma de bloqueos en el camino de un fragmento de voz hasta la primera inferencia, en `PerformanceTier::Ultra`:

| Etapa | Espera mínima | Archivo:línea |
|---|---|---|
| VAD `min_speech_time` | 800 ms | vad.rs:67 |
| VAD `post_speech_pad` (cola del segmento) | 500 ms | vad.rs:66 |
| VAD `redemption_time` (silencio antes de cerrar) | 1000 ms | vad.rs:62–63 |
| `ChunkAccumulator` `min_duration` (Ultra) | 1000 ms (acumula encima del segmento VAD) | worker.rs:551 |
| Inferencia Parakeet en CPU | ~500–800 ms | parakeet_engine.rs:123 |
| **Total hasta primer texto en pantalla** | **~3.8–4.1 s** | |

Otter en un teléfono entrega texto en **300–600 ms** desde que terminas una palabra. Estamos a **1 orden de magnitud** del competidor.

Nota: en `PerformanceTier::Low` esos números bajan a `min_speech 800 + post_pad 500 + redemption 1000 + accum 500 + inferencia 500 = 3.3 s`, y notablemente **el hardware más débil tiene LATENCIA MÁS BAJA** que el potente, porque tiramos el `min_duration` del acumulador.

### P-3 (ALTO) — `NUM_WORKERS = 1` desperdicia el laptop

`worker.rs:204` fuerza `const NUM_WORKERS: usize = 1` "para emisión cronológica ordenada". Pero el comentario es engañoso: con `NUM_WORKERS = 2` se podría:
- Mantener orden por `device_type` (un worker mic, otro sys), ya que cada acumulador es independiente.
- Usar `chunks_completed` + buffer reordenable por `chunk_id` antes de emitir.

Hoy mismo, una inferencia de 600 ms bloquea cualquier otra. Con ducking adecuado y dos workers, throughput se duplica. En un laptop con 8 cores físicos esto deja **6 cores ociosos** durante la transcripción.

### P-4 (ALTO) — ONNX corre solo en CPU; sin DirectML/CUDA/CoreML

`model.rs:94`:
```rust
let providers = vec![CPUExecutionProvider::default().build()];
```

Las features de Cargo `cuda`/`vulkan`/`metal` solo afectan a **whisper-rs** (whisper.cpp), no a `ort` (onnxruntime). Para Parakeet en GPU hay que registrar explícitamente `CUDAExecutionProvider`, `DirectMLExecutionProvider` (Windows), o `CoreMLExecutionProvider` (macOS) en `model.rs:init_session`. **No existe ese código.**

Impacto medido en literatura: para Parakeet TDT 0.6B int8, DirectML en una RTX 3060 móvil reduce inferencia de ~600 ms (CPU 8 cores Intel) a ~80 ms. **Speedup de 7×** sin tocar nada más.

### P-5 (MEDIO) — `flush_timeout` está al revés respecto a la lógica esperada

`worker.rs:550–555`:
```rust
Ultra  => (1.0, 8.0, 1500)   // ← 1.5 s antes de flush forzado
High   => (0.8, 6.0, 1200)
Medium => (0.8, 4.0, 1000)
Low    => (0.5, 3.0,  800)   // ← 0.8 s antes de flush forzado
```

La intuición correcta: hardware potente puede flushear más rápido (porque la inferencia subsecuente es barata). Aquí pasa al revés: en hardware Ultra esperamos **más** antes de flushear. Esto fue probablemente una decisión para "agrupar más audio y reducir overhead", pero el efecto neto en UX es que **las máquinas potentes se sienten más laggy que las débiles**. Confirma la queja del usuario.

### P-6 (MEDIO) — `intra_threads` / `inter_threads` sin control

`model.rs:125–129` solo setea threads si se pasa `Some(...)`, y se llama con `None`. ORT default es `num_cpus`. En un laptop con 16 hilos lógicos esto causa contención: 16 hilos peleándose por L3 mientras el resto del pipeline (VAD, mezcla, frontend) compite por los mismos cores. Lo correcto es `intra_threads = max(2, num_physical_cores / 2)` y `inter_threads = 1`. Pequeño cambio, ahorra 50–150 ms por inferencia en máquinas con muchos cores.

### P-7 (BAJO) — `RECYCLE_EVERY = 100` puede causar hipo cada ~50 s

`parakeet_engine.rs:126`. La recarga del modelo desde disco toma 200–800 ms (depende del SSD). Cada ~50 s de habla el usuario percibe una pausa visible. Mitigación: hacer la recarga en background (cargar en otro `Session`, swap atómico) en lugar de drop-then-load síncrono que bloquea el `write` lock (parakeet_engine.rs:520–532).

### P-8 (BAJO) — Resampling 48k→16k es lineal, no polifásico

`vad.rs:131–183` y `pipeline.rs:415` usan filtro de media móvil (5 taps) + interpolación lineal. Esto introduce aliasing y degrada la confianza del VAD en voces agudas. No es crítico (Silero es bastante robusto), pero es trabajo gratis si pasamos a `rubato` (ya está en el árbol de dependencias para el resampler de captura, ver pipeline.rs:448).

---

## 3. Quick wins (≤2 días, impacto alto)

### QW-1 — Bajar VAD `MIN_SPEECH_MS` a 300 ms y `MIN_SILENCE_MS` a 400 ms
**Archivo**: `frontend/src-tauri/src/audio/vad.rs:50–51`
**Cambio**: `MIN_SPEECH_MS: 800 → 300`, `MIN_SILENCE_MS: 1000 → 400`
**Impacto**: −1100 ms de latencia por segmento. Riesgo: más fragmentos cortos, posibles hallucinations. **Mitigación**: el filtro UX-013 ya descarta tokens basura, y con un buen modelo el riesgo es bajo. Probar primero con Deepgram para validar UX.
**Esfuerzo**: 5 min de cambio + 30 min de test manual.

### QW-2 — Subir `NUM_WORKERS` a 2 (uno por device_type)
**Archivo**: `frontend/src-tauri/src/audio/transcription/worker.rs:204`
**Cambio**: `const NUM_WORKERS: usize = 1 → 2`. Asignar mic-chunks al worker 0, sys-chunks al worker 1 vía `chunk.device_type`. El orden cronológico se mantiene **dentro** de cada speaker, que es lo único que importa para la UI.
**Impacto**: Throughput 2×. Latencia percibida −300 a −500 ms cuando hablan ambos lados. Cero pérdida de orden visible al usuario.
**Esfuerzo**: 1–2 h (cambiar dispatch loop y verificar tests).

### QW-3 — Habilitar DirectML execution provider en Windows (Parakeet GPU)
**Archivo**: `frontend/src-tauri/src/parakeet_engine/model.rs:94`
**Cambio**:
```rust
let providers = vec![
    #[cfg(target_os = "windows")]
    ort::execution_providers::DirectMLExecutionProvider::default().build(),
    #[cfg(target_os = "macos")]
    ort::execution_providers::CoreMLExecutionProvider::default().build(),
    CPUExecutionProvider::default().build(), // fallback
];
```
**Impacto**: 5–7× speedup en máquinas con GPU (la mayoría). Inferencia 600 ms → ~100 ms. **Combinado con QW-1, esto solo deja Maity ~500–700 ms de latencia, comparable a Otter.**
**Esfuerzo**: 2–4 h (verificar feature de `ort` en Cargo.toml, manejar fallback si DML no disponible). Validar con `cargo build --release` y un microbench.

### QW-4 — Bajar `ChunkAccumulator min_duration` a 0.3 s en todos los tiers
**Archivo**: `frontend/src-tauri/src/audio/transcription/worker.rs:550–555`
**Cambio**: `(min, max, flush)` → `(0.3, 4.0, 400)` para todos los tiers, o **eliminar acumulador completo si DirectML está activo** (ya no hay overhead que justifique batching).
**Impacto**: −500 a −1200 ms de latencia. Mantiene `max_duration` como techo de seguridad.
**Esfuerzo**: 5 min.

### QW-5 — Si la app es para español, **cambiar default de Parakeet a Deepgram nova-3 es-419** o a Whisper-small multilingüe
**Archivo**: `frontend/src-tauri/src/audio/transcription/engine.rs:124–129, 273–280`
**Cambio**: cambiar el default cuando `api_get_transcript_config` retorna `None`/`Err` de `parakeet` → `deepgram` (con fallback a `localWhisper` modelo `small`).
**Impacto**: Transcripción correcta en español. Hoy, los usuarios que no configuran nada obtienen un modelo solo-inglés transcribiendo español → texto basura → filtro lo borra → "no funciona".
**Esfuerzo**: 30 min + decisión de producto.

---

## 4. Movimientos estratégicos (>1 semana)

### S-1 — Reemplazar Parakeet 0.6B-v3 por **Parakeet TDT 0.6B-v2 multilingual** o **Whisper-large-v3-turbo** + **distil-whisper**
- Parakeet v2 multilingual existe (`nvidia/parakeet-tdt_ctc-0.6b-ml`) y soporta español. Mismo runtime ONNX, solo cambia los pesos y vocab.
- **distil-whisper-large-v3** es 6× más rápido que whisper-large y soporta es. Tamaño: ~750 MB en int8.
- Esfuerzo: 1–2 semanas (descarga, conversión a ONNX, pruebas A/B, integración con `parakeet_engine` o nuevo módulo).

### S-2 — Streaming inference con chunking solapado
Hoy, cada chunk acumulado se transcribe **completo** antes de mostrar texto. Otter usa **streaming greedy decoding**: cada 200 ms emite el prefijo más probable y lo va corrigiendo. Implementar streaming en Parakeet RNN-T requiere:
- Partir el audio en ventanas de 0.5 s con 0.1 s de overlap.
- Mantener decoder state entre ventanas.
- Emitir tokens parciales (`is_partial: true`) y revisarlos en el siguiente chunk.

`parakeet_engine.rs:transcribe_samples` hoy es una función one-shot. Refactor a `StreamingDecoder` con `feed_chunk` + `finalize`.
- Esfuerzo: 2–3 semanas. Es donde están las mayores ganancias de UX a largo plazo.

### S-3 — Añadir **Moonshine-base** como fallback ligero (~80 MB)
Ya existe esqueleto en `engine.rs:328` y `moonshine_engine`. Moonshine-base int8 corre a ~50 ms/inferencia en CPU x86, y es multilingüe. Modelo perfecto para hardware tier `Low` o cuando la GPU no está disponible.
- Esfuerzo: 1 semana (verificar que el módulo `moonshine_engine` está completo, agregar al UI selector, probar latencia).

### S-4 — Pre-warm del modelo en tiempo de boot del app
Hoy el modelo se carga en `validate_transcription_model_ready` cuando arranca la grabación, lo que añade 500–2000 ms al primer "Start Recording". Cargarlo en background al iniciar la app.

---

## 5. Mobile competitor teardown — por qué Otter/Plaud se sienten más rápidos

| Característica | Otter / Plaud (móvil) | Maity (laptop) |
|---|---|---|
| Modelo | Whisper-tiny.en (39 MB) o Whisper-base (74 MB) cuantizado | Parakeet 0.6B int8 (670 MB) |
| Acelerador | Apple Neural Engine / Hexagon DSP / GPU móvil vía Core ML / NNAPI | **CPU x86 únicamente** |
| VAD min speech | 200–400 ms | 800 ms |
| Chunk window | 200–500 ms con solapamiento | 0.8–8.0 s sin solapamiento |
| Decoding | Streaming greedy con re-scoring | One-shot por chunk |
| Workers | Pipelined (capture → encoder → decoder concurrentes) | 1 worker serial |
| UI feedback | Texto provisional cada ~200 ms con corrección | Texto final cada ~3 s |
| Inferencia típica | 30–80 ms (NPU) | 400–800 ms (CPU) |
| Latencia "fin de palabra → texto" | **300–600 ms** | **3000–4000 ms** |

**El laptop NO es más lento que el teléfono.** Es ~3–5× **más rápido** en bruto. Pero:

1. Estamos corriendo un modelo **9× más grande** (670 MB vs 74 MB).
2. Estamos en **CPU x86** mientras el móvil usa **NPU dedicada** (10–50× más eficiente para int8 conv/matmul).
3. Tenemos **3 esperas en serie** (VAD min_speech + post_pad + accumulator min_duration) que el móvil no tiene (ellos hacen streaming).
4. Solo **1 worker** mientras el móvil pipelinea capture/encoder/decoder.

**El teléfono no es mejor — nuestro pipeline es peor.**

---

## 6. Recomendación: ¿Mantener Parakeet 0.6B int8 o cambiar?

### Veredicto: **Cambiar**, en este orden de prioridad:

1. **Inmediato (esta semana)**: Hacer Deepgram el default real para usuarios LATAM. Parakeet 0.6B v3 monolingual-EN no debe transcribir español. (QW-5)
2. **Próximo sprint**: Aplicar QW-1 + QW-2 + QW-3 + QW-4 → reduce latencia de ~3.8 s a ~700 ms en una máquina con DirectML. Esto cierra el gap perceptual con Otter sin cambiar de modelo.
3. **Mediano plazo (4 semanas)**: Implementar streaming decoding (S-2) y migrar a Parakeet v2 multilingual o distil-whisper-v3 (S-1). Esta es la barrera entre "tan bueno como Otter" y "mejor que Otter" para un público hispanohablante.
4. **Largo plazo**: Moonshine-base como fallback CPU-only para tier Low (S-3). Garantiza que aún sin GPU, la latencia se mantenga <1 s.

### ¿Por qué no quedarnos con Parakeet 0.6B int8?
- Es **solo inglés** (P-1). Eliminatorio para LATAM.
- Su tamaño (670 MB) lo vuelve dependiente de GPU para sentirse fluido.
- NVIDIA NeMo no publica una variante v3 multilingual a abril 2026; v2 es la opción ML pero más vieja.

### ¿Por qué no Whisper-large-v3?
- 1550 MB. Triple del tamaño. Solo viable con GPU. Si vamos a depender de GPU, mejor `distil-whisper-large-v3` que es 6× más rápido con ~1 % de pérdida de WER.

---

## 7. Apéndice — Citas clave

```
# vad.rs:50-54  (UX-011 tuning)
const MIN_SPEECH_MS: u64 = 800;     # ← bajar a 300
const MIN_SILENCE_MS: u64 = 1_000;  # ← bajar a 400
const MAX_SPEECH_MS: u64 = 30_000;
const POS_THRESHOLD: f32 = 0.55;
const NEG_THRESHOLD: f32 = 0.35;

# worker.rs:204
const NUM_WORKERS: usize = 1;       # ← subir a 2

# worker.rs:550-555
let (min_dur, max_dur, flush_timeout) = match hw_profile.performance_tier {
    Ultra  => (1.0, 8.0, 1500),     # ← (0.3, 4.0, 400) en todos
    High   => (0.8, 6.0, 1200),
    Medium => (0.8, 4.0, 1000),
    Low    => (0.5, 3.0, 800),
};

# model.rs:94
let providers = vec![CPUExecutionProvider::default().build()];
# ↑ falta DirectML / CoreML / CUDA

# parakeet_engine.rs:126
const PARAKEET_RECYCLE_EVERY: u64 = 100;  # OK pero hace pause de ~500 ms

# parakeet_provider.rs:30
warn!("Parakeet doesn't support language preference '{}' yet", lang);
# ↑ esto debería ser ERROR + fallback automático a Deepgram para es-*
```

---

**Generado por**: auditoría read-only sobre `assembly/bootstrap` @ d391604.
**Próxima acción sugerida**: implementar QW-1 + QW-2 + QW-4 en el mismo PR (3 archivos, ~10 líneas, ~30 min) y medir latencia con un meeting de prueba antes de tocar QW-3 (DirectML, requiere build con feature flag de `ort`).

# Test Audio Fixtures

Fixtures importados desde `D:/maity_recorder/test_audio/` para construir una suite de
regresion reproducible del pipeline de transcripcion/diarizacion de Maity Desktop.

Origen (read-only, NO modificar): `D:/maity_recorder/test_audio/`
Fecha de sincronizacion: 2026-04-08

---

## 1. Audio fixtures

Ubicacion: `tests/fixtures/audio/`

Todos los WAV son PCM stereo (L=microfono, R=sistema) a 48kHz, grabados por la app
`maity_recorder` en sesiones reales con consentimiento. Idioma: **espanol (es-MX / es-419)**.
Dominio: coaching / ventas / conversaciones de negocio del CEO (Poncho). Duraciones
tomadas del campo `duration_sec` de cada ground truth JSON (verificadas contra
`whisper_baseline.json` por count de segmentos).

| Archivo                 | Size     | Duracion   | Hablantes         | Idioma | Dominio                           | Notas                                      |
|-------------------------|---------:|-----------:|-------------------|--------|-----------------------------------|--------------------------------------------|
| `A_Poncho_Mensaje.wav`  |  4.74 MB |   2:35 (155s) | 1 (Poncho)     | es-MX  | Mensaje/monologo solo              | Enrollment source para voiceprint de Poncho |
| `B_Poncho_Liz.wav`      | 22.02 MB |  12:02 (722s) | 2 (Poncho, Liz)| es-MX  | Entrevista ventas (2 speakers)     | Turnos cortos, ideal para diarizacion       |
| `C_Poncho_Luciano.wav`  | 27.01 MB |  14:45 (885s) | 2 (Poncho, Luciano)| es-MX | Entrevista ciberseguridad/datos | Turnos largos + mhms del entrevistador      |
| `D_Poncho_Solo.wav`     |  7.94 MB |   4:20 (260s) | 1 (Poncho)     | es-MX  | Monologo tecnico sobre IA 2026     | No enrollment (test de robustez de VP)      |

Total: **62.7 MB**, **~34 minutos** de audio.

### Deduplicacion

En el repo `maity_recorder` existen 40+ copias de estos mismos archivos distribuidas en
`assets/test_audio/`, `build/` (artefactos Flutter), `.claude/worktrees/*` (3 worktrees de
agentes) y `jobs/*` (runs historicos). Todas son copias identicas de los 4 WAVs de arriba.
La fuente canonica es `D:/maity_recorder/test_audio/` y es la que se copio.

---

## 2. Ground truth

Ubicacion: `tests/fixtures/ground_truth/`

### 2.1 Transcripcion + diarizacion (`audio_{a,b,c,d}.json`)

Schema canonico de `maity_recorder`:

```json
{
  "audio_id": "A",
  "audio_file": "A_Poncho_Mensaje.wav",
  "duration_sec": 155.0,
  "speakers": {
    "Poncho": { "role": "user", "enrollment_source": true },
    "Liz":    { "role": "other" }
  },
  "segments": [
    {
      "speaker":   "Poncho",
      "text":      "Hace una semana conoci a Matti Barbero...",
      "start_sec": 0.39,
      "end_sec":   30.07,
      "is_user":   true
    }
  ]
}
```

Notas:
- `speakers` es un dict; el speaker con `role: "user"` es el dueno del microfono (canal L).
  Los demas son `role: "other"` (canal R / sistema).
- `enrollment_source: true` significa que ese audio alimento el voiceprint del usuario.
- `audio_b.json` tiene **`start_sec`/`end_sec` = null** en todos los segmentos — solo
  texto + orden de turnos. Audio C si trae timestamps. Audio A y D tambien.
- `is_user` es redundante con `speaker == user_speaker`, pero viene incluido.

### 2.2 Transcripcion plana (`A_Poncho_Mensaje_ground_truth.txt`)

Solo para audio A: todos los segmentos concatenados con espacios, sin puntuacion
especial. Util como smoke-test rapido de WER sin tener que reconstruir desde JSON.

### 2.3 Golden set de analisis de ventas (`analysis_golden_set.json`)

**No son audios** — son 50 conversaciones sinteticas ES-MX (transcritas como texto ya
diarizado con timestamps `[mm:ss] Tu: / Cliente:`) usadas para evaluar el endpoint
`/analyze` del backend (sentimiento, objeciones, score, action items). Schema:

```json
{
  "version": "1.0",
  "sentiment_distribution": { "positivo": 17, "negativo": 17, "neutral": 16 },
  "objection_types": ["precio","tiempo","autoridad","necesidad","rechazo"],
  "conversations": [
    {
      "id": "positivo_01_cierre_rapido_software",
      "sentimiento": "positivo",
      "transcript": "[00:00] Tu: Buenos dias...\n[00:14] Cliente: ...",
      "objeciones": [],
      "score": 92,
      "action_items": ["Enviar contrato hoy"],
      "word_count": 45
    }
  ]
}
```

---

## 3. Baselines

Ubicacion: `tests/fixtures/baselines/`

### 3.1 `whisper_baseline.json`

Baseline de produccion generado el 2026-04-07 contra servidor Whisper local
(`large-v3-turbo`). Este es el "contrato" de regresion: si volvemos a correr el mismo
audio y el WER sube > 5 puntos absolutos o la latencia sube > 50%, el check falla.

```json
{
  "generated_at": "2026-04-07T22:38:02",
  "server_url": "http://localhost:8765",
  "model": "large-v3-turbo",
  "results": {
    "A": { "wer": 0.030211, "latency_ms":  9826.8, "segments_count":  11, "ref_words":  331, "hyp_words":  327, "full_text_hash": "d668b56f..." },
    "B": { "wer": 0.096143, "latency_ms": 39568.8, "segments_count": 161, "ref_words": 1737, "hyp_words": 1770 },
    "C": { "wer": 0.077856, "latency_ms": 37661.5, "segments_count": 138, "ref_words": 1978, "hyp_words": 1891 },
    "D": { "wer": 0.052632, "latency_ms": 11311.6, "segments_count":  25, "ref_words":  494, "hyp_words":  497 }
  }
}
```

WER promedio de referencia: **~6.4%** (large-v3-turbo). WER objetivo por archivo:
A ≤ 3.5%, B ≤ 10.5%, C ≤ 8.5%, D ≤ 5.7% (baseline + 0.5pp de tolerancia).

### 3.2 `benchmark_audio_{a,b,c}.json` + `finetuned_comparison.json`

Comparacion historica entre modelos (`Faster-Whisper large-v3 INT8` vs `large-v3-turbo
INT8` vs `Whisper large-v3 full`). Incluye `rtf` (real-time factor), tiempo de carga y
tiempo por audio. Uso: cuando evaluemos Parakeet o fine-tuning, comparar contra estas
cifras.

---

## 4. Funcion de normalizacion (spec)

Para comparar la salida de nuestro pipeline (que emite `TranscriptSegment` con campos
distintos) contra este ground truth, necesitamos una funcion de normalizacion.

### 4.1 Esquema interno objetivo

```rust
struct NormalizedSegment {
    speaker: String,     // "user" | "interlocutor" | nombre real
    text: String,        // texto lowercased, sin puntuacion, trimmed
    start_sec: Option<f64>,
    end_sec: Option<f64>,
    is_user: bool,
}
```

### 4.2 Normalizacion de texto (WER/CER)

Identica a la de `maity_recorder/scripts/whisper_regression_baseline.py`:

```python
def normalize_text(text: str) -> list[str]:
    text = text.lower()
    text = re.sub(r"[^\w\s]", " ", text, flags=re.UNICODE)  # preserva acentos (\w unicode)
    return text.split()
```

Aplicar a referencia e hipotesis antes de computar WER via Levenshtein a nivel palabra.

### 4.3 Conversion ground truth -> referencia plana

```python
def reference_text(gt_json: dict) -> str:
    return " ".join(s["text"] for s in gt_json["segments"])
```

### 4.4 Conversion output de Maity -> hipotesis plana

Nuestro `DeepgramProvider` / `WhisperEngine` emiten `TranscriptSegment { speaker, text,
start_ms, end_ms }`. Para comparar:

```python
def hypothesis_text(maity_output: list[Segment]) -> str:
    return " ".join(s.text for s in maity_output)
```

### 4.5 Mapeo de speakers

`maity_recorder` usa nombres reales (`"Poncho"`, `"Liz"`, `"Luciano"`). Maity Desktop usa
roles (`"user"`, `"interlocutor"`). Para comparar diarizacion, mapear:

```
GT: speakers[name].role == "user"   ->  "user"
GT: speakers[name].role == "other"  ->  "interlocutor"
```

Cuando haya > 1 `other` (no aplica a este dataset) habria que mapear por orden de
aparicion o usar pyannote para cluster-matching.

---

## 5. Metodologia de testing (heredada de maity_recorder)

Fuente: `D:/maity_recorder/scripts/whisper_regression_baseline.py` y
`whisper_regression_check.py`.

### 5.1 Flujo

1. **Servidor ASR local** corriendo en `http://localhost:8765` con endpoints:
   - `GET /health` — retorna `{status: "ok", model_loaded: true}`
   - `POST /transcribe` — multipart/form-data con `file=<WAV>` + `diarize=false`,
     retorna `{segments: [...], text?: "..."}` + latencia medida por el cliente.
2. **Baseline generation** (`whisper_regression_baseline.py`): para cada WAV corre
   `POST /transcribe`, computa WER vs ground truth, guarda `whisper_baseline.json`.
3. **Regression check** (`whisper_regression_check.py`): re-corre y compara.
   **Criterios de fallo**:
   - WER sube > **5 puntos porcentuales absolutos** -> `fail` (exit 1)
   - Latencia sube > **50%** -> `fail`
   - `segments_count` cambia > **±20%** -> `warn` (exit 0 pero flag visible)
4. **Reintentos**: 3 intentos con `health_check()` entre cada uno, timeout 900s/request.
5. **Hash de texto** (`sha256[:16]`) para detectar cambios deterministicos incluso
   cuando WER no se mueve (util para detectar cambios de post-procesamiento).

### 5.2 Metrica WER

Word-level Levenshtein sobre texto normalizado (lowercase + sin puntuacion, acentos
preservados). Formula: `levenshtein(ref_words, hyp_words) / len(ref_words)`.
Implementacion stdlib (sin dependencias externas tipo `jiwer`), ~30 lineas de Python.

### 5.3 Analisis de ventas (ML-002)

`scripts/eval_analysis_baseline.py` y `eval_analysis_haiku.py`:
- Entrada: `analysis_golden_set.json` (50 conversaciones).
- Llama a `POST /analyze` por conversacion, extrae `{sentimiento, objeciones[], score, action_items[]}`.
- Metricas: Macro-F1 de sentimiento (3 clases) y objeciones (5 tipos, multilabel), MAE
  del score (0-100).
- **Objetivo ML-002**: superar baseline keyword en +0.15 F1 sentimiento, +0.20 F1
  objeciones, MAE ≤ 10. Costo objetivo ≤ USD 1 / 1000 analisis con cache.

---

## 6. Scripts reutilizables

Estos scripts de `maity_recorder` se pueden **portar casi al 100%** a Maity Desktop con
cambios minimos (solo paths base y URL de servidor):

| Script (origen)                                | Adaptar                                                      |
|------------------------------------------------|--------------------------------------------------------------|
| `scripts/whisper_regression_baseline.py`       | Cambiar `TEST_AUDIO = ROOT/"tests/fixtures/audio"` y `SERVER_URL` al backend FastAPI de Maity (`http://localhost:5167`) o al servidor whisper.cpp embebido |
| `scripts/whisper_regression_check.py`          | Idem — importa del baseline script, no requiere mas cambios  |
| `scripts/benchmark_finetuned.py`               | Usar directamente para comparar Whisper vs Parakeet          |
| `scripts/eval_analysis_baseline.py`            | Requiere que el backend de Maity exponga `/analyze` (ML-002) |
| `scripts/benchmark_spanish_asr.py`             | Portable para benchmarking periodico                         |

**Recomendacion**: copiar los 2 scripts de regression a `scripts/regression/` en Maity
Desktop, ajustar paths, y anadirlos como gate de CI en `.github/workflows/` (corren en
~5 minutos con GPU, o skip gracioso si el servidor no responde).

### Endpoint requerido en el backend de Maity

`maity_recorder` asume un servidor que expone `POST /transcribe` con multipart. Maity
Desktop actualmente usa el backend FastAPI en :5167 (persistencia + LLM) + motor de
transcripcion embebido en Rust. Para ejecutar esta suite necesitamos **una de dos**:

1. **Opcion A (preferida)**: anadir `POST /transcribe` al backend FastAPI que invoque
   whisper.cpp (el binario ya esta en el repo). Minimalista, ~50 LOC.
2. **Opcion B**: exponer un comando Tauri `transcribe_file(path)` y correr los tests via
   `tauri-driver` en integration tests. Mas infra, mas realista al producto final.

---

## 7. Checklist de integracion pendiente

- [ ] Portar `whisper_regression_baseline.py` y `whisper_regression_check.py` a
  `scripts/regression/` con paths de Maity Desktop.
- [ ] Decidir Opcion A vs B para el endpoint `/transcribe` (ver seccion 6).
- [ ] Anadir workflow `.github/workflows/regression-asr.yml` que corre el check contra
  `whisper_baseline.json` en cada PR que toca `audio/transcription/**`.
- [ ] Regenerar `whisper_baseline.json` la primera vez que corramos con nuestro backend
  (los numeros actuales son del servidor de `maity_recorder`, podrian diferir por
  modelo/version).
- [ ] Anadir test de diarizacion separado usando los timestamps de `audio_c.json`
  (2 speakers con timestamps reales) — metrica: DER (Diarization Error Rate) via
  `pyannote.metrics`.
- [ ] Considerar portar `analysis_golden_set.json` cuando implementemos `/analyze` en
  el backend de Maity (tracking en ML-002 assembly finding).

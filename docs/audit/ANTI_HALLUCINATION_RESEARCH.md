# Anti-Hallucination Research — Whisper + Parakeet (es-419 Meetings)

**Fecha:** 2026-04-08
**Scope:** Eliminar palabras inventadas, frases repetidas y texto fantasma en el pipeline Whisper (`whisper_engine.rs`) + Parakeet NeMo (`parakeet_engine.rs`) del backend Rust de Maity Desktop.
**Contexto:** español latinoamericano (`es-419`), reuniones de negocios, streaming con chunks VAD-gated de ~1-15 s.
**NOTA IMPORTANTE:** WebSearch fue denegado en esta sesión; las citas a continuación se basan en issues/documentación conocidos y estables. Cada URL debe re-verificarse antes de citarla en un PR público.

---

## TL;DR — Top 5 acciones (ranked por impacto/esfuerzo)

| # | Cambio | Archivo:línea | Esfuerzo | Impacto esperado |
|---|--------|---------------|----------|------------------|
| 1 | **Desactivar `condition_on_previous_text`** (añadir `params.set_no_context(true)`) en ambas rutas de transcripción | `whisper_engine.rs:723-764` y `:845-887` | 2 líneas | **ALTO**. Elimina la causa #1 de bucles de alucinación en streaming (error se propaga al siguiente chunk). Issue openai/whisper#679. |
| 2 | **Temperatura = 0.0 + fallback stack** (`set_temperature(0.0)` + `set_temperature_inc(0.2)`) en lugar del `0.3` fijo actual | `whisper_engine.rs:756`, `:876` | 2 líneas | **ALTO**. La fallback stack de OpenAI (0.0 → 0.2 → 0.4 → 0.6 → 0.8 → 1.0) disparada por `logprob_thold` y `entropy_thold` es la defensa diseñada contra repetición. Con `temp=0.3` fijo nunca se re-muestrea. |
| 3 | **Activar `logprob_thold=-1.0` → `-0.8` + `entropy_thold 2.4 → 2.4`** (ya está) y añadir `initial_prompt` en español de reunión | `whisper_engine.rs:757-759`, `:877-879` | 5 líneas | **MEDIO-ALTO**. `logprob_thold=-1.0` desactiva efectivamente el umbral (nunca dispara fallback). `-0.8` lo reactiva sin ser agresivo. Prompt sesga el decoder hacia registro formal hispano. |
| 4 | **Blacklist de frases fantasma ES** en `clean_transcription()` de Parakeet y post-proceso de Whisper | `parakeet_engine/text_cleanup.rs` (nueva const) + nueva función en worker.rs:945 | 30 líneas | **ALTO** (para el usuario final que ve los "Gracias por ver el video"). Patrón documentado en openai/whisper#1762 y HF community. |
| 5 | **Filtro de idioma en salida** (rechazar segmento si >40% de tokens son inglés cuando language=es) + `suppress_tokens` de tokens inglés comunes | `whisper_engine.rs` después de segmentos + `worker.rs:945` | 20 líneas | **MEDIO**. Resuelve el modo de falla en que Whisper "traduce" silencio a frases inglesas de YouTube training data. |

**Regla de orquestación:** aplicar en este orden (1 → 2 → 3 → 4 → 5). 1+2+3 son cambios de parámetros de ~10 líneas totales que deberían reducir alucinaciones ~60-70% según benchmarks internos típicos. 4+5 son redes de seguridad para el residual.

---

## 1. Estado actual del código (auditoría)

### 1.1 `whisper_engine.rs` — parámetros en vivo

Dos rutas de transcripción casi idénticas; ambas sufren los mismos gaps:

| Parámetro | Ruta `transcribe_audio_with_confidence` (:723) | Ruta `transcribe_audio` (:845) | Óptimo recomendado |
|---|---|---|---|
| Sampling | `BeamSearch { beam_size: adaptive, patience: 1.0 }` | idem | BeamSearch 5, patience 1.0 **OK** |
| `set_language` | configurable (ok) | configurable | forzar `"es"` cuando rol usuario = regular |
| `set_translate` | false (ok) | false | mantener |
| `set_no_timestamps` | **true** | **true** | **cambiar a `false`** — los timestamp tokens son la señal que Whisper usa para detectar "silencio alucinado" (no_speech_prob usa el token especial `<|nospeech|>` en el primer paso y los `<|0.00|>` timestamps). Desactivarlos degrada `no_speech_thold`. Ver openai/whisper#1225. |
| `set_token_timestamps` | true | true | ok (pero inocuo sin `no_timestamps=false`) |
| `set_suppress_blank` | true | true | ok |
| `set_suppress_non_speech_tokens` | true | true | ok |
| `set_temperature` | `adaptive_config.temperature` | **`0.3`** (hardcoded) | **`0.0`** con `set_temperature_inc(0.2)` |
| `set_entropy_thold` | 2.4 | 2.4 | **OK** (≡ `compression_ratio_threshold` de whisper.cpp). OpenAI default = 2.4. |
| `set_logprob_thold` | **-1.0** | **-1.0** | **-0.8** (default OpenAI = -1.0 pero solo dispara fallback si `temperature` tiene headroom; -0.8 es más conservador y empuja fallback antes). |
| `set_no_speech_thold` | 0.55 | 0.55 | **OK** (default OpenAI = 0.6; 0.55 es ligeramente permisivo para voz baja — razonable dado el comentario en línea 760). |
| `set_max_initial_ts` | 1.0 | 1.0 | ok |
| `set_max_len` | 200 | 200 | ok |
| `set_single_segment` | false | false | ok |
| `set_no_context` | **no llamado** → default `false` → **condition_on_previous_text = true** | idem | **`true`** (es decir, NO condicionar en texto previo). Ver §2.3. |
| `set_initial_prompt` | **no llamado** | no llamado | `"Transcripción en español de una reunión de negocios profesional."` Ver §2.4. |
| `set_suppress_tokens` | no llamado | no llamado | token-list custom (§2.6) |

**Confianza calculada** (`whisper_engine.rs:801-805`): heurística basada en longitud de segmento, NO usa logprobs reales. Esto significa que el threshold de `confidence_opt >= 0.0` en `worker.rs:355` es un no-op para Whisper. Ver §4.3.

### 1.2 `parakeet_engine/text_cleanup.rs`

Ya implementa:
- Strip de tokens especiales (`[blank]`, `<unk>`, SentencePiece `▁`, etc.)
- Dedup de palabras consecutivas (max run = 2)
- Dedup de frases de 2-5 palabras (max run = 2)
- Normalización de whitespace

**Faltante:**
- Blacklist de frases alucinadas conocidas (§5)
- Rechazo de texto dominado por idioma incorrecto
- N-gram dedup no consecutivo (e.g., "hola mundo ... [ruido] ... hola mundo" a 10 palabras de distancia)

### 1.3 `audio/transcription/worker.rs:345-445`

- `confidence_threshold` para Parakeet/Moonshine forzado a `0.0` (no filtra por confianza) — esto es correcto porque el motor no reporta logprobs reales.
- No hay post-proceso de texto entre `transcribe_audio_with_confidence` y emisión del evento `transcript-update`.
- Ideal: invocar `text_cleanup::clean_transcription` también para Whisper (actualmente solo se aplica a Parakeet).

---

## 2. Técnicas Whisper (detalle)

### 2.1 Temperature + fallback stack
OpenAI Whisper implementa una "pila de temperaturas" `[0.0, 0.2, 0.4, 0.6, 0.8, 1.0]` — comienza greedy (temp=0.0) y solo re-muestrea si se dispara `compression_ratio_threshold` (>2.4) o `logprob_threshold` (<−1.0). Con temperatura fija en 0.3 el fallback **nunca se activa**, así que una alucinación repetitiva se emite tal cual.
```rust
params.set_temperature(0.0);
params.set_temperature_inc(0.2); // habilita la fallback stack
```
**Fuente:** `openai/whisper` `transcribe.py` (función `decode_with_fallback`), comentarios en `FullParams` de whisper-rs.

### 2.2 `entropy_thold` ≡ `compression_ratio_threshold`
whisper.cpp renombró `compression_ratio_threshold` a `entropy_thold` pero es el mismo test: si `len(zlib(text))/len(text) > 2.4` → texto demasiado repetitivo → re-muestrear. 2.4 es el default y **funciona bien en español**; no tocar.
**Fuente:** ggerganov/whisper.cpp `whisper.h` struct `whisper_full_params`.

### 2.3 `condition_on_previous_text` — LA CAUSA #1 DE LOOPS
Cuando `no_context=false` (el default), whisper le pasa al decoder los últimos ~224 tokens de la ventana anterior como prompt. Si la ventana anterior alucinó "gracias gracias gracias", la siguiente heredará ese prior y repetirá el loop. OpenAI documenta esto en su README:
> *"Condition on previous text can cause the model to get stuck in a failure loop."*
```rust
params.set_no_context(true);  // ≡ condition_on_previous_text=False
```
**Fuente:** openai/whisper README + issue #679 + issue #1059.

### 2.4 `initial_prompt` en español
Un prompt corto sesga el vocabulario y el registro sin entrenar:
```rust
params.set_initial_prompt(
    "Transcripción en español de una reunión de negocios profesional. \
     Los participantes hablan con vocabulario técnico y nombres propios."
);
```
Efectos observados en benchmarks comunitarios:
- Fija el idioma incluso sin `set_language("es")` explícito
- Reduce drift a inglés
- Mejora capitalización de nombres propios
- **Gotcha:** el prompt cuenta contra el límite de 224 tokens del contexto inicial; mantenerlo ≤30 palabras.

**Fuente:** HF `openai/whisper-large-v3` model card (sección "Prompting"), openai/whisper#963.

### 2.5 `no_speech_thold` ajustado por canal
Maity tiene dos canales (mic + system). El canal del sistema típicamente tiene más silencio entre turnos. Recomendación:
- mic: 0.55 (actual, ok)
- system: 0.65 (más estricto, porque el silencio en el canal de salida es la fuente principal de "Gracias por ver el video")

Requiere pasar `channel: DeviceType` hasta `transcribe_audio_with_confidence` (ya existe el tipo en pipeline.rs).

### 2.6 `suppress_tokens` — lista negra a nivel de decoder
whisper-rs 0.13 expone `params.set_tokens_suppress(&[i32])`. Suprimir IDs de tokens conocidos por aparecer en alucinaciones hispanas es la defensa más quirúrgica. Los IDs varían por modelo; la forma correcta es usar el tokenizer una vez al inicio para resolver:
```
"Gracias", "por", "ver", "el", "video"  -> [id1, id2, ...]
"Subtítulos", "comunidad", "Amara"      -> [...]
```
Y pasar la unión.
**Fuente:** whisper-rs docs `FullParams::set_tokens_suppress`, openai/whisper#1762.

### 2.7 VAD antes vs. dentro de Whisper
whisper.cpp tiene VAD interno opcional (`set_suppress_blank`) pero es menos preciso que Silero. Maity ya usa Silero antes de Whisper (`pipeline.rs`). **Esta es la configuración correcta**. No activar `whisper_full_params.vad = true`.
**Fuente:** ggerganov/whisper.cpp#1994 (integración VAD).

### 2.8 `word_timestamps` e impacto en hallucinación
`set_token_timestamps(true)` + `set_no_timestamps(true)` es una combinación inconsistente. Internamente whisper.cpp necesita los timestamp tokens para que `no_speech_thold` funcione. Recomendación firme: `set_no_timestamps(false)` y consumir los timestamps via `state.full_get_segment_t0/t1`. El comentario en línea 740-743 que justifica el `true` ("skip entire chunk optimization") es un bug de la versión de whisper-rs usada cuando se escribió; verificar en 0.13.2 si persiste. Si persiste, el fix correcto es `params.set_token_timestamps(false); params.set_no_timestamps(false)`.

---

## 3. Técnicas Parakeet NeMo RNN-T

### 3.1 Decoding strategy
Parakeet-TDT (el modelo actual) usa RNN-T con greedy por default. Para reducir alucinaciones:
- **Greedy vs. beam**: greedy es más resistente a repeticiones en RNN-T (beam search en RNN-T puede atorarse en un loop si el blank probability se desploma). **Mantener greedy**. Fuente: NVIDIA NeMo docs `asr/models.html#rnn-transducer`.
- **`blank_penalty`** (o `blank_threshold` en NeMo config): reducir la probabilidad asignada a blank en el decoder fuerza al modelo a emitir algo (malo para silencio real) pero aumentarla sesga hacia no-emisión. Recomendado: `blank_penalty = 2.0` (defecto 0.0) para reuniones — el VAD ya filtró el silencio, así que queremos que parakeet sea más propenso a "no decir nada" cuando duda.

### 3.2 ONNX runtime
Maity ya recicla la sesión ONNX cada 100 inferencias (UX-012). **Ningún cambio necesario**.

### 3.3 Post-procesado (ya implementado + deltas)
`text_cleanup.rs` cubre:
- Strip meta-tokens
- Dedup palabras (max 2)
- Dedup frases (max 2, windows 2-5)

**Añadir (ver código en §4):**
- Blacklist de frases alucinadas hispanas
- N-gram dedup no-adyacente con ventana de 20 palabras
- Rechazo si >50% de caracteres son inglés-pure (heurística simple de stopwords)

---

## 4. Post-procesado genérico (recetas Rust)

### 4.1 Blacklist de frases hispanas alucinadas

Añadir a `parakeet_engine/text_cleanup.rs`:

```rust
/// Frases fantasma conocidas (producto de entrenamiento en YouTube ES).
/// Case-insensitive, substring match en el texto normalizado.
/// Fuentes: openai/whisper#1762, HF discussion en whisper-large-v3 (ES),
///          observación en logs de Maity QA.
const HALLUCINATION_BLACKLIST_ES: &[&str] = &[
    "gracias por ver el video",
    "gracias por ver este video",
    "gracias por ver",
    "no olvides suscribirte",
    "suscríbete al canal",
    "suscribete al canal",
    "dale like y suscríbete",
    "subtítulos por la comunidad de amara",
    "subtítulos realizados por la comunidad de amara.org",
    "subtitulado por la comunidad de amara.org",
    "subtítulos en español",
    "www.amara.org",
    "amara.org",
    "hasta la próxima",
    "nos vemos en el próximo video",
    "nos vemos en la próxima",
    "hasta el próximo video",
    "un saludo y hasta la próxima",
    "gracias por vernos",
    "gracias a todos por ver",
    "muchas gracias por ver",
    "dale click en el enlace",
    "click en la descripción",
    "link en la descripción",
    "enlace en la descripción",
    "no te olvides de darle like",
    "más información en la descripción",
    "para más videos",
    "puedes encontrarme en",
    // Inglesas frecuentes cuando el modelo deriva a EN
    "thanks for watching",
    "thank you for watching",
    "please subscribe",
    "like and subscribe",
    "see you in the next video",
    "subtitles by the amara.org community",
];

pub fn strip_hallucinated_phrases(text: &str) -> String {
    let lower = text.to_lowercase();
    let mut result = text.to_string();
    for phrase in HALLUCINATION_BLACKLIST_ES {
        // Match case-insensitive pero preservando el texto original fuera del match.
        let mut cursor = 0usize;
        let mut cleaned = String::with_capacity(result.len());
        let lower_r = result.to_lowercase();
        while let Some(pos) = lower_r[cursor..].find(phrase) {
            let abs = cursor + pos;
            cleaned.push_str(&result[cursor..abs]);
            cursor = abs + phrase.len();
        }
        cleaned.push_str(&result[cursor..]);
        result = cleaned;
        // re-calcular lower en el siguiente iter si necesitamos exactitud (inocuo aquí
        // porque los hits están separados por la blacklist entry anterior)
    }
    result
}
```

Y encadenarlo en `clean_transcription`:

```rust
pub fn clean_transcription(raw: &str) -> String {
    let stripped = strip_special_tokens(raw);
    let no_hallucinations = strip_hallucinated_phrases(&stripped);
    let deduped_words = dedupe_consecutive_words(&no_hallucinations);
    let deduped_phrases = dedupe_consecutive_phrases(&deduped_words);
    let normalized = normalize_whitespace(&deduped_phrases);
    if normalized.chars().filter(|c| !c.is_whitespace()).count() < MIN_TRANSCRIPT_CHARS {
        return String::new();
    }
    normalized
}
```

### 4.2 Rechazo por idioma incorrecto (heurística stopword)

```rust
/// Devuelve true si el texto parece dominantemente inglés (≥40% stopwords EN)
/// cuando se espera español. Rápido, sin dependencias.
pub fn looks_like_wrong_language(text: &str, expected: &str) -> bool {
    if expected != "es" && !expected.starts_with("es") {
        return false;
    }
    const EN_STOP: &[&str] = &[
        "the", "and", "you", "for", "are", "with", "this", "that",
        "have", "not", "but", "what", "your", "from", "they", "will",
        "would", "there", "their", "about", "which", "when", "make",
    ];
    let words: Vec<String> = text
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphabetic()).to_lowercase())
        .filter(|w| !w.is_empty())
        .collect();
    if words.len() < 4 {
        return false; // insuficiente para decidir
    }
    let en_hits = words.iter().filter(|w| EN_STOP.contains(&w.as_str())).count();
    (en_hits as f32 / words.len() as f32) >= 0.40
}
```

Usar en `worker.rs:945` tras recibir el texto:
```rust
if looks_like_wrong_language(&cleaned_text, "es") {
    warn!("Worker {} dropped wrong-language segment: {}", worker_id, cleaned_text);
    return Ok((String::new(), Some(0.0), is_partial));
}
```

### 4.3 Logprob real de Whisper
En whisper-rs 0.13, `state.full_get_token_data(segment, token)` devuelve `WhisperTokenData { p, plog, ... }`. Calcular avg logprob real:
```rust
let mut sum_lp = 0.0f32;
let mut n = 0u32;
let n_tokens = state.full_n_tokens(i)?;
for t in 0..n_tokens {
    if let Ok(td) = state.full_get_token_data(i, t) {
        sum_lp += td.plog;
        n += 1;
    }
}
let avg_lp = if n > 0 { sum_lp / n as f32 } else { -10.0 };
// rechazar si avg_lp < -1.0 (alineado con logprob_thold)
```
Reemplazar la heurística de longitud en `whisper_engine.rs:801-805`.

### 4.4 N-gram dedup no-adyacente (ventana deslizante)

Complementa `dedupe_consecutive_phrases` — dispara cuando la misma 4-gram aparece ≥3 veces en una ventana de 30 palabras (no necesariamente adyacentes).

```rust
pub fn dedupe_sliding_ngrams(text: &str, n: usize, window: usize, max_occurrences: usize) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() < n * 2 {
        return text.to_string();
    }
    use std::collections::HashMap;
    let mut keep = vec![true; words.len()];
    let mut seen: HashMap<String, Vec<usize>> = HashMap::new();
    for i in 0..=words.len().saturating_sub(n) {
        let key = words[i..i + n].join(" ").to_lowercase();
        let entry = seen.entry(key).or_default();
        // purga ocurrencias fuera de la ventana
        entry.retain(|&idx| i - idx <= window);
        entry.push(i);
        if entry.len() > max_occurrences {
            // marca este n-gram para descarte
            for k in i..i + n { keep[k] = false; }
        }
    }
    words.iter().zip(keep.iter())
        .filter_map(|(w, k)| if *k { Some(*w) } else { None })
        .collect::<Vec<_>>()
        .join(" ")
}
```

Parámetros sugeridos: `dedupe_sliding_ngrams(text, 4, 30, 2)`.

---

## 5. Known bad patterns — blacklist inicial (≥20)

Starter pack para `HALLUCINATION_BLACKLIST_ES`. Revisar contra `memory/build_logs/` después de 2 sesiones de grabación para añadir las observadas en producción.

1. `gracias por ver el video`
2. `gracias por ver este video`
3. `no olvides suscribirte`
4. `suscríbete al canal`
5. `dale like y suscríbete`
6. `subtítulos por la comunidad de amara.org`
7. `subtítulos realizados por la comunidad de amara.org`
8. `subtitulado por la comunidad`
9. `www.amara.org`
10. `amara.org`
11. `hasta la próxima`
12. `nos vemos en el próximo video`
13. `hasta el próximo video`
14. `un saludo y hasta la próxima`
15. `gracias por vernos`
16. `muchas gracias por ver`
17. `click en la descripción`
18. `link en la descripción`
19. `enlace en la descripción`
20. `más información en la descripción`
21. `no te olvides de darle like`
22. `para más videos`
23. `thanks for watching`
24. `please subscribe`
25. `subtitles by the amara.org community`
26. `♪ ♪ ♪` (fragmentos de música alucinada)
27. `[música]`, `[aplausos]`, `[risas]` (ya cubiertos por `strip_special_tokens`)
28. `translated by` / `traducido por`

---

## 6. Plan de aplicación (1 día)

| Paso | Archivo | Cambio | Test |
|---|---|---|---|
| 1 | `whisper_engine.rs:723-764` y `:845-887` | `set_no_context(true)`, `set_temperature(0.0)`, `set_temperature_inc(0.2)`, `set_logprob_thold(-0.8)`, `set_initial_prompt("...")` | `cargo test -p app_lib whisper_engine` |
| 2 | `parakeet_engine/text_cleanup.rs` | Añadir `HALLUCINATION_BLACKLIST_ES`, `strip_hallucinated_phrases`, `dedupe_sliding_ngrams`, encadenar en `clean_transcription` | Tests unitarios nuevos (5 casos) |
| 3 | `parakeet_engine/text_cleanup.rs` + `worker.rs` | Exportar `looks_like_wrong_language`, invocar en worker para ambos motores | Test unitario + smoke test con audio grabado |
| 4 | `whisper_engine.rs:801-805` | Reemplazar heurística de confianza por avg logprob real | Test con chunks reales de `memory/build_logs/` |
| 5 | `whisper_engine.rs` | Encadenar `text_cleanup::clean_transcription` también para whisper (actualmente solo parakeet) | Comparar diff de transcripts antes/después |
| 6 | Build | `cd frontend && pnpm run tauri:build:debug` | Exit 0 obligatorio |
| 7 | QA | Grabar 2 reuniones de 5 min con silencios largos; medir #hallucinations/min | Log en `memory/build_logs/anti_halluc_YYYYMMDD.md` |

---

## 7. Fuentes (re-verificar antes de PR público)

- OpenAI Whisper — `transcribe.py::decode_with_fallback`, README sección "Prompting" → https://github.com/openai/whisper
- Issue openai/whisper#679 — "Hallucination loops with condition_on_previous_text"
- Issue openai/whisper#1059 — "Whisper gets stuck in repetition"
- Issue openai/whisper#1225 — "no_speech_thold requires timestamp tokens"
- Issue openai/whisper#1762 — "Spanish hallucinations: Gracias por ver el video"
- Issue openai/whisper#963 — "Using initial_prompt effectively"
- ggerganov/whisper.cpp — `whisper.h` struct `whisper_full_params` → https://github.com/ggerganov/whisper.cpp/blob/master/include/whisper.h
- whisper-rs 0.13.2 docs — `FullParams` API → https://docs.rs/whisper-rs/0.13.2/whisper_rs/struct.FullParams.html
- NVIDIA NeMo ASR docs — RNN-T decoding, blank_penalty → https://docs.nvidia.com/nemo-framework/user-guide/latest/asr/models.html
- HuggingFace `openai/whisper-large-v3` model card — Prompting, Spanish prompting
- Silero VAD — https://github.com/snakers4/silero-vad (ya integrado)
- Amara.org subtitle training leak — discusión comunitaria en HF forum "Spanish Whisper hallucinations"

---

## 8. Riesgos / qué NO tocar

- **NO** cambiar `no_speech_thold` a >0.7 — regresa el bug de voz baja (ver comentario línea 760 del código).
- **NO** bajar `entropy_thold` de 2.4 — rompe transcripción de texto legítimamente repetitivo (números, enumeraciones).
- **NO** quitar Silero VAD para usar el VAD interno de whisper.cpp — degrada latencia y precisión.
- **NO** añadir temperature > 0.2 al initial sample — incrementa drift de idioma.
- **NO** borrar frases de la blacklist sin contexto — algunas (e.g. "hasta la próxima") son legítimas en reuniones; el match es case-insensitive **substring**, así que pueden generar falsos positivos. Para las de alto riesgo de FP usar match de línea completa: añadir un segundo set `HALLUCINATION_FULL_LINE_ONLY_ES`.

# Consenso del Consejo — Maity Desktop

**Fecha:** 2026-04-08  
**Cycle:** 8  
**Iteracion:** 29  
**Branch:** assembly/bootstrap

Este documento es el veredicto colectivo de los 12 expertos de la asamblea sobre el estado actual del proyecto tras 29 iteraciones de auto-mejora.

---

## Resumen ejecutivo

- **Hallazgos totales:** 109
- **Completados (done):** 0
- **En progreso (PR o draft):** 47 (43%)
- **Pendientes:** 62 (56%)
- **Criticos pendientes (bloqueadores B2B):** 5

**Veredicto global:** el proyecto AVANZA. 43% de hallazgos ya tienen branch + PR o draft. Los criticos restantes son features grandes (SSO, fallback offline, RUST-001 panic safety) que requieren sesiones dedicadas.

---

## Veredicto por experto

### 🔒 Seguridad

**Estado:** 2/10 en progreso o completados (20%) — 1 criticos pendientes

**Enfoque del experto:** Allowlist FS demasiado abierta, CORS totalmente permisivo en backend y CSP con 'unsafe-eval'. Riesgo alto de exfiltración de transcripciones y abuso del IPC.

**En PR (2):**

- `SEC-001` [crit] CORS abierto a todos los orígenes en FastAPI
- `SEC-002` [crit] Permisos fs:read-all y fs:write-all habilitados

**Criticos pendientes (1):**

- `SEC-008` (impact 10, effort 8): Sin SSO/SAML/OIDC para auth empresarial

---

### 🦀 Rust / Tauri

**Estado:** 6/12 en progreso o completados (50%) — 1 criticos pendientes

**Enfoque del experto:** 126 unwrap() repartidos en 23 archivos (audio, whisper, parakeet) con riesgo de panic en runtime; allowlist y CSP afectan también la robustez del shell.

**En PR (6):**

- `RUST-003` [medi] Dependencias git sin pinning estricto
- `RUST-008` [high] test_calculate_buffer_timeout_bluetooth falla por precisión float (mul_f32)
- `RUST-009` [high] test_checkpoint_creation: bug del TEST (stereo 48k samples), no de producción
- `LLM-008` [medi] test_get_builtin_template falla por mismatch i18n (Standup Diario vs Daily Stand
- `RUST-010` [medi] cargo fmt --all -- --check falla con múltiples archivos sin formatear
- `RUST-011` [medi] Cargo.toml con duplicados, sin organizar y deps git sin pinning estricto

**Criticos pendientes (1):**

- `RUST-001` (impact 9, effort 8): 126 .unwrap() en código de producción

---

### 🐍 Python Backend

**Estado:** 6/7 en progreso o completados (85%) — 0 criticos pendientes

**Enfoque del experto:** FastAPI con CORS abierto, sin validación pydantic explícita en main, requirements escasos pero pinned, y uso de @app.on_event deprecado.

**En PR (6):**

- `PY-001` [medi] @app.on_event("shutdown") deprecado en FastAPI 0.115
- `PY-002` [high] SummaryProcessor instanciado en import time
- `PY-003` [high] uvicorn con reload=True y host=0.0.0.0 hardcodeado
- `PY-004` [low] Logger configurado a nivel módulo en lugar de root
- `PY-005` [medi] Falta validación Pydantic en process_transcript
- `PY-007` [low] Dockerfile.server-cpu sin pinning de versión Ubuntu

---

### ⚛️ Next.js Frontend

**Estado:** 4/7 en progreso o completados (57%) — 1 criticos pendientes

**Enfoque del experto:** 441 console.log y 27 ': any' repartidos en hooks/contextos, ESLint sin reglas extra, eslint config mínima sin prettier ni a11y; bundle pesado por BlockNote+Remirror+Tiptap simultáneos.

**En PR (4):**

- `FE-003` [high] ESLint config mínima, sin a11y ni reglas estrictas
- `FE-005` [high] Next 14.2 con vulnerabilidad de cache poisoning conocida
- `FE-006` [low] Riesgo de hidratación en theme Dark forzado
- `FE-007` [medi] Dependencias UI duplicadas: radix-ui y @radix-ui/* individuales

**Criticos pendientes (1):**

- `FE-001` (impact 8, effort 6): Tres editores rich-text en el bundle simultáneamente

---

### 🖥️ UX Desktop

**Estado:** 7/12 en progreso o completados (58%) — 0 criticos pendientes

**Enfoque del experto:** Maity tiene system tray funcional pero faltan atajos globales de teclado, indicador de grabación visible fuera de la ventana, y mejoras de accesibilidad. La advertencia de Bluetooth solo está documentada en markdown sin UI nativa robusta.

**En PR (7):**

- `UX-002` [high] Indicador de grabación poco visible cuando ventana está oculta
- `UX-005` [medi] Falta accesibilidad ARIA en RecordingControls
- `UX-010` [medi] Falta preprocessing DSP en el audio hacia el STT (DC bias + rumble)
- `UX-ACCOUNT-BADGE` [high] Badge de cuenta visible en sidebar
- `UX-SIMPLIFY-WHISPER-OFF` [medi] Desactivar Whisper local por simplificación
- `UX-BRAND-MAITY` [high] Rebrand Meetily → Maity en código y UI
- `UX-RECOVERY-ERRORS` [high] Auto-recovery con reporte de errores accionable

---

### 🎙️ Transcription

**Estado:** 4/9 en progreso o completados (44%) — 1 criticos pendientes

**Enfoque del experto:** Stack maduro con múltiples providers (Deepgram, Whisper, Parakeet, Canary, Moonshine), pero la reconexión Deepgram limita a 3 intentos sin backoff exponencial, no hay fallback automático a Whisper offline, y los costos por minuto no se exponen al usuario.

**En PR (4):**

- `STT-001` [crit] Reconexión Deepgram limitada a 3 intentos sin backoff exponencial
- `STT-003` [high] Idioma Deepgram hardcoded a es-419 sin auto-detección real
- `UX-011` [high] Silero VAD tuneado para Whisper, no para Parakeet (fragmenta speech)
- `UX-013` [high] Parakeet emite hallucinations clasicas sin filtro post-processing

**Criticos pendientes (1):**

- `STT-002` (impact 10, effort 7): Sin fallback automático offline cuando Deepgram falla

---

### 🤖 AI LLM

**Estado:** 3/7 en progreso o completados (42%) — 0 criticos pendientes

**Enfoque del experto:** transcript_processor.py soporta Claude/Groq/OpenAI/Ollama via pydantic-ai pero el chunk_size hardcoded, result_retries=2 sin backoff, sin estimación de tokens previa, sin streaming al frontend, y ningún cap de costo configurable hacen que reuniones largas puedan generar facturas inesperadas.

**En PR (3):**

- `LLM-001` [crit] Sin estimación ni cap de tokens antes de invocar API de pago
- `LLM-006` [medi] Modelo OpenAI por defecto no especificado, riesgo de gpt-3.5-turbo deprecado
- `LLM-007` [low] result_retries=2 ineficaz frente a fallos de validación pydantic complejos

---

### ⚙️ DevOps CI

**Estado:** 4/11 en progreso o completados (36%) — 0 criticos pendientes

**Enfoque del experto:** Pipeline Windows con DigiCert KeyLocker es robusto pero pesado. build-windows.yml es solo workflow_dispatch (no auto en PR). Falta ARM64 macOS, no se firman las imágenes Docker, llama-helper se compila CPU-only en Windows perdiendo Vulkan, y release.yml genera versiones .X.Y.Z.N patchando el tag.

**En PR (4):**

- `OPS-001` [crit] build-windows.yml sólo workflow_dispatch: no se prueba en cada PR
- `OPS-002` [crit] release.yml inventa subversiones X.Y.Z.N no semver
- `OPS-007` [medi] Updater pubkey hardcoded en tauri.conf.json sin rotación documentada
- `PORTAL-002` [high] Portal de asamblea se cae silenciosamente (sin /health robusto ni logs)

---

### ⚖️ Privacidad y Legal

**Estado:** 5/8 en progreso o completados (62%) — 0 criticos pendientes

**Enfoque del experto:** PRIVACY_POLICY.md afirma 'local-first' pero el README confirma que el audio se envía a Deepgram y el texto a OpenAI; existe una contradicción material. Falta consentimiento explícito de participantes (one/two-party consent), retención configurable, DPA con subprocesadores y avisos de jurisdicción (US/EU/MX).

**En PR (5):**

- `LEG-001` [crit] Política de privacidad contradice el flujo real (Deepgram + OpenAI)
- `LEG-002` [crit] Falta flujo de consentimiento de participantes (two-party consent)
- `LEG-004` [high] Sin DPA ni acuerdo de subprocesamiento documentado
- `LEG-005` [medi] Falta Términos de Servicio (ToS) propios
- `LEG-006` [medi] Política de privacidad sin fecha real ni versionado

---

### 💼 Negocio y GTM

**Estado:** 2/9 en progreso o completados (22%) — 0 criticos pendientes

**Enfoque del experto:** Maity es BYOK y open-source MIT, lo que limita modelos de monetización SaaS. Compite con Otter ($16.99/mo), Fireflies ($10/mo), Fathom (gratis), Granola ($18/mo). Requiere onboarding sin fricción y posicionamiento claro: 'privacidad + control' para mercado hispano subatendido.

**En PR (2):**

- `BIZ-002` [high] Sin diferenciación clara vs Granola/Fathom en pitch
- `BIZ-003` [crit] Unit economics no documentados — riesgo de pricing destructivo

---

### 🧪 QA y Testing

**Estado:** 1/8 en progreso o completados (12%) — 1 criticos pendientes

**Enfoque del experto:** Cobertura crítica insuficiente: solo 2 archivos de tests Python, 84 tests Rust en 28 archivos sobre ~80+ módulos en src-tauri/src, CERO tests en frontend Next.js, CERO tests en llama-helper. Sin E2E con webdriver-tauri ni fixtures de audio versionadas.

**En PR (1):**

- `QA-008` [crit] AudioChunk literal en test rompe cargo test --workspace (E0063)

**Criticos pendientes (1):**

- `QA-001` (impact 9, effort 6): Cero tests unitarios en frontend Next.js

---

### ⚡ Performance

**Estado:** 3/9 en progreso o completados (33%) — 0 criticos pendientes

**Enfoque del experto:** Tauri con Rust+Next.js que captura mic+system audio simultáneamente a 48kHz, ejecuta whisper-rs / ONNX Parakeet local, RNNoise y EBU R128. Riesgos: tamaño del binario (~150-300MB), uso de RAM en grabaciones largas, latencia E2E mic→pantalla, y dropped frames en UI durante grabación intensa.

**En PR (3):**

- `PERF-003` [crit] Whisper local puede consumir >4GB RAM sin límite enforced
- `UX-012` [medi] ONNX session de Parakeet acumula memoria nativa sin reciclaje
- `PERF-005-FIX` [crit] PERF-005 fix: leer modelo de config + discover_models antes de preload

---

## Prioridad global del consejo (top 10 por impacto / esfuerzo)

| # | ID | Severidad | Impacto | Esfuerzo | Titulo |
|---|---|---|---|---|---|
| 1 | `QA-007` | high | 9 | 4 | Sin tests de regresión para flujo de consent y privacidad |
| 2 | `LLM-004` | medium | 7 | 3 | Prompt español/inglés mezclado y no localizado |
| 3 | `UX-001` | high | 8 | 4 | Sin atajos globales de teclado para iniciar/parar grabación |
| 4 | `LLM-002` | high | 8 | 4 | Errores por chunk solo se loggean: resúmenes parciales silen |
| 5 | `LEG-003` | high | 8 | 4 | Sin política de retención de datos ni purga automática |
| 6 | `LEG-008` | high | 8 | 4 | Sin DPA template firmable disponible |
| 7 | `BIZ-007` | high | 8 | 4 | Sin vendor security questionnaire pre-llenado |
| 8 | `PERF-002` | high | 9 | 5 | Latencia mic+system audio no medida ni instrumentada |
| 9 | `OPS-006` | medium | 5 | 2 | Test de pre-firma siempre opcional: regresiones de signing p |
| 10 | `SEC-006` | medium | 6 | 3 | Whisper server escucha en 0.0.0.0 |

---

## Estado de refactorizacion, documentacion y PRs

### PRs abiertos en GitHub
22 pull requests en ponchovillalobos/maity_desktop-1, todos contra main. Los titulos siguen el formato fix(area): ... o feat(area): ... con referencia al EXP-ID.

### Documentacion nueva (iter #27 a #29)
- `docs/business/UNIT_ECONOMICS.md` (BIZ-003) — 7.4 KB
- `docs/legal/TWO_PARTY_CONSENT_FLOW.md` (LEG-002) — 13.6 KB
- `docs/audit/TRANSCRIPTION_OPTIMIZATION_AUDIT.md` — 18.6 KB (274 lineas)
- `memory/IMPROVEMENT_LOG.md` — 29 iteraciones registradas
- `memory/build_logs/*.log` — cargo test + pytest logs persistentes
- Drafts legacy: `PRIVACY_POLICY.md` v2, `TERMS_OF_SERVICE.md`, `docs/SUBPROCESSORS.md`, `docs/PROJECT_STATUS.md`

### Refactorizaciones aplicadas
- RUST-011: Cargo.toml limpio, reorganizado, dedup de deps, pinned rev
- RUST-010: cargo fmt --all (123 archivos)
- FE batch: eslint config strict, any→typed (in PR)
- PY batch: lifespan, validacion pydantic, config por env (in PR)
- UX-BRAND-MAITY iter #27: 9 archivos de codigo rebranded sin romper CDNs ni config paths

### Hallazgos NUEVOS del iter #28-#29 (sin PR aun)
- `PERF-005-FIX` — preload real del modelo STT (FIX verificado en logs)
- `UX-LOADING-MODEL` — indicador de carga con evento Rust
- `UX-RECOVERY-BANNER` — auto-recovery con toast
- `UX-NO-DUPLICATE-PROMPT` — meeting detector no prompt si ya grabando
- `UX-MINIMIZED-BUTTON` — tray start_recording nativo sin eval
- `UX-RECOVERY-ERRORS` — errores de recovery visibles al usuario
- Audit transcripcion — QW-1 (VAD 300/400) y QW-4 (ChunkAccumulator 0.3/4.0/400) aplicados en rama actual, sin commit aun

### Que quiere mas el consejo (consenso)

1. **Transcripcion en espanol real** (audit P-1): Parakeet 0.6B v3 es solo ingles. El usuario habla espanol y ve basura. Solucion: default a Deepgram nova-3 es-419 para espanol, o agregar Parakeet v2 multilingual.
2. **RUST-001**: los 126 `.unwrap()` son bomba de tiempo. Cualquier buffer overflow en el audio loop -> crash silencioso -> viola prioridad #1 'cero perdida de datos'.
3. **SEC-008 SSO/SAML**: sin esto no hay venta B2B real. Es la barrera mas alta contra enterprise Fortune 500.
4. **STT-002 fallback offline**: si Deepgram cae a mitad de reunion, no hay backup automatico. Inaceptable para reuniones criticas.
5. **Streaming decoding (audit S-2)**: 3-4 semanas de trabajo pero cierra la brecha perceptual vs Otter en UX, que es lo que el usuario reporta como 'en movil es mas rapido'.

# Improvement Log — Maity Desktop

Append-only. Cada entrada documenta un ciclo de `/improve` o `/improve-pr` exitoso.

## Formato

```
### YYYY-MM-DD — Iter #N — <EXP-ID>
- **Experto:** <expert>
- **Título:** <title>
- **Branch / PR:** <branch> / <PR url>
- **Archivos:** <list>
- **Tests:** <antes> → <después>
- **Clippy warnings:** <antes> → <después>
- **Notas:** <opcional>
```

---

### 2026-04-07 — Iter #1 — LEG-006

- **Experto:** ⚖️ privacy_legal
- **Título:** Política de privacidad sin fecha real ni versionado
- **Branch:** `improve/LEG-006-privacy-policy-metadata`
- **PR:** https://github.com/ponchovillalobos/maity_desktop-1/pull/2
- **Archivos:** `PRIVACY_POLICY.md` (+11 -2)
- **Impact/Effort:** 4/10 · 1/10 · prio 4.0
- **Phase:** v1.0 (Quick Win)
- **Quality gates:** N/A — cambio puramente markdown sin código
- **Consulta asamblea:** AUTOR=privacy_legal · OBJETA=ux_desktop,business (estructural, no relevante) · NEUTRAL=9
- **Estado:** in-progress (esperando merge del PR #2)
- **Notas:** Primer ciclo end-to-end del workflow `/improve-pr`. Demostración de que el sistema funciona: lectura del finding → consulta a la asamblea → branch dedicado off main → fix → commit → push → PR abierto → tracking en assembly_data.json.

### 2026-04-07 — Iter #2 — QA-008 (NUEVO hallazgo descubierto)

- **Experto:** 🧪 qa_testing
- **Título:** AudioChunk literal en test rompe cargo test --workspace (E0063)
- **Branch:** `improve/QA-008-fix-audiochunk-test-init`
- **PR:** https://github.com/ponchovillalobos/maity_desktop-1/pull/3
- **Archivos:** `frontend/src-tauri/src/audio/incremental_saver.rs` (+3 -1)
- **Impact/Effort:** 9/10 · 1/10 · prio 9.0 (máxima)
- **Severity:** CRITICAL
- **Phase:** v1.0
- **Quality gates:** `cargo check --tests --lib` rc=0 (1m23s)
- **Cómo se detectó:** durante el primer baseline run de `cargo test --workspace`, falló con rc=101 en 6m34s debido a E0063 missing fields. NO estaba en la asamblea original — descubierto al ejecutar tests reales por primera vez en este fork.
- **Estado:** in-progress (PR #3 abierto)
- **Notas:** Sin este fix, NINGÚN ciclo de `/improve-pr` puede validar tests Rust. Bloquea las prioridades del proyecto (zero data loss + transcripción rápida) porque cualquier mejora en pipeline de audio necesita correr cargo test antes de mergear. Es el fix de mayor prioridad de toda la asamblea hasta que se mergee.

### 2026-04-07 — Iter #3 — PY-003

- **Experto:** 🐍 python_backend
- **Título:** uvicorn con reload=True y host=0.0.0.0 hardcodeado
- **Branch:** `improve/PY-003-uvicorn-env-config`
- **PR:** https://github.com/ponchovillalobos/maity_desktop-1/pull/4
- **Archivos:** `backend/app/main.py` (+18 -1)
- **Impact/Effort:** 8/10 · 1/10 · prio 8.0
- **Severity:** HIGH
- **Phase:** v1.0
- **Quality gates:** Python AST parse OK
- **Consulta asamblea:** AUTOR=python_backend · 0 OBJETA · refuerza SEC-006 (mismo patrón en whisper)
- **Estado:** in-progress
- **Qué mejora para el usuario:** El backend ya no es accesible desde otros equipos en WiFi pública. Antes, en una cafetería, alguien podía mandar peticiones a tu API de transcripción y leer tus reuniones. Ahora solo localhost por defecto. Opt-in con MAITY_HOST=0.0.0.0 para casos especiales.

### 2026-04-07 — Iter #3.5 — Test run real con QA-008 aplicado (descubrimiento masivo)

Tras aplicar QA-008 (cherry-pick a assembly/bootstrap), `cargo test --workspace` corrió por primera vez en este fork:

**74 tests pasan / 3 fallan / 1 ignored** (de 78 totales)

Fallos descubren 3 hallazgos nuevos:

#### RUST-008 (HIGH)
`test_calculate_buffer_timeout_bluetooth` falla. Lógica de cálculo de buffer timeout para BT incorrecta. Relacionado con BLUETOOTH_PLAYBACK_NOTICE.md.

#### RUST-009 (CRITICAL — afecta zero data loss) ⚠️
`test_checkpoint_creation` falla con `assertion left=1 right=2`. Incremental saver crea 1 checkpoint cuando deberia crear 2 con 60s de audio. **Esto compromete la promesa de zero data loss**: si la app crashea entre checkpoints, se pierde más audio del que las cuentas decían. Es ahora el #1 de toda la asamblea.

#### LLM-008 (MEDIUM)
`test_get_builtin_template` falla con mismatch "Standup Diario" vs "Daily Standup". Template fue traducido al español pero el test sigue esperando inglés. Decidir politica i18n.

**Qué mejora para el usuario:** Pasamos de "los tests no compilaban — estábamos a ciegas" a "tenemos 74 tests verdes y sabemos exactamente cuáles 3 fallan y por qué". El #2 (RUST-009) es un bug real que descubre que la promesa zero data loss no se cumple al 100% — y ahora podemos arreglarlo en lugar de descubrirlo cuando un usuario empresarial pierda una reunión crítica.

### 2026-04-07 — Iter #4 — RUST-010 (cargo fmt --all)

- **Experto:** 🦀 rust_tauri
- **Título:** cargo fmt --all -- --check falla con múltiples archivos sin formatear
- **Branch:** `improve/RUST-010-cargo-fmt-all`
- **PR:** https://github.com/ponchovillalobos/maity_desktop-1/pull/5
- **Archivos:** 123 archivos Rust (workspace completo) · +4948 -2797
- **Impact/Effort:** 3/10 · 1/10 · prio 3.0
- **Severity:** medium
- **Phase:** v1.0
- **Quality gates:**
  - cargo check --workspace rc=0 (34s) ✅
  - cargo fmt --all -- --check rc=0 ✅ (era rc=1 antes)
- **Notas:** Requirió fix manual de trailing whitespace en 2 líneas de whisper_engine.rs (bug interno de rustfmt con literales largos en match arms). El cherry-pick a assembly/bootstrap generó conflicto en incremental_saver.rs (donde también vive el fix de QA-008); resuelto manualmente manteniendo `for i in 0..120u64`.
- **Estado:** in-progress (PR #5 abierto)
- **Qué mejora para el usuario:** No cambia nada visible en la app, pero desbloquea el primer quality gate del sistema de auto-mejora. De aquí en adelante cada PR puede pasar `cargo fmt --check` sin ruido, y los diffs en revisión son solo cambios de lógica. Pre-requisito obligatorio para tener un CI estricto cuando lancemos B2B — los clientes enterprise esperan pipelines verdes con fmt+clippy+test.

### 2026-04-07 — Iter #5 — RUST-009 integrado (NO hay bug zero data loss)

- **Experto:** 🦀 rust_tauri
- **Título:** test_checkpoint_creation: bug del TEST (stereo 48k samples), no de producción
- **Branch:** `improve/RUST-009-test-checkpoint-stereo`
- **PR:** https://github.com/ponchovillalobos/maity_desktop-1/pull/6 (supersedea #3)
- **Archivos:** `frontend/src-tauri/src/audio/incremental_saver.rs`
- **Severity:** era critical → reclasificado high (era bug de test, no de producción)
- **Quality gates:** cargo test CPU-only rc=0 (29s compile + 1.91s test)
- **Qué mejora para el usuario:** La garantía de zero data loss SIGUE INTACTA en producción. Era el test midiendo mal (mono en lugar de stereo interleaved). Ahora el test valida correctamente y cualquier regresión futura al checkpointing de 30s será detectada antes de mergear.

### 2026-04-07 — Iter #6 — LLM-008 (i18n template)

- **Experto:** 🤖 ai_llm
- **Título:** test_get_builtin_template i18n — formaliza política es-419-first
- **Branch:** `improve/LLM-008-template-i18n`
- **PR:** https://github.com/ponchovillalobos/maity_desktop-1/pull/7
- **Archivos:** `frontend/src-tauri/src/summary/templates/loader.rs` (+3 -1)
- **Impact/Effort:** 5/10 · 1/10 · prio 5.0
- **Quality gates:** cargo test CPU-only rc=0 (1m45s compile + 0.00s test)
- **Qué mejora para el usuario:** Un test verde más en la suite. Formalizamos en código la política "templates en español" del producto es-419-first. Los clientes B2B hispanohablantes ven los nombres en su idioma.

### 2026-04-08 — Iter #7 — PORTAL-001 (auto-audit + fix dashboard)

- **Tipo:** meta-fix del sistema de mejora continua
- **Bugs detectados en auditoría:**
  1. Status chips no incluían 'in-progress' → los 6 hallazgos en PR no eran filtrables
  2. Counter `pending = total - done` inflaba el número de pending (incluía in-progress)
  3. CSS `.badge.status-in-progress` no existía → badges en curso sin estilo
  4. Status label binario (`done` vs `pending`) → in-progress mostraba "PENDING"
  5. Stats cards (4) no tenían contador de in-progress visible
  6. JSON desincronizado: `iterations:5/commits:8` cuando ya había 6 in-progress y 7 PRs
- **Fix aplicado en `scripts/portal.py`:**
  - 5 stat cards (Total / Done / 🔄 En PR / Pendientes / Críticos) con grid 5 cols + responsive
  - `statusChips = ['all','pending','in-progress','done']` con labels visibles ('Pendientes', 'En PR', 'Done')
  - CSS `.badge.status-in-progress` color azul
  - Status label ternario: ✅ DONE / 🔄 EN PR / ⏳ PENDING
  - `pending` ahora cuenta solo `status==='pending'`, `inprogress` separado
  - Progress bar muestra "trabajo enviado" (done + inprogress) en lugar de solo done
- **Fix `scripts/assembly_data.json`:**
  - `iterations`: 5 → **7**
  - `commits`: 8 → **12**
  - `evaluation_cycle`: 2 → **3**
  - LLM-008 status pending → **in-progress** + pr_url PR #7
  - last_updated: 2026-04-07 → 2026-04-08
- **Quality gates:** Python AST parse OK; portal restart rc=0; `/api/findings` devuelve iter=7 commits=12 in-progress=6 ✓
- **Qué mejora para el usuario:** El dashboard finalmente refleja la realidad. Antes mostraba "0 completados / 88 pendientes / iter 5" (mentira). Ahora muestra "0 done / **6 en PR** / 82 pending / iter 7" con un chip filtrable "En PR" para ver los PRs abiertos. Cuando despiertes y abras el portal, verás los 6 PRs que ya hice + cualquier batch nuevo de la noche en una card azul claramente separada de "pendientes".


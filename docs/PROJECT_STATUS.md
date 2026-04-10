# Maity Desktop — Project Status

> Snapshot completo del estado del proyecto al **2026-04-08**.
> Para detalles del sistema de Asamblea ver [`docs/ASSEMBLY.md`](ASSEMBLY.md).
> Para detalles de Cargo deps ver [`docs/CARGO_DEPENDENCIES.md`](CARGO_DEPENDENCIES.md).

---

## 🎯 Resumen ejecutivo

Maity Desktop es un fork de `Sixale730/maity_desktop` mantenido en `ponchovillalobos/maity_desktop-1`. Stack: **Tauri 2.x + Rust + Next.js 14 + Python FastAPI**. Asistente de reuniones IA con captura dual mic+sistema, transcripción Deepgram/Whisper en vivo y resúmenes ChatGPT/local LLM.

**Estado al 2026-04-08:**

| Métrica | Valor |
|---|---|
| Versión actual | **0.2.31** |
| Branch principal de trabajo | `assembly/bootstrap` |
| Tests Rust | **77/77 verde** ✅ |
| Cargo fmt | **rc=0** ✅ |
| Cargo check workspace | **rc=0** (~30s con cache) |
| Build Next.js | **rc=0** (~30s) |
| Build cargo --debug | **rc=0** (~60s con cache, binario 91 MB) |
| App smoke test | **63 MB RAM at idle** ✅ |
| PRs abiertos en fork | **18** (incluye RUST-011) |
| Iteraciones del sistema | **20** |
| Commits del sistema | **31** |
| Hallazgos asamblea | **98** (88 originales + 10 B2B) |
| In-progress (en PR) | **30** (33% del total) |
| Críticos | **16** |

---

## 📚 Estructura del proyecto

```
D:\Maity_Desktop\
├── frontend/
│   ├── src/                          # Next.js 14 + TypeScript + React 18
│   │   ├── app/                      # App router (layout.tsx, pages)
│   │   ├── components/               # UI components (BlockNote, RecordingControls, ...)
│   │   ├── contexts/                 # React contexts (Auth, Recording, Theme, ...)
│   │   ├── hooks/                    # Custom hooks
│   │   └── lib/                      # logger.ts, deepgram.ts, etc.
│   ├── src-tauri/
│   │   ├── src/                      # Rust shell (148 archivos)
│   │   │   ├── audio/                # Captura, mezcla, VAD, encoding (núcleo)
│   │   │   ├── audio/transcription/  # Whisper, Deepgram, Parakeet, Moonshine
│   │   │   ├── database/             # SQLite + repositorios
│   │   │   ├── summary/              # Templates + LLM client
│   │   │   ├── tray.rs               # System tray (UX-002 dynamic tooltip)
│   │   │   └── main.rs / lib.rs
│   │   ├── templates/                # JSON templates (Standup Diario, etc.)
│   │   ├── Cargo.toml                # 75 deps runtime, refactorizado en RUST-011
│   │   └── tauri.conf.json           # Allowlist restringido (SEC-002)
│   ├── package.json                  # Next 14.2.33 (parchado FE-005)
│   └── eslint.config.mjs             # Strict rules (FE-003)
├── backend/
│   ├── app/                          # FastAPI Python (16 archivos)
│   │   ├── main.py                   # Lifespan + CORS lista blanca + degraded mode
│   │   ├── transcript_processor.py   # Whitelist + retries=5
│   │   └── routes/                   # meetings, transcripts, summaries, config
│   ├── tests/                        # 9 def test_
│   ├── Dockerfile.server-cpu         # Pinned digest (PY-007)
│   └── requirements.txt
├── llama-helper/                     # Sidecar Rust para LLM local
│   ├── src/main.rs
│   └── Cargo.toml
├── scripts/
│   ├── assembly_data.json            # 12 expertos / 98 hallazgos
│   ├── portal.py                     # FastAPI dashboard puerto 8770 (v2.3)
│   └── requirements.txt
├── memory/                           # Sistema de memoria persistente
│   ├── IMPROVEMENT_LOG.md
│   ├── METRICS_HISTORY.md
│   ├── FAILED_ATTEMPTS.md
│   ├── ANALYSIS_STATE.md
│   ├── OVERNIGHT_REPORT_2026-04-08.md
│   ├── APP_TEST_RESULTS_2026-04-08.md
│   └── build_logs/                   # Logs persistentes de cada test/build
├── docs/                             # Documentación
│   ├── ASSEMBLY.md                   # Sistema de Asamblea de Expertos
│   ├── CARGO_DEPENDENCIES.md         # Mapa de deps (este refactor)
│   ├── PROJECT_STATUS.md             # ESTE archivo
│   ├── SUBPROCESSORS.md              # B2B subprocesadores con DPAs (LEG-004)
│   └── (otros docs originales)
├── .claude/
│   ├── skills/improve/SKILL.md       # Skill /improve
│   ├── commands/improve.md           # /improve
│   ├── commands/improve-pr.md        # /improve-pr (1 PR por hallazgo)
│   ├── commands/deploy.md
│   └── agents/{auditor,validator,janitor}/AGENT.md
├── PRIVACY_POLICY.md                 # v2.0 honesto (LEG-001)
├── TERMS_OF_SERVICE.md               # v1.0 nuevo (LEG-005)
├── README.md                         # Hero rebrandeado (BIZ-002)
├── CLAUDE.md                         # Instrucciones para Claude Code
├── SETUP_WINDOWS.md                  # +sección rotación pubkey (OPS-007)
└── Cargo.toml                        # Workspace root
```

---

## 🛠️ Stack tecnológico

### Frontend (UI)
- **Tauri 2.10.1** (Rust shell + WebView)
- **Next.js 14.2.33** (parchado contra GHSA-fr5h-rqp8-mj6g)
- **React 18.2** + **TypeScript**
- **TailwindCSS** + **shadcn/ui** + **Radix UI** (sin agregador duplicado, FE-007)
- **BlockNote** (rich text editor)
- **TanStack Query** (state management)
- **next-themes** (dark/light, FE-006 hydration fix)

### Frontend Rust shell (`frontend/src-tauri/`)
Ver [`docs/CARGO_DEPENDENCIES.md`](CARGO_DEPENDENCIES.md) para detalles. Highlights:
- **whisper-rs 0.13.2** (Metal+CoreML auto en macOS, CPU+opt-in CUDA/Vulkan en Win/Linux)
- **cpal 0.15.3** + **WASAPI** (Windows) / **ScreenCaptureKit** (macOS)
- **silero_rs** (VAD)
- **nnnoiseless** (noise reduction RNNoise)
- **ebur128** (normalización broadcast)
- **rubato** (sample rate conversion)
- **ort 2.0** (ONNX Runtime para Parakeet)
- **sqlx 0.8** (SQLite)
- **sentry 0.34** (rustls, no openssl)
- **tracing 0.1** (con tracing-appender para rotación)
- **tokio 1.32** (multi-thread + sync + time + io-util + process)

### Backend Python (`backend/`)
- **FastAPI 0.115** con `lifespan` (PY-001)
- **Pydantic 2.x** con field_validators (PY-005)
- **pydantic-ai** (orquestación LLM)
- **whisper.cpp** (build via `build_whisper.cmd small`)
- **OpenAI / Anthropic / Groq / Ollama** (BYOK)
- **sqlite3** (no ORM)

### llama-helper (Rust sidecar)
- **llama-cpp-2** (bindings llama.cpp)
- **stdio JSON RPC** con Tauri parent

### CI/CD
- **GitHub Actions**: build-windows.yml (DigiCert KeyLocker), build-macos.yml (Apple notarize), build-linux.yml, release.yml
- **Status actual**: build-windows.yml es manual (workflow_dispatch) — OPS-001 propone añadirlo a PRs

---

## 🚀 Comandos esenciales

### Build & run

```bash
# Frontend Next.js
cd D:\Maity_Desktop\frontend
npm install            # 697 packages
npm run build          # → out/ con páginas estáticas
npm run lint           # ESLint strict (FE-003)
npx tsc --noEmit       # TypeScript check

# Rust Tauri shell
cd D:\Maity_Desktop
cargo check --manifest-path frontend/src-tauri/Cargo.toml --lib
cargo test --manifest-path frontend/src-tauri/Cargo.toml --lib   # 77/77 ✅
cargo build --manifest-path frontend/src-tauri/Cargo.toml --bin maity-desktop
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets

# Lanzar la app (binario debug)
powershell -Command "Start-Process 'D:\Maity_Desktop\target\debug\maity-desktop.exe'"

# Backend Python
cd D:\Maity_Desktop\backend
pip install -r requirements.txt
MAITY_HOST=127.0.0.1 MAITY_PORT=5167 python app/main.py
```

### Portal de Asamblea

```bash
cd D:\Maity_Desktop
python scripts/portal.py
# o detached:
powershell -Command "Start-Process python -ArgumentList 'scripts/portal.py' -WindowStyle Hidden"
# → http://127.0.0.1:8770
```

### Workflow de mejora (`/improve-pr`)

```bash
# 1. Pre-flight
gh auth status
git checkout main
git pull upstream main

# 2. Branch para un finding
git checkout -b improve/SEC-003-csp-no-unsafe-eval

# 3. Aplicar fix (ver assembly_data.json para el detalle)
# ...

# 4. Quality gates CPU-only
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --manifest-path frontend/src-tauri/Cargo.toml --lib
cd frontend && npm run lint && npx tsc --noEmit && cd ..

# 5. Commit + push + PR
git add <files>
git commit -m "fix(security): ..."
git push -u origin improve/SEC-003-csp-no-unsafe-eval
gh pr create --base main --head improve/... --title "..." --body "..."

# 6. Cherry-pick a assembly/bootstrap
git checkout assembly/bootstrap
git cherry-pick <sha>

# 7. Update assembly_data.json (status pending → in-progress + pr_url)
# 8. Update memory/IMPROVEMENT_LOG.md y memory/METRICS_HISTORY.md
# 9. Push assembly/bootstrap
```

---

## 🎯 Prioridades absolutas del proyecto

(en este orden estricto, cualquier decisión las respeta)

1. **Cero pérdidas de datos** — ninguna grabación, transcript o summary debe perderse jamás
2. **Velocidad de transcripción post-sesión casi instantánea** — al colgar, el resumen final está disponible inmediatamente
3. **Velocidad/latencia durante grabación en vivo** — segunda prioridad
4. **Resto** (UX, features nuevos, refactors)

---

## 📊 Estado actual de los 98 hallazgos

### Por estado
- **30 in-progress** (33% — en PRs esperando merge)
- **0 done** (esperando que el ingeniero merge en main)
- **68 pending** (no atacados todavía)

### Por severidad
- **16 critical** (de los cuales 6 ya en PR: LEG-001, SEC-001, SEC-002, RUST-009, QA-008)
- **27 high**
- **34 medium**
- **21 low**

### Por fase
- **v1.0** (Quick Wins, effort ≤3): 35 items — **mayoría in-progress o pending fácil**
- **v2.0** (Major features, effort 4-7): 56 items
- **v3.0** (Expansión, effort ≥8): 7 items (incluye SEC-008/009/010, OPS-008)

### Por experto
| Experto | Total | Done | In-PR | Pending |
|---|---|---|---|---|
| 🔒 security | 10 | 0 | 2 | 8 |
| 🦀 rust_tauri | 11 | 0 | 5 | 6 |
| 🐍 python_backend | 7 | 0 | 5 | 2 |
| ⚛️ nextjs_frontend | 7 | 0 | 4 | 3 |
| 🖥️ ux_desktop | 7 | 0 | 2 | 5 |
| 🎙️ transcription | 7 | 0 | 1 | 6 |
| 🤖 ai_llm | 8 | 0 | 3 | 5 |
| ⚙️ devops_ci | 10 | 0 | 1 | 9 |
| ⚖️ privacy_legal | 9 | 0 | 4 | 5 |
| 💼 business | 9 | 0 | 1 | 8 |
| 🧪 qa_testing | 8 | 0 | 1 | 7 |
| ⚡ performance | 7 | 0 | 0 | 7 |

---

## 🔗 Los 18 PRs abiertos

`https://github.com/ponchovillalobos/maity_desktop-1/pulls`

| # | EXP | Título corto |
|---|---|---|
| #1 | bootstrap | Asamblea + Auto-mejora + Portal v2.3+ |
| #2 | LEG-006 | privacy policy fecha+versión |
| #3 | QA-008 | AudioChunk init (superseded por #6) |
| #4 | PY-003 | uvicorn env (superseded por #12) |
| #5 | RUST-010 | cargo fmt --all 123 archivos |
| #6 | QA-008+RUST-009 | AudioChunk stereo 48k |
| #7 | LLM-008 | i18n template Standup Diario |
| #8 | RUST-008 | bluetooth buffer timeout precision |
| #9 | LEG-001 | privacy policy v2.0 honesto |
| #10 | SEC-001 | CORS lista blanca (superseded por #12) |
| #11 | SEC-002 | Tauri allowlist least privilege |
| #12 | PY-batch | PY-001/002/003/004/005/007 + SEC-001 |
| #13 | FE-batch | FE-003/005/006/007 |
| #14 | UX-batch | UX-002 tray + UX-005 ARIA |
| #15 | docs | LEG-004/005 + OPS-007 + BIZ-002 |
| #16 | RUST-003 | pin ffmpeg-sidecar rev |
| #17 | LLM-STT | STT-003 + LLM-006 + LLM-007 |
| #18 | RUST-011 | Cargo.toml refactor (este PR) |

**Orden recomendado de merge** (de menor a mayor riesgo):
1. **Trivial / docs**: #2, #5, #7, #9, #15, #18 (markdown puro o config)
2. **Tests-only**: #6, #8 (cierran 77/77 verde)
3. **Config**: #11, #16 (Tauri allowlist + Cargo pinning)
4. **Frontend**: #13, #14 (eslint + tray + ARIA)
5. **Backend medium**: #12, #17 (Python lifespan + STT/LLM)
6. **Bootstrap**: #1 (al final, cuando quieras activar el sistema completo)
- **Cerrar como superseded**: #3 (por #6), #4 (por #12), #10 (por #12)

---

## 🚧 Pendiente para sesiones futuras

### Críticos sin atacar
- **STT-001/002** (critical, v2.0): Deepgram reconnect + fallback Whisper offline
- **LLM-001** (critical, v2.0): cap de tokens antes de invocar API pago
- **PERF-003** (critical, v2.0): RAM enforcement antes de cargar Whisper
- **LEG-002** (critical, v2.0): consent flow UI (two-party consent)
- **RUST-001** (critical, v3.0): refactor 126 unwrap() → Result

### Build app
- ✅ Build debug pasa (npm + cargo)
- ⏳ Build release con signing (requiere cert Windows)
- ⏳ Smoke test de grabación real (mic + Deepgram)
- ⏳ Medir tiempo desde stop_recording hasta resumen visible

### Asamblea
- ✅ Expandida con 10 findings B2B (SSO, RBAC, SOC2, MSA/SLA/DPA, etc.)
- ⏳ Generar más expertos especializados (DevEx integraciones, ML evaluation, etc.) si se necesita

---

## 📞 Contactos / referencias

- **Repo upstream**: https://github.com/Sixale730/maity_desktop
- **Fork de trabajo**: https://github.com/ponchovillalobos/maity_desktop-1
- **PRs**: https://github.com/ponchovillalobos/maity_desktop-1/pulls
- **Portal local**: http://127.0.0.1:8770
- **Memoria persistente**: `C:\Users\alfon\.claude\projects\D--Maity-Desktop\memory\`
- **Logs**: `D:\Maity_Desktop\memory\build_logs\`

---

*Última actualización: 2026-04-08 (post RUST-011 refactor)*

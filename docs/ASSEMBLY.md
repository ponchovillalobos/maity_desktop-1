# Asamblea de Expertos + Sistema de Auto-Mejora

> Documentación del sistema replicado desde Maity Recorder y adaptado a Maity Desktop (Tauri / Rust / Next.js / Python).

## 1. Concepto

La **Asamblea de Expertos** es un sistema donde 12 perspectivas especializadas auditan el proyecto, generan hallazgos priorizados, y alimentan un ciclo de auto-mejora continua donde cada hallazgo se convierte en un Pull Request aislado.

```
                    ┌─────────────┐
                    │   ASAMBLEA   │
                    │  12 Expertos │
                    └──────┬──────┘
                           │ genera
                    ┌──────▼──────┐
                    │  84 Hallazgos│
                    │  priorizados │
                    └──────┬──────┘
                           │ alimenta
              ┌────────────▼────────────┐
              │   /improve-pr (skill)    │
              │ branch → fix → quality   │
              │ gates → push → PR        │
              └────────────┬────────────┘
                           │ visualiza
                    ┌──────▼──────┐
                    │   PORTAL    │
                    │  Dashboard  │
                    │  + Activity │
                    └─────────────┘
```

---

## 2. Componentes

### 2.1 Base de datos de hallazgos

**`scripts/assembly_data.json`** — JSON con metadata del proyecto, 12 expertos y sus 84 hallazgos. Cada hallazgo incluye:

```json
{
  "id": "PREFIX-NNN",
  "title": "Título corto en español",
  "severity": "critical|high|medium|low",
  "description": "Problema concreto con archivo:línea cuando aplique",
  "recommendation": "Solución paso a paso",
  "impact": 1-10,
  "effort": 1-10,
  "phase": "v1.0|v2.0|v3.0",
  "status": "pending|in-progress|done",
  "references": ["URLs/standards"],
  "pr_url": "https://github.com/.../pull/NN"
}
```

**Prioridad** = `impact / effort`. Los items de mayor ratio se atacan primero.

### 2.2 Los 12 expertos

| ID | Experto | Foco |
|---|---|---|
| 🔒 `security` | Seguridad | CORS, allowlist Tauri, IPC, secrets, audio sin cifrar |
| 🦀 `rust_tauri` | Rust / Tauri | unwrap(), panic safety, ownership, deps git pinning |
| 🐍 `python_backend` | Python Backend | FastAPI, lifespan, validación pydantic, requirements |
| ⚛️ `nextjs_frontend` | Next.js Frontend | console.log, ': any', bundle size, eslint config |
| 🖥️ `ux_desktop` | UX Desktop | Tray icon, atajos globales, onboarding, ARIA |
| 🎙️ `transcription` | Transcripción | Deepgram reconnect, Whisper fallback, costos por minuto |
| 🤖 `ai_llm` | AI / LLM | Tokens cap, retry, streaming, prompts es/en, llama-helper |
| ⚙️ `devops_ci` | DevOps CI | GitHub Actions, code signing, releases, Docker SBOM |
| ⚖️ `privacy_legal` | Privacidad / Legal | Consent, retención, GDPR, DPAs, jurisdicción |
| 💼 `business` | Negocio / GTM | Pricing, BYOK vs Cloud, unit economics, freemium |
| 🧪 `qa_testing` | QA / Testing | Cobertura, fixtures, smoke, E2E webdriver |
| ⚡ `performance` | Performance | Latencia, RAM Whisper, startup, jank UI |

**Tensiones cruzadas conocidas** (usado por `/api/consult`):
- security ↔ ux_desktop / business (más cifrado vs menos fricción)
- privacy_legal ↔ business / ux_desktop (más consent vs onboarding rápido)
- performance ↔ qa_testing / ai_llm (más optimización vs más validación)
- devops_ci ↔ business (más rigor CI vs releases rápidos)

### 2.3 Roadmap

3 fases distribuidas por effort:
- **v1.0 — Quick Wins (effort ≤3)**: 32 items (~2-4 semanas)
- **v2.0 — Major Features (effort 4-7)**: 49 items (~2-3 meses)
- **v3.0 — Expansión (effort ≥8)**: 3 items (~6+ meses)

### 2.4 Portal Web (`scripts/portal.py`)

FastAPI + HTML SPA en puerto **8770**. Vistas:

| Vista | Endpoint API | Qué muestra |
|---|---|---|
| 📋 Asamblea | `GET /api/findings` | Cards de hallazgos con problema/solución/razón visibles, filtros (experto/severidad/fase/estado/búsqueda), ordenado por prioridad |
| 📰 Actividad | `GET /api/activity` | PRs abiertos, iteraciones del IMPROVEMENT_LOG, últimos commits |
| 🗺️ Roadmap | `/api/findings` | 3 columnas v1.0/v2.0/v3.0 con cada item + recomendación abreviada |
| 📈 Dashboard | `GET /api/metrics` | Métricas live: archivos por stack, tests, .unwrap(), console.log, build status, git |
| 🧠 Memoria | `GET /api/memory` | Render markdown de IMPROVEMENT_LOG, FAILED_ATTEMPTS, METRICS_HISTORY, ANALYSIS_STATE |
| 👥 Expertos | `/api/findings` | Card por experto con summary y conteos |

**Auto-refresh cada 8s** sin perder filtros. Banner verde "última actualización hace Xs" siempre visible.

**Botón 🗣️ Consultar asamblea** en cada hallazgo: hace `GET /api/consult/{ID}` y muestra modal con:
- Veredictos de los 12 expertos: AUTOR / APOYA / OBJETA / PRECAUCIÓN / NEUTRAL con razón
- Hallazgos relacionados (palabras clave en común)
- Conflictos potenciales (basados en tensiones estructurales)

**Lanzamiento:**
```bash
python scripts/portal.py
# o detached en Windows:
powershell -Command "Start-Process python -ArgumentList 'scripts/portal.py' -WindowStyle Hidden"
```

### 2.5 Skill `/improve` y comando `/improve-pr`

Definidos en `.claude/skills/improve/SKILL.md` y `.claude/commands/improve-pr.md`.

**Flujo de cada ciclo `/improve-pr`:**

1. **Pre-flight**: `gh auth status`, working tree limpio, `git pull upstream main`
2. **Elegir finding**: leer `assembly_data.json`, filtrar `pending`, ordenar por `impact/effort`
3. **Consultar asamblea**: `curl /api/consult/<ID>` y registrar veredictos
4. **Crear branch**: `git checkout main && git checkout -b improve/<ID>-<slug>`
5. **Implementar fix**: ≤100 LOC, single concern, test primero si añade comportamiento
6. **Quality gates**:
   - `cargo fmt --all -- --check`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `cargo test --all`
   - `cd frontend && npm run lint && npm run typecheck && npm test`
7. **Commit**: mensaje en español con sección "Consulta a la asamblea"
8. **Push + PR**: `gh pr create --base main --head improve/...` con body que incluye descripción + recommendation + verdicts
9. **Marcar `status: in-progress`** en `assembly_data.json` con `pr_url`
10. **Append** a `memory/IMPROVEMENT_LOG.md` y `memory/METRICS_HISTORY.md`

**Si falla un quality gate:**
- `git restore .`
- `git checkout main && git branch -D improve/...`
- Append a `memory/FAILED_ATTEMPTS.md` con error exacto + causa + lección
- Salir del ciclo (no intentar otro finding hasta que el usuario revise)

### 2.6 Agentes (`.claude/agents/`)

| Agente | Modelo | Función |
|---|---|---|
| `auditor` | sonnet | Analiza código + asamblea, devuelve top 3 candidatos por impact/effort. Solo lectura. |
| `validator` | haiku | Ejecuta quality gates y decide PASS/FAIL escribiendo `memory/validation_result.json` |
| `janitor` | haiku | Limpieza de target/, node_modules/, reporte de salud del repo |

### 2.7 Memoria persistente (`memory/`)

| Archivo | Contenido |
|---|---|
| `IMPROVEMENT_LOG.md` | Append-only de cada ciclo exitoso: fecha, EXP-ID, branch, PR, archivos, métricas antes/después, consulta asamblea |
| `FAILED_ATTEMPTS.md` | Append-only de ciclos abortados: error, causa raíz, lección |
| `METRICS_HISTORY.md` | Tabla histórica: tests, warnings, done/total por iteración |
| `ANALYSIS_STATE.md` | Cola de prioridades vigente |
| `build_logs/` | Logs persistentes de cada run de cargo/clippy/npm/tauri (gitignored, solo en disco) |

---

## 3. Reglas inquebrantables

1. **NUNCA bajar el número de tests** entre iteraciones
2. **NUNCA aumentar warnings** de clippy
3. **NUNCA romper el build** de Tauri
4. **SIEMPRE consultar la asamblea** antes de proponer/aplicar un cambio
5. **SIEMPRE actualizar `assembly_data.json`** + `memory/*` + portal después de cada cambio
6. **SIEMPRE documentar fallos** con causa raíz y lección
7. **Single concern por PR**, ≤100 LOC de diff
8. **1 branch + 1 PR por hallazgo** — no agrupar
9. **Quality gates obligatorios** antes de commit
10. **Prioridades del proyecto** (en este orden): zero data loss → transcripción rápida post-sesión → latencia en vivo → resto

---

## 4. Métricas baseline (2026-04-07)

Capturadas en el primer run real sobre el fork:

| Métrica | Valor | Estado |
|---|---|---|
| Frontend TS/TSX files | 261 | – |
| Rust src-tauri files | 148 | – |
| Python backend files | 16 | – |
| llama-helper files | 1 | – |
| Tests Rust `#[test]` | 84 | ⚠️ (sub-cobertura) |
| Tests Python | 9 | ⚠️ |
| Tests Frontend | **0** | 🔴 confirma QA-001 |
| `.unwrap()` Rust | **126** | 🔴 confirma RUST-001 |
| `console.log` TS | **442** | ⚠️ confirma FE-002 |
| `: any` TS | **47** | ⚠️ confirma FE-004 |
| TODO/FIXME | 7 | – |
| `cargo check --workspace` | rc=0 (5m28s) | ✅ |
| `npm install` | rc=0 (3m, 697 pkg) | ✅ |
| `tsc --noEmit` | rc=0 (33s) | ✅ |
| `cargo test --workspace` | **rc=101** (E0063) → fix en PR #3 | 🔴→✅ |
| `cargo fmt --check` | rc=1 | ⏳ |
| `next lint` | rc=1 (no configurado) | ⏳ |

---

## 5. Iteraciones registradas

Ver `memory/IMPROVEMENT_LOG.md` y la vista 📰 Actividad del portal para el detalle live.

| Iter | EXP-ID | Severidad | Estado | PR |
|---|---|---|---|---|
| 1 | LEG-006 | medium | in-progress | [#2](https://github.com/ponchovillalobos/maity_desktop-1/pull/2) |
| 2 | QA-008 (nuevo) | critical | in-progress | [#3](https://github.com/ponchovillalobos/maity_desktop-1/pull/3) |

---

## 6. Cómo arrancar el sistema desde cero

```bash
# 1. Clonar el fork
git clone https://github.com/ponchovillalobos/maity_desktop-1.git
cd maity_desktop-1

# 2. Cambiar al branch de la asamblea
git checkout assembly/bootstrap

# 3. Instalar deps del portal
pip install -r scripts/requirements.txt

# 4. Lanzar portal
python scripts/portal.py
# → http://127.0.0.1:8770

# 5. Para ejecutar un ciclo de mejora con Claude Code:
# /improve-pr            # toma el siguiente top por impact/effort
# /improve-pr SEC-001    # arregla un hallazgo específico
```

---

## 7. Referencias

- Sistema original: [Maity Recorder Assembly](D:\maity_recorder)
- Spec del concepto: ver issue/doc del proyecto Recorder
- Tauri 2 capabilities: https://v2.tauri.app/security/capabilities/
- pydantic-ai: https://ai.pydantic.dev/

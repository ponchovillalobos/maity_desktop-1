"""
Maity Desktop — Portal de Asamblea de Expertos + Dashboard de Mejora Continua

Lanzar:
    python scripts/portal.py
    # o
    uvicorn scripts.portal:app --host 127.0.0.1 --port 8770 --reload

Endpoints:
    /                  Portal SPA con sidebar (asamblea + roadmap + dashboard + consulta + memoria)
    /api/findings      JSON crudo del assembly_data.json
    /api/metrics       Métricas live del repo (counts, tests, unwraps, builds)
    /api/consult/{id}  Consulta a la asamblea sobre un hallazgo (devuelve relacionados/conflictos)
    /health            Health check robusto (uptime, RAM, CPU, assembly_loaded)
    /api/activity      Resumen de iteraciones recientes
    /api/memory        Render markdown de memory/*.md

PORTAL-002 (estabilidad, 2026-04-08): añadidos /health robusto, semaphore global,
content-length cap, try/except wrappers, JSONL request logging con rotación 30d,
single-instance lock file, JS safeFetch con timeout y graceful degradation.
"""
from __future__ import annotations

import asyncio
import atexit
import json
import logging
import os
import re
import signal
import subprocess
import sys
import tempfile
import time
import traceback
from contextlib import asynccontextmanager
from pathlib import Path
from typing import Any, Optional

TMP = Path(tempfile.gettempdir())

import markdown as md
from fastapi import FastAPI, HTTPException, Request
from fastapi.responses import HTMLResponse, JSONResponse, PlainTextResponse

# psutil is optional — degrade gracefully if missing
try:
    import psutil  # type: ignore
    _HAS_PSUTIL = True
except ImportError:
    _HAS_PSUTIL = False

ROOT = Path(__file__).resolve().parent.parent
ASSEMBLY_FILE = ROOT / "scripts" / "assembly_data.json"
MEMORY_DIR = ROOT / "memory"
REQUEST_LOG_DIR = MEMORY_DIR / "build_logs" / "portal_requests"
LOCK_FILE = TMP / "maity_portal_8770.lock"
PORTAL_VERSION = "2.4-PORTAL-002"
STARTUP_TIME = time.time()

# ──────────────────────────── PORTAL-002: stability ──────────────────────────── #
# A1: Health endpoint robusto incluye uptime, memoria, CPU
# A2: Semaphore global limita concurrencia para prevenir OOM en JSON loads grandes
# A3: Try/except wrapper en endpoints (via _safe_route decorator)
# A4: Request logging a JSONL con rotación 30 días
# A5: Single-instance lock file (atexit cleanup)
# A6: Frontend safeFetch con graceful degradation
_REQUEST_SEMAPHORE = asyncio.Semaphore(8)  # max 8 requests paralelos (local-only)
_MAX_BODY_BYTES = 10 * 1024 * 1024  # 10 MB cap
_REQUEST_TIMEOUT_S = 10.0  # cualquier endpoint que tarde más se considera fail

# Logger configurado al startup (vs print) para captura controlada
logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s [%(levelname)s] %(message)s",
    datefmt="%Y-%m-%d %H:%M:%S",
)
logger = logging.getLogger("maity_portal")


def _acquire_lock_file() -> bool:
    """A5: single-instance via lock file. Returns True if lock acquired."""
    try:
        if LOCK_FILE.exists():
            # Check if the PID inside is alive
            try:
                pid_str = LOCK_FILE.read_text(encoding="utf-8").strip()
                pid = int(pid_str)
                if _HAS_PSUTIL and psutil.pid_exists(pid):
                    logger.warning(
                        "Portal already running with PID %s (lock file %s) — aborting",
                        pid, LOCK_FILE,
                    )
                    return False
                else:
                    logger.info("Stale lock file from PID %s, removing", pid)
                    LOCK_FILE.unlink()
            except Exception:
                LOCK_FILE.unlink()
        LOCK_FILE.write_text(str(os.getpid()), encoding="utf-8")
        atexit.register(_release_lock_file)
        # Also handle Ctrl+C / SIGTERM
        for sig in (signal.SIGINT, signal.SIGTERM):
            try:
                signal.signal(sig, lambda *_: (_release_lock_file(), sys.exit(0)))
            except (ValueError, OSError):
                pass  # signal not available on this platform/thread
        logger.info("PORTAL-002: lock acquired at %s (PID %s)", LOCK_FILE, os.getpid())
        return True
    except Exception as e:
        logger.warning("PORTAL-002: could not acquire lock file: %s (continuing anyway)", e)
        return True  # don't block startup if lock fails


def _release_lock_file() -> None:
    try:
        if LOCK_FILE.exists():
            content = LOCK_FILE.read_text(encoding="utf-8").strip()
            if content == str(os.getpid()):
                LOCK_FILE.unlink()
                logger.info("PORTAL-002: lock released")
    except Exception:
        pass


def _log_request_jsonl(method: str, path: str, status: int, latency_ms: float,
                       error: Optional[str] = None) -> None:
    """A4: request log a JSONL persistente con rotación 30 días."""
    try:
        REQUEST_LOG_DIR.mkdir(parents=True, exist_ok=True)
        day = time.strftime("%Y-%m-%d")
        log_path = REQUEST_LOG_DIR / f"requests_{day}.jsonl"
        record = {
            "ts": time.time(),
            "method": method,
            "path": path,
            "status": status,
            "latency_ms": round(latency_ms, 2),
        }
        if error:
            record["error"] = error[:500]
        with log_path.open("a", encoding="utf-8") as f:
            f.write(json.dumps(record, ensure_ascii=False) + "\n")
        # Rotación: borra logs >30 días una vez por hora
        global _LAST_ROTATION_CHECK
        if time.time() - _LAST_ROTATION_CHECK > 3600:
            _LAST_ROTATION_CHECK = time.time()
            _rotate_request_logs()
    except Exception:
        pass  # NUNCA crashear el server por logging


_LAST_ROTATION_CHECK = 0.0


def _rotate_request_logs() -> None:
    try:
        if not REQUEST_LOG_DIR.exists():
            return
        cutoff = time.time() - (30 * 86400)
        for f in REQUEST_LOG_DIR.glob("requests_*.jsonl"):
            try:
                if f.stat().st_mtime < cutoff:
                    f.unlink()
                    logger.info("PORTAL-002: rotated old log %s", f.name)
            except Exception:
                pass
    except Exception:
        pass


@asynccontextmanager
async def lifespan(app: FastAPI):
    """Startup/shutdown hooks."""
    logger.info("PORTAL-002: portal starting (version %s, PID %s)", PORTAL_VERSION, os.getpid())
    yield
    logger.info("PORTAL-002: portal shutting down")
    _release_lock_file()


app = FastAPI(title="Maity Desktop — Portal", version=PORTAL_VERSION, lifespan=lifespan)


# A2 + A3: middleware global para semaphore + try/except + request logging + content-length
@app.middleware("http")
async def stability_middleware(request: Request, call_next):
    start = time.time()
    method = request.method
    path = request.url.path

    # Content-Length cap (A2)
    cl_header = request.headers.get("content-length")
    if cl_header:
        try:
            cl = int(cl_header)
            if cl > _MAX_BODY_BYTES:
                latency = (time.time() - start) * 1000
                _log_request_jsonl(method, path, 413, latency, error="payload too large")
                return JSONResponse(
                    status_code=413,
                    content={"error": f"payload too large ({cl} bytes > {_MAX_BODY_BYTES})"},
                )
        except ValueError:
            pass

    # Semaphore (A2) — limita concurrencia para evitar OOM en json loads
    try:
        async with _REQUEST_SEMAPHORE:
            try:
                # Timeout (A3)
                response = await asyncio.wait_for(
                    call_next(request), timeout=_REQUEST_TIMEOUT_S
                )
                latency = (time.time() - start) * 1000
                _log_request_jsonl(method, path, response.status_code, latency)
                return response
            except asyncio.TimeoutError:
                latency = (time.time() - start) * 1000
                _log_request_jsonl(method, path, 504, latency, error="timeout")
                logger.warning("Request timeout: %s %s", method, path)
                return JSONResponse(
                    status_code=504,
                    content={"error": f"request timeout after {_REQUEST_TIMEOUT_S}s"},
                )
            except HTTPException as he:
                latency = (time.time() - start) * 1000
                _log_request_jsonl(method, path, he.status_code, latency, error=str(he.detail))
                raise
            except Exception as e:
                latency = (time.time() - start) * 1000
                err_msg = f"{type(e).__name__}: {e}"
                _log_request_jsonl(method, path, 500, latency, error=err_msg)
                logger.error("Unhandled error in %s %s: %s\n%s",
                             method, path, err_msg, traceback.format_exc()[:1500])
                # Graceful degradation: return 500 JSON instead of crash
                return JSONResponse(
                    status_code=500,
                    content={"error": "internal server error", "type": type(e).__name__},
                )
    except Exception as outer:
        # Last-resort safety net: if even the middleware crashes, log and return 500
        logger.critical("PORTAL-002: middleware itself crashed: %s", outer)
        return JSONResponse(
            status_code=500,
            content={"error": "portal middleware crashed", "details": str(outer)[:200]},
        )

SEVERITY_COLORS = {
    "critical": "#e74c3c",
    "high": "#e67e22",
    "medium": "#f1c40f",
    "low": "#95a5a6",
}
SEVERITY_LABEL = {
    "critical": "CRÍTICO",
    "high": "ALTO",
    "medium": "MEDIO",
    "low": "BAJO",
}

# Cross-cutting expert relationships (who tends to conflict with whom)
EXPERT_TENSIONS = {
    "security": ["ux_desktop", "business"],
    "privacy_legal": ["business", "ux_desktop"],
    "ux_desktop": ["security", "privacy_legal"],
    "business": ["security", "privacy_legal", "qa_testing"],
    "performance": ["qa_testing", "ai_llm"],
    "qa_testing": ["business", "performance"],
    "rust_tauri": ["performance"],
    "devops_ci": ["business"],
}


# ──────────────────────────── helpers ──────────────────────────── #
def load_assembly() -> dict:
    if not ASSEMBLY_FILE.exists():
        raise HTTPException(500, f"assembly_data.json not found at {ASSEMBLY_FILE}")
    with ASSEMBLY_FILE.open(encoding="utf-8") as f:
        return json.load(f)


def all_findings_flat(data: dict) -> list[dict]:
    out = []
    for exp_key, exp in data["experts"].items():
        for f in exp["findings"]:
            out.append({**f, "expert": exp_key, "expert_name": exp["name"], "expert_icon": exp["icon"], "expert_color": exp["color"]})
    return out


_metrics_cache: dict[str, Any] = {"ts": 0, "data": None}


def collect_metrics(force: bool = False) -> dict:
    """Live metrics from the repo. Cached 60s."""
    if not force and _metrics_cache["data"] and time.time() - _metrics_cache["ts"] < 60:
        return _metrics_cache["data"]

    def count_files(root: Path, *exts: str) -> int:
        if not root.exists():
            return 0
        n = 0
        for ext in exts:
            n += sum(1 for _ in root.rglob(f"*{ext}"))
        return n

    def grep_count(root: Path, pattern: str, *exts: str) -> int:
        if not root.exists():
            return 0
        n = 0
        rx = re.compile(pattern)
        for ext in exts:
            for f in root.rglob(f"*{ext}"):
                try:
                    text = f.read_text(encoding="utf-8", errors="ignore")
                    n += len(rx.findall(text))
                except Exception:
                    pass
        return n

    fe_src = ROOT / "frontend" / "src"
    rust_src = ROOT / "frontend" / "src-tauri" / "src"
    py_src = ROOT / "backend" / "app"
    llama_src = ROOT / "llama-helper" / "src"

    metrics = {
        "files": {
            "frontend_ts": count_files(fe_src, ".ts", ".tsx"),
            "rust_tauri": count_files(rust_src, ".rs"),
            "python_backend": count_files(py_src, ".py"),
            "llama_helper": count_files(llama_src, ".rs"),
        },
        "tests": {
            "rust_markers": grep_count(rust_src, r"#\[(test|tokio::test)\]", ".rs") + grep_count(llama_src, r"#\[(test|tokio::test)\]", ".rs"),
            "python_funcs": grep_count(ROOT / "backend" / "tests", r"def test_", ".py"),
            "frontend_specs": count_files(fe_src, ".test.ts", ".test.tsx", ".spec.ts", ".spec.tsx"),
        },
        "quality": {
            "unwrap_calls": grep_count(rust_src, r"\.unwrap\(\)", ".rs"),
            "console_log": grep_count(fe_src, r"console\.(log|debug|info)", ".ts", ".tsx"),
            "ts_any": grep_count(fe_src, r": any\b", ".ts", ".tsx"),
            "todo_fixme": grep_count(fe_src, r"TODO|FIXME", ".ts", ".tsx") + grep_count(rust_src, r"TODO|FIXME", ".rs") + grep_count(py_src, r"TODO|FIXME", ".py"),
        },
        "build": {
            "target_exists": (ROOT / "frontend" / "src-tauri" / "target").exists() or (ROOT / "target").exists(),
            "node_modules_exists": (ROOT / "frontend" / "node_modules").exists(),
            "cargo_lock": (ROOT / "Cargo.lock").exists(),
        },
        "git": {},
    }

    # Git info
    try:
        branch = subprocess.run(["git", "branch", "--show-current"], cwd=ROOT, capture_output=True, text=True, timeout=3)
        commit = subprocess.run(["git", "log", "-1", "--format=%h %s"], cwd=ROOT, capture_output=True, text=True, timeout=3)
        ahead = subprocess.run(["git", "rev-list", "--count", "HEAD", "^main"], cwd=ROOT, capture_output=True, text=True, timeout=3)
        metrics["git"] = {
            "branch": branch.stdout.strip(),
            "last_commit": commit.stdout.strip(),
            "ahead_of_main": ahead.stdout.strip() or "0",
        }
    except Exception:
        pass

    # Background build status (poll log files written by background tasks)
    cargo_log = TMP / "cargo_check.log"
    npm_log = TMP / "npm_install.log"
    metrics["build"]["cargo_status"] = _read_status(cargo_log, "CARGO_DONE")
    metrics["build"]["npm_status"] = _read_status(npm_log, "NPM_DONE")

    _metrics_cache["data"] = metrics
    _metrics_cache["ts"] = time.time()
    return metrics


def _read_status(log: Path, marker: str) -> dict:
    if not log.exists():
        return {"state": "not_started"}
    try:
        text = log.read_text(encoding="utf-8", errors="ignore")
    except Exception:
        return {"state": "unknown"}
    if marker in text:
        m = re.search(rf"{marker} rc=(\d+)", text)
        rc = int(m.group(1)) if m else -1
        return {"state": "ok" if rc == 0 else "failed", "rc": rc, "log_tail": text[-800:]}
    return {"state": "running", "log_tail": text[-800:]}


def consult_finding(finding_id: str) -> dict:
    """Para un EXP-ID, devuelve: el finding, expertos relacionados, conflictos potenciales."""
    data = load_assembly()
    flat = all_findings_flat(data)
    target = next((f for f in flat if f["id"] == finding_id), None)
    if not target:
        raise HTTPException(404, f"Finding {finding_id} no encontrado")

    expert_key = target["expert"]
    target_words = set(re.findall(r"[a-záéíóúñ]+", (target["title"] + " " + target["description"]).lower()))
    target_words = {w for w in target_words if len(w) > 4}

    # Related: same words / same files referenced
    related = []
    for f in flat:
        if f["id"] == finding_id:
            continue
        words = set(re.findall(r"[a-záéíóúñ]+", (f["title"] + " " + f["description"]).lower()))
        overlap = len(target_words & words)
        if overlap >= 3:
            related.append({**f, "overlap_score": overlap})
    related.sort(key=lambda x: -x["overlap_score"])
    related = related[:6]

    # Conflicts: experts with structural tension
    tension_experts = EXPERT_TENSIONS.get(expert_key, [])
    conflicts = [f for f in flat if f["expert"] in tension_experts and f != target][:6]

    # "Votes" by all experts: simulated based on whether they have related findings or conflict tension
    votes = {}
    for ek, exp in data["experts"].items():
        if ek == expert_key:
            votes[ek] = {"verdict": "AUTOR", "reason": "Este experto propuso el hallazgo."}
            continue
        has_related = any(r["expert"] == ek for r in related)
        in_tension = ek in tension_experts
        if has_related and in_tension:
            verdict = "PRECAUCIÓN"
            reason = f"{exp['name']} tiene hallazgos relacionados pero su perspectiva puede entrar en conflicto con esta solución."
        elif has_related:
            verdict = "APOYA"
            reason = f"{exp['name']} tiene hallazgos relacionados y se beneficia del cambio."
        elif in_tension:
            verdict = "OBJETA"
            reason = f"La solución podría afectar negativamente la perspectiva de {exp['name']} (ver tensiones cruzadas)."
        else:
            verdict = "NEUTRAL"
            reason = f"{exp['name']} no tiene posición directa sobre este hallazgo."
        votes[ek] = {"verdict": verdict, "reason": reason, "icon": exp["icon"], "name": exp["name"]}

    return {
        "target": target,
        "related": related,
        "conflicts": conflicts,
        "votes": votes,
        "tension_experts": tension_experts,
    }


# ──────────────────────────── routes ──────────────────────────── #
@app.get("/health")
async def health():
    result = {
        "ok": True,
        "version": PORTAL_VERSION,
        "uptime_sec": round(time.time() - STARTUP_TIME, 1),
        "assembly_loaded": ASSEMBLY_FILE.exists(),
        "pid": os.getpid(),
    }
    if _HAS_PSUTIL:
        try:
            p = psutil.Process()
            result["memory_mb"] = round(p.memory_info().rss / 1024 / 1024, 1)
            result["cpu_percent"] = round(p.cpu_percent(interval=0.05), 1)
        except Exception:
            pass
    return result


@app.get("/api/findings")
async def api_findings():
    return JSONResponse(load_assembly())


@app.get("/api/metrics")
async def api_metrics():
    return JSONResponse(collect_metrics())


@app.get("/api/consult/{finding_id}")
async def api_consult(finding_id: str):
    return JSONResponse(consult_finding(finding_id.upper()))


@app.get("/api/activity")
async def api_activity():
    """Devuelve resumen estructurado de actividad reciente desde IMPROVEMENT_LOG y git log."""
    log_file = MEMORY_DIR / "IMPROVEMENT_LOG.md"
    entries = []
    if log_file.exists():
        text = log_file.read_text(encoding="utf-8")
        # Parse markdown sections starting with "### YYYY-MM-DD"
        chunks = re.split(r"\n### ", text)
        for chunk in chunks[1:]:  # skip header
            lines = chunk.split("\n")
            header = lines[0].strip()
            body = {}
            for line in lines[1:]:
                m = re.match(r"- \*\*([^*]+):\*\* (.+)", line)
                if m:
                    body[m.group(1).strip()] = m.group(2).strip()
            entries.append({"header": header, "body": body})

    # Git log of last 15 commits
    commits = []
    try:
        result = subprocess.run(
            ["git", "log", "-15", "--format=%h|%ai|%an|%s"],
            cwd=ROOT, capture_output=True, text=True, timeout=5,
        )
        for line in result.stdout.strip().split("\n"):
            parts = line.split("|", 3)
            if len(parts) == 4:
                commits.append({"hash": parts[0], "date": parts[1], "author": parts[2], "subject": parts[3]})
    except Exception:
        pass

    # Open PRs from gh CLI (best-effort, may be slow)
    prs = []
    try:
        result = subprocess.run(
            ["gh", "pr", "list", "--repo", "ponchovillalobos/maity_desktop-1", "--state", "open",
             "--json", "number,title,headRefName,state,url,createdAt"],
            capture_output=True, text=True, timeout=8,
        )
        if result.returncode == 0:
            prs = json.loads(result.stdout)
    except Exception:
        pass

    return JSONResponse({
        "entries": entries,
        "commits": commits,
        "prs": prs,
        "generated_at": time.strftime("%Y-%m-%d %H:%M:%S"),
    })


@app.get("/api/memory")
async def api_memory():
    files = ["IMPROVEMENT_LOG.md", "FAILED_ATTEMPTS.md", "METRICS_HISTORY.md", "ANALYSIS_STATE.md"]
    out = {}
    for fname in files:
        fpath = MEMORY_DIR / fname
        if fpath.exists():
            out[fname] = fpath.read_text(encoding="utf-8")
        else:
            out[fname] = "_(missing)_"
    return JSONResponse(out)


# ──────────────────────────── HTML ──────────────────────────── #
INDEX_HTML = r"""<!doctype html>
<html lang="es">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Maity Desktop — Portal de Asamblea</title>
<script src="https://cdn.jsdelivr.net/npm/chart.js@4.4.0/dist/chart.umd.min.js"></script>
<script src="https://cdn.jsdelivr.net/npm/marked@12.0.0/marked.min.js"></script>
<style>
:root {
  --bg:#0b0d12; --bg2:#11141b; --card:#161a23; --card2:#1c2130;
  --fg:#e8ecf3; --muted:#7e8694; --border:#252b3a; --accent:#27ae60;
  --critical:#e74c3c; --high:#e67e22; --medium:#f1c40f; --low:#95a5a6;
  --link:#3498db;
}
* { box-sizing:border-box; }
html,body { margin:0; padding:0; height:100%; background:var(--bg); color:var(--fg);
  font-family:-apple-system,Segoe UI,Roboto,Helvetica,Arial,sans-serif; font-size:14px; }
.app { display:grid; grid-template-columns:240px 1fr; height:100vh; }

/* Sidebar */
aside { background:var(--bg2); border-right:1px solid var(--border); display:flex; flex-direction:column; }
aside .brand { padding:18px 20px; border-bottom:1px solid var(--border); }
aside .brand h1 { margin:0; font-size:16px; font-weight:700; }
aside .brand p { margin:2px 0 0; font-size:11px; color:var(--muted); }
aside nav { padding:12px 0; flex:1; overflow:auto; }
aside nav button { display:block; width:100%; text-align:left; background:transparent; border:0;
  color:var(--fg); padding:10px 20px; font-size:13px; cursor:pointer; border-left:3px solid transparent; }
aside nav button:hover { background:rgba(255,255,255,0.04); }
aside nav button.active { background:rgba(39,174,96,0.12); border-left-color:var(--accent); color:#fff; }
aside nav .sep { height:1px; background:var(--border); margin:8px 0; }
aside .footer { padding:14px 20px; font-size:11px; color:var(--muted); border-top:1px solid var(--border); }

/* Main */
main { overflow:auto; padding:24px 32px; }
main h2 { margin:0 0 6px; font-size:22px; font-weight:700; }
main h3 { margin:24px 0 12px; font-size:15px; font-weight:600; color:#bdc3c7; text-transform:uppercase; letter-spacing:0.5px; }
main p.lead { margin:0 0 18px; color:var(--muted); font-size:13px; }

/* Cards grid */
.stats { display:grid; grid-template-columns:repeat(5,1fr); gap:14px; margin-bottom:20px; }
@media (max-width:1100px) { .stats { grid-template-columns:repeat(3,1fr); } }
.stat { background:var(--card); border:1px solid var(--border); border-radius:12px; padding:18px; }
.stat .num { font-size:30px; font-weight:700; line-height:1; }
.stat .lbl { font-size:11px; color:var(--muted); text-transform:uppercase; letter-spacing:0.5px; margin-top:6px; }
.stat.crit .num { color:var(--critical); }
.stat.warn .num { color:var(--high); }
.stat.ok .num { color:#2ecc71; }

.progress { background:var(--card); border:1px solid var(--border); border-radius:8px;
  height:28px; overflow:hidden; margin-bottom:24px; position:relative; }
.progress-bar { background:linear-gradient(90deg,#27ae60,#2ecc71); height:100%; transition:width 0.4s; }
.progress-text { position:absolute; inset:0; display:flex; align-items:center; justify-content:center;
  font-size:12px; font-weight:600; }

/* Filter bar */
.filters { display:flex; flex-wrap:wrap; gap:8px; margin-bottom:16px; align-items:center; }
.filters input[type=text] { background:var(--card); border:1px solid var(--border); color:var(--fg);
  padding:8px 12px; border-radius:8px; font-size:13px; min-width:200px; }
.chip { background:var(--card); border:1px solid var(--border); color:var(--fg); padding:6px 12px;
  border-radius:20px; font-size:12px; cursor:pointer; }
.chip.active { background:var(--accent); border-color:var(--accent); color:#fff; }
.chip:hover { background:#252b3a; }
.chip.active:hover { background:#229954; }

/* Finding cards */
.findings { display:grid; gap:14px; }
.finding {
  background:var(--card); border:1px solid var(--border); border-radius:12px;
  display:grid; grid-template-columns:6px 1fr; overflow:hidden;
}
.finding .stripe { width:6px; }
.finding .body { padding:18px 22px; }
.finding header { display:flex; align-items:flex-start; gap:12px; margin-bottom:10px; flex-wrap:wrap; }
.finding header .id { font-family:Consolas,Monaco,monospace; background:rgba(52,152,219,0.15);
  color:#5dade2; padding:3px 8px; border-radius:4px; font-size:11px; font-weight:600; }
.finding header .title { flex:1; font-size:16px; font-weight:600; min-width:200px; }
.finding header .badges { display:flex; gap:6px; flex-wrap:wrap; }
.badge { display:inline-block; padding:3px 9px; border-radius:10px; font-size:10px;
  font-weight:700; text-transform:uppercase; letter-spacing:0.3px; }
.badge.sev { color:#fff; }
.badge.expert { background:rgba(255,255,255,0.06); border:1px solid var(--border); color:#bdc3c7; }
.badge.phase { background:rgba(46,204,113,0.15); color:#2ecc71; border:1px solid rgba(46,204,113,0.3); }
.badge.status-pending { background:rgba(241,196,15,0.15); color:#f1c40f; border:1px solid rgba(241,196,15,0.3); }
.badge.status-in-progress { background:rgba(52,152,219,0.2); color:#5dade2; border:1px solid rgba(52,152,219,0.4); }
.badge.status-done { background:rgba(46,204,113,0.2); color:#2ecc71; border:1px solid rgba(46,204,113,0.4); }

.finding .section { margin:10px 0; }
.finding .section .lbl { font-size:10px; color:var(--muted); text-transform:uppercase;
  letter-spacing:0.5px; font-weight:700; margin-bottom:4px; }
.finding .section .txt { font-size:13px; line-height:1.55; color:#d8dde6; }
.finding .meta { display:flex; gap:18px; margin-top:14px; padding-top:12px; border-top:1px solid var(--border);
  flex-wrap:wrap; align-items:center; }
.bar { display:flex; align-items:center; gap:6px; font-size:11px; color:var(--muted); }
.bar .track { width:80px; height:6px; background:var(--bg); border-radius:3px; overflow:hidden; }
.bar .fill { height:100%; }
.bar.impact .fill { background:#3498db; }
.bar.effort .fill { background:#e67e22; }
.bar .val { font-weight:700; color:var(--fg); }
.refs a { color:var(--link); text-decoration:none; font-size:11px; margin-right:10px; }
.refs a:hover { text-decoration:underline; }
.consult-btn { margin-left:auto; background:transparent; border:1px solid var(--accent); color:var(--accent);
  padding:6px 14px; border-radius:6px; font-size:12px; cursor:pointer; }
.consult-btn:hover { background:var(--accent); color:#fff; }

/* Roadmap */
.roadmap { display:grid; grid-template-columns:1fr 1fr 1fr; gap:14px; }
.rm-col { background:var(--card); border:1px solid var(--border); border-radius:12px; padding:18px; }
.rm-col h4 { margin:0 0 4px; font-size:14px; }
.rm-col .sub { font-size:11px; color:var(--muted); margin-bottom:14px; }
.rm-col .count { float:right; background:var(--bg); padding:2px 8px; border-radius:10px; font-size:11px; color:var(--muted); }
.rm-item { padding:10px 0; border-bottom:1px solid var(--border); display:flex; gap:10px; align-items:flex-start; font-size:12px; }
.rm-item:last-child { border-bottom:0; }
.rm-item .id { font-family:Consolas,monospace; background:rgba(52,152,219,0.15); color:#5dade2;
  padding:1px 6px; border-radius:3px; font-size:10px; min-width:64px; text-align:center; flex-shrink:0; }
.rm-item .dot { width:8px; height:8px; border-radius:50%; margin-top:5px; flex-shrink:0; }
.rm-item.done { opacity:0.45; text-decoration:line-through; }

/* Modal Consult */
.modal { display:none; position:fixed; inset:0; background:rgba(0,0,0,0.7); z-index:100;
  align-items:flex-start; justify-content:center; padding:40px 20px; overflow:auto; }
.modal.open { display:flex; }
.modal-content { background:var(--bg2); border:1px solid var(--border); border-radius:14px;
  max-width:900px; width:100%; padding:28px; max-height:none; }
.modal-content h3 { margin:0 0 14px; font-size:18px; }
.modal-content .close { float:right; background:transparent; border:0; color:var(--fg); font-size:24px; cursor:pointer; }
.votes { display:grid; grid-template-columns:repeat(2,1fr); gap:8px; margin:14px 0; }
.vote { background:var(--card); border:1px solid var(--border); border-radius:8px; padding:10px 12px; font-size:12px; }
.vote .v { font-weight:700; font-size:11px; }
.vote.APOYA { border-left:3px solid #2ecc71; }
.vote.APOYA .v { color:#2ecc71; }
.vote.OBJETA { border-left:3px solid #e74c3c; }
.vote.OBJETA .v { color:#e74c3c; }
.vote.PRECAUCIÓN { border-left:3px solid #e67e22; }
.vote.PRECAUCIÓN .v { color:#e67e22; }
.vote.NEUTRAL { border-left:3px solid var(--muted); }
.vote.NEUTRAL .v { color:var(--muted); }
.vote.AUTOR { border-left:3px solid #3498db; }
.vote.AUTOR .v { color:#3498db; }

/* Dashboard tiles */
.dash-grid { display:grid; grid-template-columns:repeat(3,1fr); gap:14px; }
.dash-tile { background:var(--card); border:1px solid var(--border); border-radius:12px; padding:18px; }
.dash-tile h4 { margin:0 0 14px; font-size:13px; color:#bdc3c7; }
.kv { display:flex; justify-content:space-between; padding:8px 0; border-bottom:1px solid var(--border); font-size:12px; }
.kv:last-child { border-bottom:0; }
.kv .v { font-weight:700; font-family:Consolas,monospace; }
.kv .v.crit { color:var(--critical); }
.kv .v.warn { color:var(--high); }
.kv .v.ok { color:#2ecc71; }
.build-status { display:inline-block; padding:3px 10px; border-radius:10px; font-size:11px; font-weight:600; }
.build-status.ok { background:rgba(46,204,113,0.2); color:#2ecc71; }
.build-status.failed { background:rgba(231,76,60,0.2); color:var(--critical); }
.build-status.running { background:rgba(241,196,15,0.2); color:var(--medium); }
.build-status.not_started { background:rgba(149,165,166,0.2); color:var(--muted); }

#chart-wrap { background:var(--card); border:1px solid var(--border); border-radius:12px;
  padding:18px; height:380px; margin-bottom:20px; }

.memory-section { background:var(--card); border:1px solid var(--border); border-radius:12px;
  padding:20px 24px; margin-bottom:14px; }
.memory-section h4 { margin:0 0 12px; font-size:14px; color:#bdc3c7; }
.memory-section table { width:100%; border-collapse:collapse; font-size:12px; }
.memory-section th, .memory-section td { border:1px solid var(--border); padding:6px 10px; text-align:left; }
.memory-section code { background:var(--bg); padding:2px 6px; border-radius:3px; font-size:11px; }
.memory-section pre { background:var(--bg); padding:12px; border-radius:6px; overflow:auto; font-size:11px; }
.refresh-btn { background:var(--card); border:1px solid var(--border); color:var(--fg);
  padding:6px 14px; border-radius:6px; font-size:12px; cursor:pointer; }
.refresh-btn:hover { background:var(--accent); border-color:var(--accent); }

.refresh-banner { background:linear-gradient(90deg, rgba(39,174,96,0.15), rgba(39,174,96,0.05));
  border:1px solid rgba(39,174,96,0.4); border-radius:8px; padding:8px 14px;
  font-size:11px; color:#2ecc71; margin-bottom:18px; display:flex; align-items:center;
  gap:8px; font-weight:600; letter-spacing:0.3px; }

.global-report { background:linear-gradient(135deg, rgba(39,174,96,0.08), rgba(52,152,219,0.05));
  border:1px solid rgba(39,174,96,0.3); border-radius:12px; padding:20px 24px; margin-bottom:20px; }
.global-grid { display:grid; grid-template-columns:repeat(4, 1fr); gap:14px; }
.global-item { background:var(--card); border:1px solid var(--border); border-radius:8px;
  padding:14px; text-align:center; }
.global-item .gn { font-size:28px; font-weight:800; line-height:1; }
.global-item .gl { font-size:10px; color:var(--muted); text-transform:uppercase;
  letter-spacing:0.5px; margin-top:6px; font-weight:600; }
</style>
</head>
<body>
<div class="app">
  <aside>
    <div class="brand">
      <h1>🎙️ Maity Desktop</h1>
      <p>Portal de Asamblea v2.0</p>
    </div>
    <nav>
      <button class="active" data-view="assembly">📋 Asamblea</button>
      <button data-view="activity">📰 Actividad</button>
      <button data-view="roadmap">🗺️ Roadmap</button>
      <button data-view="dashboard">📈 Dashboard</button>
      <button data-view="memory">🧠 Memoria</button>
      <div class="sep"></div>
      <button data-view="experts">👥 Expertos</button>
    </nav>
    <div class="footer" id="footer-info">cargando…</div>
  </aside>

  <main id="main">
    <div id="view"></div>
  </main>
</div>

<div class="modal" id="modal" onclick="if(event.target.id==='modal')closeModal()">
  <div class="modal-content" id="modal-content"></div>
</div>

<script>
let DATA = null;
let METRICS = null;
let FILTERS = { search:'', expert:'all', severity:'all', phase:'all', status:'all' };

let CURRENT_VIEW = 'assembly';
let LAST_UPDATE = null;

// PORTAL-002: safeFetch with timeout + graceful degradation
async function safeFetch(url, timeoutMs=3000, fallback=null) {
  const ctrl = new AbortController();
  const tid = setTimeout(() => ctrl.abort(), timeoutMs);
  try {
    const r = await fetch(url, { signal: ctrl.signal });
    if (!r.ok) throw new Error('HTTP ' + r.status);
    return await r.json();
  } catch (err) {
    console.warn('safeFetch failed:', url, err.message);
    return fallback;
  } finally {
    clearTimeout(tid);
  }
}

async function loadAll(silent=false) {
  const [d, m] = await Promise.all([
    safeFetch('/api/findings?_=' + Date.now(), 5000, DATA),
    safeFetch('/api/metrics?_=' + Date.now(), 5000, METRICS),
  ]);
  if (!d || !m) { console.warn('loadAll: partial failure, keeping old data'); return; }
  DATA = d; METRICS = m;
  LAST_UPDATE = new Date();
  document.getElementById('footer-info').innerHTML =
    `Branch: <b>${m.git.branch || '?'}</b><br>Commit: ${(m.git.last_commit||'').slice(0,40)}<br>` +
    `v${d.project.version} · iter ${d.iterations} · commits ${d.commits}`;
  if (!silent) show(CURRENT_VIEW);
  updateRefreshBanner();
}

function updateRefreshBanner() {
  const banner = document.getElementById('refresh-banner');
  if (!banner || !LAST_UPDATE) return;
  const secs = Math.round((Date.now() - LAST_UPDATE.getTime()) / 1000);
  banner.innerHTML = `🟢 Auto-refresh activo · última actualización hace ${secs}s · iter ${DATA.iterations} · commits ${DATA.commits} · ${DATA.experts && Object.keys(DATA.experts).length} expertos`;
}

// Auto-refresh cada 8 segundos
setInterval(async () => {
  try {
    await loadAll(true);
    // Re-render solo la vista actual sin perder filtros
    show(CURRENT_VIEW);
  } catch (e) { console.error('refresh error', e); }
}, 8000);

// Update banner timer cada segundo
setInterval(updateRefreshBanner, 1000);

function flat() {
  const out = [];
  for (const [k, exp] of Object.entries(DATA.experts)) {
    for (const f of exp.findings) {
      out.push({...f, expert:k, expert_name:exp.name, expert_icon:exp.icon, expert_color:exp.color});
    }
  }
  return out;
}

function applyFilters(items) {
  return items.filter(f => {
    if (FILTERS.expert !== 'all' && f.expert !== FILTERS.expert) return false;
    if (FILTERS.severity !== 'all' && f.severity !== FILTERS.severity) return false;
    if (FILTERS.phase !== 'all' && f.phase !== FILTERS.phase) return false;
    if (FILTERS.status !== 'all' && f.status !== FILTERS.status) return false;
    if (FILTERS.search) {
      const q = FILTERS.search.toLowerCase();
      const hay = (f.id + ' ' + f.title + ' ' + f.description + ' ' + f.recommendation).toLowerCase();
      if (!hay.includes(q)) return false;
    }
    return true;
  });
}

const SEV_COLOR = {critical:'#e74c3c', high:'#e67e22', medium:'#f1c40f', low:'#95a5a6'};
const SEV_LABEL = {critical:'CRÍTICO', high:'ALTO', medium:'MEDIO', low:'BAJO'};

function findingCard(f) {
  const refs = (f.references||[]).map(r => `<a href="${r}" target="_blank">↗ ${r.replace(/^https?:\/\//,'').slice(0,50)}</a>`).join('');
  return `
    <div class="finding">
      <div class="stripe" style="background:${SEV_COLOR[f.severity]}"></div>
      <div class="body">
        <header>
          <span class="id">${f.id}</span>
          <div class="title">${f.title}</div>
          <div class="badges">
            <span class="badge sev" style="background:${SEV_COLOR[f.severity]}">${SEV_LABEL[f.severity]}</span>
            <span class="badge expert">${f.expert_icon} ${f.expert_name}</span>
            <span class="badge phase">${f.phase}</span>
            <span class="badge status-${f.status}">${f.status === 'done' ? '✅ DONE' : f.status === 'in-progress' ? '🔄 EN PR' : '⏳ PENDING'}</span>
          </div>
        </header>
        <div class="section">
          <div class="lbl">🔍 Problema (descripción)</div>
          <div class="txt">${f.description}</div>
        </div>
        <div class="section">
          <div class="lbl">💡 Solución (recomendación)</div>
          <div class="txt">${f.recommendation}</div>
        </div>
        <div class="section">
          <div class="lbl">🎯 Razón / Por qué importa</div>
          <div class="txt">Severidad <b>${SEV_LABEL[f.severity]}</b>. Impacto ${f.impact}/10, esfuerzo ${f.effort}/10 → ratio prioridad ${(f.impact/Math.max(f.effort,1)).toFixed(2)}. Asignado a fase <b>${f.phase}</b>.</div>
        </div>
        <div class="meta">
          <div class="bar impact" title="Impact">
            <span>Impact</span><div class="track"><div class="fill" style="width:${f.impact*10}%"></div></div><span class="val">${f.impact}/10</span>
          </div>
          <div class="bar effort" title="Effort">
            <span>Effort</span><div class="track"><div class="fill" style="width:${f.effort*10}%"></div></div><span class="val">${f.effort}/10</span>
          </div>
          ${refs ? `<div class="refs">${refs}</div>` : ''}
          <button class="consult-btn" onclick="consult('${f.id}')">🗣️ Consultar asamblea</button>
        </div>
      </div>
    </div>`;
}

function show(view) {
  CURRENT_VIEW = view;
  document.querySelectorAll('aside nav button').forEach(b => b.classList.toggle('active', b.dataset.view === view));
  const target = document.getElementById('view');
  if (view === 'assembly') target.innerHTML = renderAssembly();
  else if (view === 'roadmap') target.innerHTML = renderRoadmap();
  else if (view === 'dashboard') { target.innerHTML = renderDashboard(); renderChart(); }
  else if (view === 'memory') renderMemory(target);
  else if (view === 'experts') target.innerHTML = renderExperts();
  else if (view === 'activity') renderActivity(target);
  // Inject refresh banner at top of every view
  if (target.firstElementChild && target.firstElementChild.id !== 'refresh-banner-wrap') {
    const wrap = document.createElement('div');
    wrap.id = 'refresh-banner-wrap';
    wrap.innerHTML = '<div id="refresh-banner" class="refresh-banner">cargando...</div>';
    target.insertBefore(wrap, target.firstChild);
    updateRefreshBanner();
  }
}

async function renderActivity(target) {
  target.innerHTML = '<h2>📰 Actividad reciente</h2><p class="lead">cargando...</p>';
  const a = await fetch('/api/activity?_=' + Date.now()).then(r => r.json());
  let html = `<h2>📰 Actividad reciente</h2><p class="lead">Timeline de iteraciones, commits y PRs. Generado a las ${a.generated_at}.</p>`;

  // PRs abiertos
  if (a.prs && a.prs.length) {
    html += '<h3>🔗 Pull Requests abiertos en el fork</h3><div class="findings">';
    for (const pr of a.prs) {
      html += `<div class="finding"><div class="stripe" style="background:#3498db"></div><div class="body">
        <header>
          <span class="id">PR #${pr.number}</span>
          <div class="title">${pr.title}</div>
          <div class="badges"><span class="badge phase">OPEN</span></div>
        </header>
        <div class="section"><div class="lbl">Branch</div><div class="txt"><code>${pr.headRefName}</code></div></div>
        <div class="section"><div class="lbl">Creado</div><div class="txt">${pr.createdAt}</div></div>
        <div class="meta"><a href="${pr.url}" target="_blank" class="consult-btn">↗ Ver en GitHub</a></div>
      </div></div>`;
    }
    html += '</div>';
  }

  // Iteraciones del IMPROVEMENT_LOG
  if (a.entries && a.entries.length) {
    html += '<h3>📋 Iteraciones registradas (IMPROVEMENT_LOG)</h3><div class="findings">';
    for (const e of a.entries) {
      const expSplit = (e.body['Experto'] || '').split(' ');
      const ico = expSplit[0] || '📝';
      const expName = expSplit.slice(1).join(' ');
      const sev = (e.body['Severity'] || '').toLowerCase();
      const sevColor = SEV_COLOR[sev] || '#3498db';
      html += `<div class="finding"><div class="stripe" style="background:${sevColor}"></div><div class="body">
        <header>
          <div class="title">${ico} ${e.header}</div>
          <div class="badges">
            ${e.body['Phase'] ? `<span class="badge phase">${e.body['Phase']}</span>` : ''}
            ${e.body['Estado'] ? `<span class="badge status-pending">${e.body['Estado']}</span>` : ''}
          </div>
        </header>
        ${e.body['Título'] ? `<div class="section"><div class="lbl">Título</div><div class="txt">${e.body['Título']}</div></div>` : ''}
        ${e.body['Branch'] ? `<div class="section"><div class="lbl">Branch</div><div class="txt"><code>${e.body['Branch']}</code></div></div>` : ''}
        ${e.body['PR'] ? `<div class="section"><div class="lbl">Pull Request</div><div class="txt"><a href="${e.body['PR'].replace(/[<>]/g,'')}" target="_blank" style="color:#5dade2">${e.body['PR']}</a></div></div>` : ''}
        ${e.body['Archivos'] ? `<div class="section"><div class="lbl">Archivos</div><div class="txt">${e.body['Archivos']}</div></div>` : ''}
        ${e.body['Quality gates'] ? `<div class="section"><div class="lbl">Quality gates</div><div class="txt">${e.body['Quality gates']}</div></div>` : ''}
        ${e.body['Consulta asamblea'] ? `<div class="section"><div class="lbl">Consulta asamblea</div><div class="txt">${e.body['Consulta asamblea']}</div></div>` : ''}
        ${e.body['Cómo se detectó'] ? `<div class="section"><div class="lbl">Cómo se detectó</div><div class="txt">${e.body['Cómo se detectó']}</div></div>` : ''}
        ${e.body['Notas'] ? `<div class="section"><div class="lbl">Notas</div><div class="txt">${e.body['Notas']}</div></div>` : ''}
      </div></div>`;
    }
    html += '</div>';
  }

  // Commits recientes
  if (a.commits && a.commits.length) {
    html += '<h3>📝 Últimos commits del repo</h3><div class="dash-tile">';
    for (const c of a.commits) {
      html += `<div class="kv"><span><code>${c.hash}</code> <small style="color:var(--muted)">${c.date.slice(0,10)}</small> ${c.subject}</span><span class="v" style="font-size:10px">${c.author}</span></div>`;
    }
    html += '</div>';
  }

  target.innerHTML = html;
  // Re-inject banner after async render
  if (target.firstElementChild && target.firstElementChild.id !== 'refresh-banner-wrap') {
    const wrap = document.createElement('div');
    wrap.id = 'refresh-banner-wrap';
    wrap.innerHTML = '<div id="refresh-banner" class="refresh-banner">cargando...</div>';
    target.insertBefore(wrap, target.firstChild);
    updateRefreshBanner();
  }
}

function renderAssembly() {
  const all = flat();
  const total = all.length;
  const done = all.filter(f => f.status === 'done').length;
  const inprogress = all.filter(f => f.status === 'in-progress').length;
  const pending = all.filter(f => f.status === 'pending').length;
  const critical = all.filter(f => f.severity === 'critical').length;
  const shipped = done + inprogress;
  const pct = total ? Math.round(shipped/total*100) : 0;

  const items = applyFilters(all).sort((a,b) => (b.impact/Math.max(b.effort,1)) - (a.impact/Math.max(a.effort,1)));

  const expertChips = ['all', ...Object.keys(DATA.experts)].map(k => {
    const lbl = k === 'all' ? 'Todos' : `${DATA.experts[k].icon} ${DATA.experts[k].name}`;
    return `<button class="chip ${FILTERS.expert===k?'active':''}" onclick="setFilter('expert','${k}')">${lbl}</button>`;
  }).join('');
  const sevChips = ['all','critical','high','medium','low'].map(s =>
    `<button class="chip ${FILTERS.severity===s?'active':''}" onclick="setFilter('severity','${s}')">${s==='all'?'Todas':SEV_LABEL[s]}</button>`
  ).join('');
  const phaseChips = ['all','v1.0','v2.0','v3.0'].map(p =>
    `<button class="chip ${FILTERS.phase===p?'active':''}" onclick="setFilter('phase','${p}')">${p==='all'?'Todas':p}</button>`
  ).join('');
  const statusLabels = {all:'Todos', pending:'Pendientes', 'in-progress':'En PR', done:'Done'};
  const statusChips = ['all','pending','in-progress','done'].map(s =>
    `<button class="chip ${FILTERS.status===s?'active':''}" onclick="setFilter('status','${s}')">${statusLabels[s]}</button>`
  ).join('');

  return `
    <h2>📋 Asamblea de Expertos</h2>
    <p class="lead">${DATA.project.unique_value}</p>
    <div class="stats">
      <div class="stat"><div class="num">${total}</div><div class="lbl">Total Hallazgos</div></div>
      <div class="stat ok"><div class="num">${done}</div><div class="lbl">Done (merged)</div></div>
      <div class="stat" style="border-color:#3498db;background:rgba(52,152,219,0.06)"><div class="num" style="color:#5dade2">${inprogress}</div><div class="lbl">🔄 En PR (listo merge)</div></div>
      <div class="stat warn"><div class="num">${pending}</div><div class="lbl">Pendientes</div></div>
      <div class="stat crit"><div class="num">${critical}</div><div class="lbl">Críticos</div></div>
    </div>
    <div class="progress">
      <div class="progress-bar" style="width:${pct}%"></div>
      <div class="progress-text">${pct}% trabajo enviado · ${shipped}/${total} (${done} merged + ${inprogress} en PR)</div>
    </div>
    <h3>Filtros</h3>
    <div class="filters">
      <input type="text" placeholder="🔎 buscar en título/descripción/solución…" oninput="setFilter('search', this.value)">
    </div>
    <div class="filters"><b style="font-size:11px;color:var(--muted);width:60px">EXPERTO</b>${expertChips}</div>
    <div class="filters"><b style="font-size:11px;color:var(--muted);width:60px">SEV</b>${sevChips}</div>
    <div class="filters"><b style="font-size:11px;color:var(--muted);width:60px">FASE</b>${phaseChips}</div>
    <div class="filters"><b style="font-size:11px;color:var(--muted);width:60px">ESTADO</b>${statusChips}</div>
    <h3>Hallazgos (${items.length})</h3>
    <div class="findings">${items.map(findingCard).join('')}</div>
  `;
}

function setFilter(k, v) {
  FILTERS[k] = v;
  show('assembly');
  // restore search focus
  if (k === 'search') {
    const inp = document.querySelector('input[type=text]');
    if (inp) { inp.focus(); inp.setSelectionRange(v.length, v.length); }
  }
}

function renderRoadmap() {
  const all = flat();
  const cols = ['v1.0','v2.0','v3.0'].map(phase => {
    const items = (DATA.roadmap[phase]?.items || []).map(id => all.find(f => f.id === id)).filter(Boolean);
    const itemsHtml = items.map(f => `
      <div class="rm-item ${f.status==='done'?'done':''}">
        <span class="dot" style="background:${SEV_COLOR[f.severity]}"></span>
        <span class="id">${f.id}</span>
        <div style="flex:1">
          <div><b>${f.title}</b></div>
          <div style="color:var(--muted);font-size:11px;margin-top:4px;line-height:1.4">${f.recommendation.slice(0,180)}${f.recommendation.length>180?'…':''}</div>
        </div>
      </div>`).join('');
    return `
      <div class="rm-col">
        <h4>${phase} <span class="count">${items.length}</span></h4>
        <div class="sub">${DATA.roadmap[phase]?.label || ''}</div>
        ${itemsHtml || '<div style="color:var(--muted);font-size:12px">— vacío —</div>'}
      </div>`;
  }).join('');
  return `<h2>🗺️ Roadmap</h2><p class="lead">Distribución por fases — cada item con su recomendación abreviada. Los items con prioridad más alta deben hacerse primero.</p><div class="roadmap">${cols}</div>`;
}

function renderDashboard() {
  const m = METRICS;
  const cargoSt = m.build.cargo_status;
  const npmSt = m.build.npm_status;
  const all = flat();
  const done = all.filter(f => f.status === 'done').length;
  const ipArr = all.filter(f => f.status === 'in-progress');
  const ip = ipArr.length;
  const pending = all.filter(f => f.status === 'pending').length;
  const crit = all.filter(f => f.severity === 'critical').length;
  const critDone = all.filter(f => f.severity === 'critical' && f.status !== 'pending').length;
  const totalShipped = done + ip;

  return `
    <h2>📈 Dashboard de Mejora Continua</h2>
    <p class="lead">Métricas live del repo. <button class="refresh-btn" onclick="refreshMetrics()">↻ Refrescar</button></p>

    <div class="global-report">
      <h3 style="margin:0 0 14px;color:#27ae60;font-size:13px">🎯 REPORTE GLOBAL — Lo que ya se hizo</h3>
      <div class="global-grid">
        <div class="global-item"><div class="gn" style="color:#27ae60">${DATA.iterations || 0}</div><div class="gl">iteraciones completadas</div></div>
        <div class="global-item"><div class="gn" style="color:#27ae60">${DATA.commits || 0}</div><div class="gl">commits del sistema</div></div>
        <div class="global-item"><div class="gn" style="color:#3498db">${ip}</div><div class="gl">hallazgos en PR (in-progress)</div></div>
        <div class="global-item"><div class="gn" style="color:#2ecc71">${done}</div><div class="gl">hallazgos done (merged)</div></div>
        <div class="global-item"><div class="gn" style="color:#f1c40f">${pending}</div><div class="gl">hallazgos pending</div></div>
        <div class="global-item"><div class="gn" style="color:#e74c3c">${crit}</div><div class="gl">críticos totales</div></div>
        <div class="global-item"><div class="gn" style="color:#9b59b6">${all.length}</div><div class="gl">hallazgos totales asamblea</div></div>
        <div class="global-item"><div class="gn" style="color:${Math.round((totalShipped/all.length)*100) > 10 ? '#2ecc71' : '#f39c12'}">${Math.round((totalShipped/all.length)*100)}%</div><div class="gl">cubierto (done + in-progress)</div></div>
      </div>
      ${ipArr.length ? `
        <div style="margin-top:14px;padding-top:14px;border-top:1px solid var(--border)">
          <div style="font-size:11px;color:var(--muted);text-transform:uppercase;letter-spacing:0.5px;margin-bottom:8px">🔗 PRs abiertos (in-progress)</div>
          ${ipArr.map(f => `<div class="kv" style="border-bottom:0;padding:4px 0">
            <span><span style="background:${SEV_COLOR[f.severity]};color:#fff;padding:1px 6px;border-radius:3px;font-size:10px;font-weight:700">${f.id}</span> ${f.title.slice(0,60)}</span>
            <span class="v">${f.pr_url ? `<a href="${f.pr_url}" target="_blank" style="color:#5dade2">↗ PR</a>` : '—'}</span>
          </div>`).join('')}
        </div>
      ` : ''}
    </div>

    <div class="stats">
      <div class="stat"><div class="num">${m.files.frontend_ts + m.files.rust_tauri + m.files.python_backend + m.files.llama_helper}</div><div class="lbl">Archivos de código</div></div>
      <div class="stat ok"><div class="num">${m.tests.rust_markers + m.tests.python_funcs + m.tests.frontend_specs}</div><div class="lbl">Tests totales</div></div>
      <div class="stat crit"><div class="num">${m.quality.unwrap_calls}</div><div class="lbl">.unwrap() en Rust</div></div>
      <div class="stat warn"><div class="num">${m.quality.console_log}</div><div class="lbl">console.log en TS</div></div>
    </div>

    <div class="dash-grid">
      <div class="dash-tile">
        <h4>📂 Archivos por stack</h4>
        <div class="kv"><span>Frontend (TS/TSX)</span><span class="v">${m.files.frontend_ts}</span></div>
        <div class="kv"><span>Rust Tauri (.rs)</span><span class="v">${m.files.rust_tauri}</span></div>
        <div class="kv"><span>Python Backend (.py)</span><span class="v">${m.files.python_backend}</span></div>
        <div class="kv"><span>llama-helper (.rs)</span><span class="v">${m.files.llama_helper}</span></div>
      </div>
      <div class="dash-tile">
        <h4>🧪 Cobertura de tests</h4>
        <div class="kv"><span>Rust #[test]</span><span class="v ${m.tests.rust_markers<100?'warn':'ok'}">${m.tests.rust_markers}</span></div>
        <div class="kv"><span>Python def test_</span><span class="v ${m.tests.python_funcs<20?'warn':'ok'}">${m.tests.python_funcs}</span></div>
        <div class="kv"><span>Frontend .test/.spec</span><span class="v ${m.tests.frontend_specs===0?'crit':'ok'}">${m.tests.frontend_specs}</span></div>
      </div>
      <div class="dash-tile">
        <h4>⚠️ Markers de calidad</h4>
        <div class="kv"><span>.unwrap() en Rust</span><span class="v ${m.quality.unwrap_calls>50?'crit':'warn'}">${m.quality.unwrap_calls}</span></div>
        <div class="kv"><span>console.log en TS</span><span class="v ${m.quality.console_log>100?'crit':'warn'}">${m.quality.console_log}</span></div>
        <div class="kv"><span>: any en TS</span><span class="v warn">${m.quality.ts_any}</span></div>
        <div class="kv"><span>TODO/FIXME</span><span class="v">${m.quality.todo_fixme}</span></div>
      </div>
      <div class="dash-tile">
        <h4>🔨 Build / dependencias</h4>
        <div class="kv"><span>cargo check</span><span class="v"><span class="build-status ${cargoSt.state}">${cargoSt.state}${cargoSt.rc!==undefined?' rc='+cargoSt.rc:''}</span></span></div>
        <div class="kv"><span>npm install</span><span class="v"><span class="build-status ${npmSt.state}">${npmSt.state}${npmSt.rc!==undefined?' rc='+npmSt.rc:''}</span></span></div>
        <div class="kv"><span>target/ existe</span><span class="v ${m.build.target_exists?'ok':'warn'}">${m.build.target_exists?'sí':'no'}</span></div>
        <div class="kv"><span>node_modules/</span><span class="v ${m.build.node_modules_exists?'ok':'warn'}">${m.build.node_modules_exists?'sí':'no'}</span></div>
      </div>
      <div class="dash-tile">
        <h4>🌿 Git</h4>
        <div class="kv"><span>Branch</span><span class="v">${m.git.branch || '?'}</span></div>
        <div class="kv"><span>Último commit</span><span class="v" style="font-size:10px">${(m.git.last_commit||'').slice(0,30)}</span></div>
        <div class="kv"><span>Commits ahead</span><span class="v">${m.git.ahead_of_main || '0'}</span></div>
      </div>
      <div class="dash-tile">
        <h4>📋 Asamblea</h4>
        ${(()=>{const f=flat();const t=f.length;const d=f.filter(x=>x.status==='done').length;const c=f.filter(x=>x.severity==='critical').length;return `
        <div class="kv"><span>Total findings</span><span class="v">${t}</span></div>
        <div class="kv"><span>Done</span><span class="v ok">${d}</span></div>
        <div class="kv"><span>Critical pending</span><span class="v crit">${c}</span></div>
        <div class="kv"><span>v1.0 (quick wins)</span><span class="v">${DATA.roadmap['v1.0'].items.length}</span></div>
        <div class="kv"><span>v2.0 (major)</span><span class="v">${DATA.roadmap['v2.0'].items.length}</span></div>
        <div class="kv"><span>v3.0 (expansión)</span><span class="v">${DATA.roadmap['v3.0'].items.length}</span></div>
        `})()}
      </div>
    </div>

    <h3>📊 Impact vs Effort — todos los hallazgos</h3>
    <div id="chart-wrap"><canvas id="scatter"></canvas></div>

    <h3>📦 Logs de build (últimas líneas)</h3>
    <div class="dash-grid">
      <div class="dash-tile" style="grid-column:span 3">
        <h4>cargo check</h4>
        <pre style="background:var(--bg);padding:12px;border-radius:6px;font-size:11px;max-height:200px;overflow:auto">${(cargoSt.log_tail||'(sin output todavía)').replace(/</g,'&lt;')}</pre>
      </div>
      <div class="dash-tile" style="grid-column:span 3">
        <h4>npm install</h4>
        <pre style="background:var(--bg);padding:12px;border-radius:6px;font-size:11px;max-height:200px;overflow:auto">${(npmSt.log_tail||'(sin output todavía)').replace(/</g,'&lt;')}</pre>
      </div>
    </div>
  `;
}

async function refreshMetrics() {
  METRICS = await fetch('/api/metrics').then(r => r.json());
  show('dashboard');
}

function renderChart() {
  const ctx = document.getElementById('scatter');
  if (!ctx) return;
  const points = flat().map(f => ({
    x: f.effort, y: f.impact, id: f.id, title: f.title.slice(0,60),
    backgroundColor: SEV_COLOR[f.severity],
  }));
  new Chart(ctx, {
    type: 'scatter',
    data: { datasets: [{ label:'Hallazgos', data: points, pointRadius:7, pointHoverRadius:10,
      backgroundColor: points.map(p => p.backgroundColor) }] },
    options: {
      responsive:true, maintainAspectRatio:false,
      scales: {
        x: { title:{display:true,text:'Effort (1=fácil → 10=difícil)',color:'#bdc3c7'}, min:0, max:11, ticks:{color:'#7e8694'}, grid:{color:'#252b3a'} },
        y: { title:{display:true,text:'Impact (1=bajo → 10=alto)',color:'#bdc3c7'}, min:0, max:11, ticks:{color:'#7e8694'}, grid:{color:'#252b3a'} }
      },
      plugins: {
        legend: { display:false },
        tooltip: { callbacks: { label: ctx => ctx.raw.id + ': ' + ctx.raw.title } }
      }
    }
  });
}

async function renderMemory(target) {
  target.innerHTML = '<h2>🧠 Memoria del Sistema</h2><p class="lead">cargando…</p>';
  const m = await fetch('/api/memory').then(r => r.json());
  let html = '<h2>🧠 Memoria del Sistema</h2><p class="lead">Archivos donde el sistema guarda contexto entre ciclos de mejora.</p>';
  for (const [name, content] of Object.entries(m)) {
    html += `<div class="memory-section"><h4>📄 ${name}</h4>${marked.parse(content)}</div>`;
  }
  target.innerHTML = html;
}

function renderExperts() {
  const cards = Object.entries(DATA.experts).map(([k,exp]) => {
    const findings = exp.findings;
    const crit = findings.filter(f => f.severity==='critical').length;
    const done = findings.filter(f => f.status==='done').length;
    return `
      <div class="dash-tile">
        <h4>${exp.icon} ${exp.name}</h4>
        <p style="font-size:12px;color:var(--muted);font-style:italic;margin:0 0 12px">${exp.summary}</p>
        <div class="kv"><span>Total findings</span><span class="v">${findings.length}</span></div>
        <div class="kv"><span>Críticos</span><span class="v crit">${crit}</span></div>
        <div class="kv"><span>Completados</span><span class="v ok">${done}</span></div>
        <button class="consult-btn" style="margin-top:12px;width:100%" onclick="filterByExpert('${k}')">Ver hallazgos →</button>
      </div>`;
  }).join('');
  return `<h2>👥 Expertos de la Asamblea</h2><p class="lead">12 perspectivas que auditan el proyecto.</p><div class="dash-grid">${cards}</div>`;
}

function filterByExpert(k) {
  FILTERS = { search:'', expert:k, severity:'all', phase:'all', status:'all' };
  show('assembly');
}

async function consult(id) {
  const r = await fetch('/api/consult/' + id).then(r => r.json());
  const t = r.target;
  const votes = Object.values(r.votes).map(v =>
    `<div class="vote ${v.verdict}"><div class="v">${v.verdict}</div>${v.icon||''} ${v.name||''}<br><span style="color:var(--muted);font-size:11px">${v.reason}</span></div>`
  ).join('');
  const related = r.related.map(f => `<li><b>${f.id}</b> [${f.expert_icon} ${f.expert_name}] ${f.title}</li>`).join('') || '<li>(sin relacionados)</li>';
  const conflicts = r.conflicts.map(f => `<li><b>${f.id}</b> [${f.expert_icon} ${f.expert_name}] ${f.title}</li>`).join('') || '<li>(sin conflictos directos)</li>';
  document.getElementById('modal-content').innerHTML = `
    <button class="close" onclick="closeModal()">×</button>
    <h3>🗣️ Consulta a la asamblea sobre <span style="font-family:monospace;color:#5dade2">${t.id}</span></h3>
    <p style="color:var(--muted);font-size:12px;margin-top:-8px">${t.title}</p>
    <h4 style="margin:18px 0 8px;font-size:13px">Veredictos por experto</h4>
    <div class="votes">${votes}</div>
    <h4 style="margin:18px 0 8px;font-size:13px">🔗 Hallazgos relacionados (${r.related.length})</h4>
    <ul style="font-size:12px;line-height:1.7;color:#bdc3c7">${related}</ul>
    <h4 style="margin:18px 0 8px;font-size:13px">⚠️ Conflictos potenciales (${r.conflicts.length})</h4>
    <ul style="font-size:12px;line-height:1.7;color:#bdc3c7">${conflicts}</ul>
    <p style="margin-top:18px;font-size:11px;color:var(--muted)">Tensiones estructurales detectadas con: ${r.tension_experts.join(', ') || '(ninguna)'}</p>
  `;
  document.getElementById('modal').classList.add('open');
}

function closeModal() { document.getElementById('modal').classList.remove('open'); }

document.querySelectorAll('aside nav button').forEach(b => b.addEventListener('click', () => show(b.dataset.view)));
loadAll();
</script>
</body>
</html>
"""


@app.get("/", response_class=HTMLResponse)
async def index():
    return HTMLResponse(INDEX_HTML)


if __name__ == "__main__":
    import uvicorn
    if not _acquire_lock_file():
        print(f"[Maity Desktop] Portal ya esta corriendo (lock: {LOCK_FILE})")
        sys.exit(0)
    atexit.register(_release_lock_file)
    print("\n[Maity Desktop] Portal de Asamblea " + PORTAL_VERSION)
    print("    http://127.0.0.1:8770\n")
    try:
        uvicorn.run(app, host="127.0.0.1", port=8770, log_level="info")
    finally:
        _release_lock_file()

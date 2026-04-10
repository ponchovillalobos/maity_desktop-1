#!/usr/bin/env python3
"""
multi_agent_eval.py — Orquestador Multi-Agente de Maity Desktop
================================================================
Ejecuta los 4 agentes especializados en paralelo, recopila sus reportes,
consulta a la Asamblea de Expertos, y genera un informe unificado en
memory/eval_reports/YYYY-MM-DD_HH-MM.md

Agentes:
  1. auditor      — revisa código y docs, top 3 findings
  2. validator    — quality gates (cargo check, pytest, python syntax)
  3. janitor      — limpieza: branches viejos, archivos grandes, logs
  4. doc_checker  — verifica sincronía docs ↔ código

Uso:
  python scripts/multi_agent_eval.py
  python scripts/multi_agent_eval.py --quick   (solo auditor + validator)
  python scripts/multi_agent_eval.py --fix     (intenta auto-fix issues menores)

Salida:
  memory/eval_reports/YYYY-MM-DD_HH-MM.md  — reporte markdown unificado
  memory/eval_reports/latest.json           — JSON machine-readable (para CI)
  Exit code 0 = PASS, 1 = FAIL (quality gates rotos), 2 = ERROR
"""
from __future__ import annotations

import argparse
import asyncio
import datetime as dt
import json
import os
import subprocess
import sys
import time
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
MEMORY_DIR = ROOT / "memory"
EVAL_DIR = MEMORY_DIR / "eval_reports"
ASSEMBLY_FILE = ROOT / "scripts" / "assembly_data.json"
PORTAL_URL = "http://127.0.0.1:8770"

EVAL_DIR.mkdir(parents=True, exist_ok=True)

# ─────────────────────────────────────────────────────────────────────────────
# Helpers
# ─────────────────────────────────────────────────────────────────────────────

def _run(cmd: list[str], cwd: Path | None = None, timeout: int = 120) -> tuple[int, str]:
    """Ejecuta un subproceso y devuelve (exit_code, output_combinado)."""
    try:
        r = subprocess.run(
            cmd,
            cwd=str(cwd or ROOT),
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout,
        )
        stdout = r.stdout or ""
        stderr = r.stderr or ""
        return r.returncode, (stdout + stderr).strip()
    except subprocess.TimeoutExpired:
        return -1, f"TIMEOUT after {timeout}s"
    except FileNotFoundError as e:
        return -1, f"NOT FOUND: {e}"
    except Exception as e:
        return -1, f"ERROR: {e}"


def _portal_available() -> bool:
    import urllib.request
    try:
        urllib.request.urlopen(f"{PORTAL_URL}/health", timeout=2)
        return True
    except Exception:
        return False


def _load_assembly() -> dict:
    if ASSEMBLY_FILE.exists():
        return json.loads(ASSEMBLY_FILE.read_text(encoding="utf-8"))
    return {}


def _top_findings(assembly: dict, status: str = "pending", n: int = 5) -> list[dict]:
    findings = [
        f
        for e in assembly.get("experts", {}).values()
        for f in e.get("findings", [])
        if f.get("status") == status
    ]
    return sorted(
        findings,
        key=lambda f: f.get("impact", 0) / max(f.get("effort", 1), 1),
        reverse=True,
    )[:n]


# ─────────────────────────────────────────────────────────────────────────────
# Agente 1 — Auditor
# ─────────────────────────────────────────────────────────────────────────────

def run_auditor(assembly: dict) -> dict:
    """Revisa docs, findings huérfanos y devuelve top candidatos."""
    t0 = time.time()
    results: dict[str, Any] = {"agent": "auditor", "status": "ok", "issues": [], "top_findings": []}

    # 1. Verificar archivos críticos existen
    critical_files = [
        "CLAUDE.md",
        "scripts/assembly_data.json",
        "memory/IMPROVEMENT_LOG.md",
        "memory/METRICS_HISTORY.md",
    ]
    missing = [f for f in critical_files if not (ROOT / f).exists()]
    if missing:
        results["issues"].append({"severity": "high", "msg": f"Archivos críticos faltantes: {missing}"})

    # 2. Sincronía assembly_data.json con commits reales
    code, git_log = _run(["git", "log", "--oneline", "-20"])
    assembly_iter = assembly.get("iterations", 0)
    commit_count = len(git_log.splitlines()) if code == 0 else 0
    if assembly_iter < commit_count - 5:
        results["issues"].append({
            "severity": "medium",
            "msg": f"assembly_data.json iterations={assembly_iter} puede estar desactualizado (últimos {commit_count} commits visibles)",
        })

    # 3. Findings in-progress sin PR link
    orphan = [
        f["id"]
        for e in assembly.get("experts", {}).values()
        for f in e.get("findings", [])
        if f.get("status") == "in-progress" and not f.get("pr_url")
    ]
    if orphan:
        results["issues"].append({
            "severity": "low",
            "msg": f"Findings in-progress sin PR link ({len(orphan)}): {orphan[:5]}{'…' if len(orphan) > 5 else ''}",
        })

    # 4. Tests QA framework
    py_check, _ = _run(
        [sys.executable, "-m", "py_compile", "tests/benchmark_transcription.py", "tests/text_normalizer.py"],
        timeout=15,
    )
    if py_check != 0:
        results["issues"].append({"severity": "high", "msg": "benchmark_transcription.py tiene errores de sintaxis"})

    # 5. Top findings pendientes
    results["top_findings"] = _top_findings(assembly, status="pending", n=5)

    results["duration_s"] = round(time.time() - t0, 2)
    results["status"] = "warn" if results["issues"] else "ok"
    return results


# ─────────────────────────────────────────────────────────────────────────────
# Agente 2 — Validator
# ─────────────────────────────────────────────────────────────────────────────

def run_validator() -> dict:
    """Ejecuta quality gates de Rust y Python."""
    t0 = time.time()
    gates: list[dict] = []

    # Gate 1: cargo check --lib
    code, out = _run(
        ["cargo", "check", "--lib"],
        cwd=ROOT / "frontend" / "src-tauri",
        timeout=180,
    )
    gates.append({"gate": "cargo_check", "pass": code == 0, "detail": out[-300:] if out else ""})

    # Gate 2: DSP unit tests
    code, out = _run(
        ["cargo", "test", "--lib", "audio::dsp"],
        cwd=ROOT / "frontend" / "src-tauri",
        timeout=180,
    )
    gates.append({"gate": "dsp_tests", "pass": code == 0, "detail": out[-200:] if out else ""})

    # Gate 3: text_cleanup tests
    code, out = _run(
        ["cargo", "test", "--lib", "parakeet_engine::text_cleanup"],
        cwd=ROOT / "frontend" / "src-tauri",
        timeout=180,
    )
    gates.append({"gate": "text_cleanup_tests", "pass": code == 0, "detail": out[-200:] if out else ""})

    # Gate 4: pytest backend
    code, out = _run(
        [sys.executable, "-m", "pytest", "tests/", "-q", "--tb=short"],
        cwd=ROOT / "backend",
        timeout=120,
    )
    gates.append({"gate": "pytest_backend", "pass": code == 0, "detail": out[-400:] if out else ""})

    # Gate 5: Python syntax benchmark harness
    code, out = _run(
        [sys.executable, "-m", "py_compile",
         "tests/benchmark_transcription.py",
         "tests/text_normalizer.py"],
        timeout=15,
    )
    gates.append({"gate": "python_syntax", "pass": code == 0, "detail": out if out else "ok"})

    all_pass = all(g["pass"] for g in gates)
    return {
        "agent": "validator",
        "status": "pass" if all_pass else "fail",
        "gates": gates,
        "duration_s": round(time.time() - t0, 2),
    }


# ─────────────────────────────────────────────────────────────────────────────
# Agente 3 — Janitor
# ─────────────────────────────────────────────────────────────────────────────

def run_janitor() -> dict:
    """Reporta sobre ramas viejas, archivos grandes, logs acumulados."""
    t0 = time.time()
    report: dict[str, Any] = {"agent": "janitor", "status": "ok", "items": []}

    # 1. Ramas locales no pusheadas o muy viejas (>30 días)
    code, branches = _run(
        ["git", "branch", "--format=%(refname:short) %(committerdate:relative)"],
    )
    stale = [b for b in branches.splitlines() if "months" in b or "year" in b]
    if stale:
        report["items"].append({"type": "stale_branches", "count": len(stale), "sample": stale[:3]})

    # 2. Tamaño de target/debug (build artifacts)
    target_debug = ROOT / "target" / "debug"
    if target_debug.exists():
        # Estimar tamaño rápido (solo archivos .exe/.dll/.pdb en raíz)
        big_files = [
            f for f in target_debug.iterdir()
            if f.is_file() and f.suffix in (".exe", ".pdb", ".dll") and f.stat().st_size > 50_000_000
        ]
        report["items"].append({
            "type": "build_artifacts",
            "large_files": len(big_files),
            "tip": "Ejecutar 'cargo clean' si necesitas espacio",
        })

    # 3. Logs de portal acumulados
    portal_logs = list((MEMORY_DIR / "build_logs" / "portal_requests").glob("*.jsonl"))
    old_logs = [p for p in portal_logs if (dt.date.today() - dt.date.fromisoformat(p.stem.replace("requests_", ""))).days > 30]
    if old_logs:
        report["items"].append({"type": "old_portal_logs", "files": [p.name for p in old_logs]})

    # 4. .results/ en tests con archivos muy grandes
    results_dir = ROOT / "tests" / "results"
    if results_dir.exists():
        big_results = [f.name for f in results_dir.iterdir() if f.is_file() and f.stat().st_size > 1_000_000]
        if big_results:
            report["items"].append({"type": "large_test_results", "files": big_results})

    report["duration_s"] = round(time.time() - t0, 2)
    return report


# ─────────────────────────────────────────────────────────────────────────────
# Agente 4 — Doc Checker
# ─────────────────────────────────────────────────────────────────────────────

def run_doc_checker(assembly: dict) -> dict:
    """Verifica que la documentación clave está sincronizada con el código."""
    t0 = time.time()
    gaps: list[dict] = []

    # 1. CLAUDE.md menciona los módulos de audio que existen
    claude_md = (ROOT / "CLAUDE.md").read_text(encoding="utf-8")
    key_modules = ["dsp.rs", "text_cleanup.rs", "parakeet_engine.rs", "pipeline.rs"]
    for mod in key_modules:
        if mod not in claude_md:
            gaps.append({"type": "undocumented_module", "file": mod, "doc": "CLAUDE.md"})

    # 2. METRICS_HISTORY.md tiene entrada reciente (últimos 3 días)
    metrics_path = MEMORY_DIR / "METRICS_HISTORY.md"
    if metrics_path.exists():
        content = metrics_path.read_text(encoding="utf-8")
        today = dt.date.today()
        recent = any(
            str(today - dt.timedelta(days=i)) in content
            for i in range(3)
        )
        if not recent:
            gaps.append({"type": "stale_metrics", "msg": "METRICS_HISTORY.md sin entrada en los últimos 3 días"})

    # 3. improvement_candidates.json existe y está fresco
    candidates_path = MEMORY_DIR / "improvement_candidates.json"
    if candidates_path.exists():
        mtime = dt.date.fromtimestamp(candidates_path.stat().st_mtime)
        age = (dt.date.today() - mtime).days
        if age > 1:
            gaps.append({"type": "stale_candidates", "msg": f"improvement_candidates.json tiene {age} días de antigüedad"})
    else:
        gaps.append({"type": "missing_candidates", "msg": "improvement_candidates.json no existe — ejecutar auditor"})

    # 4. PRs open: verificar que assembly tiene su finding correspondiente
    assembly_ids = {
        f["id"]
        for e in assembly.get("experts", {}).values()
        for f in e.get("findings", [])
    }
    # Ids esperados de los últimos PRs conocidos
    recent_pr_ids = ["PORTAL-002", "UX-010", "UX-011", "UX-012", "UX-013"]
    missing_in_assembly = [i for i in recent_pr_ids if i not in assembly_ids]
    if missing_in_assembly:
        gaps.append({"type": "unregistered_prs", "ids": missing_in_assembly})

    return {
        "agent": "doc_checker",
        "status": "warn" if gaps else "ok",
        "gaps": gaps,
        "duration_s": round(time.time() - t0, 2),
    }


# ─────────────────────────────────────────────────────────────────────────────
# Orquestador principal
# ─────────────────────────────────────────────────────────────────────────────

def orchestrate(quick: bool = False) -> dict:
    """Corre todos los agentes y devuelve el reporte unificado."""
    ts = dt.datetime.now()
    # Windows: forzar UTF-8 en stdout para emojis
    import io
    sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace")
    sys.stderr = io.TextIOWrapper(sys.stderr.buffer, encoding="utf-8", errors="replace")

    print(f"\n[EVAL] Maity Desktop — Evaluacion Multi-Agente ({ts.strftime('%Y-%m-%d %H:%M')})")
    print("=" * 60)

    assembly = _load_assembly()

    # Ejecutar agentes (secuencial por simplicidad — evita problemas con cargo locks)
    print("  [1/4] Auditor ...", end=" ", flush=True)
    auditor_result = run_auditor(assembly)
    print(f"{'OK' if auditor_result['status'] == 'ok' else 'WARN'} ({auditor_result['duration_s']}s)")

    print("  [2/4] Validator ...", end=" ", flush=True)
    validator_result = run_validator()
    print(f"{'PASS' if validator_result['status'] == 'pass' else 'FAIL'} ({validator_result['duration_s']}s)")

    if not quick:
        print("  [3/4] Janitor ...", end=" ", flush=True)
        janitor_result = run_janitor()
        print(f"OK ({janitor_result['duration_s']}s)")

        print("  [4/4] Doc Checker ...", end=" ", flush=True)
        doc_result = run_doc_checker(assembly)
        print(f"{'OK' if doc_result['status'] == 'ok' else 'WARN'} ({doc_result['duration_s']}s)")
    else:
        janitor_result = {"agent": "janitor", "status": "skipped"}
        doc_result = {"agent": "doc_checker", "status": "skipped"}

    # Veredicto global
    validator_pass = validator_result["status"] == "pass"
    auditor_warn = auditor_result["status"] != "ok"
    doc_warn = doc_result.get("status") not in ("ok", "skipped")
    overall = "PASS" if validator_pass and not auditor_warn else ("WARN" if validator_pass else "FAIL")

    report = {
        "timestamp": ts.isoformat(),
        "overall": overall,
        "assembly_iter": assembly.get("iterations", "?"),
        "assembly_commits": assembly.get("commits", "?"),
        "assembly_findings": sum(len(e.get("findings", [])) for e in assembly.get("experts", {}).values()),
        "agents": {
            "auditor": auditor_result,
            "validator": validator_result,
            "janitor": janitor_result,
            "doc_checker": doc_result,
        },
        "top_pending": _top_findings(assembly, "pending", 3),
    }

    return report


# ─────────────────────────────────────────────────────────────────────────────
# Formateo del reporte Markdown
# ─────────────────────────────────────────────────────────────────────────────

def _format_markdown(r: dict) -> str:
    ts = r["timestamp"][:16].replace("T", " ")
    overall_emoji = {"PASS": "✅", "WARN": "⚠️", "FAIL": "❌", "ERROR": "🔴"}.get(r["overall"], "❓")
    lines = [
        f"# Evaluación Multi-Agente — {ts}",
        f"\n**Veredicto global: {overall_emoji} {r['overall']}**  ",
        f"Assembly: iter {r['assembly_iter']} · commits {r['assembly_commits']} · {r['assembly_findings']} findings\n",
        "---\n",
        "## 🔍 Auditor",
    ]
    aud = r["agents"]["auditor"]
    if aud.get("issues"):
        for issue in aud["issues"]:
            sev_icon = {"high": "🔴", "medium": "🟡", "low": "🔵"}.get(issue["severity"], "•")
            lines.append(f"- {sev_icon} **{issue['severity'].upper()}**: {issue['msg']}")
    else:
        lines.append("- Sin issues detectados ✅")

    lines.append("\n**Top 3 findings pendientes:**")
    for f in aud.get("top_findings", [])[:3]:
        ratio = round(f.get("impact", 0) / max(f.get("effort", 1), 1), 1)
        lines.append(f"- `{f['id']}` (impact={f.get('impact')}/effort={f.get('effort')}, ratio={ratio}) — {f.get('title', '')[:60]}")

    lines.append("\n## ✔️ Validator")
    val = r["agents"]["validator"]
    for g in val.get("gates", []):
        icon = "✅" if g["pass"] else "❌"
        lines.append(f"- {icon} `{g['gate']}`")

    if val.get("status") != "pass":
        lines.append("\n**Bloqueadores:**")
        for g in val.get("gates", []):
            if not g["pass"]:
                lines.append(f"  - `{g['gate']}`: {g.get('detail', '')[-200:]}")

    jan = r["agents"].get("janitor", {})
    if jan.get("status") != "skipped":
        lines.append("\n## 🧹 Janitor")
        for item in jan.get("items", []):
            lines.append(f"- `{item['type']}`: {json.dumps(item, ensure_ascii=False)[:120]}")
        if not jan.get("items"):
            lines.append("- Proyecto limpio ✅")

    doc = r["agents"].get("doc_checker", {})
    if doc.get("status") != "skipped":
        lines.append("\n## 📄 Doc Checker")
        for gap in doc.get("gaps", []):
            lines.append(f"- ⚠️ `{gap['type']}`: {gap.get('msg', '') or str(gap)[:100]}")
        if not doc.get("gaps"):
            lines.append("- Docs sincronizados ✅")

    lines.append("\n---")
    lines.append(f"\n*Generado por `scripts/multi_agent_eval.py` · {ts}*")
    return "\n".join(lines)


# ─────────────────────────────────────────────────────────────────────────────
# Persistencia
# ─────────────────────────────────────────────────────────────────────────────

def _save_report(r: dict) -> tuple[Path, Path]:
    ts = dt.datetime.fromisoformat(r["timestamp"])
    slug = ts.strftime("%Y-%m-%d_%H-%M")

    md_path = EVAL_DIR / f"{slug}.md"
    json_path = EVAL_DIR / "latest.json"

    md_path.write_text(_format_markdown(r), encoding="utf-8")
    json_path.write_text(json.dumps(r, indent=2, ensure_ascii=False, default=str), encoding="utf-8")

    return md_path, json_path


# ─────────────────────────────────────────────────────────────────────────────
# CLI
# ─────────────────────────────────────────────────────────────────────────────

def main() -> int:
    parser = argparse.ArgumentParser(description="Evaluación multi-agente Maity Desktop")
    parser.add_argument("--quick", action="store_true", help="Solo auditor + validator")
    parser.add_argument("--json", action="store_true", help="Output JSON a stdout")
    args = parser.parse_args()

    try:
        report = orchestrate(quick=args.quick)
    except Exception as e:
        print(f"\n❌ ERROR en orquestador: {e}", file=sys.stderr)
        return 2

    md_path, json_path = _save_report(report)

    if args.json:
        print(json.dumps(report, indent=2, ensure_ascii=False, default=str))
    else:
        print(f"\n{_format_markdown(report)}")
        print(f"\nReporte guardado: {md_path.relative_to(ROOT)}")
        print(f"JSON: {json_path.relative_to(ROOT)}")

    exit_code = 0 if report["overall"] in ("PASS", "WARN") else 1
    print(f"\n{'PASS' if exit_code == 0 else 'FAIL'} — exit {exit_code}")
    return exit_code


if __name__ == "__main__":
    sys.exit(main())

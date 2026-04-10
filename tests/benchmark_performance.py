#!/usr/bin/env python3
"""
benchmark_performance.py — Python port of benchmark_performance.ps1
Measures the same user-visible metrics for Maity Desktop on Windows 11.

Usage:
    python tests\\benchmark_performance.py

Exit codes:
    0 = all measured metrics within thresholds
    1 = at least one metric exceeded threshold
    2 = harness error (binary missing, log not found, timeout)

Dependencies: Python 3.10+, stdlib only (uses ctypes + wmic fallback).
No admin rights required.
"""
from __future__ import annotations

import ctypes
import json
import os
import re
import subprocess
import sys
import time
from datetime import datetime
from pathlib import Path

# ------------------------------ Config --------------------------------------
BINARY_PATH     = Path(r"D:\Maity_Desktop\target\debug\maity-desktop.exe")
LOG_DIR         = Path(r"C:\Users\alfon\AppData\Local\Maity\logs")
RESULTS_DIR     = Path(r"D:\Maity_Desktop\tests\results")
METRICS_HISTORY = Path(r"D:\Maity_Desktop\memory\METRICS_HISTORY.md")
MAX_WAIT_SEC    = 60

THRESHOLDS = {
    "ColdStartMs":      3000,
    "WebViewJsReadyMs": 5000,
    "WhisperPreloadMs": 10000,
    "ClickToRecordMs":  500,
    "TranscriptionRtf": 1.5,   # greater-than
    "RamPeakMb":        2048,
    "StopToSummaryMs":  5000,
}
GREATER_THAN = {"TranscriptionRtf"}  # everything else is less-than

# ------------------------------ Helpers -------------------------------------
def log(msg: str, level: str = "INFO") -> None:
    print(f"[bench] {level} {msg}", flush=True)

def today_log_path() -> Path:
    return LOG_DIR / f"maity.{datetime.now().strftime('%Y-%m-%d')}.log"

def _ps(cmd: str) -> str:
    """Run a short PowerShell one-liner and return stdout."""
    r = subprocess.run(
        ["powershell", "-NoProfile", "-Command", cmd],
        capture_output=True, text=True, timeout=10,
    )
    return r.stdout.strip()

def get_procs(name: str) -> list[dict]:
    """Return list of {pid, ws_bytes, responding} for processes matching name."""
    cmd = (
        f"Get-Process -Name '{name}' -ErrorAction SilentlyContinue | "
        f"ForEach-Object {{ [pscustomobject]@{{Id=$_.Id; WS=$_.WorkingSet64; "
        f"Resp=$_.Responding; HWnd=[int64]$_.MainWindowHandle}} }} | "
        f"ConvertTo-Json -Compress"
    )
    out = _ps(cmd)
    if not out:
        return []
    data = json.loads(out)
    if isinstance(data, dict):
        data = [data]
    return data

def kill_maity() -> None:
    _ps("Get-Process maity-desktop -ErrorAction SilentlyContinue | Stop-Process -Force")
    _ps("Get-Process msedgewebview2 -ErrorAction SilentlyContinue | "
        "Where-Object { $_.Parent.ProcessName -eq 'maity-desktop' } | Stop-Process -Force")
    time.sleep(0.5)

def webview2_ram_mb() -> float:
    procs = get_procs("msedgewebview2")
    return round(sum(p["WS"] for p in procs) / (1024 * 1024), 1)

def combined_ram_mb() -> float:
    return round(
        (sum(p["WS"] for p in get_procs("maity-desktop")) +
         sum(p["WS"] for p in get_procs("msedgewebview2"))) / (1024 * 1024),
        1,
    )

# ------------------------------ Main ----------------------------------------
def main() -> int:
    if not BINARY_PATH.exists():
        log(f"Binary not found: {BINARY_PATH}", "ERR")
        return 2
    if not LOG_DIR.exists():
        log(f"Log directory not found: {LOG_DIR}", "ERR")
        return 2
    RESULTS_DIR.mkdir(parents=True, exist_ok=True)

    log("Killing existing processes")
    kill_maity()

    log_path = today_log_path()
    baseline_offset = log_path.stat().st_size if log_path.exists() else 0
    log(f"Baseline log offset: {baseline_offset} bytes ({log_path})")

    results = {
        "timestamp": datetime.now().isoformat(),
        "binary": str(BINARY_PATH),
        "thresholds": THRESHOLDS,
        "metrics": {},
        "errors": [],
        "pass": True,
    }

    log("Launching maity-desktop.exe")
    t0 = time.monotonic()
    proc = subprocess.Popen([str(BINARY_PATH)])
    pid = proc.pid

    def elapsed_ms() -> float:
        return (time.monotonic() - t0) * 1000.0

    # Metric 1 — cold start
    t1 = None
    while (time.monotonic() - t0) < MAX_WAIT_SEC:
        procs = get_procs("maity-desktop")
        match = [p for p in procs if p["Id"] == pid]
        if match and match[0]["Resp"] and match[0]["HWnd"] != 0:
            t1 = elapsed_ms()
            break
        time.sleep(0.1)
    if t1 is None:
        results["errors"].append("ColdStart: never became Responding")
        results["metrics"]["ColdStartMs"] = -1
        results["pass"] = False
    else:
        results["metrics"]["ColdStartMs"] = round(t1)
        log(f"ColdStartMs = {round(t1)}")

    # Metric 2 — WebView2 JS ready
    t2 = None
    while (time.monotonic() - t0) < MAX_WAIT_SEC:
        if webview2_ram_mb() >= 200:
            t2 = elapsed_ms()
            break
        time.sleep(0.15)
    if t2 is None:
        results["errors"].append("WebViewJsReady: never hit 200 MB")
        results["metrics"]["WebViewJsReadyMs"] = -1
        results["pass"] = False
    else:
        results["metrics"]["WebViewJsReadyMs"] = round(t2)
        log(f"WebViewJsReadyMs = {round(t2)}")

    # Metric 3 — Whisper preload (log scrape)
    t3 = None
    preload_line = None
    pattern = re.compile(r"PERF-005:\s*Whisper model\s*'([^']+)'\s*preloaded")
    line_pat = re.compile(r"^.*PERF-005:\s*Whisper model.*preloaded.*$", re.M)
    scrape_deadline = time.monotonic() + MAX_WAIT_SEC
    while time.monotonic() < scrape_deadline:
        if log_path.exists():
            try:
                with open(log_path, "rb") as f:
                    f.seek(baseline_offset)
                    tail = f.read().decode("utf-8", errors="replace")
                if pattern.search(tail):
                    m = line_pat.search(tail)
                    preload_line = m.group(0).strip() if m else None
                    t3 = elapsed_ms()
                    break
            except OSError:
                pass
        time.sleep(0.2)
    if t3 is None:
        results["errors"].append("WhisperPreload: PERF-005 line not found")
        results["metrics"]["WhisperPreloadMs"] = -1
        results["metrics"]["WhisperPreloadLine"] = None
        results["pass"] = False
    else:
        results["metrics"]["WhisperPreloadMs"] = round(t3)
        results["metrics"]["WhisperPreloadLine"] = preload_line
        log(f"WhisperPreloadMs = {round(t3)}")

    # Metric 6 — RAM peak for 5s
    log("Sampling RAM peak for 5s")
    ram_peak = 0.0
    sample_end = time.monotonic() + 5
    while time.monotonic() < sample_end:
        r = combined_ram_mb()
        if r > ram_peak:
            ram_peak = r
        time.sleep(0.25)
    results["metrics"]["RamPeakMb"] = ram_peak
    log(f"RamPeakMb = {ram_peak}")

    # Metrics 4,5,7 — TODO (UIAutomation out of scope)
    results["metrics"]["ClickToRecordMs"]  = -1
    results["metrics"]["TranscriptionRtf"] = -1
    results["metrics"]["StopToSummaryMs"]  = -1
    results["errors"].append(
        "ClickToRecord / TranscriptionRtf / StopToSummary are TODO (UIAutomation out of scope)"
    )

    # Teardown
    log("Stopping maity-desktop")
    kill_maity()

    # Threshold check
    for name, limit in THRESHOLDS.items():
        val = results["metrics"].get(name, -1)
        if val is None or val < 0:
            continue
        ok = (val > limit) if name in GREATER_THAN else (val < limit)
        if not ok:
            results["pass"] = False
            results["errors"].append(f"{name}={val} exceeds threshold {limit}")
            log(f"{name}={val} violates threshold", "WARN")

    # JSON output
    stamp = datetime.now().strftime("%Y-%m-%d_%H%M%S")
    out_path = RESULTS_DIR / f"perf_{stamp}.json"
    out_path.write_text(json.dumps(results, indent=2), encoding="utf-8")
    log(f"Wrote {out_path}")

    # Append to history
    status = "PASS" if results["pass"] else "FAIL"
    line = (
        f"| {datetime.now().strftime('%Y-%m-%d %H:%M')} | perf | "
        f"cold={results['metrics']['ColdStartMs']}ms "
        f"wv={results['metrics']['WebViewJsReadyMs']}ms "
        f"whisper={results['metrics']['WhisperPreloadMs']}ms "
        f"ram={results['metrics']['RamPeakMb']}MB | {status} |\n"
    )
    try:
        with open(METRICS_HISTORY, "a", encoding="utf-8") as f:
            f.write(line)
    except OSError as e:
        log(f"Could not append to history: {e}", "WARN")

    return 0 if results["pass"] else 1


if __name__ == "__main__":
    sys.exit(main())

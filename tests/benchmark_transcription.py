#!/usr/bin/env python3
"""
WER/CER golden-transcript benchmark harness for Maity Desktop.

Runs the Rust `transcribe_cli` binary against every audio fixture under
`tests/fixtures/audio/` and compares its hypothesis against the matching
ground truth in `tests/fixtures/ground_truth/`.

Outputs:
  * tests/results/transcription_{YYYY-MM-DD}.json  (machine-readable)
  * memory/METRICS_HISTORY.md                       (append-only row)
  * exit code 1 if aggregate WER exceeds --wer-threshold (default 0.25)

Run:
    python tests/benchmark_transcription.py
    python tests/benchmark_transcription.py --wer-threshold 0.25 --model base

Expected baseline on Spanish meeting audio, Whisper base:
  * WER ~0.18-0.25 (clean narration) / ~0.30-0.40 (noisy meetings)
  * CER ~0.08-0.12
"""
from __future__ import annotations

import argparse
import datetime as dt
import json
import random
import statistics
import subprocess
import sys
import time
import wave
from pathlib import Path
from typing import Any

# Local import — keep the harness self-contained.
sys.path.insert(0, str(Path(__file__).parent))
from text_normalizer import (  # noqa: E402
    NormalizerConfig,
    count_hallucinations,
    normalize_text,
)

try:
    import jiwer  # type: ignore
    _HAS_JIWER = True
except ImportError:
    _HAS_JIWER = False


# ---------------------------------------------------------------------------
# WER/CER core
# ---------------------------------------------------------------------------

def _edit_distance(a: list[str], b: list[str]) -> int:
    """Levenshtein on token sequences (fallback if jiwer missing)."""
    n, m = len(a), len(b)
    if n == 0:
        return m
    if m == 0:
        return n
    prev = list(range(m + 1))
    for i in range(1, n + 1):
        curr = [i] + [0] * m
        for j in range(1, m + 1):
            cost = 0 if a[i - 1] == b[j - 1] else 1
            curr[j] = min(
                curr[j - 1] + 1,       # insertion
                prev[j] + 1,           # deletion
                prev[j - 1] + cost,    # substitution
            )
        prev = curr
    return prev[m]


def compute_wer(reference: str, hypothesis: str) -> float:
    if _HAS_JIWER:
        return float(jiwer.wer(reference, hypothesis))
    ref_tokens = reference.split()
    hyp_tokens = hypothesis.split()
    if not ref_tokens:
        return 0.0 if not hyp_tokens else 1.0
    return _edit_distance(ref_tokens, hyp_tokens) / len(ref_tokens)


def compute_cer(reference: str, hypothesis: str) -> float:
    if _HAS_JIWER:
        return float(jiwer.cer(reference, hypothesis))
    ref_chars = list(reference.replace(" ", ""))
    hyp_chars = list(hypothesis.replace(" ", ""))
    if not ref_chars:
        return 0.0 if not hyp_chars else 1.0
    return _edit_distance(ref_chars, hyp_chars) / len(ref_chars)


def bootstrap_wer_ci(
    pairs: list[tuple[str, str]], n_resamples: int = 1000, seed: int = 17
) -> tuple[float, float]:
    """95% bootstrap CI for corpus WER by resampling sentence pairs."""
    if not pairs:
        return (0.0, 0.0)
    rng = random.Random(seed)
    wers: list[float] = []
    for _ in range(n_resamples):
        sample = [rng.choice(pairs) for _ in pairs]
        ref = " ".join(r for r, _ in sample)
        hyp = " ".join(h for _, h in sample)
        wers.append(compute_wer(ref, hyp))
    wers.sort()
    lo = wers[int(0.025 * n_resamples)]
    hi = wers[int(0.975 * n_resamples)]
    return (lo, hi)


# ---------------------------------------------------------------------------
# Fixture loading
# ---------------------------------------------------------------------------

def load_ground_truth(path: Path) -> str:
    """Parse a ground-truth file.

    Supported shapes:
    - Plain text file (.txt)
    - JSON with top-level "text" field: {"text": "..."}
    - JSON with "segments" array of {"text": "...", ...}: concatenated in order
    - Plain JSON string
    """
    if path.suffix == ".json":
        data = json.loads(path.read_text(encoding="utf-8"))
        if isinstance(data, dict):
            if "text" in data and isinstance(data["text"], str):
                return data["text"]
            # maity_recorder convention: segments[].text
            if "segments" in data and isinstance(data["segments"], list):
                parts = []
                for seg in data["segments"]:
                    if isinstance(seg, dict) and isinstance(seg.get("text"), str):
                        parts.append(seg["text"])
                if parts:
                    return " ".join(parts)
        if isinstance(data, str):
            return data
        raise ValueError(f"Unrecognized GT json shape: {path}")
    return path.read_text(encoding="utf-8")


def find_ground_truth(audio_path: Path, gt_dir: Path) -> Path | None:
    """Locate a ground-truth file matching an audio fixture.

    Supported naming conventions:
    - exact: `A_Poncho_Mensaje.wav` → `A_Poncho_Mensaje.json|txt`
    - suffixed: `A_Poncho_Mensaje_ground_truth.txt`
    - single-letter convention from maity_recorder:
        `A_Poncho_Mensaje.wav` → `audio_a.json`
        `B_Poncho_Liz.wav`     → `audio_b.json`
    """
    stem = audio_path.stem
    stems = [
        stem,
        stem + "_ground_truth",
        stem.lower(),
    ]
    # Single-letter convention: first character of the stem → `audio_<letter>`
    first_char = stem[0].lower() if stem else ""
    if first_char.isalpha():
        stems.append(f"audio_{first_char}")
    for candidate_stem in stems:
        for ext in (".json", ".txt"):
            candidate = gt_dir / (candidate_stem + ext)
            if candidate.exists():
                return candidate
    return None


def audio_duration_seconds(wav_path: Path) -> float:
    try:
        with wave.open(str(wav_path), "rb") as w:
            return w.getnframes() / float(w.getframerate() or 1)
    except Exception:
        return 0.0


# ---------------------------------------------------------------------------
# CLI invocation
# ---------------------------------------------------------------------------

def invoke_transcribe_cli(
    cli_path: Path, wav_path: Path, model: str, language: str, timeout: int
) -> tuple[str, float]:
    """Run the Rust CLI, return (hypothesis_text, wall_time_seconds).

    Contract (see docs/audit/TRANSCRIBE_CLI_SPEC.md):
      * stdout: single JSON object {"text": "...", "segments": [...],
        "duration_s": float, "model": "...", "language": "..."}
      * exit 0 on success
    """
    start = time.perf_counter()
    result = subprocess.run(
        [
            str(cli_path),
            "--audio", str(wav_path),
            "--model", model,
            "--language", language,
            "--json",
        ],
        capture_output=True,
        text=True,
        timeout=timeout,
        encoding="utf-8",
    )
    elapsed = time.perf_counter() - start
    if result.returncode != 0:
        raise RuntimeError(
            f"transcribe_cli failed (exit {result.returncode}) for {wav_path.name}: "
            f"{result.stderr.strip()[:400]}"
        )
    payload = json.loads(result.stdout)
    return str(payload.get("text", "")), elapsed


# ---------------------------------------------------------------------------
# Main harness
# ---------------------------------------------------------------------------

def run_benchmark(args: argparse.Namespace) -> int:
    repo = Path(args.repo).resolve()
    audio_dir = repo / "tests" / "fixtures" / "audio"
    gt_dir = repo / "tests" / "fixtures" / "ground_truth"
    results_dir = repo / "tests" / "results"
    results_dir.mkdir(parents=True, exist_ok=True)

    # FIX: cargo build puts the binary at <repo>/target/debug/, not
    # <repo>/frontend/src-tauri/target/debug/. The latter only applies when
    # you cd into frontend/src-tauri first. Check both locations.
    default_cli_primary = repo / "target" / "debug" / "transcribe_cli.exe"
    default_cli_secondary = repo / "frontend" / "src-tauri" / "target" / "debug" / "transcribe_cli.exe"
    if args.cli:
        cli_path = Path(args.cli)
    elif default_cli_primary.exists():
        cli_path = default_cli_primary
    else:
        cli_path = default_cli_secondary
    if not cli_path.exists() and not args.dry_run:
        print(f"[ERROR] transcribe_cli not found at {cli_path}", file=sys.stderr)
        print("        Build it first: cargo build --bin transcribe_cli", file=sys.stderr)
        return 2

    fixtures = sorted(audio_dir.glob("*.wav"))
    if not fixtures:
        print(f"[ERROR] No audio fixtures in {audio_dir}", file=sys.stderr)
        return 2

    normalizer_cfg = NormalizerConfig(
        strip_accents=not args.keep_accents,
    )

    per_file: list[dict[str, Any]] = []
    norm_pairs: list[tuple[str, str]] = []
    total_audio = 0.0
    total_wall = 0.0
    total_hallucinations = 0

    for wav in fixtures:
        gt_path = find_ground_truth(wav, gt_dir)
        if gt_path is None:
            print(f"[WARN] No ground truth for {wav.name}, skipping")
            continue

        reference_raw = load_ground_truth(gt_path)
        audio_s = audio_duration_seconds(wav)
        total_audio += audio_s

        if args.dry_run:
            hypothesis_raw, wall = "", 0.0
        else:
            try:
                hypothesis_raw, wall = invoke_transcribe_cli(
                    cli_path, wav, args.model, args.language, args.timeout
                )
            except Exception as e:
                print(f"[ERROR] {wav.name}: {e}", file=sys.stderr)
                per_file.append({
                    "file": wav.name,
                    "error": str(e),
                    "wer": 1.0, "cer": 1.0,
                })
                continue

        total_wall += wall

        ref_norm = normalize_text(reference_raw, normalizer_cfg)
        hyp_norm = normalize_text(hypothesis_raw, normalizer_cfg)
        norm_pairs.append((ref_norm, hyp_norm))

        wer = compute_wer(ref_norm, hyp_norm)
        cer = compute_cer(ref_norm, hyp_norm)
        halluc = count_hallucinations(hypothesis_raw)
        total_hallucinations += halluc
        speed_factor = (audio_s / wall) if wall > 0 else 0.0

        per_file.append({
            "file": wav.name,
            "audio_s": round(audio_s, 3),
            "wall_s": round(wall, 3),
            "speed_factor": round(speed_factor, 2),
            "wer": round(wer, 4),
            "cer": round(cer, 4),
            "hallucinations": halluc,
            "ref_tokens": len(ref_norm.split()),
            "hyp_tokens": len(hyp_norm.split()),
            "ref_normalized": ref_norm[:2000],
            "hyp_normalized": hyp_norm[:2000],
        })
        print(
            f"[OK] {wav.name:40s} WER={wer:6.2%} CER={cer:6.2%} "
            f"x{speed_factor:5.2f} halluc={halluc}"
        )

    # Aggregate (concatenate all pairs so long files dominate proportionally)
    agg_ref = " ".join(r for r, _ in norm_pairs)
    agg_hyp = " ".join(h for _, h in norm_pairs)
    agg_wer = compute_wer(agg_ref, agg_hyp) if norm_pairs else 1.0
    agg_cer = compute_cer(agg_ref, agg_hyp) if norm_pairs else 1.0
    ci_lo, ci_hi = bootstrap_wer_ci(norm_pairs) if norm_pairs else (0.0, 0.0)
    aggregate_speed = (total_audio / total_wall) if total_wall > 0 else 0.0

    per_file_wers = [r["wer"] for r in per_file if "error" not in r]
    median_wer = statistics.median(per_file_wers) if per_file_wers else 1.0

    report = {
        "schema_version": 1,
        "timestamp": dt.datetime.utcnow().isoformat(timespec="seconds") + "Z",
        "model": args.model,
        "language": args.language,
        "normalizer": {
            "lowercase": True,
            "strip_accents": not args.keep_accents,
            "strip_punctuation": True,
            "numbers_to_words": True,
            "remove_boilerplate": True,
        },
        "wer_threshold": args.wer_threshold,
        "aggregate": {
            "wer": round(agg_wer, 4),
            "cer": round(agg_cer, 4),
            "wer_ci95_low": round(ci_lo, 4),
            "wer_ci95_high": round(ci_hi, 4),
            "median_wer": round(median_wer, 4),
            "total_audio_s": round(total_audio, 2),
            "total_wall_s": round(total_wall, 2),
            "aggregate_speed_factor": round(aggregate_speed, 2),
            "total_hallucinations": total_hallucinations,
            "fixtures": len(per_file),
        },
        "files": per_file,
    }

    # Write JSON
    today = dt.date.today().isoformat()
    out_path = results_dir / f"transcription_{today}.json"
    out_path.write_text(json.dumps(report, indent=2, ensure_ascii=False), encoding="utf-8")
    print(f"\n[REPORT] {out_path}")
    print(
        f"[AGGREGATE] WER={agg_wer:.2%} "
        f"(95% CI {ci_lo:.2%}-{ci_hi:.2%})  "
        f"CER={agg_cer:.2%}  "
        f"speed=x{aggregate_speed:.2f}  "
        f"halluc={total_hallucinations}"
    )

    # Append to METRICS_HISTORY.md
    metrics_md = repo / "memory" / "METRICS_HISTORY.md"
    metrics_md.parent.mkdir(parents=True, exist_ok=True)
    row = (
        f"| {today} | {args.model} | {len(per_file)} | "
        f"{agg_wer:.2%} | {agg_cer:.2%} | x{aggregate_speed:.2f} | "
        f"{total_hallucinations} | {ci_lo:.2%}-{ci_hi:.2%} |\n"
    )
    header_needed = not metrics_md.exists() or "transcription_benchmark" not in metrics_md.read_text(
        encoding="utf-8", errors="ignore"
    )
    with metrics_md.open("a", encoding="utf-8") as f:
        if header_needed:
            f.write("\n## transcription_benchmark\n\n")
            f.write("| date | model | fixtures | WER | CER | speed | halluc | WER 95% CI |\n")
            f.write("|------|-------|----------|-----|-----|-------|--------|------------|\n")
        f.write(row)

    # Exit code policy
    if agg_wer > args.wer_threshold:
        print(
            f"[FAIL] Aggregate WER {agg_wer:.2%} > threshold {args.wer_threshold:.2%}",
            file=sys.stderr,
        )
        return 1
    print(f"[PASS] Aggregate WER {agg_wer:.2%} <= threshold {args.wer_threshold:.2%}")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", default=str(Path(__file__).resolve().parents[1]))
    parser.add_argument("--cli", default=None, help="Path to transcribe_cli binary")
    parser.add_argument("--model", default="base")
    parser.add_argument("--language", default="es")
    parser.add_argument("--wer-threshold", type=float, default=0.25)
    parser.add_argument("--timeout", type=int, default=300)
    parser.add_argument("--keep-accents", action="store_true",
                        help="Do NOT strip accents before scoring")
    parser.add_argument("--dry-run", action="store_true",
                        help="Skip CLI invocation; use empty hypothesis")
    args = parser.parse_args()
    return run_benchmark(args)


if __name__ == "__main__":
    raise SystemExit(main())

# Transcription Benchmark Harness

WER/CER golden-transcript regression harness for Maity Desktop STT.

## What it does

For every `.wav` under `tests/fixtures/audio/` it:

1. Invokes `transcribe_cli` (Rust binary — see `docs/audit/TRANSCRIBE_CLI_SPEC.md`)
2. Loads the matching ground truth from `tests/fixtures/ground_truth/<stem>.{json,txt}`
3. Normalizes both strings via `tests/text_normalizer.py`
   (lowercase, accent strip, punctuation strip, `32 -> treinta y dos`,
   whisper boilerplate removal)
4. Computes **WER** and **CER** per file (via `jiwer` if installed,
   else a pure-Python Levenshtein fallback)
5. Aggregates corpus WER with **95 % bootstrap CI** (1000 resamples)
6. Counts hallucinations (blacklisted whisper boilerplate phrases)
7. Writes `tests/results/transcription_YYYY-MM-DD.json` (machine readable)
8. Appends one row to `memory/METRICS_HISTORY.md` under the
   `## transcription_benchmark` section
9. Exits **1** if aggregate WER > threshold (default **0.25**)

## Run it

```bash
# One-liner
python tests/benchmark_transcription.py

# With custom threshold and model
python tests/benchmark_transcription.py --model base --wer-threshold 0.25

# Keep accents (stricter scoring, Spanish purists)
python tests/benchmark_transcription.py --keep-accents

# Smoke test without the Rust CLI (all hypotheses empty)
python tests/benchmark_transcription.py --dry-run
```

## Dependencies

Added to `backend/requirements.txt`:

```text
jiwer>=3.0.4      # WER/CER (optional — Python fallback exists)
num2words>=0.5.13 # Spanish number expansion (optional)
```

The harness runs without them; they just improve accuracy of scoring.

## Expected baseline (Whisper base, Spanish)

Based on the HuggingFace Open ASR Leaderboard methodology and published
Whisper-base Spanish WER figures (CommonVoice ES, FLEURS es_419):

| Corpus type            | WER          | CER         |
|------------------------|--------------|-------------|
| Clean read speech      | 0.12 – 0.18  | 0.05 – 0.08 |
| Podcast / narration    | 0.18 – 0.25  | 0.08 – 0.12 |
| Noisy meeting audio    | 0.28 – 0.40  | 0.12 – 0.20 |

Our initial production threshold is **WER <= 0.25** on the curated
Maity fixture set (short meeting-style clips, clean microphone). Tighten
once the corpus grows past ~30 minutes of audio.

## File layout

```
tests/
├── benchmark_transcription.py   # Main harness (this file drives CI)
├── text_normalizer.py           # Shared normalizer (pure, deterministic)
├── README_BENCHMARK.md          # You are here
├── fixtures/
│   ├── audio/*.wav              # Staged by sibling agent
│   └── ground_truth/*.{json,txt}
└── results/
    └── transcription_<date>.json
```

Ground-truth format (either):

```json
{ "text": "Hola, ¿cómo estás? Son las tres de la tarde." }
```

or plain `.txt` with the same content.

## CI integration

The script exits with code `1` on regression, so plug it into any
pipeline:

```yaml
- name: Transcription benchmark
  run: python tests/benchmark_transcription.py --wer-threshold 0.25
```

## Known limitations

- Number expansion falls back to a small built-in map when `num2words`
  is missing; numbers > 100 may be left as digits and counted as their
  own tokens.
- Speaker-diarized WER is not computed — we score raw text only.
- Requires the Rust `transcribe_cli` binary (see spec doc).

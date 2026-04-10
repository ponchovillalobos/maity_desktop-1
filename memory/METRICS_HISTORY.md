# Metrics History — Maity Desktop

Histórico de métricas por iteración. Línea por commit/ciclo.

## Baseline

| Métrica | Valor inicial | Fecha |
|---|---|---|
| Versión | (leer Cargo.toml) | 2026-04-07 |
| Tests Rust pasando | TBD | 2026-04-07 |
| Tests TS pasando | TBD | 2026-04-07 |
| Clippy warnings | TBD | 2026-04-07 |
| TS errors | TBD | 2026-04-07 |
| Build size (release) | TBD | 2026-04-07 |
| Findings totales | 0 | 2026-04-07 |
| Findings done | 0 | 2026-04-07 |

## Histórico

| Fecha | Iter | EXP-ID | Tests Rust | Tests TS | Clippy W | Done/Total | Notas |
|---|---|---|---|---|---|---|---|
| 2026-04-07 | 0 | (baseline) | 84 | 0 | TBD | 0/83 | cargo check rc=0 (5m28s), npm install rc=0 (3m, 697 pkg), 126 unwrap, 442 console.log, 47 any |
| 2026-04-07 | 1 | LEG-006 | 84 | 0 | TBD | 0/83 (1 in-progress) | PR #2 abierto, esperando merge |
| 2026-04-07 | 1.5 | (test run) | FAIL E0063 | 0 | TBD (log 862KB) | – | cargo test rc=101 6m34s, fmt rc=1, lint rc=1 (eslint no configurado), tsc rc=0. Descubre QA-008. |
| 2026-04-07 | 2 | QA-008 | 84+ (post-fix) | 0 | TBD | 0/84 (2 in-progress) | PR #3 fix de test broken AudioChunk init. cargo check --tests --lib rc=0 en 1m23s tras fix. |
| 2026-04-07 | 3 | PY-003 | 74 ok / 3 fail | 0 | TBD | 0/87 (3 in-progress) | PR #4 uvicorn env config. cargo test --workspace ahora corre real (1m08s): 74 pass, 3 fail, 1 ignored. Descubre RUST-008/009/LLM-008. |
| 2026-04-07 | 3.5 | (test run discovery) | 74/77 | 0 | TBD | 0/87 | RUST-009 CRITICAL: checkpoint logic compromete zero data loss. Top priority. |
| 2026-04-07 | 4 | RUST-010 | 74/77 | 0 | TBD | 0/88 (4 in-progress) | PR #5 cargo fmt --all (123 archivos). cargo check rc=0 34s, cargo fmt --check rc=0 (era rc=1). Primer quality gate desbloqueado. |
| 2026-04-07 | 5 | RUST-009 | 75/77 ok | 0 | TBD | 0/88 (5 in-progress) | PR #6 fix checkpoint test stereo 48k. NO era bug de zero data loss (reclasificado high). cargo test CPU-only rc=0. |
| 2026-04-07 | 6 | LLM-008 | 76/77 ok | 0 | TBD | 0/88 (6 in-progress) | PR #7 i18n template 'Standup Diario'. Formaliza política es-419-first. cargo test CPU-only rc=0. |
| 2026-04-08 | 7 | PORTAL-001 | 76/77 ok | 0 | TBD | 0/88 (6 in-progress) | Meta-fix: portal v2.3 5 bugs + JSON sync iter 5→7 commits 8→12 cycle 2→3. Dashboard finalmente refleja reality. |
| 2026-04-08 | 8 | RUST-008 | **77/77 ok** 🎉 | 0 | TBD | 0/88 (7 in-progress) | PR #8 fix mul_f32 → * 2. **PRIMER 100% test suite verde de la historia del fork.** |
| 2026-04-08 | 9 | LEG-001 | 77/77 ok | 0 | TBD | 0/88 (8 in-progress) | PR #9 PRIVACY_POLICY v2.0 honesto sobre Deepgram+OpenAI. CRITICAL B2B legal cerrado. |
| 2026-04-08 | 10 | SEC-001 | 77/77 ok | 0 | TBD | 0/88 (9 in-progress) | PR #10 CORS lista blanca + MAITY_CORS_ORIGINS env override. CRITICAL XSS-from-other-sites cerrado. |
| 2026-04-08 | 11 | SEC-002 | 77/77 ok | 0 | TBD | 0/88 (10 in-progress) | PR #11 Tauri allowlist sin fs:read-all/write-all. CRITICAL principle-of-least-privilege SOC2. |

## transcription_benchmark

| date | model | fixtures | WER | CER | speed | halluc | WER 95% CI |
|------|-------|----------|-----|-----|-------|--------|------------|
| 2026-04-08 | base | 1 | 100.00% | 100.00% | x0.00 | 0 | 0.00%-0.00% |
| 2026-04-08 | base | 4 | 22.62% | 14.95% | x14.13 | 0 | 11.15%-29.61% |

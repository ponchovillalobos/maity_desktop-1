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

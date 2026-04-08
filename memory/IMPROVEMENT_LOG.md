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

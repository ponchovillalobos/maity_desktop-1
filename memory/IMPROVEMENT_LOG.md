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

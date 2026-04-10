# /improve — Ciclo de Auto-Mejora Continua (Maity Desktop)

Ejecuta un ciclo completo: ANALIZAR → MEJORAR → VERIFICAR → APRENDER.
Argumento opcional: área de foco (ej: `tests`, `security`, `performance`, o un EXP-ID concreto como `SEC-001`).

---

## Paso 0: Leer memoria + asamblea

Lee estos archivos para contexto acumulado:

1. `memory/IMPROVEMENT_LOG.md` — qué ya se hizo
2. `memory/FAILED_ATTEMPTS.md` — qué NO repetir
3. `memory/ANALYSIS_STATE.md` — cola de prioridades
4. `memory/METRICS_HISTORY.md` — métricas actuales
5. `scripts/assembly_data.json` — hallazgos de la asamblea de expertos

---

## Paso 1: ANALIZAR

```bash
cargo clippy --all-targets --all-features -- -D warnings 2>&1 | tail -20
cargo test --all 2>&1 | tail -10
cd frontend && npm run typecheck 2>&1 | tail -10 && cd ..
```

### Elegir mejora:
- Si el usuario dio un foco, filtrar findings por experto/categoría/ID
- Si no, tomar el finding `pending` con mayor ratio `impact/effort` de `assembly_data.json`
- Si no hay findings, escanear código buscando: `unwrap()`, `TODO`, tipos `any`, warnings de clippy

**Elegir UN finding específico y pequeño.**

---

## Paso 2: SNAPSHOT

```bash
cargo test --all 2>&1 | tail -3
```

Registrar count exacto de tests. Punto de rollback.

---

## Paso 3: MEJORAR

Reglas estrictas:
- **UN cambio a la vez** (single concern)
- Si agrega comportamiento → **TEST PRIMERO**
- Si corrige bug → test que **FALLA primero**, luego fix
- Seguir patrones existentes del proyecto (Rust idiomático, TS tipado)
- **Máximo 100 líneas de diff**
- Consultar `FAILED_ATTEMPTS.md` para no repetir errores

---

## Paso 4: VERIFICAR

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
cd frontend && npm run lint && npm run typecheck && npm test && cd ..
```

### Si CUALQUIER paso falla:
1. Revertir: `git restore .`
2. Registrar fallo en `FAILED_ATTEMPTS.md`:
   ```
   ### YYYY-MM-DD — <qué se intentó> (EXP-ID)
   - **Error:** <error exacto>
   - **Causa:** <por qué falló>
   - **Lección:** <qué hacer diferente>
   ```
3. Intentar siguiente candidato (máx 2 reintentos)
4. Si todos fallan, reportar y terminar

---

## Paso 5: COMMIT

```bash
git add <archivos específicos>
git commit -m "improve(<expert>): <descripción en español>

Resolves <EXP-ID> from assembly.
Impact: X/10  Effort: Y/10

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Paso 6: APRENDER

Actualizar TODOS los archivos de memoria + asamblea:

1. **`scripts/assembly_data.json`** — marcar finding como `"status": "done"`
2. **`memory/IMPROVEMENT_LOG.md`** — append: fecha, EXP-ID, archivos, métricas antes/después
3. **`memory/ANALYSIS_STATE.md`** — marcar resuelto, agregar items descubiertos
4. **`memory/METRICS_HISTORY.md`** — nuevas métricas (tests, clippy, build size)
5. **`memory/FAILED_ATTEMPTS.md`** — solo si hubo fallo

---

## Paso 7: REPORTAR

```
═══ CICLO DE MEJORA COMPLETADO ═══
Finding: <EXP-ID> — <título>
Archivos: <lista>
Tests: <antes> → <después>
Clippy warnings: <antes> → <después>
Pendientes en asamblea: <N>
═══════════════════════════════════
```

---

## Reglas de seguridad

- **NUNCA** bajar el número de tests
- **NUNCA** aumentar warnings de clippy
- **NUNCA** romper el build de Tauri
- Cada ciclo es atómico: o se completa todo o se revierte todo

---
name: improve
description: Ciclo de auto-mejora — analiza → mejora → verifica → documenta (Rust/Tauri/TS)
allowed-tools: "*"
---

# /improve — Ciclo de Auto-Mejora Continua (Maity Desktop)

Ejecuta UN ciclo completo de mejora del proyecto Maity Desktop (Tauri + Rust + TypeScript).

## Flujo

### Paso 1: Auditoría
Lanza un agente Explore (o el agente `auditor`) para analizar el código y encontrar los 3 problemas más impactantes:
- Lee `memory/METRICS_HISTORY.md` para baseline
- Lee `memory/FAILED_ATTEMPTS.md` para no repetir errores
- Lee `scripts/assembly_data.json` y filtra `status=pending` ordenados por `impact/effort`
- Genera 3 candidatos top

### Paso 2: Implementación
Para el candidato #1:
- Aplica el fix mínimo (≤100 LOC)
- Solo modifica los archivos necesarios (single concern)
- Mantén compatibilidad con todo lo existente
- Si añades comportamiento → escribe el TEST PRIMERO

### Paso 3: Validación (quality gates)
Todos deben pasar:

```bash
# Rust backend
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all

# Frontend TS
cd frontend && npm run lint && npm run typecheck && npm test && cd ..

# Build de release (smoke)
cargo tauri build --debug
```

Si CUALQUIER gate falla:
- `git restore` los archivos modificados
- Documenta en `memory/FAILED_ATTEMPTS.md`
- Intenta candidato #2

### Paso 4: Documentación
Si PASA:
- Commit con mensaje descriptivo en español: `improve(<expert>): <título>`
- Actualiza `scripts/assembly_data.json`: `status: pending → done` para el finding resuelto
- Append a `memory/IMPROVEMENT_LOG.md`: fecha, iteración, qué cambió, por qué, antes/después
- Append a `memory/METRICS_HISTORY.md`: tests count, clippy warnings, build size

### Paso 5: Repetir
Si quedan candidatos y tiempo, repite con el siguiente. **Máximo 3 candidatos por ciclo.**

## Reglas inquebrantables

- NUNCA bajar el número de tests
- NUNCA aumentar warnings de clippy
- NUNCA romper el build de Tauri
- NUNCA inventar problemas — solo arreglar lo que el código muestra
- SIEMPRE actualizar `scripts/assembly_data.json` y `memory/*` después de cada cambio
- SIEMPRE documentar intentos fallidos con razón exacta
- Single concern, máximo 100 LOC de diff por ciclo

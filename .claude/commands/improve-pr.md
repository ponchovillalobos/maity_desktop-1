# /improve-pr — Ciclo de Auto-Mejora con Pull Request

Variante de `/improve` que aísla cada cambio en su propia branch y abre un Pull Request al fork del usuario.
Workflow: **1 finding → 1 branch → 1 commit → 1 PR**.

Argumento opcional: EXP-ID concreto (ej: `/improve-pr SEC-001`) o área de foco.

---

## Paso 0: Pre-flight

```bash
gh auth status                # debe estar logueado
git status                    # working tree limpio
git checkout main
git pull upstream main        # sincronizar con upstream Sixale730
git push origin main          # propagar a fork
```

Lee:
- `memory/IMPROVEMENT_LOG.md`
- `memory/FAILED_ATTEMPTS.md`
- `scripts/assembly_data.json`

---

## Paso 1: Elegir finding

Lanza el agente `auditor` (o lee `assembly_data.json` directamente):
- Filtrar `status == "pending"`
- Si el usuario pasó EXP-ID, usar ese
- Si no, ordenar por `impact/effort` desc y tomar el top
- Cargar `id`, `title`, `description`, `recommendation`, `impact`, `effort`, expert key

---

## Paso 2: Crear branch dedicada

```bash
SLUG=$(echo "<title>" | tr '[:upper:] ' '[:lower:]-' | tr -cd 'a-z0-9-')
BRANCH="improve/<EXP-ID>-${SLUG}"
git checkout -b "$BRANCH"
```

---

## Paso 3: Implementar fix

Reglas:
- Single concern, ≤100 LOC de diff
- Test primero si agregas comportamiento
- Seguir patrones del repo
- No tocar archivos no relacionados

---

## Paso 4: Quality gates (validator agent)

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
cd frontend && npm run lint && npm run typecheck && npm test && cd ..
```

### Si falla:
```bash
git restore .
git checkout main
git branch -D "$BRANCH"
```
- Append a `memory/FAILED_ATTEMPTS.md` (error exacto + causa + lección)
- Salir del ciclo

---

## Paso 5: Commit

```bash
git add <archivos específicos>
git commit -m "improve(<expert>): <title>

Resolves <EXP-ID> from assembly.
Impact: <impact>/10  Effort: <effort>/10

<description>

Recommendation:
<recommendation>

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Paso 6: Push y abrir PR

```bash
git push -u origin "$BRANCH"

gh pr create \
  --base main \
  --head "$BRANCH" \
  --title "improve(<expert>): <title>" \
  --body "$(cat <<'EOF'
## Asamblea de Expertos — <EXP-ID>

**Experto:** <expert name>
**Severidad:** <severity>
**Impacto:** <impact>/10
**Esfuerzo:** <effort>/10

### Problema
<description>

### Solución aplicada
<recommendation>

### Quality gates
- [x] cargo fmt
- [x] cargo clippy (0 warnings)
- [x] cargo test (N pasando)
- [x] frontend lint + typecheck + test

🤖 Generado por /improve-pr
EOF
)"
```

---

## Paso 7: Marcar como done en la asamblea

Editar `scripts/assembly_data.json`: cambiar `"status": "pending"` → `"status": "done"` para `<EXP-ID>`.

Hacer commit de esta actualización **directamente en `main`** (no en la branch del PR), porque el dashboard lee de main:

```bash
git checkout main
# editar assembly_data.json
git add scripts/assembly_data.json
git commit -m "assembly: marcar <EXP-ID> como done"
git push origin main
```

---

## Paso 8: Aprender

Append a:
- `memory/IMPROVEMENT_LOG.md` — fecha, EXP-ID, branch, PR URL, métricas
- `memory/METRICS_HISTORY.md` — tests count, clippy warnings, build status

---

## Paso 9: Reportar

```
═══ PR ABIERTO ═══
Finding: <EXP-ID> — <title>
Branch: improve/<EXP-ID>-<slug>
PR: <URL>
Tests: <count>
Pendientes en asamblea: <N>
═══════════════════
```

---

## Reglas inquebrantables

- NUNCA hacer push directo a `main` excepto para `assembly_data.json` updates
- NUNCA abrir PR sin que todos los gates pasen
- NUNCA tocar archivos fuera del scope del finding
- SIEMPRE usar `--base main --head improve/...` explícito en `gh pr create`
- 1 PR = 1 finding (no agrupar)

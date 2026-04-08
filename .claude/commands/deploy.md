# /deploy — Documentar, Respaldar y Construir Release (Maity Desktop)

Flujo completo de documentación, commit y build de release Tauri.

## Pasos

### 1. Documentar cambios
- Lee archivos modificados con `git status`
- Actualiza `docs/*.md` relevantes si hay cambios no documentados
- Si hay nuevo componente, crea archivo de doc siguiendo la convención del proyecto
- Actualiza `README.md` si se agregaron features visibles para el usuario

### 2. Respaldar (Git commit)
- `git add` archivos específicos (NO usar `-A` para evitar secretos)
- Commit en español: `feat: <desc>`, `fix: <desc>`, `docs: <desc>`, `chore: <desc>`

### 3. Build release Tauri
```bash
cargo tauri build --release
```
- Si falla, reporta el error y sugiere solución
- Si tiene éxito, reporta la ruta del instalador (`src-tauri/target/release/bundle/`)

### 4. Code signing (Windows)
- Verifica que el binario está firmado si hay certificado configurado
- Si no hay cert, advertir al usuario

### 5. Resumen
- Archivos documentados
- Commit (hash + mensaje)
- Estado del build
- Path del instalador
- Estado de firma

# Guia de Configuracion para Desarrollo en Windows

Meetily (Maity Desktop) es un asistente de reuniones con IA que captura, transcribe y resume reuniones localmente. Esta guia cubre la configuracion completa del entorno de desarrollo en Windows desde cero.

El proyecto tiene dos componentes:
- **Frontend**: aplicacion de escritorio Tauri (Rust + Next.js + TypeScript)
- **Backend**: servidor FastAPI para persistencia y resumenes con LLM (Python)

---

## Requisitos del sistema

- Windows 10 (version 1709 o superior) o Windows 11
- [winget](https://aka.ms/getwinget) (App Installer) disponible — incluido por defecto en Windows 10/11 actualizados
- ~10 GB de espacio libre en disco (herramientas de desarrollo + modelos Whisper)

---

## Opcion A: Setup automatico (recomendado)

El script `setup_dev_env.ps1` detecta prerequisitos, instala lo que falte via winget, configura el proyecto (dependencias frontend + venv backend) y verifica el resultado final. Es idempotente: se puede ejecutar multiples veces sin problema.

### Ejecutar el script

```powershell
# Si la politica de ejecucion no permite scripts, habilitarla primero:
Set-ExecutionPolicy RemoteSigned -Scope CurrentUser

# Ejecutar desde la raiz del repositorio:
.\setup_dev_env.ps1
```

### Fases del script

| Fase | Que hace |
|------|----------|
| **1. Deteccion** | Verifica si Git, Node.js (>= 18), pnpm (>= 9), Rust (>= 1.77), Python (>= 3.8), VS Build Tools 2022 (C++), CMake y LLVM estan instalados. |
| **2. Instalacion** | Lista las herramientas faltantes y pide confirmacion antes de instalarlas via `winget` (o `npm` para pnpm). |
| **3. Configuracion** | Ejecuta `pnpm install` en `frontend/` y crea un virtual environment en `backend/venv` con las dependencias de `requirements.txt` y `requirements-dev.txt`. |
| **4. Verificacion** | Re-detecta todas las herramientas y muestra un resumen con estado OK/FAIL para cada una. |

### Output esperado

El script muestra indicadores con colores:
- `[OK]` — herramienta encontrada o paso exitoso
- `[FAIL]` — herramienta no encontrada o paso fallido
- `[INSTALLING]` — instalacion en progreso
- `[WARN]` — advertencia no critica

Al finalizar, muestra los comandos para iniciar el desarrollo (ver seccion [Ejecucion del proyecto](#ejecucion-del-proyecto)).

### Troubleshooting del script

| Problema | Solucion |
|----------|----------|
| `winget` no encontrado | Instalar [App Installer](https://aka.ms/getwinget) desde Microsoft Store o actualizar Windows. |
| Error de permisos al ejecutar | Ejecutar `Set-ExecutionPolicy RemoteSigned -Scope CurrentUser` y reintentar. |
| `npm` no disponible despues de instalar Node.js | Cerrar y reabrir la terminal, luego ejecutar el script de nuevo. |
| VS Build Tools tarda mucho | Es normal — la carga de trabajo C++ puede requerir varios GB de descarga. |

---

## Opcion B: Instalacion manual paso a paso

### 1. Instalar herramientas

Ejecutar cada comando en PowerShell (abrir como administrador si algun instalador lo requiere).

| Herramienta | Version requerida | Comando de instalacion |
|---|---|---|
| Git | cualquiera | `winget install --id Git.Git -e` |
| Node.js | >= 18 | `winget install --id OpenJS.NodeJS.LTS -e` |
| pnpm | >= 9 | `npm install -g pnpm` |
| Rust (rustup) | >= 1.77 | `winget install --id Rustlang.Rustup -e` |
| Python | >= 3.8 | `winget install --id Python.Python.3.12 -e` |
| VS Build Tools 2022 | con carga C++ | ver comando abajo |
| CMake | cualquiera | `winget install --id Kitware.CMake -e` |
| LLVM | >= 14 | `winget install --id LLVM.LLVM -e` |

**LLVM/Clang** — requerido para compilar el motor de transcripcion local:

El proyecto usa `whisper-rs` para transcripcion local con Whisper.cpp. Esta dependencia usa `bindgen` para generar bindings de Rust a C++, y `bindgen` necesita `libclang.dll` para parsear el codigo C/C++. Sin LLVM instalado, el build de Cargo falla con errores como:

```
error: failed to run custom build command for `whisper-rs-sys`
...
Unable to find libclang
```

> **Nota**: LLVM es necesario incluso si solo vas a usar transcripcion en la nube (Deepgram), porque `whisper-rs` actualmente es una dependencia obligatoria del proyecto.

**VS Build Tools 2022** (con carga de trabajo C++ incluida):

```powershell
winget install --id Microsoft.VisualStudio.2022.BuildTools -e --override "--add Microsoft.VisualStudio.Workload.VCTools --includeRecommended --passive"
```

**Despues de instalar Rust**, configurar el toolchain estable:

```powershell
rustup default stable
```

### 2. Verificar instalaciones

Cerrar y reabrir la terminal despues de instalar todo, luego verificar:

```powershell
git --version          # git version 2.x.x
node --version         # v18.x.x o superior
pnpm --version         # 9.x.x o superior
rustc --version        # rustc 1.77.x o superior
python --version       # Python 3.8+ (recomendado 3.12)
cmake --version        # cmake version 3.x.x
clang --version        # clang version 14.x.x o superior
```

Para verificar VS Build Tools, confirmar que existe el directorio:
```powershell
Test-Path "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2022\BuildTools"
```

---

## Configuracion del proyecto

### Frontend

```powershell
cd frontend
pnpm install
```

Esto instala todas las dependencias de Node.js. Las dependencias de Rust (crates) se descargan automaticamente la primera vez que se compila con `cargo`.

### Backend

```powershell
cd backend
python -m venv venv
.\venv\Scripts\Activate.ps1
pip install -r requirements.txt
pip install -r requirements-dev.txt
```

---

## Ejecucion del proyecto

### Modo desarrollo (frontend completo con Tauri)

```powershell
cd frontend
pnpm run tauri:dev
```

Inicia el servidor Next.js + compila y ejecuta el backend Rust en modo debug con hot reload.

### Build de produccion

```powershell
cd frontend
pnpm run tauri:build
```

Genera ejecutable e instaladores (MSI y NSIS) en `frontend/src-tauri/target/release/bundle/`.

### Backend (API + persistencia + resumenes LLM)

El backend es opcional — el frontend puede funcionar standalone con transcripcion local (Whisper). Para habilitar persistencia de reuniones y resumenes con LLM:

```powershell
cd backend
.\venv\Scripts\Activate.ps1
python app\main.py
```

### Endpoints disponibles

| Servicio | URL |
|----------|-----|
| Frontend dev | http://localhost:3118 |
| Backend API | http://localhost:5167 |
| Swagger docs | http://localhost:5167/docs |
| ReDoc | http://localhost:5167/redoc |

---

## Aceleracion GPU (opcional)

Por defecto, la compilacion en Windows usa procesamiento CPU. Para habilitar aceleracion GPU en la transcripcion Whisper:

| GPU | Feature flag | Comando |
|-----|-------------|---------|
| NVIDIA (CUDA) | `cuda` | `pnpm run tauri:dev:cuda` |
| AMD/Intel (Vulkan) | `vulkan` | `pnpm run tauri:dev:vulkan` |

Esto requiere tener instalado el SDK correspondiente (CUDA Toolkit para NVIDIA, Vulkan SDK para AMD/Intel).

Para mas detalles, consultar:
- [docs/GPU_ACCELERATION.md](docs/GPU_ACCELERATION.md)
- [docs/BUILDING.md](docs/BUILDING.md)

---

## Troubleshooting

| Problema | Causa | Solucion |
|----------|-------|----------|
| `cargo` o `rustc` no encontrado despues de instalar Rust | El PATH no se actualizo | Cerrar y reabrir la terminal. Si persiste, verificar que `%USERPROFILE%\.cargo\bin` esta en el PATH. |
| Error de MSVC linker (`link.exe` not found) | VS Build Tools sin carga C++ | Reinstalar VS Build Tools con la carga de trabajo "Desktop development with C++" (ver comando en seccion manual). |
| `pnpm install` falla con errores de Node | Version de Node.js < 18 | Verificar con `node --version` y actualizar si es necesario. |
| Error de permisos al ejecutar scripts `.ps1` | Politica de ejecucion restrictiva | Ejecutar `Set-ExecutionPolicy RemoteSigned -Scope CurrentUser`. |
| `pnpm run tauri:dev` falla en la compilacion de Rust | Falta CMake o VS Build Tools | Verificar ambas herramientas estan instaladas (ver tabla de requisitos). |
| Error `Unable to find libclang` o `failed to run custom build command for whisper-rs-sys` | LLVM no instalado | Instalar LLVM con `winget install LLVM.LLVM`. Cerrar y reabrir la terminal. Verificar con `clang --version`. |
| `clang` instalado pero `libclang` no encontrado | LLVM no esta en PATH | Agregar `C:\Program Files\LLVM\bin` al PATH del sistema. Alternativamente, definir la variable de entorno `LIBCLANG_PATH=C:\Program Files\LLVM\bin`. |
| El backend no inicia (error de modulo) | Virtual environment no activado o dependencias no instaladas | Activar venv (`.\venv\Scripts\Activate.ps1`) y ejecutar `pip install -r requirements.txt`. |
| FFmpeg no encontrado al codificar grabaciones | FFmpeg no esta en PATH | Instalar FFmpeg (`winget install Gyan.FFmpeg`) o colocarlo en el directorio de trabajo. |

---

## Rotacion de la clave del updater Tauri (OPS-007)

El updater de Tauri firma cada release con una clave privada minisign y verifica
con la clave publica embebida en `frontend/src-tauri/tauri.conf.json` (campo
`plugins.updater.pubkey`). **Si la clave privada se compromete o se pierde,
debes rotarla siguiendo este procedimiento.**

### Procedimiento de rotacion (canal puente)

1. **Generar nueva clave minisign**

   ```powershell
   # Desde PowerShell, en el repo
   tauri signer generate -w frontend/src-tauri/tauri-updater.key.new
   ```

   Esto crea un par minisign nuevo. **Guarda el archivo `.key.new` en tu password
   manager o en GitHub Secrets como `TAURI_SIGNING_PRIVATE_KEY_NEW`.**

2. **Publicar release v_X con DOBLE FIRMA (canal puente)**

   El plugin `tauri-plugin-updater` v2.x soporta verificacion de multiples
   pubkeys mediante un fork o un build personalizado. Pasos:

   - Editar `tauri.conf.json` para incluir AMBAS pubkeys (la vieja y la nueva)
     temporalmente. Esto requiere un fork del plugin updater o usar la nueva
     funcionalidad de multi-key cuando este disponible.
   - Build firmando con la NUEVA clave: `TAURI_SIGNING_PRIVATE_KEY=<nueva>`
   - Publicar como release X.Y.Z+rotation

3. **Esperar adopcion >95% de la version puente**

   Monitorear telemetria PostHog (`app.version`) hasta que >95% de los
   usuarios activos esten en X.Y.Z+rotation. Tipicamente 7-14 dias en B2B.

4. **Eliminar la pubkey vieja**

   Editar `tauri.conf.json` para dejar SOLO la pubkey nueva. Build siguiente
   release X.Y.Z+1 firmada solo con la nueva clave.

5. **Rotar el secreto de GitHub Actions**

   - Borrar `TAURI_SIGNING_PRIVATE_KEY` viejo de GitHub Actions secrets
   - Renombrar `TAURI_SIGNING_PRIVATE_KEY_NEW` -> `TAURI_SIGNING_PRIVATE_KEY`
   - Verificar que `.github/workflows/build-windows.yml` lee del secret correcto

6. **Documentar el evento**

   - Issue publico explicando que hubo una rotacion (sin detalles del compromiso)
   - Update a este documento con la fecha de rotacion

### Cuando rotar (triggers)

- Compromiso confirmado o sospechado de la clave privada (ej: leak en logs)
- Cambio de cuenta GitHub responsable de releases
- Auditoria periodica anual (best practice)
- Antes de un cambio mayor de version (v1.0 release)

### Prevencion

- Nunca commitear `.key` files al repo (incluido en `.gitignore`)
- Almacenar la clave privada solo en GitHub Secrets + un password manager con MFA
- Usar GitHub Environments con review obligatoria para releases que firmen
- Limitar quien tiene acceso al secret a un grupo pequeno de mantenedores

# Performance Benchmark Harness

Mide metricas user-visible de Maity Desktop en cada build para detectar
regresiones antes de que lleguen al usuario. Parte del Protocolo Guardian
del proyecto.

## Archivos

| Archivo | Descripcion |
|---|---|
| `benchmark_performance.ps1` | Harness principal (PowerShell 5.1+) |
| `benchmark_performance.py`  | Port en Python 3.10+ para CI/parity |
| `results/perf_{date}_{time}.json` | Output por ejecucion (consumible por dashboard) |

## Como correr

```powershell
# PowerShell (ruta canonica)
powershell -ExecutionPolicy Bypass -File tests\benchmark_performance.ps1

# Python (paridad, CI)
python tests\benchmark_performance.py
```

Ambos scripts:
1. Matan cualquier `maity-desktop.exe` / `msedgewebview2.exe` vivo.
2. Registran el offset actual del log de hoy como baseline.
3. Lanzan `target\debug\maity-desktop.exe`.
4. Muestrean metricas durante hasta 60s.
5. Matan el proceso al terminar.
6. Escriben JSON en `tests/results/`.
7. Anaden una linea resumen a `memory/METRICS_HISTORY.md`.
8. Salen `0` si todo paso el threshold, `1` si alguna metrica lo excedio, `2` si hubo error de harness.

Requisitos:
- Windows 11, sin derechos de admin.
- Binario debug existente en `target\debug\maity-desktop.exe`
  (no re-compila — corre `pnpm run tauri:build:debug` antes).
- PowerShell 5.1+ o Python 3.10+.
- Sin dependencias externas.

## Metricas

| # | Metrica | Target | Como se mide |
|---|---|---|---|
| 1 | `ColdStartMs` | <3000 | `Start-Process` -> `Get-Process.Responding && MainWindowHandle != 0` |
| 2 | `WebViewJsReadyMs` | <5000 | Suma de `WorkingSet64` de `msedgewebview2.exe` >= 200 MB |
| 3 | `WhisperPreloadMs` | <10000 | Tail del log hasta matchear `PERF-005: Whisper model '...' preloaded` |
| 4 | `ClickToRecordMs` | <500 | **TODO** — requiere UIAutomation para click sintetico |
| 5 | `TranscriptionRtf` | >1.5 | **TODO** — requiere inyeccion de audio fixture + parseo de logs |
| 6 | `RamPeakMb` | <2048 | Max de `maity-desktop + msedgewebview2` durante 5s post-preload |
| 7 | `StopToSummaryMs` | <5000 | **TODO** — requiere UIAutomation + hook al evento `recording-stop-complete` |

Metricas 1,2,3,6 se miden automaticamente. Metricas 4,5,7 estan listadas en el JSON
con valor `-1` y una nota en `errors[]` — su implementacion requiere UIAutomation
(`System.Windows.Automation`) o un hook programatico del frontend al backend Tauri,
ambos fuera del scope inicial.

## Thresholds

Editar el bloque `$Thresholds` al inicio de `benchmark_performance.ps1`
(o `THRESHOLDS` en el `.py`) — los dos scripts deben mantenerse sincronizados.

| Metrica | Threshold | Operador | Prioridad del proyecto |
|---|---|---|---|
| ColdStartMs | 3000 | `<` | P3 — UX percibido |
| WebViewJsReadyMs | 5000 | `<` | P3 — UX percibido |
| WhisperPreloadMs | 10000 | `<` | **P2** — "post-sesion casi instantaneo" |
| ClickToRecordMs | 500 | `<` | **P2** — latencia durante vivo |
| TranscriptionRtf | 1.5 | `>` | **P2** — velocidad de transcripcion |
| RamPeakMb | 2048 | `<` | P3 — estabilidad B2B |
| StopToSummaryMs | 5000 | `<` | **P2** — "al colgar, resumen listo" |

## JSON schema (`tests/results/perf_{date}_{time}.json`)

```json
{
  "timestamp": "2026-04-08T12:34:56.7890000+00:00",
  "binary": "D:\\Maity_Desktop\\target\\debug\\maity-desktop.exe",
  "thresholds": { "ColdStartMs": 3000, "WebViewJsReadyMs": 5000, ... },
  "metrics": {
    "ColdStartMs": 1842,
    "WebViewJsReadyMs": 3210,
    "WhisperPreloadMs": 7450,
    "WhisperPreloadLine": "2026-04-08T12:34:56.789Z INFO PERF-005: Whisper model 'base' preloaded in 7421ms",
    "RamPeakMb": 934.2,
    "ClickToRecordMs": -1,
    "TranscriptionRtf": -1,
    "StopToSummaryMs": -1
  },
  "errors": [
    "ClickToRecord / TranscriptionRtf / StopToSummary are TODO (UIAutomation out of scope)"
  ],
  "pass": true
}
```

## Limitaciones conocidas

1. **Click-to-record, transcription RTF, stop-to-summary son TODO.** Requieren
   UIAutomation (`Add-Type -AssemblyName UIAutomationClient`) o un modo headless
   del frontend. Documentados en el JSON como `-1` y en `errors[]`.
2. **Polling vs ETW.** El harness usa polling de `Get-Process` cada 100-250ms;
   precision ~100ms. Para precision sub-ms se necesitaria ETW (ver seccion research).
3. **WebView2 RAM >= 200 MB es un proxy.** No garantiza "JS listo", solo que el
   runtime ya cargo el bundle. Un indicador mas limpio seria un `console.log`
   que el backend Rust pueda capturar.
4. **Single run.** Los numeros son ruidosos — para CI se deberian correr 3-5 veces
   y tomar la mediana (TODO: flag `-Runs N`).
5. **No mata procesos hijos de WebView2** si su `Parent` ya murio entre polls.
6. **Log path hardcoded** al usuario actual (`alfon`). Parametrizar antes de mergear a CI.

## Research — otras formas de medir startup en Windows/Tauri/Electron

Listado breve para futura exploracion (no implementado):

1. **Windows Performance Recorder (WPR) + WPA** — ETW tracing nativo. Captura
   eventos `Process/Start`, `Image/Load`, `ThreadPool/*` con precision de
   microsegundos. `wpr.exe -start GeneralProfile -start CPU -filemode` no requiere
   admin para la mayoria de los providers de usuario.
2. **Event Tracing for Windows (ETW) via `logman`** — `logman create trace` +
   provider `Microsoft-Windows-Kernel-Process`. Mas liviano que WPR, scriptable.
3. **Electron `app.getMetrics()` / `process.getCreationTime()`** — en apps Electron
   se usa como referencia; Tauri no tiene equivalente directo pero `std::time::Instant`
   en `main()` + log emission cumple el mismo rol (ya se hace con `PERF-005`).
4. **Sentry / OpenTelemetry app-start spans** — instrumentacion embedded; reporta
   a un backend pero cuesta 1-2 MB de RAM. Recomendado para telemetria B2B.
5. **Playwright / WebdriverIO con `tauri-driver`** — permite click sintetico en
   botones reales del frontend y mediria `ClickToRecord` y `StopToSummary` end-to-end.
   Depende de `tauri-driver` + `msedgedriver` matching; no trivial pero es el camino
   correcto para los tres TODOs.

## Integracion con el Protocolo Guardian

Despues de `pnpm run tauri:build:debug` (exit code 0), correr:

```powershell
powershell -ExecutionPolicy Bypass -File tests\benchmark_performance.ps1
```

Si sale con `exit 1` significa que una metrica retrocedio vs threshold — bloquea
el commit hasta que se entienda la regresion (misma politica que `cargo clippy -D warnings`).

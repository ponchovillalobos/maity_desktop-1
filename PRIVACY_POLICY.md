# Maity Privacy Policy

*Last updated: 2026-04-08*
*Version: 2.0 (LEG-001 — corrige declaración engañosa de la versión 1.x sobre "local-first")*

## TL;DR — qué hace Maity con tus datos (en lenguaje claro)

Maity Desktop te permite **elegir entre dos modos de procesamiento**, y por defecto envía datos a proveedores externos. **Esta política refleja honestamente ese flujo**, corrigiendo versiones previas que afirmaban incorrectamente "todo se procesa localmente".

| Datos | Modo Cloud (default) | Modo Local (opt-in) |
|---|---|---|
| Audio crudo (WAV) | Se transmite a **Deepgram** para transcripción en tiempo real | Nunca sale de tu equipo (procesado por Whisper local) |
| Transcripción (texto) | Se envía a **OpenAI / Anthropic / Groq** para resúmenes | Procesado por llama-helper local (Llama.cpp) |
| Almacenamiento | SQLite local en `$APPDATA/com.maity.ai/` | Igual |
| Telemetría de uso | PostHog (opt-out disponible) | Igual |
| Crash reports | Sentry (opt-out disponible) | Igual |

**Importante:** activar el modo Local requiere descargar modelos pesados (Whisper 1-10GB, Llama 4-30GB) y **disponer de RAM y CPU suficientes**. No es viable en equipos modestos.

---

## 1. Nuestro compromiso

Maity es un asistente de reuniones de código abierto bajo licencia MIT. Estamos comprometidos con:

- **Transparencia total** sobre qué datos salen del equipo del usuario y a quién
- **Control del usuario**: tú decides si usas modo Cloud o Local
- **Minimización de datos**: solo procesamos lo necesario para transcripción y resumen
- **Borrado a solicitud**: puedes eliminar reuniones individualmente o purgar todo
- **No vendemos datos**: nunca compartimos tu contenido con terceros más allá de los subprocesadores listados

---

## 2. Cómo se procesan los datos (flujo real)

### 2.1 Captura de audio
- **Local únicamente.** El audio del micrófono y del sistema (WASAPI loopback en Windows, CoreAudio en macOS) se captura en `frontend/src-tauri/src/audio/` sin salir del proceso de Maity.
- Se mezcla en stereo interleaved (L=mic, R=sistema) y se guarda incrementalmente en `$APPDATA/com.maity.ai/<meeting-id>/audio.mp4`.
- **Política de checkpoints**: cada 30 segundos se persiste un checkpoint en disco para garantizar zero data loss ante crashes.

### 2.2 Transcripción
**Modo Cloud (default — Deepgram):**
- El audio PCM 16kHz se transmite por WebSocket cifrado (TLS 1.3) a `wss://api.deepgram.com`
- Deepgram procesa el audio bajo su política: https://deepgram.com/privacy
- Maity no almacena tu audio en ningún servidor propio
- Costo: ~$0.0043/minuto de audio (a cargo del usuario via su API key BYOK)

**Modo Local (opt-in — Whisper):**
- El audio se procesa con whisper-rs en CPU/GPU local
- **Nada sale del equipo**
- Requiere 4GB+ RAM (modelo small), 10GB+ RAM (modelo large)

### 2.3 Resúmenes con LLM
**Modo Cloud (default — ChatGPT/Claude/Groq):**
- La transcripción completa se envía vía HTTPS al proveedor de LLM elegido
- OpenAI, Anthropic y Groq procesan el texto bajo sus respectivas políticas
- El usuario provee su propia API key (BYOK)

**Modo Local (opt-in — llama-helper):**
- Se ejecuta llama-helper como sidecar Rust con un modelo GGUF local (Llama 3, Mistral, etc.)
- **Nada sale del equipo**
- Requiere GPU o CPU potente

### 2.4 Telemetría
- **PostHog**: eventos anónimos de uso (¿se completó una grabación? ¿qué proveedor STT?)
- **Sentry**: stack traces de crashes
- **Ambos son opt-OUT por defecto**: el usuario debe aceptar explícitamente en el primer arranque
- **El contenido de tus reuniones nunca se envía a PostHog/Sentry**, solo metadatos técnicos

---

## 3. Subprocesadores

Para usar el modo Cloud, los siguientes terceros procesan tus datos. **Cada uno tiene su propio DPA y política de privacidad** que debes aceptar:

| Subprocesador | Datos procesados | Propósito | Política |
|---|---|---|---|
| **Deepgram** | Audio PCM | Transcripción ASR streaming | https://deepgram.com/privacy + DPA https://www.deepgram.com/legal/dpa |
| **OpenAI** | Transcripción texto | Resúmenes LLM (gpt-4o, gpt-4o-mini) | https://openai.com/policies/privacy-policy + DPA https://openai.com/policies/data-processing-addendum |
| **Anthropic** | Transcripción texto | Resúmenes LLM (Claude 3.5/4) | https://www.anthropic.com/legal/privacy + DPA disponible bajo NDA |
| **Groq** | Transcripción texto | Resúmenes LLM (llama-3.3-70b) | https://groq.com/privacy-policy/ |
| **PostHog** | Eventos de uso anónimos | Analytics (opt-out) | https://posthog.com/privacy |
| **Sentry** | Crash reports | Diagnóstico (opt-out) | https://sentry.io/privacy/ |
| **GitHub** | Updater binario | Distribución de releases | https://docs.github.com/en/site-policy/privacy-policies |

**Para clientes B2B / enterprise**: contacta a soporte para firmar un DPA específico que cubra todos los subprocesadores listados.

---

## 4. Datos almacenados localmente

Maity almacena en tu equipo (default `$APPDATA/com.maity.ai/`):

- **`maity.db`** (SQLite): metadata de reuniones, transcripciones, resúmenes, configuración
- **`<meeting-id>/audio.mp4`**: archivo de audio mezclado (mic + sistema)
- **`<meeting-id>/.checkpoints/`**: checkpoints incrementales (se borran al finalizar)
- **`logs/`**: logs rotativos (rotación cada 24h, retención 7 días por defecto)

**El audio nunca se cifra en disco por defecto** (ver hallazgo SEC-004 en el roadmap interno). Si guardas reuniones confidenciales, mantén tu disco con cifrado de SO (BitLocker en Windows, FileVault en macOS).

---

## 5. Política de retención

- **Datos de reunión** (audio + transcript + summary): **se conservan indefinidamente** hasta que el usuario las borre manualmente desde la UI
- **Configuración planificada (roadmap LEG-003)**: ajuste "Eliminar reuniones después de N días" con purga automática
- **Logs**: rotación diaria, eliminación automática a los 7 días
- **Eventos PostHog**: 12 meses (configuración del proveedor)
- **Crash reports Sentry**: 30 días (configuración del proveedor)

Puedes borrar todos tus datos en cualquier momento:
1. Cerrar Maity
2. Borrar `$APPDATA/com.maity.ai/` completo
3. Desinstalar la aplicación

---

## 6. Tus derechos según jurisdicción

### Para usuarios en la Unión Europea (GDPR)
- **Derecho de acceso (Art. 15)**: Puedes ver todos tus datos en la base local SQLite
- **Derecho de rectificación (Art. 16)**: Edita transcripciones y resúmenes desde la UI
- **Derecho al olvido (Art. 17)**: Borra reuniones individualmente o `$APPDATA/com.maity.ai/`
- **Portabilidad (Art. 20)**: Exporta como JSON/Markdown desde Settings → Export
- **Base legal**: Art. 6(1)(b) (ejecución de contrato — uso de la app) + Art. 6(1)(a) (consentimiento — telemetría)

### Para usuarios en México (LFPDPPP)
- **Aviso de Privacidad** integrado en este documento conforme al art. 16 LFPDPPP
- **Derechos ARCO**: Acceso, Rectificación, Cancelación, Oposición — todos ejercibles localmente
- **Responsable del tratamiento**: Pancho Villalobos (ponchovillalobos@maity.local) — actualizar con razón social cuando se constituya
- **Transferencias**: a Deepgram (USA), OpenAI (USA), Anthropic (USA) bajo cláusulas contractuales tipo

### Para usuarios en California (CCPA / CPRA)
- **Derecho a saber**: Esta política describe todos los datos recopilados
- **Derecho a borrar**: Borra `$APPDATA/com.maity.ai/`
- **Derecho a opt-out de venta**: No vendemos datos personales

### Para usuarios en otras jurisdicciones
Aplican las leyes locales. Maity sigue los estándares más estrictos (GDPR) por defecto.

---

## 7. Consentimiento de participantes en grabaciones

**MUY IMPORTANTE**: en muchas jurisdicciones, grabar una conversación sin consentimiento de todos los participantes es ilegal. Específicamente:

- **EE.UU.** — 11 estados son "two-party consent" (CA, FL, IL, MD, MA, MT, NH, PA, WA, CT, MI). Grabar sin consentimiento puede ser delito.
- **UE** — GDPR Art. 6 requiere base legal (consentimiento) para procesar datos personales (incluyendo voz)
- **México** — LFPDPPP requiere aviso de privacidad y consentimiento

**Maity NO te exime de esta responsabilidad legal.** Antes de iniciar una grabación, debes informar a todos los participantes y obtener su consentimiento. La aplicación mostrará recordatorios pero la responsabilidad legal es del usuario.

---

## 8. Cambios a esta política

Esta política puede actualizarse cuando agreguemos features que cambien el manejo de datos. Mantendremos un changelog visible al final del documento. Los cambios materiales serán notificados in-app antes del primer uso tras el update.

---

## 9. Contacto

- **Issues técnicos / privacidad**: https://github.com/Sixale730/maity_desktop/issues (público) o privacy@maity.local (privado)
- **Solicitudes de DPA enterprise**: enterprise@maity.local
- **Solicitudes ARCO/GDPR**: privacy@maity.local con copia de identificación

---

## 10. Open Source

Como proyecto open-source bajo licencia MIT, puedes:
- Revisar la implementación completa de privacidad en https://github.com/Sixale730/maity_desktop
- Modificar el código para tus requerimientos
- Desplegar en tu propia infraestructura (modo local 100% on-premise)
- Contribuir mejoras de privacidad

---

## Document version history

| Version | Date | Changes |
|---|---|---|
| **2.0** | 2026-04-08 | **LEG-001 (CRITICAL fix)**: Corrige declaración engañosa de v1.x sobre "local-first never transmitted". Documenta honestamente el flujo real con Deepgram + OpenAI/Anthropic/Groq como subprocesadores default. Añade tabla de subprocesadores con DPAs. Avisos por jurisdicción (US/EU/MX/CA). Sección de consentimiento de participantes. Cumple FTC Act §5, art. 5 GDPR, art. 7 LFPDPPP. |
| 1.1 | 2026-04-07 | LEG-006: Updated metadata, real date, version 0.2.31, product rebranded to Maity |
| 1.0 | — | Initial version (Meetily fork baseline) |

---

*Esta política aplica a Maity Desktop v0.2.31+. Para deployments enterprise on-premise contactar a enterprise@maity.local.*

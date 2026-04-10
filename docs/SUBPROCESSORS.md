# Maity Desktop — Lista de subprocesadores

*Last updated: 2026-04-08*
*LEG-004: documenta los terceros que procesan datos cuando se usa el modo Cloud (default)*

Este documento lista todos los **subprocesadores** que procesan datos de usuarios de Maity Desktop bajo nuestro modelo BYOK (Bring Your Own Key). **Cada uno tiene su propia política de privacidad y DPA**, y el usuario es responsable de aceptarlos al ingresar sus API keys.

---

## Para clientes empresariales (B2B)

**Si tu organización requiere un Data Processing Agreement (DPA) específico**, contacta a `enterprise@maity.local`. Para deployments enterprise on-premise (modo Local 100%), no aplican los subprocesadores cloud listados abajo — todo el procesamiento queda en tu infraestructura.

---

## Subprocesadores activos

### 1. Deepgram (transcripción cloud — default)

| Campo | Valor |
|---|---|
| **Datos procesados** | Audio PCM 16kHz (mic + sistema interleaved stereo) |
| **Propósito** | Transcripción ASR streaming en tiempo real |
| **Modelo de cobranza** | BYOK — el usuario provee su propia API key |
| **Ubicación de procesamiento** | USA (multi-region) |
| **Política de privacidad** | https://deepgram.com/privacy |
| **DPA** | https://www.deepgram.com/legal/dpa |
| **Cumplimiento** | SOC 2 Type II, HIPAA-eligible (con BAA), GDPR DPA disponible |
| **Retención de datos** | Configurable por el usuario en su cuenta Deepgram (default: no se retiene audio tras procesamiento) |
| **Cuándo se activa** | Cuando el usuario selecciona Deepgram como provider STT en Settings (default) |
| **Cómo desactivar** | Cambiar a "Whisper local" en Settings → Transcription Provider |

---

### 2. OpenAI (resúmenes LLM — opcional)

| Campo | Valor |
|---|---|
| **Datos procesados** | Texto de transcripción (sin audio crudo) |
| **Propósito** | Generación de resúmenes con gpt-4o, gpt-4o-mini, etc. |
| **Modelo de cobranza** | BYOK |
| **Ubicación de procesamiento** | USA |
| **Política de privacidad** | https://openai.com/policies/privacy-policy |
| **DPA** | https://openai.com/policies/data-processing-addendum |
| **Cumplimiento** | SOC 2 Type II, GDPR DPA, opt-out de uso para entrenamiento por defecto en API |
| **Retención de datos** | 30 días para abuse monitoring, luego eliminado (cuando opt-out de entrenamiento) |
| **Cuándo se activa** | Cuando el usuario selecciona OpenAI como provider LLM |

---

### 3. Anthropic (resúmenes LLM — opcional)

| Campo | Valor |
|---|---|
| **Datos procesados** | Texto de transcripción |
| **Propósito** | Generación de resúmenes con Claude 3.5/4 |
| **Modelo de cobranza** | BYOK |
| **Ubicación de procesamiento** | USA |
| **Política de privacidad** | https://www.anthropic.com/legal/privacy |
| **DPA** | Disponible bajo NDA — solicitar a privacy@anthropic.com |
| **Cumplimiento** | SOC 2 Type II, GDPR DPA, no se usa contenido API para entrenamiento por defecto |
| **Retención de datos** | 30 días para abuse monitoring |
| **Cuándo se activa** | Cuando el usuario selecciona Anthropic como provider LLM |

---

### 4. Groq (resúmenes LLM — opcional)

| Campo | Valor |
|---|---|
| **Datos procesados** | Texto de transcripción |
| **Propósito** | Generación de resúmenes con Llama-3.3-70b-versatile, etc. |
| **Modelo de cobranza** | BYOK |
| **Ubicación de procesamiento** | USA |
| **Política de privacidad** | https://groq.com/privacy-policy/ |
| **DPA** | Solicitar a privacy@groq.com para use cases empresariales |
| **Cumplimiento** | En proceso SOC 2 (al momento de este documento) |
| **Cuándo se activa** | Cuando el usuario selecciona Groq como provider LLM |

---

### 5. PostHog (telemetría de uso — opt-out)

| Campo | Valor |
|---|---|
| **Datos procesados** | Eventos anónimos: feature usage, sesión duración, métricas técnicas |
| **NO procesa** | Contenido de reuniones, transcripciones, resúmenes, audio, nombres de archivo |
| **Propósito** | Mejora del producto, detección de bugs |
| **Ubicación** | EU (PostHog Cloud EU) — configurable |
| **Política de privacidad** | https://posthog.com/privacy |
| **DPA** | https://posthog.com/dpa |
| **Cumplimiento** | SOC 2 Type II, GDPR (data processor) |
| **Retención** | 12 meses, luego eliminado automáticamente |
| **Cuándo se activa** | **OPT-IN explícito** en el primer arranque (Settings → Privacy → Analytics) |
| **Cómo desactivar** | Settings → Privacy → desactivar Analytics |

---

### 6. Sentry (crash reports — opt-out)

| Campo | Valor |
|---|---|
| **Datos procesados** | Stack traces, contexto técnico (OS, app version), breadcrumbs |
| **NO procesa** | Contenido de reuniones, datos del usuario |
| **Propósito** | Diagnóstico de crashes y errores |
| **Ubicación** | USA / EU (configurable) |
| **Política de privacidad** | https://sentry.io/privacy/ |
| **DPA** | https://sentry.io/legal/dpa/ |
| **Cumplimiento** | SOC 2 Type II, ISO 27001, GDPR DPA |
| **Retención** | 30 días (configuración del proveedor) |
| **Cuándo se activa** | **OPT-IN explícito** en el primer arranque |
| **Cómo desactivar** | Settings → Privacy → desactivar Crash Reports |

---

### 7. GitHub (distribución de releases y updater)

| Campo | Valor |
|---|---|
| **Datos procesados** | IP del usuario al descargar release / verificar updates (logs estándar de GitHub) |
| **NO procesa** | Contenido de Maity, datos del usuario |
| **Propósito** | Distribución del binario instalable y verificación de updates |
| **Política de privacidad** | https://docs.github.com/en/site-policy/privacy-policies |
| **Cumplimiento** | SOC 2, ISO 27001, GDPR |
| **Cuándo se activa** | Al instalar o cuando el updater de Tauri verifica nuevas versiones |
| **Cómo desactivar** | Deshabilitar el auto-updater en Settings, descargar manualmente desde GitHub Releases |

---

## Subprocesadores **NO** utilizados

Para evitar confusiones, Maity Desktop **NO** envía datos a:

- ❌ Google Analytics, Facebook Pixel, ningún tracker comercial
- ❌ Microsoft Azure Application Insights
- ❌ AWS CloudWatch / Datadog / New Relic
- ❌ Mixpanel, Amplitude, Segment, ningún analytics adicional
- ❌ Slack, Discord, Notion, ningún integration de mensajería sin permiso explícito del usuario
- ❌ Servidores propios de Maity (no hay; el modelo es BYOK puro)

---

## Cambios en la lista de subprocesadores

Cualquier adición de un nuevo subprocesador será:

1. **Anunciada in-app** al menos 30 días antes de activarse
2. **Documentada** en este archivo con un commit en el repo público
3. **Reflejada en el changelog** de PRIVACY_POLICY.md
4. **Notificada por email** a clientes empresariales con DPA firmado

---

## Document version history

| Version | Date | Changes |
|---|---|---|
| 1.0 | 2026-04-08 | Initial public list of subprocessors (LEG-004) |

---

*Para preguntas: privacy@maity.local · Para DPAs empresariales: enterprise@maity.local*

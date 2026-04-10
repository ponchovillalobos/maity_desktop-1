# Incident Response Runbook — Maity Desktop

> Versión: 1.0 · Actualizado: 2026-04-10  
> Equipo: engineering@maity.cloud · Emergencias: oncall@maity.cloud

---

## Índice

1. [Severidades y tiempos de respuesta](#1-severidades)
2. [Runbook: Breach de datos de audio/transcripciones](#2-breach-datos)
3. [Runbook: Servicio de transcripción caído (Deepgram)](#3-deepgram-down)
4. [Runbook: Backend API no responde](#4-backend-down)
5. [Runbook: Crash / panic en cliente desktop](#5-app-crash)
6. [Runbook: Compromiso de credenciales (API keys, signing keys)](#6-credenciales)
7. [Post-mortem template](#7-postmortem)
8. [Contactos y escalado](#8-contactos)

---

## 1. Severidades

| Nivel | Criterio | Tiempo de respuesta | Tiempo de resolución |
|---|---|---|---|
| **SEV-1** | Pérdida de datos de usuarios, breach activo, firma de código comprometida | 15 min | 4h |
| **SEV-2** | Servicio cloud caído (Deepgram/Supabase), crash reproducible en release | 30 min | 8h |
| **SEV-3** | Degradación de funcionalidad, bug en feature no crítica | 4h | 48h |
| **SEV-4** | Mejora, optimización, bug cosmético | Próximo sprint | — |

---

## 2. Breach de Datos de Audio/Transcripciones

**Indicadores**: Acceso no autorizado detectado en Sentry, reporte de usuario, logs de Supabase anómalos.

### Pasos inmediatos (primeros 15 min)

```bash
# 1. Revocar JWT de Deepgram proxy en Cloudflare Worker
# → Ir a Cloudflare Dashboard > Workers > deepgram-proxy > Variables
# → Cambiar DEEPGRAM_API_KEY por una nueva key

# 2. Revocar sesiones de Supabase (si compromiso de auth)
# → Supabase Dashboard > Authentication > Users > Sign out all
# supabase --project-ref <ID> auth users list  # auditar cuentas activas

# 3. Preservar evidencia antes de cualquier limpieza
# → NO borrar logs de Sentry / CloudFlare / Supabase
# → Hacer snapshot de la BD SQLite afectada
cp "$APPDATA/com.maity.ai/maity.db" "incident-$(date +%Y%m%d).db"
```

### Contención (primeros 60 min)

- [ ] Identificar vector de entrada (XSS, API key expuesta, insider)
- [ ] Scope: ¿qué usuarios / qué reuniones están afectadas?
- [ ] Si es XSS activo: deshabilitar `unsafe-eval` (SEC-003) temporalmente
- [ ] Si es API key en código: revocar key, rotar, push hotfix
- [ ] Notificar a usuarios afectados por email en <72h (GDPR Art. 34)

### Notificación regulatoria (si >72h desde detección)

- GDPR Art. 33: notificar a autoridad de protección de datos (AEPD en España, INAI en México)
- Template de notificación: `docs/templates/gdpr_breach_notification.md` *(pendiente de crear)*

---

## 3. Servicio de Transcripción Caído (Deepgram)

**Indicadores**: Transcripciones no llegan, WebSocket errors en Sentry, `deepgram.com/status` degradado.

```bash
# Verificar estado de Deepgram
curl -s https://deepgram.com/status | jq '.incidents'
# o
curl -I https://api.deepgram.com/v1/listen  # debe responder 401 (no 5xx)
```

### Mitigación automática

El sistema **debería** caer a Parakeet local automáticamente (STT-002, pendiente).  
Mientras STT-002 no esté implementado, el usuario debe:

1. Settings → Proveedor → **Parakeet (Local)**
2. Esperar ~30s para que Parakeet cargue el modelo
3. Reiniciar grabación

### Si el proxy Cloudflare Worker falla

```bash
# Verificar proxy
curl -s https://maity-deepgram-proxy.workers.dev/health

# Si el worker está caído, redeploy:
cd cloudflare-worker/
npx wrangler deploy
```

---

## 4. Backend API No Responde (puerto 5167)

**Indicadores**: UI muestra "Backend no disponible", `/health` timeout, resúmenes no generan.

```bash
# Verificar proceso
# Windows:
netstat -ano | findstr :5167
tasklist | findstr python

# Reiniciar backend
# Windows (desde el directorio del proyecto):
cd backend
.\clean_start_backend.cmd

# macOS/Linux:
./clean_start_backend.sh

# Verificar health
curl http://localhost:5167/health
```

### Si el backend crashea en loop

```bash
# Ver últimos logs
# Windows: %APPDATA%\com.maity.ai\logs\backend.log
# macOS:   ~/Library/Logs/com.maity.ai/backend.log

# Verificar espacio en disco (SQLite puede crecer)
# Windows: dir "%APPDATA%\com.maity.ai\"

# Resetear BD si está corrupta (BACKUP PRIMERO):
cp "$APPDATA/com.maity.ai/maity.db" "maity.db.backup.$(date +%s)"
sqlite3 "$APPDATA/com.maity.ai/maity.db" "PRAGMA integrity_check;"
```

---

## 5. Crash / Panic en Cliente Desktop

**Indicadores**: Sentry recibe evento con `level: fatal`, usuario reporta crash.

```bash
# Ver eventos en Sentry
# Dashboard → Maity Desktop → Issues → filtrar por level:fatal

# Identificar si es panic de Rust o JS error
# Rust panic: "PANIC at frontend/src-tauri/src/..."
# JS error: "ChunkLoadError" o stack de React
```

### Crash por audio pipeline

Los panics de workers de transcripción ahora se reportan con `worker_id` (RUST-002).  
Buscar en Sentry: `Audio transcription worker N panicked`.

### Crash por modelo Whisper/Parakeet corrupto

```bash
# Borrar caché de modelos y recargar
# Windows:
rd /s /q "%APPDATA%\com.maity.ai\models"
# macOS:
rm -rf ~/Library/Application\ Support/com.maity.ai/models/
# Luego reiniciar la app → se descargará el modelo de nuevo
```

### Rollback a versión anterior

```bash
# Ver releases disponibles
gh release list --repo ponchovillalobos/maity_desktop-1 --limit 5

# Descargar instalador de versión anterior
gh release download v1.X.Y --pattern "Maity_*-setup.exe" \
  --repo ponchovillalobos/maity_desktop-1
```

---

## 6. Compromiso de Credenciales

### API keys de terceros (Deepgram, OpenAI, Anthropic, Groq)

1. **Revocar inmediatamente** en el dashboard del proveedor
2. Generar nueva key
3. Actualizar en GitHub Secrets (para CI/CD) y en Cloudflare Worker
4. Notificar a usuarios si la key era compartida (proxy)

### Signing key de código (DigiCert / Apple)

```
IMPACTO: CRÍTICO — builds firmados con esta key son de confianza para Windows/macOS
```

1. Contactar DigiCert/Apple para revocar el certificado
2. Generar nuevo certificado (`TAURI_SIGNING_PRIVATE_KEY`, `APPLE_CERTIFICATE`)
3. Actualizar GitHub Secrets
4. Emitir nuevo build firmado y notificar a usuarios que el anterior puede fallar actualizaciones

### JWT secret del Cloudflare Worker

```bash
# Rotar secret
npx wrangler secret put PROXY_JWT_SECRET
# (ingresar nuevo valor)
npx wrangler deploy  # redeploy para que tome efecto
```

---

## 7. Post-Mortem Template

```markdown
## Post-Mortem: [Título del Incidente]

**Fecha**: YYYY-MM-DD  
**Severidad**: SEV-X  
**Duración**: HH:MM  
**Responsable de la investigación**: @usuario  

### Resumen ejecutivo
_1-2 párrafos describiendo qué pasó y el impacto para los usuarios._

### Cronología
| Tiempo | Evento |
|---|---|
| HH:MM | Primer reporte / detección |
| HH:MM | Inicio de respuesta |
| HH:MM | Contención |
| HH:MM | Resolución |

### Causa raíz
_Describir la causa técnica sin buscar culpables._

### Impacto
- Usuarios afectados: N
- Tiempo de degradación: HH:MM
- Datos en riesgo: sí/no

### Qué salió bien
- ...

### Qué falló
- ...

### Acciones correctivas
| Acción | Responsable | Fecha límite | Issue |
|---|---|---|---|
| ... | @usuario | YYYY-MM-DD | #NNN |
```

---

## 8. Contactos y Escalado

| Rol | Contacto | Canal |
|---|---|---|
| On-call primario | engineering@maity.cloud | Email + Slack #incidents |
| Escalado CEO | contacto directo | Teléfono |
| Deepgram soporte | support@deepgram.com | Portal: console.deepgram.com/support |
| Supabase soporte | support@supabase.com | Portal: app.supabase.com/support |
| DigiCert (signing) | support.digicert.com | Portal |
| Apple Developer | developer.apple.com/contact | Portal |
| Sentry | sentry.io/support | Portal |

### Canales de comunicación durante incidentes

- **Slack #incidents**: coordinación en tiempo real
- **Status page**: *(pendiente — ver OPS-009)* → comunicación a usuarios
- **Email usuarios**: para breaches GDPR (plazo 72h)

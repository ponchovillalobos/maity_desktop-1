# LEG-002 — Two-Party Consent Flow

> Documento vivo. Última actualización: 2026-04-08.
> Objetivo: diseñar el flujo de consentimiento de grabación para cumplir con LFPDPPP (México), ley de California (Penal Code §632), Florida (§934.03) y otras jurisdicciones de consentimiento bilateral.

---

## 1. Contexto legal

Maity Desktop captura simultáneamente:
- Audio del **micrófono** del usuario (canal L)
- Audio del **sistema** (canal R), que incluye a los interlocutores en Zoom / Meet / Teams / llamadas

Esto es una **grabación de conversación** en términos legales. Varias jurisdicciones exigen consentimiento de **todas las partes**, no sólo del usuario que opera el software.

| Jurisdicción | Base legal | Requisito |
|---|---|---|
| México | LFPDPPP art. 8 + art. 16 | Consentimiento informado del titular de los datos personales (incluida la voz) |
| California, USA | Cal. Penal Code §632 | Two-party consent, confidencial communications |
| Florida, USA | Fla. Stat. §934.03 | Two-party consent |
| Illinois, Pennsylvania, Washington | Varias | Two-party consent |
| Unión Europea | GDPR art. 6(1)(a) | Consentimiento explícito |
| Chile, Argentina, Colombia | Leyes locales | Consentimiento recomendado |

**Posición de Maity**: operar siempre bajo el estándar más estricto (two-party / all-party), sin importar la jurisdicción del usuario. Es más simple, más defendible y alinea con la promesa de privacidad del producto.

---

## 2. UX flow

### 2.1 Primera vez que el usuario da clic en "Grabar"

1. Aparece **modal de consentimiento** (bloqueante, no se puede descartar con ESC).
2. Texto explica: qué se graba, quién debe estar de acuerdo, responsabilidad del usuario.
3. Dos botones:
   - **"He informado a los participantes y doy mi consentimiento"** (primario, habilitado)
   - **"Cancelar"** (secundario)
4. Checkbox opcional: **"No volver a mostrar en este dispositivo"**
5. Al aceptar → se registra en `consent_log` + se inicia grabación.
6. Al cancelar → no se graba, no se loguea nada.

### 2.2 Veces subsecuentes

- **Si NO marcó "no volver a mostrar"**: se muestra el modal cada vez. Default seguro.
- **Si marcó "no volver a mostrar"**: aparece un **banner top discreto** de 5 segundos durante los primeros 5 segundos de grabación, con texto corto: *"Grabación en curso. Verifica que los participantes hayan sido notificados."*
- El banner es **no-interactivo** pero visible; no bloquea.

### 2.3 Durante la grabación

- Indicador persistente en el system tray / title bar: **"● REC"** en rojo.
- Tooltip: *"Maity está grabando. Los participantes deben estar informados."*

### 2.4 Pausa y reanudación

- Al pausar → indicador cambia a **"❚❚ PAUSA"**.
- Al reanudar → banner de 3 segundos: *"Grabación reanudada"*, sin re-prompt de consentimiento.
- Si la pausa dura >15 min → al reanudar **sí** se re-prompta (sesión nueva a efectos prácticos).

### 2.5 Usuario que entra a media reunión

- Si el usuario **inicia Maity después** de que la reunión empezó → modal normal de consentimiento, advertencia extra: *"Eres responsable de informar a los participantes ya presentes que esta reunión ahora está siendo grabada."*

---

## 3. Texto del banner / modal (legal-grade)

### 3.1 Español (México / LATAM)

> **Consentimiento para grabar y transcribir**
>
> Estás por iniciar una grabación de audio que incluye tu micrófono y el audio del sistema (otros participantes de la reunión). Este audio se transcribe y se resume localmente en tu computadora, y —según tu configuración— puede enviarse a servicios de transcripción en la nube (Deepgram) bajo un proxy cifrado.
>
> **Al continuar, declaras que:**
>
> 1. Has **informado a todas las personas** presentes en la reunión de que la sesión será grabada y transcrita.
> 2. Has **obtenido su consentimiento** de forma previa, expresa e informada, conforme al artículo 8 de la Ley Federal de Protección de Datos Personales en Posesión de los Particulares (LFPDPPP) y la normativa aplicable en tu jurisdicción.
> 3. Asumes **responsabilidad exclusiva** sobre el uso, almacenamiento y compartición de la grabación, transcripción y resumen resultantes.
> 4. Entiendes que Maity Desktop actúa como **herramienta técnica** y no como responsable del tratamiento de los datos de terceros.
>
> Maity Desktop **no subirá el audio crudo a servidores propios** salvo los proveedores de transcripción que hayas habilitado en Configuración.
>
> ☐ No volver a mostrar este aviso en este dispositivo *(puedes reactivarlo en Configuración → Privacidad)*
>
> `[ Cancelar ]`    `[ He informado a los participantes y doy mi consentimiento ]`

### 3.2 English (US / international)

> **Recording & Transcription Consent**
>
> You are about to start recording audio that includes your microphone and your system audio (other meeting participants). This audio is transcribed and summarized locally on your computer, and — depending on your settings — may be sent to cloud transcription services (Deepgram) over an encrypted proxy.
>
> **By continuing, you acknowledge that:**
>
> 1. You have **informed all participants** in the meeting that the session will be recorded and transcribed.
> 2. You have **obtained their prior, express, and informed consent**, in accordance with applicable two-party / all-party consent laws in your jurisdiction (including but not limited to California Penal Code §632, Florida Statute §934.03, and the GDPR where applicable).
> 3. You assume **sole responsibility** for the use, storage, and sharing of the resulting recording, transcript, and summary.
> 4. You understand that Maity Desktop acts as a **technical tool** and is not a controller of third-party personal data.
>
> Maity Desktop **does not upload raw audio to its own servers**, except to the transcription providers you have enabled in Settings.
>
> ☐ Don't show this again on this device *(you can re-enable this in Settings → Privacy)*
>
> `[ Cancel ]`    `[ I have informed the participants and I consent ]`

### 3.3 Banner corto (recurring, 5s)

- **ES**: *"Grabación en curso. Asegúrate de haber notificado a los participantes."*
- **EN**: *"Recording in progress. Make sure participants have been notified."*

---

## 4. Setting para deshabilitar el prompt

**Ubicación**: Settings → Privacidad → "Mostrar aviso de consentimiento antes de grabar"

- **Default**: ON (mostrar siempre).
- **OFF**: el modal no aparece, pero:
  - El banner corto **siempre** aparece (no se puede deshabilitar).
  - El `consent_log` sigue registrando cada inicio de grabación con `method = "implicit_disabled_prompt"`.
  - Se muestra warning legal en la misma pantalla de Settings:
    > ⚠️ **Advertencia legal**: deshabilitar este aviso **no te exime de la obligación legal** de informar y obtener consentimiento de los participantes antes de grabar. En jurisdicciones de consentimiento bilateral (California, Florida, México bajo LFPDPPP, UE bajo GDPR), grabar sin consentimiento puede constituir delito o infracción administrativa. Maity Desktop no asume responsabilidad por el uso indebido de esta función.

---

## 5. Proof-of-consent: modelo de datos

### 5.1 Nueva tabla SQLite: `consent_log`

```sql
CREATE TABLE IF NOT EXISTS consent_log (
  id                     INTEGER PRIMARY KEY AUTOINCREMENT,
  meeting_id             TEXT    NOT NULL,
  user_id                TEXT    NOT NULL,
  consent_text_version   TEXT    NOT NULL,  -- ej "2026-04-08-v1"
  shown_at               TEXT    NOT NULL,  -- ISO 8601 UTC
  acknowledged_at        TEXT,              -- ISO 8601 UTC, NULL si canceló
  method                 TEXT    NOT NULL,  -- 'modal' | 'banner_only' | 'implicit_disabled_prompt'
  device_id              TEXT    NOT NULL,  -- fingerprint del dispositivo
  locale                 TEXT    NOT NULL,  -- 'es-MX', 'en-US', ...
  jurisdiction_hint      TEXT,              -- timezone/IP-derived, best-effort
  created_at             TEXT    NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_consent_log_meeting ON consent_log(meeting_id);
CREATE INDEX idx_consent_log_user ON consent_log(user_id);
```

### 5.2 Versionado del texto de consentimiento

- Cada cambio en el texto legal bumpea `consent_text_version` (formato `YYYY-MM-DD-vN`).
- El texto de cada versión vive en `backend/app/legal/consent_texts/{version}.md`.
- **Nunca** se edita una versión publicada — sólo se crean nuevas.
- Así, ante un reclamo legal, se puede reproducir **exactamente** qué vio el usuario.

### 5.3 Retención

- `consent_log` se mantiene mientras el `meeting_id` asociado exista.
- Si el usuario borra una reunión → el registro de consentimiento se conserva en `consent_log_archive` por 5 años (plazo de prescripción típico en LATAM).

---

## 6. Integration points

### 6.1 Backend (FastAPI)

- Nuevo endpoint `POST /api/consent` para persistencia remota opcional (si el usuario es enterprise y tiene backend sync activado).
- Payload idéntico a la fila SQLite.
- Respuesta: `{ "id": <int>, "persisted": true }`.
- **Fallback local**: si el backend está offline, se persiste sólo en SQLite local y se sincroniza al reconectar.

### 6.2 Tauri command

- Nuevo comando `log_consent` en `audio/recording_commands.rs` (o un módulo nuevo `legal/consent_commands.rs`).
- Firma: `log_consent(meeting_id: String, method: String, acknowledged: bool) -> Result<i64>`.
- Escribe en SQLite local, emite evento `consent-logged`.
- Bloquea el inicio de grabación hasta que devuelva OK.

### 6.3 Frontend

- Nuevo componente `ConsentModal.tsx` en `components/legal/`.
- Hook `useConsent()` que:
  1. Lee setting "mostrar aviso".
  2. Si ON → monta modal, espera respuesta.
  3. Llama `invoke('log_consent', ...)`.
  4. Devuelve promesa a `useRecordingStart`.
- Integración en `useRecordingStart.ts`: **antes** de llamar a `start_recording`, `await requestConsent()`.

---

## 7. Edge cases

| Caso | Comportamiento |
|---|---|
| Usuario cancela el modal | No se graba. Fila en `consent_log` con `acknowledged_at = NULL`, `method = 'modal'`. Útil para auditoría ("el usuario fue advertido y eligió no grabar"). |
| Usuario hace clic fuera del modal | El modal es bloqueante; no cierra. Debe presionar Cancelar o Aceptar. |
| Crash de la app durante el modal | Al reiniciar, no hay grabación huérfana. `consent_log` sin `acknowledged_at` queda como huella. |
| Pausa <15 min → reanudar | No se re-prompta. Se agrega fila con `method = 'resume_no_prompt'` apuntando al mismo `meeting_id`. |
| Pausa ≥15 min → reanudar | Se re-prompta como sesión nueva; misma `meeting_id`, nueva fila `method = 'modal'`. |
| Usuario desactivó el prompt + graba | Banner corto aparece. Fila con `method = 'implicit_disabled_prompt'`. |
| Primer arranque después de update que bumpeó `consent_text_version` | Se fuerza el modal aunque el usuario hubiera marcado "no mostrar" — debe re-consentir el nuevo texto. |
| Usuario en jurisdicción desconocida (VPN) | Se asume el estándar más estricto (all-party). Modal siempre. |
| Reunión grabada sin internet | Todo funciona local. `consent_log` en SQLite local. Sync al reconectar. |
| Multi-user en el mismo dispositivo (no soportado hoy, pero futuro) | `user_id` distingue; cada usuario consiente por separado. |
| Usuario reporta a soporte "yo no consentí" | Query `SELECT * FROM consent_log WHERE meeting_id = ? AND user_id = ?` da la prueba exacta, incluyendo versión del texto mostrado. |

---

## 8. Re-prompt policy

| Evento | ¿Re-prompt? |
|---|---|
| Nueva reunión (click en Grabar) | **Sí**, salvo que "no volver a mostrar" esté ON |
| Pausa <15 min + resume | No |
| Pausa ≥15 min + resume | Sí |
| Cambio de `consent_text_version` | **Sí, forzado** (override del setting) |
| Cambio de proveedor STT (local → cloud) en Settings | **Sí, forzado** en la próxima grabación — el usuario debe saber que ahora el audio sale a la nube |
| Nueva versión de la app con cambio legal material | **Sí, forzado** en el primer arranque post-update |
| Usuario cambia de cuenta Supabase | Sí (nuevo `user_id`) |

---

## 9. Acciones pendientes (no-go sin esto antes de GA)

1. [ ] Revisar texto legal con abogado LATAM (México) y counsel US (California/Florida).
2. [ ] Crear `backend/app/legal/consent_texts/2026-04-08-v1.md` con el texto final.
3. [ ] Implementar tabla `consent_log` y migration en `backend/app/db/`.
4. [ ] Implementar comando Tauri `log_consent`.
5. [ ] Implementar `ConsentModal.tsx` + hook `useConsent`.
6. [ ] Integrar en `useRecordingStart.ts`.
7. [ ] Agregar setting "Mostrar aviso de consentimiento" en Settings → Privacidad.
8. [ ] Agregar banner persistente de "● REC" en title bar.
9. [ ] Pruebas E2E del flujo completo (grabar, cancelar, re-grabar, pausa, resume).
10. [ ] Actualizar Terms of Service y Privacy Policy del producto para referenciar este flujo.

---

## 10. Referencias

- LFPDPPP (México): https://www.diputados.gob.mx/LeyesBiblio/pdf/LFPDPPP.pdf
- California Penal Code §632: https://leginfo.legislature.ca.gov/faces/codes_displaySection.xhtml?lawCode=PEN&sectionNum=632
- Florida Statute §934.03: http://www.leg.state.fl.us/Statutes/index.cfm?App_mode=Display_Statute&URL=0900-0999/0934/Sections/0934.03.html
- GDPR art. 6 & art. 7: https://gdpr.eu/article-6-how-to-process-personal-data-legally/
- Asamblea experto LEG: `scripts/assembly_data.json` → LEG-002

---

**Owner**: Legal + Producto.
**Próxima revisión**: antes del primer release GA (no piloto) y ante cualquier cambio legal material en LATAM o US.

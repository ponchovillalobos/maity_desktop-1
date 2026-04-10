# SOC 2 Type II Compliance Roadmap — Maity Desktop

> Estado actual: **Pre-certificación** · Actualizado: 2026-04-10

Este documento describe el camino de Maity hacia la certificación SOC 2 Type II,
los controles ya implementados y los gaps pendientes por Trust Service Criteria.

---

## Estado Resumen

| Criterio TSC | Estado | Cobertura estimada |
|---|---|---|
| CC1 — Control Environment | 🟡 Parcial | 40% |
| CC2 — Communication & Information | 🟡 Parcial | 50% |
| CC3 — Risk Assessment | 🔴 Pendiente | 15% |
| CC4 — Monitoring Activities | 🟡 Parcial | 35% |
| CC5 — Control Activities | 🟡 Parcial | 45% |
| CC6 — Logical Access | 🟡 Parcial | 60% |
| CC7 — System Operations | 🟡 Parcial | 40% |
| CC8 — Change Management | 🟢 Mayormente completo | 75% |
| CC9 — Risk Mitigation | 🔴 Pendiente | 20% |
| A1 — Availability | 🟡 Parcial | 30% |
| C1 — Confidentiality | 🟡 Parcial | 55% |
| P — Privacy | 🟡 Parcial | 50% |

---

## Controles Ya Implementados

### Seguridad lógica (CC6)
- ✅ Autenticación Google OAuth via Supabase (MFA disponible)
- ✅ JWT con TTL de 5 min para proxy Deepgram (nunca expone API key al cliente)
- ✅ CORS restringido a orígenes Tauri conocidos (`com.maity.ai`)
- ✅ API keys almacenadas en SQLite local cifrado (no en código)

### Gestión de cambios (CC8)
- ✅ CI/CD con GitHub Actions (build-windows, build-macos, build-linux)
- ✅ Firma de código: DigiCert HSM (Windows), Apple notarización (macOS)
- ✅ Rama protegida `main` con revisión de PR requerida
- ✅ Versionado semver con tags firmados

### Criptografía
- ✅ TLS 1.2+ para todas las conexiones externas (Deepgram, Supabase, Sentry)
- ✅ SQLite WAL mode para integridad de datos

### Observabilidad (CC7)
- ✅ Sentry crash reporting con panic hook en main.rs
- ✅ Logs estructurados con tracing (Rust) y logging (Python)
- ✅ Health endpoint en portal de métricas interno

### Privacidad (P)
- ✅ `PRIVACY_POLICY.md` actualizada para reflejar uso de Deepgram/OpenAI
- ✅ Opción local-first (Parakeet/Whisper) sin transmisión a terceros
- ✅ Consent toggle de analytics (PostHog opt-in)

---

## Gaps Pendientes (Por Prioridad)

### Alta prioridad — Blocker para contrato enterprise

| Gap | Hallazgo | Descripción | Esfuerzo |
|---|---|---|---|
| DPA template | LEG-008 | Sin Data Processing Agreement firmable disponible | 4h |
| Política de retención | LEG-003 | Transcripts y audio acumulan indefinidamente, sin purga GDPR | 2d |
| Vendor questionnaire | BIZ-007 | Sin SIG/CAIQ pre-llenado para due-diligence de compradores | 4h |
| Audit log | SEC-009 | Sin log de acceso inmutable (quién vio qué reunión, cuándo) | 3d |
| RBAC | SEC-008 | Sin roles (admin/member/viewer) para equipos enterprise | 1 semana |

### Media prioridad — Requerido para SOC 2 Type I

| Gap | Hallazgo | Descripción | Esfuerzo |
|---|---|---|---|
| Incident response plan | OPS-010 | Sin runbook para breach/outage | 1d |
| Vulnerability disclosure | — | Sin política de responsible disclosure | 2h |
| Penetration test | — | Sin pen-test externo documentado | 2 semanas |
| Background checks | — | Sin proceso documentado para empleados con acceso a datos | — |
| Business continuity | — | Sin BCP/DR documentado | 1d |

### Baja prioridad — Type II evidence

| Gap | Descripción | Esfuerzo |
|---|---|---|
| User access reviews | Revisión trimestral de accesos activos | 4h/trimestre |
| Vendor risk management | Evaluación de Deepgram/OpenAI/Supabase como sub-processors | 1d |
| Security training | Evidencia de training anual para el equipo | 4h |
| Encryption key rotation | Procedimiento documentado para rotation de JWT secrets | 2h |

---

## Hoja de Ruta Propuesta

### Fase 1 — Readiness (Q2 2026) · ~6 semanas
```
Semana 1-2:  LEG-008 DPA + BIZ-007 vendor questionnaire (markdown)
Semana 2-3:  LEG-003 política de retención (SQLite cleanup job 90 días)
Semana 3-4:  OPS-010 incident response runbook
Semana 4-5:  Audit log básico (tabla SQLite append-only)
Semana 5-6:  Gap assessment por auditor externo (Vanta/Drata recomendado)
```

### Fase 2 — Type I (Q3 2026) · ~8 semanas
```
Pen-test externo + remediación de hallazgos críticos
RBAC básico (admin/member)
Políticas formales documentadas y firmadas
Type I report con auditor acreditado (AICPA)
```

### Fase 3 — Type II (Q4 2026 — Q1 2027) · 6 meses de observación
```
Período de observación de 6 meses con controles activos
Evidence collection automatizada (Vanta/Drata)
Type II report (cubre el período de observación)
```

---

## Herramientas Recomendadas

| Herramienta | Uso | Precio estimado |
|---|---|---|
| **Vanta** | Automatización de compliance + evidence collection | ~$12k/año |
| **Drata** | Alternativa a Vanta, mejor UI | ~$10k/año |
| **Tugboat Logic** | Más barato, menos automatización | ~$6k/año |
| **Manual** | Solo para startups <5 personas | ~$25k audit |

Recomendación: **Vanta** desde Fase 1 para maximizar automatización de evidencia.

---

## Sub-processors con Acceso a Datos de Usuario

Ver `docs/SUBPROCESSORS.md` para la lista completa. Procesadores relevantes para SOC 2:

| Proveedor | Tipo | Datos procesados | DPA disponible |
|---|---|---|---|
| Deepgram | STT cloud | Audio de reuniones | Sí — [deepgram.com/dpa](https://deepgram.com/dpa) |
| Supabase | Auth | Email, OAuth tokens | Sí (GDPR-compliant) |
| Sentry | Crash reporting | Stack traces, device info | Sí |
| PostHog | Analytics | Eventos de uso (opt-in) | Sí (EU hosting disponible) |

---

## Contacto

Para preguntas sobre el programa de compliance:
**security@maity.cloud** · [PRIVACY_POLICY.md](../PRIVACY_POLICY.md) · [SUBPROCESSORS.md](SUBPROCESSORS.md)

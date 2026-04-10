# BIZ-003 — Unit Economics (Maity Desktop)

> Documento vivo. Última actualización: 2026-04-08.
> Objetivo: entender costo marginal por usuario, margen bruto por tier, y gatillos de pérdida.

---

## 1. Supuestos base

| Parámetro | Valor | Nota |
|---|---|---|
| Duración promedio de reunión | 45 min | Observado en pilotos LATAM enterprise |
| Reuniones por usuario por mes | 20 | Perfil mid-market (PM / ventas / ops) |
| Minutos de audio / usuario / mes | **900 min** | 45 × 20 |
| Tokens LLM promedio por resumen | ~8k input / ~1.5k output | Transcript 45 min ≈ 6–10k tokens |
| Resúmenes generados / mes / usuario | 20 | 1 por reunión |
| Almacenamiento audio / usuario / mes | 0 GB cloud | SQLite local, audio queda en disco del usuario |
| Tasa de retries por fallos de red | 5% | Reconexiones WS Deepgram |

---

## 2. Costos unitarios de proveedores (2026-Q1)

### 2.1 STT (Speech-to-Text)

| Proveedor | Precio | Costo / 900 min |
|---|---|---|
| Deepgram Nova-3 | $0.0043 / min | **$3.87** |
| OpenAI Whisper API | $0.006 / min | $5.40 |
| Parakeet (local, ONNX) | $0 compute | $0 + requiere +3 GB RAM en máquina del usuario |
| Whisper.cpp (local, GPU) | $0 compute | $0 + modelo ya bundleado |

**Default de producto**: Deepgram Nova-3 (`es-419`) vía Cloudflare Worker proxy.

### 2.2 LLM para resúmenes

Asumiendo 20 resúmenes/mes × (8k in + 1.5k out) = **160k tokens input + 30k tokens output**.

| Proveedor | Input ($/1M) | Output ($/1M) | Costo / usuario / mes |
|---|---|---|---|
| OpenAI gpt-4o-mini | $0.15 | $0.60 | **$0.024 + $0.018 = $0.042** |
| Claude Haiku 3.5 | $0.80 | $4.00 | $0.128 + $0.120 = **$0.248** |
| Ollama local (llama3.1) | $0 | $0 | $0 (usuario paga en RAM/CPU) |
| Groq llama-3.3-70b | $0.59 | $0.79 | $0.094 + $0.024 = $0.118 |

**Default recomendado (cloud)**: gpt-4o-mini para el tier económico; Haiku para el tier premium donde el usuario pide "mejor calidad".

### 2.3 Infraestructura compartida

| Ítem | Costo mensual (total empresa) | Costo / usuario (asumiendo 500 users) |
|---|---|---|
| Supabase (auth + metadata, hobby/pro) | $25 | $0.05 |
| Cloudflare Worker (proxy Deepgram) | ~$5 + $0.50/1M req | ~$0.02 |
| Vercel (landing + API proxy config) | $20 | $0.04 |
| Dominio + email transaccional | $15 | $0.03 |
| **Subtotal infra compartida** | | **~$0.14 / usuario / mes** |

### 2.4 Almacenamiento y ancho de banda

| Ítem | Costo |
|---|---|
| SQLite local (transcripts + summaries) | $0 — vive en el disco del usuario |
| Audio raw (.wav/.m4a) | $0 — no se sube a cloud por defecto |
| Bandwidth Deepgram WS (≈1 Mbps × 45 min × 20) | Incluido en el precio por minuto |
| Bandwidth LLM API | Despreciable (<$0.001) |

---

## 3. Costo total marginal por usuario (cloud-hosted)

### Escenario A — Deepgram + gpt-4o-mini (default económico)

| Componente | Costo |
|---|---|
| Deepgram Nova-3 (900 min) | $3.87 |
| gpt-4o-mini (20 resúmenes) | $0.042 |
| Retries (5%) | $0.20 |
| Infra compartida prorrateada | $0.14 |
| **TOTAL** | **~$4.25 / usuario / mes** |

### Escenario B — Deepgram + Claude Haiku (premium)

| Componente | Costo |
|---|---|
| Deepgram Nova-3 | $3.87 |
| Claude Haiku | $0.25 |
| Retries | $0.20 |
| Infra | $0.14 |
| **TOTAL** | **~$4.46 / usuario / mes** |

### Escenario C — Local full (Parakeet + Ollama, BYOK / self-hosted)

| Componente | Costo |
|---|---|
| STT local | $0 |
| LLM local | $0 |
| Infra compartida (auth, updates) | $0.14 |
| **TOTAL** | **~$0.14 / usuario / mes** |

---

## 4. Márgenes por tier de pricing propuesto

| Tier | Precio / mes | Costo (Esc. A) | Margen bruto $ | Margen bruto % |
|---|---|---|---|---|
| **Starter** (cloud, caps estrictos) | $19 | $4.25 | $14.75 | 77.6% |
| **Pro** (cloud, más minutos) | $49 | $6.50 (45 reuniones/mo) | $42.50 | 86.7% |
| **Business** (cloud + equipos) | $149 / 3 users | $12.75 (3 × $4.25) | $136.25 | 91.4% |
| **Local / BYOK** | $19 flat | $0.14 | $18.86 | **99.3%** |

**Nota**: los márgenes no incluyen CAC, soporte, desarrollo, ni infra baseline. Sirven solo como **margen de contribución**.

---

## 5. Break-even y punto de dolor

### BYOK (usuario trae su propia Deepgram / OpenAI key)

- Costo residual empresa: **$0.14 / user / mes** (solo infra compartida)
- Break-even a $19/mes: se alcanza en el **minuto 1** — el usuario BYOK es rentable desde la primera reunión.
- **Regla**: cualquier tier BYOK con precio ≥ $5/mes es rentable incluso con soporte.

### Cloud-hosted Starter ($19/mes, Esc. A)

- Costo fijo mensual: $0.14 infra
- Costo variable: ~$0.0043/min Deepgram + ~$0.002/resumen LLM ≈ **~$0.0047/min de reunión**
- Break-even en minutos: `($19 - $0.14) / $0.0047 ≈ **4,012 min/mes**` → ≈ **89 reuniones de 45 min**
- **Margen se evapora si un usuario pasa de ~4,000 min/mes**. Con el supuesto de 900 min/mes queda holgura de 4.5×.

---

## 6. Riesgos — cost explosions

| Riesgo | Impacto | Mitigación |
|---|---|---|
| Reuniones maratónicas (>4h) | 1 sesión = $1+ en STT | Hard cap 3h por sesión, warning a 2h |
| Usuario deja micrófono abierto todo el día | $6-10/día en Deepgram | Auto-pause si VAD=0 por >10 min |
| Retries descontrolados (red inestable) | Multiplicador 2-5× en STT | Backoff exponencial + cap de 3 retries |
| Alucinaciones del LLM → re-generación | 2× tokens LLM | Cache de resumen por transcript hash |
| Resumen de transcripts largos con gpt-4 (no mini) | 10× tokens | Forzar gpt-4o-mini en Starter, sin override |
| Deepgram cambia pricing (+30%) | Margen Starter cae a ~65% | Cláusula contractual anual + fallback a Whisper.cpp local |
| Abuso (bots, cuentas compartidas) | Ilimitado | Rate limit por device_id + fingerprint |

---

## 7. Recomendaciones

### 7.1 Soft caps (warning al usuario)
- **Starter**: 1,500 min/mes (33 reuniones). Banner amarillo a 80%.
- **Pro**: 3,000 min/mes. Banner amarillo a 80%.
- **Business**: 3,000 min/user/mes, pool compartido por equipo.

### 7.2 Hard caps (bloqueo)
- **Starter**: 2,000 min/mes → transcripción se detiene, resumen queda disponible.
- **Pro**: 4,500 min/mes.
- **Business**: negociable por contrato.
- **Sesión individual**: 3h, hard stop con guardado de emergencia.

### 7.3 BYOK vs Cloud-hosted
- **BYOK** = precio flat barato ($19/mes o $190/año) + el cliente paga su propia API key → margen 99%+, cero riesgo de cost explosion.
- **Cloud-hosted** = precio por minutos con caps claros → mayor conveniencia, margen 77-91%.
- **Enterprise/Business** = siempre BYOK con contrato de soporte + SLA, la empresa cliente controla su gasto Deepgram/OpenAI.

### 7.4 Guardrails de producto (no-go sin esto en v1)
1. Hard cap de 3h por sesión grabada.
2. Auto-pause por silencio prolongado (>10 min sin VAD).
3. Dashboard de consumo de minutos visible al usuario en tiempo real.
4. Alerta por email al owner del workspace a 80% y 100% del cap.
5. Cache de resúmenes (mismo transcript → no re-llamar LLM).

---

## 8. Referencias

- Deepgram pricing: https://deepgram.com/pricing (verificado 2026-Q1)
- OpenAI pricing: https://openai.com/api/pricing
- Anthropic pricing: https://www.anthropic.com/pricing
- Supabase pricing: https://supabase.com/pricing
- Asamblea experto BIZ: `scripts/assembly_data.json` → BIZ-003

---

**Owner**: Equipo Producto + Finanzas.
**Próxima revisión**: al cierre del primer piloto pago (ETA Q2 2026) o ante cualquier cambio >10% en pricing de proveedores.

"""
templates.py — Plantillas de prompt por idioma para el summarizer de Maity.

LLM-004: el prompt anterior era 100% inglés para reuniones en español.
Ahora se detecta el idioma del chunk y se elige la plantilla correcta.

Idiomas soportados: "es" (español, predeterminado), "en" (inglés).
Añadir otros idiomas: agregar entrada en _PROMPTS y palabras en _ES_WORDS/_EN_WORDS.
"""
from __future__ import annotations

# ─────────────────────────────────────────────────────────────────────────────
# Detección de idioma (heurística de palabras función)
# ─────────────────────────────────────────────────────────────────────────────

# Palabras función de alta frecuencia únicas de cada idioma.
# Suficientes para diferenciar español de inglés en conversaciones de negocio.
_ES_WORDS = frozenset([
    "de", "la", "el", "en", "que", "y", "los", "se", "del", "las",
    "un", "una", "por", "con", "no", "una", "su", "para", "es", "al",
    "lo", "como", "más", "pero", "sus", "le", "ya", "o", "fue", "si",
    "sobre", "este", "entre", "cuando", "muy", "sin", "sobre", "también",
    "me", "hasta", "hay", "donde", "quien", "desde", "todo", "nos",
    "durante", "estados", "todos", "uno", "les", "ni", "contra",
    "ese", "eso", "ante", "ellos", "e", "esto", "mí", "antes",
    "algunos", "qué", "unos", "yo", "otro", "otras", "otra", "él",
    "tanto", "esa", "estos", "mucho", "quienes", "nada", "muchos",
    "cual", "poco", "ella", "estar", "estas", "algún", "algo",
])

_EN_WORDS = frozenset([
    "the", "be", "to", "of", "and", "a", "in", "that", "have", "it",
    "for", "not", "on", "with", "he", "as", "you", "do", "at", "this",
    "but", "his", "by", "from", "they", "we", "say", "her", "she", "or",
    "an", "will", "my", "one", "all", "would", "there", "their", "what",
    "so", "up", "out", "if", "about", "who", "get", "which", "go", "me",
    "when", "make", "can", "like", "time", "no", "just", "him", "know",
    "take", "people", "into", "year", "your", "good", "some", "could",
    "them", "see", "other", "than", "then", "now", "look", "only", "come",
    "its", "over", "think", "also", "back", "after", "use", "two", "how",
    "our", "work", "first", "well", "way", "even", "new", "want", "because",
    "any", "these", "give", "day", "most", "us",
])


def detect_lang(text: str, default: str = "es") -> str:
    """Detecta si el texto es mayoritariamente español ("es") o inglés ("en").

    Usa conteo de palabras función de alta frecuencia. Rápido y sin deps.
    Si el resultado es ambiguo, devuelve `default` (predeterminado: "es",
    porque Maity está orientado al mercado LATAM).

    Args:
        text:    Fragmento de transcripción a analizar.
        default: Idioma a devolver si no hay señal clara.

    Returns:
        "es" o "en".
    """
    words = text.lower().split()
    if not words:
        return default

    es_score = sum(1 for w in words if w.rstrip(".,;:!?") in _ES_WORDS)
    en_score = sum(1 for w in words if w.rstrip(".,;:!?") in _EN_WORDS)

    total = es_score + en_score
    if total == 0:
        return default

    # Requiere ventaja del 20% para cambiar del default
    ratio = es_score / total
    if ratio >= 0.4:
        return "es"
    elif ratio <= 0.25:
        return "en"
    return default


# ─────────────────────────────────────────────────────────────────────────────
# Plantillas
# ─────────────────────────────────────────────────────────────────────────────

_PROMPTS: dict[str, str] = {
    # ── Español ──────────────────────────────────────────────────────────────
    "es": """\
Dado el siguiente fragmento de transcripción de una reunión de negocios en español,
extrae la información relevante según la estructura JSON requerida.

REGLAS:
- Si una sección (por ejemplo, "Tareas críticas" o "Plazos") no tiene información
  en este fragmento, devuelve una lista vacía para su campo 'blocks'.
- El resultado debe ser ÚNICAMENTE el JSON; sin explicaciones ni texto adicional.
- Tipos de bloque permitidos: 'text', 'bullet', 'heading1', 'heading2'.
  · 'text'     → párrafo de texto normal.
  · 'bullet'   → elemento de lista con viñeta.
  · 'heading1' → encabezado principal de sección.
  · 'heading2' → sub-encabezado.
- Para el campo 'color': usa 'gray' para contenido de menor importancia o '' (cadena
  vacía) para el color predeterminado.
- Corrige errores ortográficos obvios del transcriptor sin cambiar el significado.
- Presta atención especial a: nombres propios en español, fechas, responsables de
  tareas y compromisos concretos.

Fragmento de transcripción:
---
{chunk}
---

{custom_section}
Asegúrate de que la salida sea únicamente el JSON.\
""",

    # ── English ───────────────────────────────────────────────────────────────
    "en": """\
Given the following meeting transcript chunk, extract the relevant information
according to the required JSON structure.

RULES:
- If a specific section (e.g. Critical Deadlines) has no relevant information in
  this chunk, return an empty list for its 'blocks'.
- Output ONLY the JSON data; no explanations or additional text.
- Block types must be one of: 'text', 'bullet', 'heading1', 'heading2'.
  · 'text'     → regular text paragraph.
  · 'bullet'   → bulleted list item.
  · 'heading1' → major section heading.
  · 'heading2' → sub-heading.
- For the 'color' field: use 'gray' for less important content or '' (empty string)
  for the default color.
- Correct obvious transcription spelling mistakes without changing meaning.
- Pay special attention to: proper nouns, dates, owners of action items, and
  concrete commitments.

Transcript Chunk:
---
{chunk}
---

{custom_section}
Make sure the output is only the JSON data.\
""",
}

_CUSTOM_ES = """\
Contexto adicional proporcionado por el usuario:
---
{custom_prompt}
---"""

_CUSTOM_EN = """\
Additional context provided by the user:
---
{custom_prompt}
---"""


def build_prompt(lang: str, chunk: str, custom_prompt: str = "") -> str:
    """Construye el prompt localizado para el LLM summarizer.

    Args:
        lang:          Código de idioma ("es" | "en"). Fallback a "es".
        chunk:         Fragmento de transcripción a resumir.
        custom_prompt: Contexto extra del usuario (puede estar vacío).

    Returns:
        String listo para pasar al agente LLM.
    """
    template = _PROMPTS.get(lang, _PROMPTS["es"])
    custom_tpl = _CUSTOM_ES if lang == "es" else _CUSTOM_EN

    custom_section = (
        custom_tpl.format(custom_prompt=custom_prompt.strip())
        if custom_prompt and custom_prompt.strip()
        else ""
    )

    return template.format(chunk=chunk, custom_section=custom_section)

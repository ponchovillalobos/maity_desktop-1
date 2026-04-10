"""
Plantillas de prompt localizadas para el procesador de transcripciones.

LLM-004: los prompts estaban enteramente en inglés aunque las reuniones
sean en español, lo que degradaba la extracción de Action Items y nombres
propios en español.

Uso:
    from prompts import build_prompt, detect_lang

    lang = detect_lang(transcript_chunk)          # "es" | "en"
    prompt = build_prompt(lang, chunk, custom)
"""
from .templates import build_prompt, detect_lang

__all__ = ["build_prompt", "detect_lang"]

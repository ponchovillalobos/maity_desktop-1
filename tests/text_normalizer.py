"""
Shared text normalizer for WER/CER evaluation of Spanish STT.

Designed to match the normalization strategy used by the HuggingFace
Open ASR Leaderboard for non-English languages (derived from
`whisper.normalizers.BasicTextNormalizer`) while adding Spanish-aware
options (optional accent folding, inverted punctuation, number->word).

Usage:
    from text_normalizer import normalize_text, default_normalizer

    ref = normalize_text("¿Cuántos años tienes? Tengo 32.")
    hyp = normalize_text("cuantos anos tienes tengo treinta y dos")

The normalizer is intentionally pure/deterministic so hypothesis and
reference can be piped through the exact same transform before WER/CER
is computed.
"""
from __future__ import annotations

import re
import unicodedata
from dataclasses import dataclass
from typing import Iterable

# Whisper boilerplate / hallucination phrases commonly emitted on silence.
# Keep lowercase, already-normalized form (no punctuation, no accents).
WHISPER_BOILERPLATE: tuple[str, ...] = (
    "subtitulos realizados por la comunidad de amara org",
    "subtitulos realizados por la comunidad de amaraorg",
    "subtitulado por la comunidad de amara org",
    "subtitulos por la comunidad de amara org",
    "mas informacion en www mquiles com",
    "gracias por ver el video",
    "gracias por ver este video",
    "suscribete al canal",
    "no olvides suscribirte",
    "thanks for watching",
    "thank you for watching",
    "please subscribe",
    "music",
    "musica",
    "aplausos",
    "risas",
)

# Unicode punctuation class we want to strip.  Includes Spanish inverted
# marks and common typographic quotes.
_PUNCT_RE = re.compile(
    r"[.,;:!?¿¡\"'`´’‘“”«»\(\)\[\]\{\}\-–—_/\\\*\|~<>@#\$%\^&\+=]"
)
_WS_RE = re.compile(r"\s+")

# Basic Spanish number word map (0-20 + tens).  For anything larger we
# fall back to `num2words` if available; otherwise we leave the digit
# in place (documented limitation).
_SPANISH_SMALL_NUMBERS: dict[int, str] = {
    0: "cero", 1: "uno", 2: "dos", 3: "tres", 4: "cuatro",
    5: "cinco", 6: "seis", 7: "siete", 8: "ocho", 9: "nueve",
    10: "diez", 11: "once", 12: "doce", 13: "trece", 14: "catorce",
    15: "quince", 16: "dieciseis", 17: "diecisiete", 18: "dieciocho",
    19: "diecinueve", 20: "veinte", 30: "treinta", 40: "cuarenta",
    50: "cincuenta", 60: "sesenta", 70: "setenta", 80: "ochenta",
    90: "noventa", 100: "cien",
}

try:  # optional
    from num2words import num2words as _num2words  # type: ignore
    _HAS_NUM2WORDS = True
except Exception:  # pragma: no cover
    _HAS_NUM2WORDS = False


def _number_to_spanish_words(token: str) -> str:
    try:
        n = int(token)
    except ValueError:
        return token
    if _HAS_NUM2WORDS:
        try:
            return _num2words(n, lang="es")
        except Exception:
            pass
    if n in _SPANISH_SMALL_NUMBERS:
        return _SPANISH_SMALL_NUMBERS[n]
    # Compose 21-99 from tens + "y" + units (e.g. 32 -> treinta y dos)
    if 21 <= n <= 99:
        tens = (n // 10) * 10
        units = n % 10
        if tens in _SPANISH_SMALL_NUMBERS and units in _SPANISH_SMALL_NUMBERS:
            if units == 0:
                return _SPANISH_SMALL_NUMBERS[tens]
            return f"{_SPANISH_SMALL_NUMBERS[tens]} y {_SPANISH_SMALL_NUMBERS[units]}"
    return token  # leave untouched; counts as its own token


_NUMBER_RE = re.compile(r"\d+")


@dataclass(frozen=True)
class NormalizerConfig:
    lowercase: bool = True
    strip_accents: bool = True          # á->a, ñ->n, etc.
    strip_punctuation: bool = True
    numbers_to_words: bool = True
    remove_boilerplate: bool = True
    collapse_whitespace: bool = True


def _strip_accents(text: str) -> str:
    # NFKD decomposes accents; drop combining marks.  Keep base letters.
    nfkd = unicodedata.normalize("NFKD", text)
    return "".join(ch for ch in nfkd if not unicodedata.combining(ch))


def _remove_boilerplate(text: str, phrases: Iterable[str]) -> str:
    # Text is expected already-normalized (lowercase, no punct).
    for phrase in phrases:
        if phrase and phrase in text:
            text = text.replace(phrase, " ")
    return text


def normalize_text(text: str, config: NormalizerConfig | None = None) -> str:
    """Normalize `text` for WER/CER comparison.

    Applies (in order):
      1. Unicode NFKC normalization
      2. Lowercase
      3. Accent stripping (optional)
      4. Punctuation stripping
      5. Digit -> Spanish word expansion
      6. Whisper boilerplate removal
      7. Whitespace collapsing + trim
    """
    if text is None:
        return ""
    cfg = config or NormalizerConfig()
    t = unicodedata.normalize("NFKC", text)
    if cfg.lowercase:
        t = t.lower()
    if cfg.strip_accents:
        t = _strip_accents(t)
    if cfg.strip_punctuation:
        t = _PUNCT_RE.sub(" ", t)
    if cfg.numbers_to_words:
        t = _NUMBER_RE.sub(lambda m: " " + _number_to_spanish_words(m.group(0)) + " ", t)
    if cfg.collapse_whitespace:
        t = _WS_RE.sub(" ", t).strip()
    if cfg.remove_boilerplate:
        t = _remove_boilerplate(t, WHISPER_BOILERPLATE)
        if cfg.collapse_whitespace:
            t = _WS_RE.sub(" ", t).strip()
    return t


def default_normalizer(text: str) -> str:
    return normalize_text(text, NormalizerConfig())


def count_hallucinations(text: str) -> int:
    """Return how many blacklisted boilerplate phrases appear in `text`.

    Text is normalized first with boilerplate removal *disabled*, so the
    phrases remain visible for counting.
    """
    probe_cfg = NormalizerConfig(remove_boilerplate=False)
    probed = normalize_text(text, probe_cfg)
    return sum(1 for p in WHISPER_BOILERPLATE if p and p in probed)

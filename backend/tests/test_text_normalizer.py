"""Unit tests for tests/text_normalizer.py.

These tests document and lock down the normalization contract used by
the WER/CER benchmark harness.  If any of these change, the reported
WER numbers will shift, so bumping the schema_version in
benchmark_transcription.py is expected.
"""
from __future__ import annotations

import sys
from pathlib import Path

# Make tests/text_normalizer.py importable from backend/tests/
_REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(_REPO / "tests"))

import pytest  # noqa: E402

from text_normalizer import (  # noqa: E402
    NormalizerConfig,
    count_hallucinations,
    normalize_text,
)


# ---------------------------------------------------------------------------
# Accents
# ---------------------------------------------------------------------------

def test_strips_accents_by_default():
    assert normalize_text("Canción") == "cancion"


def test_strips_multiple_accents():
    assert normalize_text("áéíóúÁÉÍÓÚñÑ") == "aeiouaeiounn"


def test_keep_accents_flag():
    cfg = NormalizerConfig(strip_accents=False)
    assert normalize_text("canción", cfg) == "canción"


# ---------------------------------------------------------------------------
# Punctuation (including Spanish inverted marks)
# ---------------------------------------------------------------------------

def test_strips_spanish_inverted_punctuation():
    assert normalize_text("¿Cómo estás?") == "como estas"


def test_strips_exclamations_and_commas():
    assert normalize_text("¡Hola, mundo!") == "hola mundo"


def test_strips_typographic_quotes():
    assert normalize_text("“Buenos días”, dijo.") == "buenos dias dijo"


# ---------------------------------------------------------------------------
# Numbers
# ---------------------------------------------------------------------------

def test_small_number_expansion():
    # 5 -> cinco (covered by built-in map even without num2words)
    out = normalize_text("Tengo 5 perros")
    assert "cinco" in out
    assert "5" not in out


def test_twenty_expansion():
    out = normalize_text("Son las 20")
    assert "veinte" in out


def test_number_in_sentence_preserves_other_words():
    out = normalize_text("La reunión es a las 3 de la tarde")
    assert "tres" in out
    assert "reunion" in out  # accent stripped
    assert "tarde" in out


# ---------------------------------------------------------------------------
# Whitespace
# ---------------------------------------------------------------------------

def test_collapses_whitespace():
    assert normalize_text("hola    mundo\n\t  adios") == "hola mundo adios"


def test_empty_and_none_safe():
    assert normalize_text("") == ""
    assert normalize_text(None) == ""  # type: ignore[arg-type]


# ---------------------------------------------------------------------------
# Whisper boilerplate removal
# ---------------------------------------------------------------------------

def test_removes_amara_boilerplate():
    raw = "Hola, esto es una prueba. Subtítulos realizados por la comunidad de Amara.org"
    out = normalize_text(raw)
    assert "amara" not in out
    assert "hola" in out
    assert "prueba" in out


def test_removes_thanks_for_watching():
    out = normalize_text("Gracias por ver el video. Suscríbete al canal.")
    # Both boilerplate phrases are stripped → only residual words remain
    assert "gracias" not in out
    assert "suscribete" not in out


def test_boilerplate_flag_disabled_keeps_text():
    cfg = NormalizerConfig(remove_boilerplate=False)
    out = normalize_text("Gracias por ver el video", cfg)
    assert "gracias por ver el video" in out


# ---------------------------------------------------------------------------
# Hallucination counter
# ---------------------------------------------------------------------------

def test_count_hallucinations_detects_amara():
    assert count_hallucinations(
        "Subtítulos realizados por la comunidad de Amara.org"
    ) >= 1


def test_count_hallucinations_zero_for_clean_text():
    assert count_hallucinations("Buenos días, vamos a comenzar la reunión.") == 0


# ---------------------------------------------------------------------------
# Combined / integration
# ---------------------------------------------------------------------------

@pytest.mark.parametrize(
    "raw,expected_contains",
    [
        ("¿Cuántos años tienes? Tengo 32.", ["cuantos", "anos", "tienes"]),
        ("El presupuesto es de 100 pesos.", ["presupuesto", "cien", "pesos"]),
        ("María, ¿puedes repetir?", ["maria", "puedes", "repetir"]),
    ],
)
def test_realistic_sentences(raw: str, expected_contains: list[str]):
    out = normalize_text(raw)
    for token in expected_contains:
        assert token in out, f"expected {token!r} in {out!r}"
    # No punctuation should survive
    for ch in ".,;:!?¿¡":
        assert ch not in out


def test_deterministic():
    s = "¿Hola? ¡Son las 3!"
    assert normalize_text(s) == normalize_text(s)

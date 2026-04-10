"""LLM-001: tests del cap de tokens previo a invocación de APIs LLM."""
import os
import sys

import pytest

# Asegurar que `app` esté en path desde cualquier cwd de test
ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
if ROOT not in sys.path:
    sys.path.insert(0, ROOT)

from app.transcript_processor import TranscriptProcessor  # noqa: E402


class TestTokenEstimation:
    def test_empty_text_returns_zero(self):
        assert TranscriptProcessor.estimate_tokens("") == 0

    def test_short_text(self):
        # 4 chars ≈ 1 token
        assert TranscriptProcessor.estimate_tokens("abcd") == 1

    def test_long_text_proportional(self):
        text = "x" * 4000
        # 4000 / 4 = 1000
        assert TranscriptProcessor.estimate_tokens(text) == 1000


class TestEnforceTokenCap:
    def test_under_cap_passes(self):
        text = "x" * 100  # ~25 tokens
        n = TranscriptProcessor.enforce_token_cap(text)
        assert n > 0
        assert n < TranscriptProcessor.LLM_MAX_INPUT_TOKENS

    def test_over_cap_raises(self, monkeypatch):
        # Simulamos un cap bajo para el test
        monkeypatch.setattr(TranscriptProcessor, "LLM_MAX_INPUT_TOKENS", 10)
        with pytest.raises(ValueError, match="LLM-001"):
            TranscriptProcessor.enforce_token_cap("x" * 1000)

    def test_includes_custom_prompt_in_cap(self, monkeypatch):
        monkeypatch.setattr(TranscriptProcessor, "LLM_MAX_INPUT_TOKENS", 50)
        # transcript solo = 40 tokens OK; + prompt 40 tokens = 80 > 50 => fail
        text = "x" * 160  # 40 tokens
        prompt = "y" * 160  # 40 tokens
        with pytest.raises(ValueError, match="LLM-001"):
            TranscriptProcessor.enforce_token_cap(text, prompt)

    def test_error_message_is_actionable(self, monkeypatch):
        monkeypatch.setattr(TranscriptProcessor, "LLM_MAX_INPUT_TOKENS", 5)
        try:
            TranscriptProcessor.enforce_token_cap("x" * 100)
        except ValueError as e:
            msg = str(e)
            assert "MAITY_LLM_MAX_INPUT_TOKENS" in msg
            assert "LLM-001" in msg

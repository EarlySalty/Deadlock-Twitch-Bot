"""LLM-Dispatcher mit Provider-Auswahl und Consent-Gate.

Default: lokales Ollama (`bot/social_media/llm/ollama.py`).

Externe Provider (MiniMax, Claude Haiku) werden *nur* aufgerufen, wenn
beide Bedingungen erfuellt sind:

1. Globaler Consent-Toggle in `social_media_settings.external_llm_consent` ist `true`.
2. Per Env-Variable `SOCIAL_MEDIA_LLM_PROVIDER` ist explizit ein externer
   Provider gewaehlt (`minimax` oder `claude_haiku`).

Ohne Consent oder ohne explizite Provider-Wahl: lokales Ollama.
Bei Fehlern auf dem gewaehlten Provider erfolgt - sofern moeglich -
ein Fallback auf Ollama, sonst wird der Fehler propagiert.
"""

from __future__ import annotations

import logging
import os
from typing import Optional

from ..settings import external_llm_consent
from .base import (
    LLMProviderError,
    LLMProviderUnavailable,
    LLMRequest,
    LLMResponse,
    LLMUnavailable,
)

log = logging.getLogger("TwitchStreams.SocialMedia.LLM.Dispatcher")

_DEFAULT_LOCAL = "ollama"
_EXTERNAL_PROVIDERS = frozenset({"minimax", "claude_haiku"})


class LLMDispatcher:
    """Waehlt + ruft den passenden Provider auf."""

    def __init__(
        self,
        *,
        provider_override: str | None = None,
        consent_override: bool | None = None,
    ) -> None:
        self.provider_override = provider_override
        self.consent_override = consent_override

    def _resolve_consent(self) -> bool:
        if self.consent_override is not None:
            return bool(self.consent_override)
        return external_llm_consent()

    def _resolve_provider_name(self) -> str:
        if self.provider_override:
            return str(self.provider_override).strip().lower()
        from_env = (os.getenv("SOCIAL_MEDIA_LLM_PROVIDER") or "").strip().lower()
        if from_env:
            return from_env
        return _DEFAULT_LOCAL

    def _instantiate_provider(self, name: str):
        if name == "ollama":
            from .ollama import OllamaProvider
            return OllamaProvider()
        if name == "minimax":
            from .minimax import MiniMaxProvider
            return MiniMaxProvider()
        if name == "claude_haiku":
            from .claude_haiku import ClaudeHaikuProvider
            return ClaudeHaikuProvider()
        raise LLMProviderUnavailable(f"Unknown LLM provider: {name!r}")

    async def generate(self, request: LLMRequest) -> LLMResponse:
        chosen = self._resolve_provider_name()
        consent = self._resolve_consent()

        if chosen in _EXTERNAL_PROVIDERS and not consent:
            log.warning(
                "External LLM provider %r requested without external_llm_consent — "
                "falling back to local ollama.",
                chosen,
            )
            chosen = _DEFAULT_LOCAL

        attempted: list[str] = []
        last_error: Optional[BaseException] = None

        for candidate in _candidate_chain(chosen):
            attempted.append(candidate)
            try:
                provider = self._instantiate_provider(candidate)
            except LLMProviderUnavailable as exc:
                log.info("Provider %s unavailable: %s", candidate, exc)
                last_error = exc
                continue
            try:
                response = await provider.generate(request)
                if attempted[0] != candidate:
                    log.warning(
                        "LLM dispatcher used fallback provider %s after primary %s failed",
                        candidate,
                        attempted[0],
                    )
                return response
            except LLMProviderUnavailable as exc:
                log.info("Provider %s became unavailable mid-call: %s", candidate, exc)
                last_error = exc
                continue
            except LLMProviderError as exc:
                log.warning("Provider %s failed (%s)", candidate, exc)
                last_error = exc
                continue
            except Exception as exc:
                log.exception("Provider %s raised unexpectedly", candidate)
                last_error = exc
                continue

        raise LLMUnavailable(
            f"All LLM providers failed (tried: {attempted}). last_error={last_error!r}"
        )


def _candidate_chain(primary: str) -> list[str]:
    chain = [primary]
    if primary != _DEFAULT_LOCAL:
        chain.append(_DEFAULT_LOCAL)
    return chain


async def generate_enrichment(request: LLMRequest) -> LLMResponse:
    """Convenience: Default-Dispatcher mit Consent-Gate."""
    dispatcher = LLMDispatcher()
    return await dispatcher.generate(request)

"""Datenklassen + Provider-Schnittstelle fuer den Phase-2 LLM-Layer."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Protocol


class LLMUnavailable(RuntimeError):
    """Generic LLM-related error."""


class LLMProviderUnavailable(LLMUnavailable):
    """Raised wenn ein konkreter Provider (kein Key, kein SDK) nicht verfuegbar ist."""


class LLMProviderError(LLMUnavailable):
    """Raised bei einem Laufzeitfehler des Providers (HTTP/Parse)."""


SOCIAL_PLATFORMS: tuple[str, ...] = ("youtube", "tiktok", "instagram")


@dataclass(frozen=True)
class StreamerProfile:
    streamer_login: str
    display_name: str | None = None
    language: str | None = None
    persona_hint: str | None = None


@dataclass(frozen=True)
class LLMRequest:
    transcript: str
    detected_terms: tuple[str, ...] = field(default_factory=tuple)
    streamer: StreamerProfile | None = None
    clip_title: str | None = None
    game_name: str | None = None
    duration_seconds: float | None = None


@dataclass(frozen=True)
class PlatformEnrichment:
    title: str
    description: str
    hashtags: tuple[str, ...]


@dataclass(frozen=True)
class LLMResponse:
    youtube: PlatformEnrichment
    tiktok: PlatformEnrichment
    instagram: PlatformEnrichment
    provider: str
    model: str
    cost_usd_estimate: float | None = None
    raw_payload: dict[str, Any] | None = None


@dataclass(frozen=True)
class LLMTextResponse:
    content: str
    provider: str
    model: str
    cost_usd_estimate: float | None = None
    raw_payload: dict[str, Any] | None = None


class LLMProvider(Protocol):
    name: str
    model: str

    async def generate(self, request: LLMRequest) -> LLMResponse:
        """Run the LLM and return platform-specific enrichment."""
        ...

    async def generate_text(
        self,
        system_prompt: str,
        user_prompt: str,
        *,
        max_tokens: int = 1200,
        temperature: float = 0.2,
    ) -> LLMTextResponse:
        """Run the LLM and return free-form text."""
        ...

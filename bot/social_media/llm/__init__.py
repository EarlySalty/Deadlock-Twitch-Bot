"""LLM adapters for the Phase 2 enrichment pipeline.

- base:          Datenklassen + abstrakte Provider-Schnittstelle
- prompts:       Prompts pro Plattform (YouTube/TikTok/Instagram)
- minimax:       MiniMax-2.7 Adapter (OpenAI-kompatible Schnittstelle)
- claude_haiku:  Claude Haiku 4.5 Adapter (Anthropic-Schnittstelle)
- dispatcher:    Auswahl + Fallback-Logik
"""

from .base import (
    LLMRequest,
    LLMResponse,
    LLMProviderError,
    LLMProviderUnavailable,
    LLMUnavailable,
    PlatformEnrichment,
    StreamerProfile,
)
from .dispatcher import LLMDispatcher, generate_enrichment

__all__ = [
    "LLMDispatcher",
    "LLMProviderError",
    "LLMProviderUnavailable",
    "LLMRequest",
    "LLMResponse",
    "LLMUnavailable",
    "PlatformEnrichment",
    "StreamerProfile",
    "generate_enrichment",
]

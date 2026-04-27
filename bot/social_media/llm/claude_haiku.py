"""Claude-Haiku-Adapter (extern, *nur* mit external_llm_consent=True nutzbar).

Nutzt das offizielle `anthropic`-SDK. Erfordert `ANTHROPIC_API_KEY` und
optional `ANTHROPIC_HAIKU_MODEL` (Default: `claude-haiku-4-5-20251001`).
"""

from __future__ import annotations

import asyncio
import logging
import os
from typing import Any

from ._parsing import parse_llm_payload
from .base import (
    LLMProviderError,
    LLMProviderUnavailable,
    LLMRequest,
    LLMResponse,
    LLMTextResponse,
)
from .prompts import SYSTEM_PROMPT, render_user_prompt

log = logging.getLogger("TwitchStreams.SocialMedia.LLM.Claude")

DEFAULT_MODEL = "claude-haiku-4-5-20251001"
GENERATE_TIMEOUT_SECONDS = 60

_INPUT_USD_PER_1K = float(os.getenv("CLAUDE_HAIKU_PRICE_INPUT_PER_1K", "0.001") or 0.001)
_OUTPUT_USD_PER_1K = float(os.getenv("CLAUDE_HAIKU_PRICE_OUTPUT_PER_1K", "0.005") or 0.005)


class ClaudeHaikuProvider:
    name = "claude_haiku"

    def __init__(
        self,
        *,
        model: str | None = None,
        api_key: str | None = None,
        temperature: float = 0.4,
    ) -> None:
        api_key = api_key or os.getenv("ANTHROPIC_API_KEY")
        if not api_key:
            raise LLMProviderUnavailable("ANTHROPIC_API_KEY not set")
        try:
            from anthropic import AsyncAnthropic  # type: ignore
        except Exception as exc:
            raise LLMProviderUnavailable("anthropic SDK not installed") from exc
        self.model = model or os.getenv("ANTHROPIC_HAIKU_MODEL") or DEFAULT_MODEL
        self._client = AsyncAnthropic(api_key=api_key)
        self.temperature = float(temperature)

    async def generate(self, request: LLMRequest) -> LLMResponse:
        prompt = render_user_prompt(request)
        text_response = await self.generate_text(
            SYSTEM_PROMPT,
            prompt,
            max_tokens=600,
            temperature=self.temperature,
        )
        return parse_llm_payload(
            text_response.content,
            provider=self.name,
            model=self.model,
            cost_usd_estimate=text_response.cost_usd_estimate,
        )

    async def generate_text(
        self,
        system_prompt: str,
        user_prompt: str,
        *,
        max_tokens: int = 1200,
        temperature: float = 0.2,
    ) -> LLMTextResponse:
        try:
            response: Any = await asyncio.wait_for(
                self._client.messages.create(
                    model=self.model,
                    max_tokens=max_tokens,
                    temperature=temperature,
                    system=system_prompt,
                    messages=[{"role": "user", "content": user_prompt}],
                ),
                timeout=GENERATE_TIMEOUT_SECONDS,
            )
        except asyncio.TimeoutError as exc:
            raise LLMProviderError("Claude timeout") from exc
        except Exception as exc:
            raise LLMProviderError(f"Claude error: {exc}") from exc

        text = ""
        for block in getattr(response, "content", None) or []:
            text += str(getattr(block, "text", "") or "")
        if not text.strip():
            raise LLMProviderError("Claude returned empty content")

        usage = getattr(response, "usage", None)
        prompt_tokens = int(getattr(usage, "input_tokens", 0) or 0)
        completion_tokens = int(getattr(usage, "output_tokens", 0) or 0)
        cost_estimate = (
            (prompt_tokens / 1000.0) * _INPUT_USD_PER_1K
            + (completion_tokens / 1000.0) * _OUTPUT_USD_PER_1K
        )
        return LLMTextResponse(
            content=text,
            provider=self.name,
            model=self.model,
            cost_usd_estimate=round(cost_estimate, 6),
            raw_payload={"usage": {"input_tokens": prompt_tokens, "output_tokens": completion_tokens}},
        )

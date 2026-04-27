"""MiniMax-LLM-Adapter (extern, *nur* mit external_llm_consent=True nutzbar).

Nutzt MiniMax's OpenAI-kompatible Schnittstelle. Erfordert `MINIMAX_API_KEY`
und optional `MINIMAX_BASE_URL` (Default: `https://api.minimax.chat/v1`) sowie
`MINIMAX_MODEL` (Default: `MiniMax-Text-2.7`).
"""

from __future__ import annotations

import asyncio
import logging
import os
from typing import Any

from ._parsing import parse_llm_payload
from .base import LLMProviderError, LLMProviderUnavailable, LLMRequest, LLMResponse
from .prompts import SYSTEM_PROMPT, render_user_prompt

log = logging.getLogger("TwitchStreams.SocialMedia.LLM.MiniMax")

DEFAULT_BASE_URL = "https://api.minimax.chat/v1"
DEFAULT_MODEL = "MiniMax-Text-2.7"
GENERATE_TIMEOUT_SECONDS = 60

# Best-effort estimation. Adjust via env if MiniMax pricing changes.
_INPUT_USD_PER_1K = float(os.getenv("MINIMAX_PRICE_INPUT_PER_1K", "0.0008") or 0.0008)
_OUTPUT_USD_PER_1K = float(os.getenv("MINIMAX_PRICE_OUTPUT_PER_1K", "0.0024") or 0.0024)


class MiniMaxProvider:
    name = "minimax"

    def __init__(
        self,
        *,
        model: str | None = None,
        base_url: str | None = None,
        api_key: str | None = None,
        temperature: float = 0.4,
    ) -> None:
        api_key = api_key or os.getenv("MINIMAX_API_KEY")
        if not api_key:
            raise LLMProviderUnavailable("MINIMAX_API_KEY not set")
        try:
            from openai import AsyncOpenAI  # type: ignore
        except Exception as exc:
            raise LLMProviderUnavailable("openai SDK not installed") from exc
        self.model = model or os.getenv("MINIMAX_MODEL") or DEFAULT_MODEL
        self.base_url = base_url or os.getenv("MINIMAX_BASE_URL") or DEFAULT_BASE_URL
        self._client = AsyncOpenAI(api_key=api_key, base_url=self.base_url)
        self.temperature = float(temperature)

    async def generate(self, request: LLMRequest) -> LLMResponse:
        prompt = render_user_prompt(request)
        try:
            response: Any = await asyncio.wait_for(
                self._client.chat.completions.create(
                    model=self.model,
                    messages=[
                        {"role": "system", "content": SYSTEM_PROMPT},
                        {"role": "user", "content": prompt},
                    ],
                    temperature=self.temperature,
                    response_format={"type": "json_object"},
                    max_tokens=600,
                ),
                timeout=GENERATE_TIMEOUT_SECONDS,
            )
        except asyncio.TimeoutError as exc:
            raise LLMProviderError("MiniMax timeout") from exc
        except Exception as exc:
            raise LLMProviderError(f"MiniMax error: {exc}") from exc

        choice = (response.choices or [None])[0]
        if not choice or not getattr(choice, "message", None):
            raise LLMProviderError("MiniMax returned no choices")
        text = str(choice.message.content or "")
        if not text.strip():
            raise LLMProviderError("MiniMax returned empty content")

        usage = getattr(response, "usage", None)
        prompt_tokens = int(getattr(usage, "prompt_tokens", 0) or 0)
        completion_tokens = int(getattr(usage, "completion_tokens", 0) or 0)
        cost_estimate = (
            (prompt_tokens / 1000.0) * _INPUT_USD_PER_1K
            + (completion_tokens / 1000.0) * _OUTPUT_USD_PER_1K
        )

        return parse_llm_payload(
            text,
            provider=self.name,
            model=self.model,
            cost_usd_estimate=round(cost_estimate, 6),
        )

"""DeepSeek adapter for the social-media enrichment layer via Fireworks AI.

Uses Fireworks' OpenAI-compatible API by default. Required env:
`FIREWORKS_API_KEY` or the existing vault name `FIREWORK_API_KEY`.
Optional env: `DEEPSEEK_MODEL`, `DEEPSEEK_BASE_URL`.
"""

from __future__ import annotations

import asyncio
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

DEFAULT_BASE_URL = "https://api.fireworks.ai/inference/v1"
DEFAULT_MODEL = "accounts/fireworks/models/deepseek-v4-pro"
GENERATE_TIMEOUT_SECONDS = 180

_FIREWORKS_V4_PRO_PRICES = {"hit": 0.14, "miss": 1.74, "out": 3.48}


class DeepSeekProvider:
    name = "deepseek"

    def __init__(
        self,
        *,
        model: str | None = None,
        base_url: str | None = None,
        api_key: str | None = None,
        temperature: float = 0.35,
        client: Any | None = None,
    ) -> None:
        self.model = model or os.getenv("DEEPSEEK_MODEL") or DEFAULT_MODEL
        self.base_url = base_url or os.getenv("DEEPSEEK_BASE_URL") or DEFAULT_BASE_URL
        self.temperature = float(temperature)
        if client is not None:
            self._client = client
            return
        api_key = (
            api_key
            or os.getenv("FIREWORKS_API_KEY")
            or os.getenv("FIREWORK_API_KEY")
        )
        if not api_key:
            raise LLMProviderUnavailable("FIREWORKS_API_KEY/FIREWORK_API_KEY not set")
        try:
            from openai import AsyncOpenAI  # type: ignore
        except Exception as exc:
            raise LLMProviderUnavailable("openai SDK not installed") from exc
        self._client = AsyncOpenAI(api_key=api_key, base_url=self.base_url)

    async def generate(self, request: LLMRequest) -> LLMResponse:
        text_response = await self.generate_text(
            SYSTEM_PROMPT + "\n- No analysis. Return the JSON object only; first character must be `{`.",
            render_user_prompt(request),
            max_tokens=6500,
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
        request_kwargs: dict[str, Any] = {
            "model": self.model,
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": user_prompt},
            ],
            "temperature": temperature,
            "max_tokens": max_tokens,
        }
        if "strict json" in system_prompt.lower():
            request_kwargs["response_format"] = {"type": "json_object"}
        effort = (os.getenv("DEEPSEEK_REASONING_EFFORT") or "").strip()
        if effort:
            request_kwargs["reasoning_effort"] = effort

        try:
            response: Any = await asyncio.wait_for(
                self._client.chat.completions.create(**request_kwargs),
                timeout=GENERATE_TIMEOUT_SECONDS,
            )
        except asyncio.TimeoutError as exc:
            raise LLMProviderError("DeepSeek timeout") from exc
        except Exception as exc:
            raise LLMProviderError(f"DeepSeek error: {exc}") from exc

        choice = (getattr(response, "choices", None) or [None])[0]
        message = getattr(choice, "message", None) if choice else None
        text = str(getattr(message, "content", "") or "")
        if not text.strip():
            raise LLMProviderError("DeepSeek returned empty content")

        usage = getattr(response, "usage", None)
        cost_estimate = _estimate_cost_usd(usage)
        return LLMTextResponse(
            content=text,
            provider=self.name,
            model=self.model,
            cost_usd_estimate=cost_estimate,
            raw_payload={"usage": _usage_payload(usage)},
        )


def _usage_payload(usage: Any) -> dict[str, int]:
    if usage is None:
        return {}
    keys = (
        "prompt_tokens",
        "prompt_cache_hit_tokens",
        "prompt_cache_miss_tokens",
        "completion_tokens",
        "total_tokens",
    )
    return {key: int(getattr(usage, key, 0) or 0) for key in keys}


def _cached_prompt_tokens(usage: Any) -> int:
    details = getattr(usage, "prompt_tokens_details", None)
    if details is None:
        return 0
    if isinstance(details, dict):
        return int(details.get("cached_tokens") or 0)
    return int(getattr(details, "cached_tokens", 0) or 0)


def _estimate_cost_usd(usage: Any) -> float | None:
    if usage is None:
        return None
    prompt_tokens = int(getattr(usage, "prompt_tokens", 0) or 0)
    hit = int(getattr(usage, "prompt_cache_hit_tokens", 0) or 0) or _cached_prompt_tokens(usage)
    miss = int(getattr(usage, "prompt_cache_miss_tokens", 0) or 0) or max(prompt_tokens - hit, 0)
    out = int(getattr(usage, "completion_tokens", 0) or 0)
    cost = (
        (hit / 1_000_000.0)
        * float(os.getenv("DEEPSEEK_PRICE_INPUT_HIT_PER_1M", _FIREWORKS_V4_PRO_PRICES["hit"]))
        + (miss / 1_000_000.0)
        * float(os.getenv("DEEPSEEK_PRICE_INPUT_MISS_PER_1M", _FIREWORKS_V4_PRO_PRICES["miss"]))
        + (out / 1_000_000.0)
        * float(os.getenv("DEEPSEEK_PRICE_OUTPUT_PER_1M", _FIREWORKS_V4_PRO_PRICES["out"]))
    )
    return round(cost, 8)

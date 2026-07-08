"""DeepSeek adapter for the social-media enrichment layer.

Uses DeepSeek's OpenAI-compatible API. Required env: `DEEPSEEK_API_KEY`.
Optional env: `DEEPSEEK_MODEL`, `DEEPSEEK_BASE_URL`, `DEEPSEEK_THINKING`.
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

DEFAULT_BASE_URL = "https://api.deepseek.com"
DEFAULT_MODEL = "deepseek-v4-pro"
GENERATE_TIMEOUT_SECONDS = 90

_FLASH_PRICES = {"hit": 0.0028, "miss": 0.14, "out": 0.28}
_PRO_PRICES = {"hit": 0.003625, "miss": 0.435, "out": 0.87}


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
        api_key = api_key or os.getenv("DEEPSEEK_API_KEY")
        if not api_key:
            raise LLMProviderUnavailable("DEEPSEEK_API_KEY not set")
        try:
            from openai import AsyncOpenAI  # type: ignore
        except Exception as exc:
            raise LLMProviderUnavailable("openai SDK not installed") from exc
        self._client = AsyncOpenAI(api_key=api_key, base_url=self.base_url)

    async def generate(self, request: LLMRequest) -> LLMResponse:
        text_response = await self.generate_text(
            SYSTEM_PROMPT,
            render_user_prompt(request),
            max_tokens=700,
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
            # ponytail: copywriting probe, enable thinking via DEEPSEEK_THINKING if needed.
            "extra_body": {"thinking": {"type": _thinking_mode()}},
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
        cost_estimate = _estimate_cost_usd(self.model, usage)
        return LLMTextResponse(
            content=text,
            provider=self.name,
            model=self.model,
            cost_usd_estimate=cost_estimate,
            raw_payload={"usage": _usage_payload(usage)},
        )


def _thinking_mode() -> str:
    value = (os.getenv("DEEPSEEK_THINKING") or "disabled").strip().lower()
    return "enabled" if value == "enabled" else "disabled"


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


def _estimate_cost_usd(model: str, usage: Any) -> float | None:
    if usage is None:
        return None
    prices = _PRO_PRICES if "pro" in model.lower() else _FLASH_PRICES
    prompt_tokens = int(getattr(usage, "prompt_tokens", 0) or 0)
    hit = int(getattr(usage, "prompt_cache_hit_tokens", 0) or 0)
    miss = int(getattr(usage, "prompt_cache_miss_tokens", 0) or 0)
    if hit == 0 and miss == 0:
        miss = prompt_tokens
    out = int(getattr(usage, "completion_tokens", 0) or 0)
    cost = (
        (hit / 1_000_000.0) * float(os.getenv("DEEPSEEK_PRICE_INPUT_HIT_PER_1M", prices["hit"]))
        + (miss / 1_000_000.0) * float(os.getenv("DEEPSEEK_PRICE_INPUT_MISS_PER_1M", prices["miss"]))
        + (out / 1_000_000.0) * float(os.getenv("DEEPSEEK_PRICE_OUTPUT_PER_1M", prices["out"]))
    )
    return round(cost, 8)

"""Ollama-Adapter (lokal, Default-Provider).

Erwartet einen lokalen Ollama-Server unter `OLLAMA_HOST` (Default
`127.0.0.1:11434`). Nutzt den `/api/generate`-Endpoint mit `format=json`,
damit der Output strikt JSON ist.
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

log = logging.getLogger("TwitchStreams.SocialMedia.LLM.Ollama")

DEFAULT_MODEL = "qwen2.5:7b-instruct-q4_K_M"
DEFAULT_HOST = "127.0.0.1:11434"
GENERATE_TIMEOUT_SECONDS = 240


class OllamaProvider:
    name = "ollama"

    def __init__(
        self,
        *,
        model: str | None = None,
        host: str | None = None,
        temperature: float = 0.4,
    ) -> None:
        try:
            import aiohttp  # noqa: F401
        except ImportError as exc:
            raise LLMProviderUnavailable("aiohttp not installed") from exc
        self.model = model or os.getenv("OLLAMA_MODEL") or DEFAULT_MODEL
        host = host or os.getenv("OLLAMA_HOST") or DEFAULT_HOST
        if "://" not in host:
            host = f"http://{host}"
        self.host = host.rstrip("/")
        self.temperature = float(temperature)

    @property
    def url(self) -> str:
        return f"{self.host}/api/generate"

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
        payload = await self._generate_payload(
            system_prompt=system_prompt,
            user_prompt=user_prompt,
            max_tokens=max_tokens,
            temperature=temperature,
            response_format="json" if "strict json" in system_prompt.lower() else None,
        )
        raw_response = str(payload.get("response") or "")
        if not raw_response.strip():
            raise LLMProviderError("Ollama returned empty response")

        eval_count = int(payload.get("eval_count") or 0)
        prompt_eval_count = int(payload.get("prompt_eval_count") or 0)
        cost_estimate = _estimate_cost(prompt_eval_count, eval_count)
        return LLMTextResponse(
            content=raw_response,
            provider=self.name,
            model=self.model,
            cost_usd_estimate=cost_estimate,
            raw_payload=payload,
        )

    async def _generate_payload(
        self,
        *,
        system_prompt: str,
        user_prompt: str,
        max_tokens: int,
        temperature: float,
        response_format: str | None,
    ) -> dict[str, Any]:
        import aiohttp

        body: dict[str, Any] = {
            "model": self.model,
            "prompt": user_prompt,
            "system": system_prompt,
            "stream": False,
            "options": {
                "temperature": float(temperature),
                "num_predict": max_tokens,
                "top_p": 0.9,
            },
        }
        if response_format:
            body["format"] = response_format

        timeout = aiohttp.ClientTimeout(total=GENERATE_TIMEOUT_SECONDS)
        try:
            async with aiohttp.ClientSession(timeout=timeout) as session:
                async with session.post(self.url, json=body) as resp:
                    if resp.status != 200:
                        text = await resp.text()
                        raise LLMProviderError(f"Ollama HTTP {resp.status}: {text[:200]}")
                    return await resp.json()
        except asyncio.TimeoutError as exc:
            raise LLMProviderError("Ollama generate timeout") from exc
        except aiohttp.ClientConnectorError as exc:
            raise LLMProviderUnavailable(f"Ollama not reachable at {self.host}") from exc
        except aiohttp.ClientError as exc:
            raise LLMProviderError(f"Ollama client error: {exc}") from exc


def _estimate_cost(prompt_tokens: int, completion_tokens: int) -> float:
    """Lokales LLM hat keinen monetaeren Per-Call-Cost. 0.0 als Marker."""
    _ = (prompt_tokens, completion_tokens)
    return 0.0


async def healthcheck(*, host: str | None = None, timeout: float = 3.0) -> bool:
    """Schneller Health-Check ob Ollama erreichbar ist."""
    try:
        import aiohttp
    except ImportError:
        return False
    host = host or os.getenv("OLLAMA_HOST") or DEFAULT_HOST
    if "://" not in host:
        host = f"http://{host}"
    url = f"{host.rstrip('/')}/api/tags"
    try:
        async with aiohttp.ClientSession(timeout=aiohttp.ClientTimeout(total=timeout)) as session:
            async with session.get(url) as resp:
                if resp.status != 200:
                    return False
                data = await resp.json()
                return isinstance(data, dict) and "models" in data
    except Exception:
        return False


def is_available() -> bool:
    """Sync-Check (nutzt asyncio.run intern)."""
    try:
        return asyncio.run(healthcheck())
    except RuntimeError:
        # Inside event loop -> can't sync-check.
        return True

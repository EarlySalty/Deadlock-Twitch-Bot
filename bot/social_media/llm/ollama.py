"""Ollama-Adapter (lokal, Default-Provider).

Erwartet einen lokalen Ollama-Server unter `OLLAMA_HOST` (Default
`127.0.0.1:11434`). Nutzt den `/api/generate`-Endpoint mit `format=json`,
damit der Output strikt JSON ist.
"""

from __future__ import annotations

import asyncio
import json
import logging
import os
from typing import Any

from ._parsing import parse_llm_payload
from .base import LLMProviderError, LLMProviderUnavailable, LLMRequest, LLMResponse
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
        import aiohttp

        prompt = render_user_prompt(request)
        body = {
            "model": self.model,
            "prompt": prompt,
            "system": SYSTEM_PROMPT,
            "stream": False,
            "format": "json",
            "options": {
                "temperature": self.temperature,
                "num_predict": 600,
                "top_p": 0.9,
            },
        }

        timeout = aiohttp.ClientTimeout(total=GENERATE_TIMEOUT_SECONDS)
        try:
            async with aiohttp.ClientSession(timeout=timeout) as session:
                async with session.post(self.url, json=body) as resp:
                    if resp.status != 200:
                        text = await resp.text()
                        raise LLMProviderError(
                            f"Ollama HTTP {resp.status}: {text[:200]}"
                        )
                    payload: dict[str, Any] = await resp.json()
        except asyncio.TimeoutError as exc:
            raise LLMProviderError("Ollama generate timeout") from exc
        except aiohttp.ClientConnectorError as exc:
            raise LLMProviderUnavailable(
                f"Ollama not reachable at {self.host}"
            ) from exc
        except aiohttp.ClientError as exc:
            raise LLMProviderError(f"Ollama client error: {exc}") from exc

        raw_response = str(payload.get("response") or "")
        if not raw_response.strip():
            raise LLMProviderError("Ollama returned empty response")

        eval_count = int(payload.get("eval_count") or 0)
        prompt_eval_count = int(payload.get("prompt_eval_count") or 0)
        cost_estimate = _estimate_cost(prompt_eval_count, eval_count)

        return parse_llm_payload(
            raw_response,
            provider=self.name,
            model=self.model,
            cost_usd_estimate=cost_estimate,
        )


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

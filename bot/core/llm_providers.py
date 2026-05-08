from __future__ import annotations

import os
from functools import lru_cache
from typing import Any, Callable

from ..secret_store import keyring_enabled

MINIMAX_DEFAULT_BASE_URL = "https://api.minimax.io/v1"


class LLMProviderBootstrapError(RuntimeError):
    pass


class LLMSecretNotFoundError(LLMProviderBootstrapError):
    pass


class LLMSDKUnavailableError(LLMProviderBootstrapError):
    pass


def _load_secret(*secret_names: str) -> str:
    for secret_name in secret_names:
        value = ""
        if keyring_enabled():
            try:
                import keyring

                value = (
                    keyring.get_password(f"{secret_name}@DeadlockBot", secret_name)
                    or keyring.get_password("DeadlockBot", secret_name)
                    or ""
                )
            except Exception:
                pass
        if not value:
            value = os.environ.get(secret_name, "")
        if value:
            return value
    return ""


def get_anthropic_client(
    *,
    api_key: str | None = None,
    timeout: float | None = None,
    async_client: bool = True,
    client_factory: Callable[[str], Any] | None = None,
) -> Any:
    resolved_api_key = api_key or _load_secret("ANTHROPIC_API_KEY")
    if not resolved_api_key:
        raise LLMSecretNotFoundError(
            "ANTHROPIC_API_KEY nicht gefunden. Setze via keyring "
            "(service=DeadlockBot, key=ANTHROPIC_API_KEY) oder als Umgebungsvariable."
        )
    if client_factory is not None:
        return client_factory(resolved_api_key)
    return _build_anthropic_client(resolved_api_key, timeout, async_client)


def get_openai_client(
    *,
    api_key: str | None = None,
    base_url: str | None = None,
    timeout: float | None = None,
    async_client: bool = True,
) -> Any:
    resolved_api_key = api_key or _load_secret("OPENAI_API_KEY")
    if not resolved_api_key:
        raise LLMSecretNotFoundError(
            "OPENAI_API_KEY nicht gefunden. Setze den Key via keyring oder Umgebungsvariable."
        )
    return _build_openai_client(resolved_api_key, base_url, timeout, async_client)


def get_minimax_client(
    *,
    api_key: str | None = None,
    base_url: str | None = None,
    timeout: float | None = None,
    async_client: bool = True,
) -> Any:
    resolved_api_key = api_key or _load_secret("MINIMAX_TOKEN_PLAN_KEY", "MINIMAX_API_KEY", "MINMAX")
    if not resolved_api_key:
        raise LLMSecretNotFoundError(
            "MiniMax-Key nicht gefunden. Setze MINIMAX_TOKEN_PLAN_KEY, "
            "MINIMAX_API_KEY oder MINMAX via keyring/Umgebung."
        )
    return _build_openai_client(
        resolved_api_key,
        base_url or MINIMAX_DEFAULT_BASE_URL,
        timeout,
        async_client,
    )


@lru_cache(maxsize=None)
def _build_anthropic_client(
    api_key: str,
    timeout: float | None,
    async_client: bool,
) -> Any:
    try:
        import anthropic as anthropic_lib
    except ImportError as exc:
        raise LLMSDKUnavailableError(
            "anthropic package not installed. Run: pip install anthropic"
        ) from exc

    client_class = anthropic_lib.AsyncAnthropic if async_client else anthropic_lib.Anthropic
    client_kwargs: dict[str, object] = {"api_key": api_key}
    if timeout is not None:
        client_kwargs["timeout"] = timeout
    return client_class(**client_kwargs)


@lru_cache(maxsize=None)
def _build_openai_client(
    api_key: str,
    base_url: str | None,
    timeout: float | None,
    async_client: bool,
) -> Any:
    try:
        from openai import AsyncOpenAI, OpenAI
    except ImportError as exc:
        raise LLMSDKUnavailableError(
            "openai package not installed. Run: pip install openai"
        ) from exc

    client_class = AsyncOpenAI if async_client else OpenAI
    client_kwargs: dict[str, object] = {"api_key": api_key}
    if base_url:
        client_kwargs["base_url"] = base_url
    if timeout is not None:
        client_kwargs["timeout"] = timeout
    return client_class(**client_kwargs)


__all__ = [
    "LLMProviderBootstrapError",
    "LLMSDKUnavailableError",
    "LLMSecretNotFoundError",
    "MINIMAX_DEFAULT_BASE_URL",
    "get_anthropic_client",
    "get_minimax_client",
    "get_openai_client",
]

"""Gemeinsame Parser-Helfer fuer LLM-JSON-Output."""

from __future__ import annotations

import json
import re
from typing import Any

from .base import (
    LLMProviderError,
    LLMResponse,
    PlatformEnrichment,
    SOCIAL_PLATFORMS,
)

_PLATFORM_TITLE_LIMITS: dict[str, int] = {
    "youtube": 100,
    "instagram": 125,
    "tiktok": 150,
}

_HASHTAG_TOKEN_RE = re.compile(r"^[A-Za-z][A-Za-z0-9_]{0,49}$")


def _coerce_str(value: Any, fallback: str = "") -> str:
    if value is None:
        return fallback
    if isinstance(value, str):
        return value.strip()
    return str(value).strip()


def _normalize_hashtag(raw: Any) -> str | None:
    token = _coerce_str(raw)
    if not token:
        return None
    token = token.lstrip("#").strip()
    if not token:
        return None
    token = token.replace(" ", "")
    token = re.sub(r"[^A-Za-z0-9_]", "", token)
    if not token:
        return None
    if not _HASHTAG_TOKEN_RE.match(token):
        return None
    return f"#{token}"


def _coerce_hashtags(raw: Any, ensure_default: bool = True) -> list[str]:
    out: list[str] = []
    seen: set[str] = set()
    if isinstance(raw, list):
        for entry in raw:
            normalized = _normalize_hashtag(entry)
            if not normalized:
                continue
            if normalized.lower() in seen:
                continue
            seen.add(normalized.lower())
            out.append(normalized)
    elif isinstance(raw, str):
        for entry in re.split(r"[,\s]+", raw):
            normalized = _normalize_hashtag(entry)
            if not normalized:
                continue
            if normalized.lower() in seen:
                continue
            seen.add(normalized.lower())
            out.append(normalized)
    if ensure_default:
        deadlock_tag = "#Deadlock"
        if deadlock_tag.lower() not in seen:
            out.insert(0, deadlock_tag)
    return out


def _truncate(text: str, limit: int) -> str:
    if not text:
        return text
    if len(text) <= limit:
        return text
    cut = text[: limit - 1].rstrip()
    return cut + "…"


def _extract_platform(payload: dict[str, Any], platform: str) -> PlatformEnrichment:
    block = payload.get(platform)
    if not isinstance(block, dict):
        raise LLMProviderError(f"missing platform block: {platform}")
    title = _coerce_str(block.get("title"))
    description = _coerce_str(block.get("description"))
    hashtags = _coerce_hashtags(block.get("hashtags"))
    if not title:
        raise LLMProviderError(f"empty title for platform: {platform}")
    title = _truncate(title, _PLATFORM_TITLE_LIMITS[platform])
    return PlatformEnrichment(
        title=title,
        description=description,
        hashtags=tuple(hashtags),
    )


def _strip_code_fence(text: str) -> str:
    stripped = text.strip()
    if stripped.startswith("```"):
        # ```json ... ``` or ``` ... ```
        body = stripped[3:]
        if body.lower().startswith("json"):
            body = body[4:]
        if body.endswith("```"):
            body = body[:-3]
        return body.strip()
    return stripped


def _find_json_object(text: str) -> str:
    cleaned = _strip_code_fence(text)
    start = cleaned.find("{")
    if start == -1:
        raise LLMProviderError("LLM output contained no JSON object")
    depth = 0
    in_string = False
    escape = False
    for idx in range(start, len(cleaned)):
        ch = cleaned[idx]
        if escape:
            escape = False
            continue
        if ch == "\\":
            escape = True
            continue
        if ch == '"':
            in_string = not in_string
            continue
        if in_string:
            continue
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
            if depth == 0:
                return cleaned[start : idx + 1]
    raise LLMProviderError("LLM output had no balanced JSON object")


def parse_llm_payload(
    raw_text: str,
    *,
    provider: str,
    model: str,
    cost_usd_estimate: float | None = None,
) -> LLMResponse:
    json_text = _find_json_object(raw_text)
    try:
        payload = json.loads(json_text)
    except json.JSONDecodeError as exc:
        raise LLMProviderError(f"invalid JSON from LLM: {exc}") from exc
    if not isinstance(payload, dict):
        raise LLMProviderError("LLM JSON must be an object")
    return LLMResponse(
        youtube=_extract_platform(payload, "youtube"),
        tiktok=_extract_platform(payload, "tiktok"),
        instagram=_extract_platform(payload, "instagram"),
        provider=provider,
        model=model,
        cost_usd_estimate=cost_usd_estimate,
        raw_payload=payload,
    )


def expected_platforms() -> tuple[str, ...]:
    return SOCIAL_PLATFORMS

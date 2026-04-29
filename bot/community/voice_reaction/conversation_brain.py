"""Anthropic-Claude-Adapter für den Voice-Reaction-Conversation-Brain.

Nutzt Tool-Use mit dem `respond`-Tool aus `prompts.py`, sodass das Modell
strukturiert antwortet (Stance, should_respond, response_text, …).

Der Adapter ist so geschnitten, dass Tests ihn rein synchron mocken können —
der `client_factory` liefert ein Objekt mit `messages.create(...)` und gibt
ein dataclass mit `content` und `usage` zurück (analog Anthropic-SDK).
"""

from __future__ import annotations

import asyncio
import json
import logging
import os
import time
from dataclasses import dataclass
from typing import Any, Awaitable, Callable, Mapping, Sequence

from .prompts import RESPOND_TOOL, SYSTEM_PROMPT, SYSTEM_PROMPT_VERSION, TOOL_NAME, render_user_prompt

log = logging.getLogger("TwitchStreams.VoiceReaction.Brain")

DEFAULT_MODEL = "claude-sonnet-4-6"
DEFAULT_MAX_TOKENS = 600
DEFAULT_TIMEOUT_SECONDS = 45

_INPUT_USD_PER_1K = float(
    os.getenv("VOICE_REACTION_PRICE_INPUT_PER_1K")
    or os.getenv("CLAUDE_SONNET_PRICE_INPUT_PER_1K")
    or "0.003"
)
_OUTPUT_USD_PER_1K = float(
    os.getenv("VOICE_REACTION_PRICE_OUTPUT_PER_1K")
    or os.getenv("CLAUDE_SONNET_PRICE_OUTPUT_PER_1K")
    or "0.015"
)


class BrainUnavailable(RuntimeError):
    """Brain konnte nicht initialisiert werden (z. B. SDK / API-Key fehlt)."""


class BrainError(RuntimeError):
    """Laufzeit-Fehler während eines Brain-Calls."""


@dataclass(frozen=True)
class BrainCallInput:
    system_prompt_version: str
    model: str
    user_prompt: str
    history_length: int
    latest_signal_kind: str
    latest_signal_text: str

    def to_audit_payload(self) -> dict[str, object]:
        return {
            "system_prompt_version": self.system_prompt_version,
            "model": self.model,
            "user_prompt": self.user_prompt,
            "history_length_messages": self.history_length,
            "latest_signal_kind": self.latest_signal_kind,
            "latest_signal_text": self.latest_signal_text,
        }


@dataclass(frozen=True)
class BrainDecision:
    stance: str
    confidence: float
    reasoning_summary: str
    should_respond: bool
    response_text: str | None
    should_notify_human: bool
    should_close: bool
    close_reason: str | None
    suggest_voice_recheck_after_seconds: int | None
    raw_tool_input: Mapping[str, object]

    def to_audit_payload(self) -> dict[str, object]:
        return {
            "stance": self.stance,
            "confidence": self.confidence,
            "reasoning_summary": self.reasoning_summary,
            "should_respond": self.should_respond,
            "response_text": self.response_text,
            "should_notify_human": self.should_notify_human,
            "should_close": self.should_close,
            "close_reason": self.close_reason,
            "suggest_voice_recheck_after_seconds": self.suggest_voice_recheck_after_seconds,
            "raw_tool_input": dict(self.raw_tool_input),
        }


@dataclass(frozen=True)
class BrainCallOutput:
    decision: BrainDecision
    raw_response: Mapping[str, object]
    tokens_in: int
    tokens_out: int
    latency_ms: int
    cost_usd_estimate: float

    def to_audit_payload(self) -> dict[str, object]:
        return {
            "decision": self.decision.to_audit_payload(),
            "raw_response": dict(self.raw_response),
            "tokens_in": self.tokens_in,
            "tokens_out": self.tokens_out,
            "latency_ms": self.latency_ms,
            "cost_usd_estimate": self.cost_usd_estimate,
        }


class ConversationBrain:
    """Wrapper um den Anthropic-Tool-Use-Call."""

    def __init__(
        self,
        *,
        model: str | None = None,
        api_key: str | None = None,
        max_tokens: int = DEFAULT_MAX_TOKENS,
        timeout_seconds: int = DEFAULT_TIMEOUT_SECONDS,
        client: Any | None = None,
        client_factory: Callable[[str], Any] | None = None,
    ) -> None:
        self.model = model or os.getenv("VOICE_REACTION_CLAUDE_MODEL") or DEFAULT_MODEL
        self.max_tokens = int(max_tokens)
        self.timeout_seconds = int(timeout_seconds)

        if client is not None:
            self._client = client
            return

        resolved_key = api_key or os.getenv("ANTHROPIC_API_KEY")
        if not resolved_key:
            raise BrainUnavailable("ANTHROPIC_API_KEY not set")

        if client_factory is None:
            try:
                from anthropic import AsyncAnthropic  # type: ignore
            except Exception as exc:
                raise BrainUnavailable("anthropic SDK not installed") from exc
            self._client = AsyncAnthropic(api_key=resolved_key)
        else:
            self._client = client_factory(resolved_key)

    async def respond(
        self,
        *,
        streamer_context: Mapping[str, object],
        history: Sequence[Mapping[str, object]],
        latest_signal_kind: str,
        latest_signal_text: str,
        latest_signal_meta: Mapping[str, object] | None = None,
    ) -> tuple[BrainCallInput, BrainCallOutput]:
        user_prompt = render_user_prompt(
            streamer_context=streamer_context,
            history=history,
            latest_signal_kind=latest_signal_kind,
            latest_signal_text=latest_signal_text,
            latest_signal_meta=latest_signal_meta,
        )

        call_input = BrainCallInput(
            system_prompt_version=SYSTEM_PROMPT_VERSION,
            model=self.model,
            user_prompt=user_prompt,
            history_length=len(list(history)),
            latest_signal_kind=str(latest_signal_kind or ""),
            latest_signal_text=str(latest_signal_text or ""),
        )

        started = time.monotonic()
        try:
            response = await asyncio.wait_for(
                self._client.messages.create(
                    model=self.model,
                    max_tokens=self.max_tokens,
                    system=SYSTEM_PROMPT,
                    tools=[RESPOND_TOOL],
                    tool_choice={"type": "tool", "name": TOOL_NAME},
                    messages=[{"role": "user", "content": user_prompt}],
                ),
                timeout=self.timeout_seconds,
            )
        except asyncio.TimeoutError as exc:
            raise BrainError(f"Brain-Timeout nach {self.timeout_seconds}s") from exc
        except Exception as exc:
            raise BrainError(f"Brain-API-Fehler: {exc}") from exc

        latency_ms = int((time.monotonic() - started) * 1000)
        decision = _parse_tool_use(response)
        tokens_in, tokens_out = _extract_usage(response)
        cost = _estimate_cost(tokens_in, tokens_out)

        raw_payload = _response_to_dict(response)
        call_output = BrainCallOutput(
            decision=decision,
            raw_response=raw_payload,
            tokens_in=tokens_in,
            tokens_out=tokens_out,
            latency_ms=latency_ms,
            cost_usd_estimate=cost,
        )
        return call_input, call_output


# ---------- Helpers ----------


def _parse_tool_use(response: Any) -> BrainDecision:
    blocks = getattr(response, "content", None) or []
    tool_input: Mapping[str, object] | None = None
    for block in blocks:
        block_type = getattr(block, "type", None)
        block_name = getattr(block, "name", None)
        if block_type == "tool_use" and block_name == TOOL_NAME:
            candidate = getattr(block, "input", None)
            if isinstance(candidate, Mapping):
                tool_input = candidate
                break
            if isinstance(candidate, dict):
                tool_input = candidate
                break

    if tool_input is None:
        raise BrainError("Brain-Antwort enthielt kein gültiges respond-Tool-Use")

    stance = str(tool_input.get("stance") or "neutral").strip().lower()
    confidence = float(tool_input.get("confidence") or 0.0)
    reasoning_summary = str(tool_input.get("reasoning_summary") or "")
    should_respond = bool(tool_input.get("should_respond"))
    response_text_raw = tool_input.get("response_text")
    response_text = (
        str(response_text_raw) if isinstance(response_text_raw, str) and response_text_raw.strip() else None
    )
    should_notify_human = bool(tool_input.get("should_notify_human"))
    should_close = bool(tool_input.get("should_close"))
    close_reason_raw = tool_input.get("close_reason")
    close_reason = (
        str(close_reason_raw).strip().lower()
        if isinstance(close_reason_raw, str) and close_reason_raw.strip()
        else None
    )
    recheck_raw = tool_input.get("suggest_voice_recheck_after_seconds")
    if isinstance(recheck_raw, bool):
        recheck = None
    elif isinstance(recheck_raw, (int, float)):
        recheck = max(0, int(recheck_raw))
    else:
        recheck = None

    return BrainDecision(
        stance=stance,
        confidence=max(0.0, min(1.0, confidence)),
        reasoning_summary=reasoning_summary[:240],
        should_respond=should_respond,
        response_text=response_text,
        should_notify_human=should_notify_human,
        should_close=should_close,
        close_reason=close_reason,
        suggest_voice_recheck_after_seconds=recheck,
        raw_tool_input=dict(tool_input),
    )


def _extract_usage(response: Any) -> tuple[int, int]:
    usage = getattr(response, "usage", None)
    tokens_in = int(getattr(usage, "input_tokens", 0) or 0)
    tokens_out = int(getattr(usage, "output_tokens", 0) or 0)
    return tokens_in, tokens_out


def _estimate_cost(tokens_in: int, tokens_out: int) -> float:
    cost = (tokens_in / 1000.0) * _INPUT_USD_PER_1K + (tokens_out / 1000.0) * _OUTPUT_USD_PER_1K
    return round(cost, 6)


def _response_to_dict(response: Any) -> dict[str, object]:
    payload: dict[str, object] = {}
    blocks: list[dict[str, object]] = []
    for block in getattr(response, "content", None) or []:
        entry: dict[str, object] = {}
        for key in ("type", "name", "id"):
            value = getattr(block, key, None)
            if value is not None:
                entry[key] = value
        block_input = getattr(block, "input", None)
        if isinstance(block_input, Mapping):
            entry["input"] = dict(block_input)
        text_value = getattr(block, "text", None)
        if isinstance(text_value, str) and text_value:
            entry["text"] = text_value
        blocks.append(entry)
    payload["content"] = blocks
    usage = getattr(response, "usage", None)
    if usage is not None:
        payload["usage"] = {
            "input_tokens": int(getattr(usage, "input_tokens", 0) or 0),
            "output_tokens": int(getattr(usage, "output_tokens", 0) or 0),
        }
    stop_reason = getattr(response, "stop_reason", None)
    if stop_reason:
        payload["stop_reason"] = str(stop_reason)
    model = getattr(response, "model", None)
    if model:
        payload["model"] = str(model)
    response_id = getattr(response, "id", None)
    if response_id:
        payload["id"] = str(response_id)
    return payload


__all__ = [
    "BrainUnavailable",
    "BrainError",
    "BrainCallInput",
    "BrainCallOutput",
    "BrainDecision",
    "ConversationBrain",
    "DEFAULT_MODEL",
]

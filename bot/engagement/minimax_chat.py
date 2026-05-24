"""Engagement-spezifischer MiniMax-M2.7-Client.

Bewusst getrennt vom Social-Media-LLM-Pfad (`bot/social_media/llm/`) — eigene
Cost-/Cooldown-/Settings-Welt. API-Key kommt aus dem bestehenden Tresor-Loader
(`MINIMAX_API_KEY` via Infisical → Env). Modell-Lock auf `MiniMax-Text-2.7`
über `ENGAGEMENT_MINIMAX_MODEL`, Provider-Lock über `ENGAGEMENT_LLM_PROVIDER`
(faktisch fix `minimax_m27`).

Liefert `ChatResponse` mit Text, Token-Counts und Latenz. Parser erkennt den
`<silent>`-Marker und gibt dann `text=None` zurück — die Pipeline interpretiert
das als bewusstes Schweigen.
"""

from __future__ import annotations

import logging
import os
import re
import time
from dataclasses import dataclass

log = logging.getLogger("TwitchStreams.Engagement.Minimax")


DEFAULT_BASE_URL = "https://api.minimax.io/v1"
DEFAULT_MODEL = "MiniMax-M2.7"
SILENT_MARKER = "<silent>"

# MiniMax M2.7 prependet ein <think>…</think>-Reasoning-Block. Müssen wir
# entfernen, sonst landen die Gedanken im Chat.
_THINK_RE = re.compile(r"<think>.*?</think>", re.DOTALL | re.IGNORECASE)


@dataclass(slots=True)
class ChatMessage:
    role: str  # 'system' | 'user' | 'assistant'
    content: str
    name: str | None = None


@dataclass(slots=True)
class ChatResponse:
    text: str | None
    model: str
    prompt_tokens: int | None
    completion_tokens: int | None
    latency_ms: int


class LLMProviderUnavailable(RuntimeError):
    """API-Key fehlt oder Endpunkt nicht erreichbar."""


class EngagementMinimaxClient:
    """Async-Client für MiniMax M2.7 über OpenAI-kompatiblen Endpunkt."""

    def __init__(
        self,
        *,
        api_key: str | None = None,
        base_url: str | None = None,
        model: str | None = None,
        timeout: float = 30.0,
    ) -> None:
        self._api_key = (
            api_key
            or os.getenv("MINIMAX_TOKEN_PLAN_KEY")
            or os.getenv("MINIMAX_API_KEY")
        )
        self._base_url = base_url or os.getenv("MINIMAX_BASE_URL") or DEFAULT_BASE_URL
        self._model = model or os.getenv("ENGAGEMENT_MINIMAX_MODEL") or DEFAULT_MODEL
        self._timeout = timeout
        self._client = None  # lazy

    def _ensure_client(self):
        if self._client is not None:
            return self._client
        if not self._api_key:
            raise LLMProviderUnavailable("MINIMAX_API_KEY not set")
        try:
            from openai import AsyncOpenAI
        except ImportError as exc:  # pragma: no cover
            raise LLMProviderUnavailable(f"openai package missing: {exc}") from exc
        self._client = AsyncOpenAI(
            api_key=self._api_key,
            base_url=self._base_url,
            timeout=self._timeout,
        )
        return self._client

    async def generate(
        self,
        *,
        system_prompt: str,
        history: list[ChatMessage],
        max_output_tokens: int = 200,
    ) -> ChatResponse:
        client = self._ensure_client()

        messages: list[dict] = [{"role": "system", "content": system_prompt}]
        for turn in history:
            entry: dict = {"role": turn.role, "content": turn.content}
            if turn.name:
                entry["name"] = turn.name
            messages.append(entry)

        started = time.perf_counter()
        try:
            response = await client.chat.completions.create(
                model=self._model,
                messages=messages,
                max_tokens=max_output_tokens,
                temperature=0.7,
            )
        except Exception as exc:
            log.warning("MiniMax-Call fehlgeschlagen: %s", type(exc).__name__)
            raise

        latency_ms = int((time.perf_counter() - started) * 1000)

        raw_text = ""
        if response.choices:
            content = response.choices[0].message.content
            raw_text = (content or "").strip()

        # Strip MiniMax reasoning blocks BEFORE silent/sanitize-check
        without_think = _THINK_RE.sub("", raw_text).strip()

        text: str | None
        if not without_think or SILENT_MARKER in without_think.lower():
            text = None
        else:
            text = _sanitize_chat_text(without_think)
            if not text:
                text = None

        prompt_tokens = getattr(response.usage, "prompt_tokens", None) if response.usage else None
        completion_tokens = (
            getattr(response.usage, "completion_tokens", None) if response.usage else None
        )

        return ChatResponse(
            text=text,
            model=self._model,
            prompt_tokens=prompt_tokens,
            completion_tokens=completion_tokens,
            latency_ms=latency_ms,
        )


def _sanitize_chat_text(text: str, *, max_len: int = 480) -> str:
    """Säubert Bot-Text bevor er an Twitch geschickt wird.

    - Newlines → Space (Twitch akzeptiert keine Multi-Line-Messages)
    - Führende `/` oder `.` weg (verhindert versehentliche Slash-Commands)
    - Strip + Max-Länge (Twitch-Limit ist 500)
    """
    cleaned = " ".join(text.split())
    while cleaned.startswith(("/", ".")):
        cleaned = cleaned[1:].lstrip()
    cleaned = cleaned.replace("@everyone", "everyone")
    if len(cleaned) > max_len:
        cleaned = cleaned[: max_len - 1].rstrip() + "…"
    return cleaned.strip()


def build_baseline_system_prompt(*, streamer_login: str) -> str:
    """V1-Minimal-System-Prompt — Persona/Threads/Lurker/Match folgen später."""
    return (
        f"Du bist ein Mitleser im Twitch-Chat von {streamer_login}. "
        "Du bist Deadlock-kundig (Heroes, Items, Builds, Patches, Meta). "
        "Du eröffnest keine Themen aus dem Nichts. "
        "Du dockst an laufende Gespräche an, baust sie aus, lässt anderen Raum. "
        "Du redest nicht über jemanden, sondern mit ihm.\n"
        "\n"
        "Sprache: Spiegele den Channel-Vibe — wenn dort deutsch geschrieben wird, "
        "antworte deutsch; wenn englisch, dann englisch. "
        "Antworten kurz, 1-2 Sätze, max ~250 Zeichen. "
        "Vermeide es, denselben Gedanken in mehreren Nachrichten zu zerlegen.\n"
        "\n"
        "Ausgabeformat: Antworte direkt, keine <think>-Blöcke, keine Meta-Kommentare. "
        "Keine /-Commands (kein /me, /ban etc.), kein @everyone.\n"
        "\n"
        f"Wenn du keinen guten Andock-Punkt siehst, antworte ausschliesslich mit {SILENT_MARKER}."
    )

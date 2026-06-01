"""Globaler Community-Sentiment für den Engagement-Layer.

Eigenständiger, entkoppelter Baustein (nicht in den Pipeline-Pfad eingehängt):
ein Background-Job wirft die letzten Chat-Nachrichten ALLER Channels zusammen und
destilliert daraus per MiniMax ein kompaktes Stimmungsbild — "wie fühlt sich
Deadlock gerade an" (Meta, Patch-Reaktionen, was nervt/gefeiert wird). Das Ergebnis
wird in ``twitch_engagement_global_sentiment`` persistiert; die Engagement-Pipeline
LIEST nur die jeweils neueste Zeile und reicht sie als ambientes Bauchgefühl in den
System-Prompt.

Halluzinations-sicher: die Destillation stützt sich AUSSCHLIESSLICH auf die echten
Nachrichten. Im Prompt wird sie als Gefühl des Bots eingespeist, nie als vorgelesene
Statistik, und die Quelle wird nie erwähnt.
"""

from __future__ import annotations

import asyncio
import logging
import re

from bot.storage.pg import query_all, query_one, transaction

from .minimax_chat import EngagementMinimaxClient

log = logging.getLogger("TwitchStreams.Engagement.GlobalSentiment")

_THINK_RE = re.compile(r"<think>.*?</think>", re.DOTALL | re.IGNORECASE)

# Pool: die letzten N user-msgs über ALLE Channels. Das Alters-Fenster ist nur ein
# Backstop gegen uralte Daten — die ORDER BY ts DESC LIMIT _POOL_LIMIT sorgt für
# Aktualität. Bei dünnem Datenstand lieber großzügig, sonst baut der Job nie.
_POOL_LIMIT = 250
_POOL_MAX_AGE_HOURS = 336  # 14 Tage Backstop
_MIN_MSGS_TO_BUILD = 8
# Reader: ist das neueste Sentiment älter, gilt es als nicht mehr "aktuell" (kein Fragment).
_FRESH_MAX_AGE_HOURS = 12
# Großzügiges Token-Budget — der M3-<think>-Block frisst viel, Budget ist vorhanden.
_BUILD_MAX_TOKENS = 4000
# Wie viele Sentiment-Zeilen behalten (Rest trimmen).
_KEEP_ROWS = 50

_SYS = (
    "Du bist ein nüchterner Analyst. Gib nur die verlangte Zusammenfassung, "
    "kein Vorwort, keine Meta."
)


def _build_user_prompt(lines: list[str]) -> str:
    block = "\n".join(f"- {m}" for m in lines)
    return (
        "Hier echte Twitch-Chat-Nachrichten aus mehreren Deadlock-Streams (zusammengeworfen). "
        "Destillier in 3-6 knappen Stichpunkten, wie sich Deadlock GERADE anfühlt — Stimmung, "
        "Meta, Patch-Reaktionen, welche Helden/Items auffallen, was nervt, was gefeiert wird. "
        "Nutze NUR was in den Nachrichten steht, erfinde NICHTS. Ist die Datenlage für einen "
        "Punkt zu dünn, lass ihn weg. Sachlich, interne Stimmungs-Notiz.\n\nNachrichten:\n"
        + block
    )


def _sync_load_pooled() -> list[str]:
    rows = query_all(
        """
        SELECT content FROM twitch_engagement_conversation
        WHERE role = 'user' AND ts > NOW() - make_interval(hours => %s)
        ORDER BY ts DESC LIMIT %s
        """,
        [int(_POOL_MAX_AGE_HOURS), int(_POOL_LIMIT)],
    )
    return [r[0].strip() for r in rows if r and r[0] and len(r[0].strip()) > 3]


def _sync_store(text: str, msg_count: int, model: str | None) -> None:
    with transaction() as conn:
        conn.execute(
            """
            INSERT INTO twitch_engagement_global_sentiment (sentiment_text, msg_count, model)
            VALUES (%s, %s, %s)
            """,
            [text, msg_count, model or ""],
        )
        conn.execute(
            """
            DELETE FROM twitch_engagement_global_sentiment
            WHERE id NOT IN (
                SELECT id FROM twitch_engagement_global_sentiment
                ORDER BY built_at DESC LIMIT %s
            )
            """,
            [int(_KEEP_ROWS)],
        )


def _sync_load_latest(max_age_hours: int):
    return query_one(
        """
        SELECT sentiment_text, built_at FROM twitch_engagement_global_sentiment
        WHERE built_at > NOW() - make_interval(hours => %s)
        ORDER BY built_at DESC LIMIT 1
        """,
        [int(max_age_hours)],
    )


async def rebuild_global_sentiment(
    *, minimax: EngagementMinimaxClient | None = None
) -> str | None:
    """Pool über alle Channels → MiniMax-Destillation → persistieren. Gibt den Text zurück."""
    lines = await asyncio.to_thread(_sync_load_pooled)
    if len(lines) < _MIN_MSGS_TO_BUILD:
        log.info("GlobalSentiment: zu wenig Material (%d msgs), skip", len(lines))
        return None

    client = minimax or EngagementMinimaxClient(timeout=180.0)
    try:
        oc = client._ensure_client()
        resp = await oc.chat.completions.create(
            model=client._model,
            messages=[
                {"role": "system", "content": _SYS},
                {"role": "user", "content": _build_user_prompt(lines)},
            ],
            max_tokens=_BUILD_MAX_TOKENS,
            temperature=0.4,
        )
    except Exception:
        log.exception("GlobalSentiment: MiniMax-Call fehlgeschlagen")
        return None

    raw = resp.choices[0].message.content if resp.choices else ""
    text = _THINK_RE.sub("", raw or "").strip()
    if not text:
        log.warning("GlobalSentiment: leere Antwort, nichts gespeichert")
        return None

    await asyncio.to_thread(_sync_store, text, len(lines), getattr(client, "_model", None))
    log.info("GlobalSentiment: neu gebaut aus %d msgs (%d chars)", len(lines), len(text))
    return text


def build_sentiment_fragment(text: str) -> str:
    if not text:
        return ""
    return (
        "So fühlt sich Deadlock in der Community gerade an — das ist DEIN eigenes Bauchgefühl "
        "zur aktuellen Lage (Meta, Patches, was nervt/gefeiert wird). Zieh daraus Meinung und "
        "Vibe, aber lies es NIEMALS als Liste oder Statistik vor und sag nie, woher du das hast. "
        "Lass nur beiläufig einfließen, was gerade zum Gespräch passt:\n"
        f"{text}"
    )


async def get_sentiment_fragment(*, max_age_hours: int = _FRESH_MAX_AGE_HOURS) -> str:
    """Neuestes (frisches) Sentiment als System-Prompt-Fragment, sonst "" ."""
    row = await asyncio.to_thread(_sync_load_latest, max_age_hours)
    if not row or not row[0]:
        return ""
    return build_sentiment_fragment(row[0])

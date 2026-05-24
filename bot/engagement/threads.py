"""Konversations-Fäden mit Lifecycle (Beziehungsführung statt Trivia-Dump).

Lifecycle: open → follow_up_due (Cron flippt due_at) → awaiting_response
(Bot fragt) → closed (Auto-Close nach 7d Inaktivität).

Extractor liest jüngste user-turns aus twitch_engagement_conversation, lässt
MiniMax JSON-strukturierte Threads vorschlagen, upsertet in twitch_user_threads.
Pipeline lädt offene Threads pro Sender und gibt sie als System-Prompt-Hint
weiter mit der Anweisung "niemals auspacken".
"""

from __future__ import annotations

import asyncio
import json
import logging
from dataclasses import dataclass
from datetime import datetime

from bot.storage.pg import query_all, transaction

from .minimax_chat import ChatMessage, EngagementMinimaxClient, LLMProviderUnavailable

log = logging.getLogger("TwitchStreams.Engagement.Threads")


@dataclass(slots=True)
class Thread:
    id: int
    twitch_user_id: str
    twitch_login: str
    channel_login: str | None
    thread_type: str
    summary: str
    due_at: datetime | None
    status: str
    last_referenced_at: datetime | None


def _sync_load_open_threads(user_id: str, channel_login: str, limit: int) -> list[tuple]:
    return query_all(
        """
        SELECT id, twitch_user_id, twitch_login, channel_login, thread_type, summary,
               due_at, status, last_referenced_at
        FROM twitch_user_threads
        WHERE twitch_user_id = %s
          AND (channel_login = %s OR channel_login IS NULL)
          AND status IN ('open', 'follow_up_due')
          AND (last_referenced_at IS NULL
               OR last_referenced_at < NOW() - INTERVAL '30 minutes')
        ORDER BY CASE WHEN status = 'follow_up_due' THEN 0 ELSE 1 END,
                 COALESCE(due_at, created_at) ASC
        LIMIT %s
        """,
        [user_id, channel_login, limit],
    )


async def load_open_threads_for_user(
    user_id: str, channel_login: str, *, limit: int = 5
) -> list[Thread]:
    if not user_id:
        return []
    rows = await asyncio.to_thread(_sync_load_open_threads, user_id, channel_login, limit)
    return [
        Thread(
            id=row[0],
            twitch_user_id=row[1],
            twitch_login=row[2],
            channel_login=row[3],
            thread_type=row[4],
            summary=row[5],
            due_at=row[6],
            status=row[7],
            last_referenced_at=row[8],
        )
        for row in rows
    ]


def threads_to_prompt_fragment(user_login: str, threads: list[Thread]) -> str:
    if not threads:
        return ""
    lines = [
        f"Was du über {user_login} (aus früheren Gesprächen) weisst — "
        "nur einsetzen wenn das Gespräch NATÜRLICH darauf führt, NIEMALS auspacken:"
    ]
    for t in threads:
        if t.status == "follow_up_due":
            marker = "↪ Follow-up wäre passend (wenn die Gelegenheit kommt)"
        else:
            marker = "•"
        lines.append(f"  {marker} ({t.thread_type}) {t.summary}")
    return "\n".join(lines)


def _sync_mark_referenced(thread_ids: list[int]) -> None:
    if not thread_ids:
        return
    with transaction() as conn:
        conn.execute(
            """
            UPDATE twitch_user_threads
               SET last_referenced_at = NOW(),
                   status = CASE
                       WHEN status = 'follow_up_due' THEN 'awaiting_response'
                       ELSE status
                   END,
                   updated_at = NOW()
             WHERE id = ANY(%s)
            """,
            [thread_ids],
        )


async def mark_referenced(thread_ids: list[int]) -> None:
    if not thread_ids:
        return
    await asyncio.to_thread(_sync_mark_referenced, thread_ids)


def _sync_auto_close_stale() -> dict[str, int]:
    counts = {"open_to_due": 0, "awaiting_to_closed": 0, "open_to_closed": 0}
    with transaction() as conn:
        cur = conn.execute(
            """
            UPDATE twitch_user_threads
               SET status = 'follow_up_due', updated_at = NOW()
             WHERE status = 'open'
               AND due_at IS NOT NULL
               AND due_at <= NOW()
            """
        )
        counts["open_to_due"] = cur.rowcount or 0
        cur = conn.execute(
            """
            UPDATE twitch_user_threads
               SET status = 'closed', updated_at = NOW()
             WHERE status = 'awaiting_response'
               AND updated_at < NOW() - INTERVAL '7 days'
            """
        )
        counts["awaiting_to_closed"] = cur.rowcount or 0
        cur = conn.execute(
            """
            UPDATE twitch_user_threads
               SET status = 'closed', updated_at = NOW()
             WHERE status = 'open'
               AND updated_at < NOW() - INTERVAL '30 days'
            """
        )
        counts["open_to_closed"] = cur.rowcount or 0
    return counts


async def auto_close_stale() -> dict[str, int]:
    return await asyncio.to_thread(_sync_auto_close_stale)


# === Thread-Extractor (Background-Job, eigentlicher Schedule in background.py) ===

_EXTRACTOR_SYSTEM_PROMPT = (
    "Du bist ein Konversations-Analyst für einen Twitch-Chat. Lies die folgenden "
    "Chat-Nachrichten und identifiziere Konversations-Fäden, die für einen späteren "
    "Follow-up wertvoll sein könnten — Dinge mit echtem zwischenmenschlichem Wert: "
    "anstehende Ereignisse (OP, Reise, Prüfung), kürzliche Erlebnisse die "
    "nachgefragt werden könnten, oder klare Dauerinteressen (Lieblings-Hero, Hobby).\n"
    "\n"
    "Antworte AUSSCHLIESSLICH als JSON-Array (kein Markdown, kein Vortext). Jeder "
    "Eintrag hat die Felder: twitch_user_id, twitch_login, thread_type "
    "(\"upcoming_event\"|\"recent_experience\"|\"recurring_interest\"|\"life_status\"), "
    "summary (knapp, max 80 Zeichen), due_at_iso (YYYY-MM-DD, optional, nur wenn ein "
    "konkretes Datum genannt wurde).\n"
    "\n"
    "Wenn nichts mit echtem Wert identifizierbar ist, antworte mit []. Erfinde nichts."
)


def _sync_load_recent_user_turns(channel_login: str, hours: int, limit: int) -> list[tuple]:
    return query_all(
        f"""
        SELECT twitch_user_id, twitch_login, content, ts
        FROM twitch_engagement_conversation
        WHERE channel_login = %s
          AND role = 'user'
          AND twitch_user_id IS NOT NULL
          AND ts > NOW() - INTERVAL '{int(hours)} hours'
        ORDER BY ts DESC
        LIMIT %s
        """,
        [channel_login, limit],
    )


def _sync_upsert_thread(
    *,
    twitch_user_id: str,
    twitch_login: str,
    channel_login: str,
    thread_type: str,
    summary: str,
    due_at: datetime | None,
) -> bool:
    """Insert nur wenn kein offener Thread mit gleichem (user, type, summary) existiert."""
    with transaction() as conn:
        existing = conn.execute(
            """
            SELECT id FROM twitch_user_threads
            WHERE twitch_user_id = %s
              AND COALESCE(channel_login, '') = %s
              AND thread_type = %s
              AND LOWER(summary) = LOWER(%s)
              AND status IN ('open', 'follow_up_due', 'awaiting_response')
            LIMIT 1
            """,
            [twitch_user_id, channel_login or "", thread_type, summary],
        ).fetchone()
        if existing:
            return False
        conn.execute(
            """
            INSERT INTO twitch_user_threads
                (twitch_user_id, twitch_login, channel_login, thread_type, summary, due_at,
                 status, created_at, updated_at)
            VALUES (%s, %s, %s, %s, %s, %s, 'open', NOW(), NOW())
            """,
            [twitch_user_id, twitch_login, channel_login, thread_type, summary, due_at],
        )
        return True


def _strip_codeblock(text: str) -> str:
    cleaned = text.strip()
    if cleaned.startswith("```"):
        cleaned = cleaned.lstrip("`")
        if cleaned.lower().startswith("json"):
            cleaned = cleaned[4:]
        if cleaned.endswith("```"):
            cleaned = cleaned[:-3]
    return cleaned.strip()


async def extract_threads(
    channel_login: str,
    *,
    minimax: EngagementMinimaxClient,
    hours: int = 6,
    limit: int = 80,
) -> int:
    """Extrahiert Threads aus jüngsten Chat-Turns. Returns Anzahl neu eingefügter."""
    rows = await asyncio.to_thread(_sync_load_recent_user_turns, channel_login, hours, limit)
    if not rows:
        return 0

    rows_chrono = list(reversed(rows))
    lines = [
        f"[{ts.isoformat(timespec='seconds')}] ({uid}|{login}): {content}"
        for uid, login, content, ts in rows_chrono
    ]
    chat_block = "\n".join(lines)
    user_prompt = (
        f"Channel: {channel_login}\n"
        f"Zeitfenster: letzte {hours} Stunden\n"
        "\n"
        f"{chat_block}\n"
    )

    try:
        response = await minimax.generate(
            system_prompt=_EXTRACTOR_SYSTEM_PROMPT,
            history=[ChatMessage(role="user", content=user_prompt)],
            max_output_tokens=800,
        )
    except LLMProviderUnavailable:
        log.warning("Thread-Extractor: MiniMax-Provider nicht verfügbar")
        return 0
    except Exception:
        log.exception("Thread-Extractor: MiniMax-Call fehlgeschlagen")
        return 0

    if not response.text:
        return 0

    cleaned = _strip_codeblock(response.text)
    try:
        items = json.loads(cleaned)
    except json.JSONDecodeError:
        log.warning("Thread-Extractor: JSON-Parse fehlgeschlagen für %r", cleaned[:200])
        return 0
    if not isinstance(items, list):
        log.warning("Thread-Extractor: Top-Level kein Array")
        return 0

    inserted = 0
    for item in items:
        if not isinstance(item, dict):
            continue
        twitch_user_id = str(item.get("twitch_user_id") or "").strip()
        twitch_login = str(item.get("twitch_login") or "").strip()
        thread_type = str(item.get("thread_type") or "").strip()
        summary = str(item.get("summary") or "").strip()
        if not (twitch_user_id and twitch_login and thread_type and summary):
            continue
        if thread_type not in (
            "upcoming_event",
            "recent_experience",
            "recurring_interest",
            "life_status",
        ):
            continue
        due_at: datetime | None = None
        raw_due = item.get("due_at_iso")
        if raw_due:
            try:
                due_at = datetime.fromisoformat(str(raw_due))
            except ValueError:
                due_at = None
        try:
            if await asyncio.to_thread(
                _sync_upsert_thread,
                twitch_user_id=twitch_user_id,
                twitch_login=twitch_login,
                channel_login=channel_login,
                thread_type=thread_type,
                summary=summary[:80],
                due_at=due_at,
            ):
                inserted += 1
        except Exception:
            log.exception("Thread-Extractor: Upsert fehlgeschlagen für %s", twitch_user_id)
    if inserted:
        log.info("Thread-Extractor: %d neue Threads für %s", inserted, channel_login)
    return inserted

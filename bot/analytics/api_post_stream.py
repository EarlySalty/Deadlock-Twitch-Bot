"""
Post-Stream KI-Analyse via Minimax.
Wird nach stream.offline EventSub automatisch fuer Plan-User getriggert.
"""

from __future__ import annotations

import json
import logging
from typing import Any

from aiohttp import web

from ..entitlements.resolver import resolve_plan_snapshot_for_login
from ..storage import pg as storage
from .api_ai import (
    AI_MODEL_MINIMAX,
    CLAUDE_MODEL,
    MINIMAX_MODEL,
    _get_async_client,
    _get_minimax_client,
    _plan_ai_model,
)
from .error_utils import analytics_internal_error_response

log = logging.getLogger("TwitchStreams.PostStreamAnalysis")

_WORD_GROUP_PROMPT_TEMPLATE = """Du analysierst den Twitch-Chat eines Gaming-Streams. Es wurden {n} Chat-Nachrichten erfasst.

Erkenne 5-10 thematische Wortgruppen (z.B. Lob, Kritik, Emote-Spam, Hero-Bezuege, Gameplay-Feedback, Fragen, Negativitaet, Hype-Momente, Community-Inside-Jokes).

Fuer jede Gruppe:
- group_name: kurzer deutscher Name (2-3 Woerter max)
- keywords: haeufigste Woerter/Phrasen dieser Gruppe (max. 15, Kleinbuchstaben)
- message_count: geschaetzte Anzahl Nachrichten dieser Gruppe

Chat-Nachrichten (Stichprobe):
{sample}

Antworte NUR als JSON-Array ohne weitere Erklaerungen:
[{{"group_name": "...", "keywords": ["..."], "message_count": 0}}]"""

_REPORT_PROMPT_TEMPLATE = """Du bist ein Twitch-Analytics-Experte. Erstelle eine direkte, ehrliche Post-Stream-Analyse.

Stream-Daten:
- Streamer: {streamer}
- Dauer: {duration_min} Minuten
- Oe Viewer: {avg_viewers}
- Peak Viewer: {peak_viewers}
- Chat-Nachrichten gesamt: {total_messages}
- Aktive Chatter: {unique_chatters}
- Sentiment: {sentiment_label} (Score: {sentiment_score:.0%}, Trend: {sentiment_trend})
- Chat-Themen: {topic_breakdown}
- Erkannte Wortgruppen: {word_groups_summary}
- Hype-Spitzen: {spike_count}
- Follower-Delta: {followers_delta:+d}

Analysiere ehrlich und konkret:
1. Was lief gut (2-4 Punkte mit kurzer Begruendung)
2. Was lief schlecht / Verbesserungspotenzial (2-4 Punkte)
3. Erkennbare Veraenderungen zum typischen Stream (nur wenn wirklich auffaellig, sonst leer)
4. Handlungsbare Empfehlungen (1-3 Punkte)

Antworte NUR als JSON ohne weitere Erklaerungen:
{{"gut": [{{"punkt": "...", "begruendung": "..."}}], "schlecht": [{{"punkt": "...", "begruendung": "..."}}], "veraenderungen": [{{"aspekt": "...", "detail": "..."}}], "empfehlungen": [{{"trend": "...", "empfehlung": "..."}}]}}"""


def _extract_json_object(text: str) -> str | None:
    """Extrahiere den ersten vollstaendigen JSON-Block ({...} oder [...])."""
    for start_char, end_char in (("{", "}"), ("[", "]")):
        start = text.find(start_char)
        if start == -1:
            continue
        depth = 0
        in_string = False
        escape_next = False
        for index, ch in enumerate(text[start:], start):
            if escape_next:
                escape_next = False
                continue
            if ch == "\\" and in_string:
                escape_next = True
                continue
            if ch == '"':
                in_string = not in_string
                continue
            if in_string:
                continue
            if ch == start_char:
                depth += 1
            elif ch == end_char:
                depth -= 1
                if depth == 0:
                    return text[start : index + 1]
    return None


def _clean_prompt_message(message: str, *, limit: int = 240) -> str:
    normalized = " ".join(str(message or "").split())
    if len(normalized) <= limit:
        return normalized
    return normalized[: limit - 3].rstrip() + "..."


async def _call_minimax(prompt: str) -> str:
    client = _get_minimax_client()
    completion = await client.chat.completions.create(
        model=MINIMAX_MODEL,
        messages=[{"role": "user", "content": prompt}],
        temperature=0.4,
        max_tokens=4000,
    )
    choices = getattr(completion, "choices", None) or []
    if not choices:
        return ""
    return getattr(choices[0].message, "content", "") or ""


async def _call_claude(prompt: str) -> str:
    client = _get_async_client()
    response = await client.messages.create(
        model=CLAUDE_MODEL,
        max_tokens=4000,
        messages=[{"role": "user", "content": prompt}],
    )
    if not response.content:
        return ""
    return getattr(response.content[0], "text", "") or ""


async def _load_session_chat_data(session_id: int) -> dict[str, Any]:
    """Lade Session-Metadaten und Chatnachrichten fuer die Analyse."""
    with storage.readonly_connection() as conn:
        session_row = conn.execute(
            """
            SELECT s.streamer_login,
                   s.started_at,
                   s.ended_at,
                   s.duration_seconds,
                   COALESCE(s.avg_viewers, 0) AS avg_viewers,
                   COALESCE(s.peak_viewers, 0) AS peak_viewers,
                   COALESCE(s.followers_delta, 0) AS followers_delta
              FROM twitch_stream_sessions s
             WHERE s.id = %s
            """,
            (session_id,),
        ).fetchone()
        if not session_row:
            return {}
        session = dict(session_row.items()) if hasattr(session_row, "items") else {
            "streamer_login": session_row[0],
            "started_at": session_row[1],
            "ended_at": session_row[2],
            "duration_seconds": session_row[3],
            "avg_viewers": session_row[4],
            "peak_viewers": session_row[5],
            "followers_delta": session_row[6],
        }

        message_rows = conn.execute(
            """
            SELECT content
              FROM twitch_chat_messages
             WHERE session_id = %s
               AND is_command = FALSE
               AND content IS NOT NULL
               AND length(content) > 1
             ORDER BY message_ts
             LIMIT 1500
            """,
            (session_id,),
        ).fetchall()
        messages = [
            str(row["content"] if hasattr(row, "keys") else row[0]).strip()
            for row in message_rows
            if str(row["content"] if hasattr(row, "keys") else row[0]).strip()
        ]

        chatter_row = conn.execute(
            """
            SELECT COUNT(DISTINCT chatter_login)
              FROM twitch_chat_messages
             WHERE session_id = %s
               AND chatter_login IS NOT NULL
            """,
            (session_id,),
        ).fetchone()
        unique_chatters = int((chatter_row[0] if chatter_row else 0) or 0)

    duration_min = max(1, int((session.get("duration_seconds") or 0) // 60))
    return {
        "session": session,
        "messages": messages,
        "duration_min": duration_min,
        "unique_chatters": unique_chatters,
    }


async def _generate_word_groups(messages: list[str], call_ai) -> list[dict[str, Any]]:
    """Erzeuge thematische Wortgruppen ueber ein KI-Modell."""
    if not messages:
        return []

    step = max(1, len(messages) // 300)
    sample = messages[::step][:300]
    sample_text = "\n".join(f"- {_clean_prompt_message(message)}" for message in sample)
    prompt = _WORD_GROUP_PROMPT_TEMPLATE.format(n=len(messages), sample=sample_text)

    try:
        raw = await call_ai(prompt)
        extracted = _extract_json_object(raw)
        if extracted and extracted.startswith("["):
            groups = json.loads(extracted)
            if isinstance(groups, list):
                normalized_groups: list[dict[str, Any]] = []
                for group in groups:
                    if not isinstance(group, dict):
                        continue
                    group_name = str(group.get("group_name", "")).strip()
                    if not group_name:
                        continue
                    keywords = [
                        str(keyword).strip().lower()
                        for keyword in (group.get("keywords") or [])
                        if str(keyword).strip()
                    ][:15]
                    normalized_groups.append(
                        {
                            "group_name": group_name[:80],
                            "keywords": keywords,
                            "message_count": max(0, int(group.get("message_count", 0) or 0)),
                        }
                    )
                return normalized_groups[:10]
    except Exception:
        log.exception("Wortgruppen-Analyse fehlgeschlagen")
    return []


async def _generate_report(
    data: dict[str, Any],
    word_groups: list[dict[str, Any]],
    call_ai,
) -> dict[str, Any]:
    """Erzeuge den strukturierten Post-Stream-Report."""
    session = data["session"]
    messages = data["messages"]
    word_groups_summary = ", ".join(
        f"{group['group_name']} ({group['message_count']}x)"
        for group in word_groups[:6]
    ) or "keine"

    pos_words = {
        "gg",
        "nice",
        "pog",
        "poggers",
        "insane",
        "clean",
        "sick",
        "geil",
        "krass",
        "stark",
        "super",
        "amazing",
        "legendary",
        "godlike",
    }
    neg_words = {
        "trash",
        "boring",
        "cringe",
        "bad",
        "worst",
        "throw",
        "mies",
        "schlecht",
        "nervig",
        "dogwater",
        "washed",
        "garbage",
    }
    pos_count = sum(1 for message in messages if any(word in message.lower() for word in pos_words))
    neg_count = sum(1 for message in messages if any(word in message.lower() for word in neg_words))
    total_scored = max(1, pos_count + neg_count)
    sentiment_score = pos_count / total_scored
    sentiment_label = (
        "positiv"
        if sentiment_score > 0.6
        else "negativ" if sentiment_score < 0.4 else "neutral"
    )
    sentiment_trend = "steigend" if pos_count > neg_count else "fallend"

    topic_words = {
        "Gameplay": ["play", "build", "item", "tower", "kill", "die", "push", "farm", "fight"],
        "Chat-Reaktionen": ["lol", "lmao", "haha", "omg", "wtf", "xd"],
        "Fragen": ["?", "wie", "was", "wann", "warum", "wieso", "who", "when", "why", "how"],
        "Lob/Hype": ["gg", "pog", "nice", "insane", "geil", "stark", "krass"],
    }
    topic_counts: dict[str, int] = {}
    for topic, keywords in topic_words.items():
        topic_counts[topic] = sum(
            1 for message in messages if any(keyword in message.lower() for keyword in keywords)
        )
    topic_breakdown = ", ".join(
        f"{topic}: {count}"
        for topic, count in sorted(topic_counts.items(), key=lambda item: -item[1])
        if count > 0
    )

    prompt = _REPORT_PROMPT_TEMPLATE.format(
        streamer=session.get("streamer_login", "?"),
        duration_min=data["duration_min"],
        avg_viewers=int(session.get("avg_viewers") or 0),
        peak_viewers=int(session.get("peak_viewers") or 0),
        total_messages=len(messages),
        unique_chatters=data["unique_chatters"],
        sentiment_label=sentiment_label,
        sentiment_score=sentiment_score,
        sentiment_trend=sentiment_trend,
        topic_breakdown=topic_breakdown or "keine Daten",
        word_groups_summary=word_groups_summary,
        spike_count=0,
        followers_delta=int(session.get("followers_delta") or 0),
    )

    try:
        raw = await call_ai(prompt)
        extracted = _extract_json_object(raw)
        if extracted and extracted.startswith("{"):
            report = json.loads(extracted)
            if isinstance(report, dict):
                return {
                    "gut": report.get("gut") or [],
                    "schlecht": report.get("schlecht") or [],
                    "veraenderungen": report.get("veraenderungen") or [],
                    "empfehlungen": report.get("empfehlungen") or [],
                }
    except Exception:
        log.exception("Report-Generierung fehlgeschlagen")
    return {
        "gut": [],
        "schlecht": [],
        "veraenderungen": [],
        "empfehlungen": [],
    }


async def trigger_post_stream_analysis(
    streamer_login: str,
    session_id: int | None = None,
) -> None:
    """Triggere nach Stream-Ende eine planbasierte Post-Stream-Analyse."""
    streamer = str(streamer_login or "").strip().lower()
    if not streamer:
        return

    try:
        model = _plan_ai_model(streamer)
        if model is None:
            log.debug("PostStream: %s hat kein KI-Entitlement, ueberspringe", streamer)
            return
    except Exception:
        log.exception("PostStream: Plan-Check fehlgeschlagen fuer %s", streamer)
        return

    if session_id is None:
        try:
            with storage.readonly_connection() as conn:
                session_row = conn.execute(
                    """
                    SELECT id
                      FROM twitch_stream_sessions
                     WHERE streamer_login = %s
                       AND ended_at IS NOT NULL
                     ORDER BY ended_at DESC
                     LIMIT 1
                    """,
                    (streamer,),
                ).fetchone()
            if not session_row:
                log.info("PostStream: Keine abgeschlossene Session fuer %s", streamer)
                return
            session_id = int(session_row["id"] if hasattr(session_row, "keys") else session_row[0])
        except Exception:
            log.exception("PostStream: Session-Lookup fehlgeschlagen fuer %s", streamer)
            return

    try:
        with storage.readonly_connection() as conn:
            existing = conn.execute(
                """
                SELECT id
                  FROM twitch_stream_ai_reports
                 WHERE session_id = %s
                   AND status IN ('done', 'pending')
                 LIMIT 1
                """,
                (session_id,),
            ).fetchone()
        if existing:
            log.debug("PostStream: Report fuer Session %d existiert bereits", session_id)
            return
    except Exception:
        log.debug("PostStream: Vorabpruefung Reports nicht verfuegbar", exc_info=True)

    report_id: int | None = None
    try:
        with storage.transaction() as conn:
            report_row = conn.execute(
                """
                INSERT INTO twitch_stream_ai_reports (
                    session_id,
                    streamer_login,
                    model,
                    status
                )
                VALUES (%s, %s, %s, 'pending')
                RETURNING id
                """,
                (session_id, streamer, model),
            ).fetchone()
            if not report_row:
                raise RuntimeError("Report-Insert lieferte keine ID")
            report_id = int(report_row["id"] if hasattr(report_row, "keys") else report_row[0])
    except Exception:
        log.exception(
            "PostStream: Konnte Report-Eintrag nicht anlegen fuer %s Session %d",
            streamer,
            session_id,
        )
        return

    log.info("PostStream: Starte Analyse fuer %s Session %d (model=%s)", streamer, session_id, model)
    call_ai = _call_minimax if model == AI_MODEL_MINIMAX else _call_claude

    try:
        data = await _load_session_chat_data(session_id)
        if not data or not data.get("messages"):
            raise ValueError("Keine Chat-Daten fuer Session")

        word_groups = await _generate_word_groups(data["messages"], call_ai)
        report = await _generate_report(data, word_groups, call_ai)

        if word_groups:
            try:
                with storage.transaction() as conn:
                    conn.execute(
                        "DELETE FROM twitch_chat_word_groups WHERE session_id = %s",
                        (session_id,),
                    )
                    for group in word_groups:
                        conn.execute(
                            """
                            INSERT INTO twitch_chat_word_groups (
                                session_id,
                                streamer_login,
                                group_name,
                                keywords,
                                message_count
                            )
                            VALUES (%s, %s, %s, %s, %s)
                            """,
                            (
                                session_id,
                                streamer,
                                group["group_name"],
                                group["keywords"],
                                group["message_count"],
                            ),
                        )
            except Exception:
                log.warning("PostStream: Wortgruppen-Insert fehlgeschlagen", exc_info=True)

        with storage.transaction() as conn:
            conn.execute(
                """
                UPDATE twitch_stream_ai_reports
                   SET status = 'done',
                       report_json = %s,
                       word_groups_json = %s,
                       generated_at = NOW(),
                       error = NULL
                 WHERE id = %s
                """,
                (json.dumps(report), json.dumps(word_groups), report_id),
            )
        log.info("PostStream: Analyse fuer %s Session %d abgeschlossen", streamer, session_id)
    except Exception as exc:
        log.exception("PostStream: Analyse fehlgeschlagen fuer %s Session %d", streamer, session_id)
        try:
            with storage.transaction() as conn:
                conn.execute(
                    """
                    UPDATE twitch_stream_ai_reports
                       SET status = 'failed',
                           error = %s
                     WHERE id = %s
                    """,
                    (str(exc)[:500], report_id),
                )
        except Exception:
            log.debug("PostStream: Fehlerstatus konnte nicht persistiert werden", exc_info=True)


class _AnalyticsPostStreamMixin:
    """API-Endpunkt fuer Post-Stream-Reports im Dashboard."""

    async def _api_v2_stream_report(self, request: web.Request) -> web.Response:
        self._require_v2_auth(request)

        streamer = str(request.rel_url.query.get("streamer") or "").strip().lower()
        session_id_raw = str(request.rel_url.query.get("session_id") or "").strip()
        session_id = int(session_id_raw) if session_id_raw.isdigit() else None
        if not streamer:
            return web.json_response({"error": "streamer required"}, status=400)

        auth_level = self._get_auth_level(request)
        session = self._get_dashboard_session(request) or {}
        session_login = str(session.get("twitch_login") or "").strip().lower()
        if auth_level not in ("localhost", "admin") and streamer != session_login:
            return web.json_response({"error": "forbidden"}, status=403)

        try:
            snapshot = resolve_plan_snapshot_for_login(streamer)
            entitlements = set(snapshot.get("entitlements") or [])
            if "analytics.ai_full" not in entitlements and "analytics.ai_mini" not in entitlements:
                return web.json_response({"error": "plan_required"}, status=403)
        except Exception:
            log.exception("PostStream API: Plan-Check fehlgeschlagen fuer %s", streamer)
            return analytics_internal_error_response(
                error="Post-Stream-Report konnte nicht geladen werden.",
                code="stream_report_plan_check_failed",
            )

        try:
            with storage.readonly_connection() as conn:
                if session_id is not None:
                    row = conn.execute(
                        """
                        SELECT *
                          FROM twitch_stream_ai_reports
                         WHERE session_id = %s
                           AND streamer_login = %s
                         ORDER BY generated_at DESC
                         LIMIT 1
                        """,
                        (session_id, streamer),
                    ).fetchone()
                else:
                    row = conn.execute(
                        """
                        SELECT *
                          FROM twitch_stream_ai_reports
                         WHERE streamer_login = %s
                         ORDER BY generated_at DESC
                         LIMIT 1
                        """,
                        (streamer,),
                    ).fetchone()
        except Exception:
            log.exception("PostStream API: Report-Lookup fehlgeschlagen fuer %s", streamer)
            return analytics_internal_error_response(
                error="Post-Stream-Report konnte nicht geladen werden.",
                code="stream_report_load_failed",
            )

        if not row:
            return web.json_response({"empty": True, "streamer": streamer})

        if hasattr(row, "items"):
            payload = dict(row.items())
        else:
            columns = (
                "id",
                "session_id",
                "streamer_login",
                "model",
                "generated_at",
                "status",
                "report_json",
                "word_groups_json",
                "error",
            )
            payload = dict(zip(columns, row, strict=False))

        report = payload.get("report_json") or {}
        word_groups = payload.get("word_groups_json") or []
        if isinstance(report, str):
            try:
                report = json.loads(report)
            except Exception:
                report = {}
        if isinstance(word_groups, str):
            try:
                word_groups = json.loads(word_groups)
            except Exception:
                word_groups = []

        return web.json_response(
            {
                "session_id": payload.get("session_id"),
                "model": payload.get("model"),
                "generated_at": str(payload.get("generated_at") or ""),
                "status": payload.get("status"),
                "report": report,
                "word_groups": word_groups,
                "error": payload.get("error"),
            }
        )

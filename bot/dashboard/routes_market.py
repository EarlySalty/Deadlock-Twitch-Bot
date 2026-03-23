"""Route group for market research handlers."""

from __future__ import annotations

from typing import Any

from aiohttp import web

from .pages import build_market_research_page
from .route_deps import MarketRouteDeps


def build_route_defs(server: Any) -> list[web.RouteDef]:
    """Return route definitions for market research routes."""
    return [
        web.get("/twitch/market", server.market_research),
        web.get("/twitch/api/market_data", server.api_market_data),
    ]


async def market_research(server: Any, request: web.Request) -> web.StreamResponse:
    """Serve the internal Market Research dashboard."""
    server._require_token(request)
    return web.Response(text=build_market_research_page(), content_type="text/html")


async def api_market_data(
    server: Any,
    request: web.Request,
    *,
    deps: MarketRouteDeps,
) -> web.Response:
    """API providing aggregated data for market research including Meta & Sentiment."""
    json_module = deps.json
    log = deps.log
    storage_module = deps.storage
    uuid4_fn = deps.uuid4

    admin_token = request.headers.get("X-Admin-Token")
    if not (
        server._is_local_request(request)
        or server._is_discord_admin_request(request)
        or server._check_admin_token(admin_token)
    ):
        return web.json_response({"error": "Unauthorized"}, status=401)

    try:
        with storage_module.readonly_connection() as conn:
            def _to_iso(val: Any) -> Any:
                return val.isoformat() if hasattr(val, "isoformat") else val

            def _json_default(obj: Any) -> str:
                return obj.isoformat() if hasattr(obj, "isoformat") else str(obj)

            rows = conn.execute(
                """
                    SELECT s.twitch_login, l.last_viewer_count
                    FROM twitch_streamers s
                    LEFT JOIN twitch_live_state l ON s.twitch_user_id = l.twitch_user_id
                    WHERE s.is_monitored_only = 1
                """
            ).fetchall()

            channels = []
            total_viewers = 0

            for row in rows:
                login = row[0]
                viewers = row[1] or 0
                total_viewers += viewers

                chat_stats = conn.execute(
                    """
                        SELECT COUNT(*), COUNT(DISTINCT chatter_login)
                        FROM twitch_chat_messages
                        WHERE streamer_login = %s
                          AND message_ts >= CURRENT_TIMESTAMP - INTERVAL '1 hour'
                    """,
                    (login,),
                ).fetchone()

                msgs = chat_stats[0] or 0
                active_chatters = chat_stats[1] or 0
                session_id_row = conn.execute(
                    "SELECT active_session_id FROM twitch_live_state WHERE streamer_login = %s",
                    (login,),
                ).fetchone()

                lurkers = 0
                total_connected = active_chatters
                if session_id_row and session_id_row[0]:
                    lurker_stats = conn.execute(
                        """
                            SELECT COUNT(*), SUM(CASE WHEN messages = 0 THEN 1 ELSE 0 END)
                            FROM twitch_session_chatters WHERE session_id = %s
                        """,
                        (session_id_row[0],),
                    ).fetchone()
                    if lurker_stats:
                        total_connected = lurker_stats[0] or active_chatters
                        lurkers = lurker_stats[1] or 0

                channels.append(
                    {
                        "login": login,
                        "viewers": viewers,
                        "is_live": viewers > 0,
                        "chat_health": min(100, (active_chatters / max(1, viewers)) * 100)
                        if viewers > 0
                        else 0,
                        "lurker_ratio": (lurkers / max(1, total_connected)) * 100,
                        "msg_per_min": msgs / 60.0,
                        "top_topic": "n/a",
                    }
                )

            channels.sort(key=lambda item: item["viewers"], reverse=True)
            avg_health = sum(item["chat_health"] for item in channels) / max(1, len(channels))
            avg_lurker = sum(item["lurker_ratio"] for item in channels) / max(1, len(channels))

            history_rows = conn.execute(
                """
                    SELECT ts_utc, SUM(viewer_count) as total_viewers, COUNT(DISTINCT streamer) as streamer_count
                    FROM twitch_stats_category
                    WHERE ts_utc >= CURRENT_TIMESTAMP - INTERVAL '24 hours'
                    GROUP BY ts_utc
                    ORDER BY ts_utc ASC
                """
            ).fetchall()
            market_history = [
                {"ts": _to_iso(row[0]), "total_viewers": row[1], "streamer_count": row[2]}
                for row in history_rows
            ]

            question_rows = conn.execute(
                """
                    SELECT content, streamer_login, message_ts
                    FROM twitch_chat_messages
                    WHERE message_ts >= CURRENT_TIMESTAMP - INTERVAL '6 hours'
                      AND content LIKE %s
                      AND length(content) > 10
                    ORDER BY message_ts DESC
                    LIMIT 20
                """,
                ("%?%",),
            ).fetchall()
            questions = [
                {"content": row[0], "streamer": row[1], "ts": _to_iso(row[2])}
                for row in question_rows
            ]

            deadlock_terms = [
                "abrams",
                "bebop",
                "dynamo",
                "grey talon",
                "haze",
                "infernus",
                "ivy",
                "kelvin",
                "lady geist",
                "mcginnis",
                "mo & krill",
                "paradox",
                "pocket",
                "seven",
                "vindicta",
                "viscous",
                "warden",
                "wraith",
                "yamato",
                "lash",
                "shiv",
                "urn",
                "midboss",
                "soul",
                "flex slot",
                "build",
                "op",
                "nerf",
                "buff",
                "patch",
            ]
            recent_msgs = conn.execute(
                "SELECT content FROM twitch_chat_messages WHERE message_ts >= CURRENT_TIMESTAMP - INTERVAL '1 hour'"
            ).fetchall()

            term_counts = {term: 0 for term in deadlock_terms}
            sentiment = {"positive": 0, "negative": 0, "neutral": 0}
            pos_words = {"pog", "gg", "nice", "cool", "krass", "lol", "win", "stark"}
            neg_words = {"rip", "bad", "lose", "troll", "cringe", "throw", "sucks", "lag"}

            for row in recent_msgs:
                content = (row[0] or "").lower()
                for term in deadlock_terms:
                    if term in content:
                        term_counts[term] += 1
                is_pos = any(word in content for word in pos_words)
                is_neg = any(word in content for word in neg_words)
                if is_pos and not is_neg:
                    sentiment["positive"] += 1
                elif is_neg and not is_pos:
                    sentiment["negative"] += 1
                else:
                    sentiment["neutral"] += 1

            meta_snapshot = sorted(
                [{"term": key, "count": value} for key, value in term_counts.items() if value > 0],
                key=lambda item: item["count"],
                reverse=True,
            )[:10]
            total_sent = sum(sentiment.values()) or 1
            sent_data = {
                "positive": sentiment["positive"],
                "negative": sentiment["negative"],
                "neutral": sentiment["neutral"],
                "pos_pct": round(sentiment["positive"] / total_sent * 100, 1),
                "neg_pct": round(sentiment["negative"] / total_sent * 100, 1),
                "neu_pct": round(sentiment["neutral"] / total_sent * 100, 1),
            }

            top_logins = [item["login"] for item in channels[:5]]
            overlap = []
            if len(top_logins) >= 2:
                login_slots = (top_logins + ["!unused!"] * 5)[:5]
                rows_overlap = conn.execute(
                    """
                        SELECT c1.streamer_login, c2.streamer_login, COUNT(DISTINCT c1.chatter_login)
                        FROM twitch_chat_messages c1
                        JOIN twitch_chat_messages c2 ON c1.chatter_login = c2.chatter_login AND c1.streamer_login < c2.streamer_login
                        WHERE c1.message_ts >= CURRENT_TIMESTAMP - INTERVAL '6 hours'
                          AND c2.message_ts >= CURRENT_TIMESTAMP - INTERVAL '6 hours'
                          AND c1.streamer_login IN (%s, %s, %s, %s, %s)
                          AND c2.streamer_login IN (%s, %s, %s, %s, %s)
                        GROUP BY 1, 2 ORDER BY 3 DESC LIMIT 5
                    """,
                    tuple(login_slots + login_slots),
                ).fetchall()
                overlap = [{"a": row[0], "b": row[1], "shared": row[2]} for row in rows_overlap]

            payload = {
                "total_monitored": len(channels),
                "total_viewers": total_viewers,
                "avg_chat_health": avg_health,
                "avg_lurker_ratio": avg_lurker,
                "total_messages": len(recent_msgs),
                "market_history": market_history,
                "questions": questions,
                "channels": channels,
                "meta_snapshot": meta_snapshot,
                "sentiment": sent_data,
                "overlap": overlap,
            }

            return web.json_response(
                payload,
                dumps=lambda data: json_module.dumps(data, default=_json_default),
            )
    except Exception:
        error_id = uuid4_fn().hex[:12]
        log.exception("Market API Error id=%s", error_id)
        return web.json_response(
            {
                "error": "market_data_failed",
                "error_id": error_id,
            },
            status=500,
        )

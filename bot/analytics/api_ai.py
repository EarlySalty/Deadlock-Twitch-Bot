"""
Analytics API v2 – KI Analyse Mixin.

Admin-only endpoint: Collects all analytics data and feeds it to Claude Opus
for a deep 10-point improvement analysis.
"""

from __future__ import annotations

import json
import logging
import os
from datetime import UTC, datetime, timedelta
from typing import Any

from aiohttp import web

from ..core.llm_providers import get_anthropic_client, get_minimax_client
from ..entitlements.resolver import resolve_plan_snapshot_for_login
from .error_utils import analytics_internal_error_response
from ..storage import pg as storage

log = logging.getLogger("TwitchStreams.AnalyticsV2.AI")

_anthropic_client = None
_minimax_client = None
_DOW_NAMES = ["So", "Mo", "Di", "Mi", "Do", "Fr", "Sa"]
_ai_table_ready = False
_in_progress_analyses: set[str] = set()  # streamer logins currently being analysed
_chat_sessions: dict[str, dict[str, Any]] = {}
_minimax_hourly_counts: dict[str, tuple[int, datetime]] = {}

AI_MODEL_OPUS = "opus"
AI_MODEL_MINIMAX = "minimax"
CLAUDE_MODEL = "claude-opus-4-6"
MINIMAX_MODEL = "MiniMax-M2.7"
MINIMAX_BASE_URL = "https://api.minimax.io/v1"
MAX_USER_CONTEXT_CHARS = 2000
MINIMAX_HOURLY_FOLLOW_UP_LIMIT = 10
OPUS_SESSION_FOLLOW_UP_LIMIT = 3
CHAT_SESSION_RETENTION_HOURS = 24
_DDC_PENTEST_DISABLE_RATE_LIMITS = str(
    os.getenv("DDC_PENTEST_DISABLE_RATE_LIMITS", "0")
).strip().lower() not in {"", "0", "false", "no", "off"}


def _extract_json_array(text: str) -> str | None:
    """Return the first complete JSON array from *text*, correctly skipping ] inside strings.

    Returns None if the array is not fully terminated (truncated response).
    """
    start = text.find("[")
    if start == -1:
        return None
    depth = 0
    in_string = False
    escape_next = False
    for i, ch in enumerate(text[start:], start):
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
        if ch == "[":
            depth += 1
        elif ch == "]":
            depth -= 1
            if depth == 0:
                return text[start : i + 1]
    return None  # truncated – no matching ]


def _ensure_ai_table(conn) -> None:
    """Create ai_analyses table if it doesn't exist (runs once per process)."""
    global _ai_table_ready
    if _ai_table_ready:
        return
    conn.execute("""
        CREATE TABLE IF NOT EXISTS ai_analyses (
            id          BIGSERIAL PRIMARY KEY,
            streamer    TEXT        NOT NULL,
            days        INTEGER     NOT NULL,
            model       TEXT        NOT NULL,
            generated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            data_snapshot JSONB     NOT NULL,
            points      JSONB       NOT NULL
        )
    """)
    conn.execute("""
        CREATE INDEX IF NOT EXISTS idx_ai_analyses_streamer_ts
        ON ai_analyses (streamer, generated_at DESC)
    """)
    _ai_table_ready = True


def _get_async_client():
    """Lazy-initialize AsyncAnthropic client via shared provider factory."""
    global _anthropic_client
    if _anthropic_client is not None:
        return _anthropic_client

    _anthropic_client = get_anthropic_client(timeout=240.0)
    return _anthropic_client


def _get_minimax_client():
    """Lazy-initialize AsyncOpenAI client for MiniMax via shared provider factory."""
    global _minimax_client
    if _minimax_client is not None:
        return _minimax_client

    _minimax_client = get_minimax_client(
        base_url=MINIMAX_BASE_URL,
        timeout=240.0,
    )
    return _minimax_client


def _cleanup_ai_chat_state(now: datetime | None = None) -> None:
    cutoff = (now or datetime.now(UTC)) - timedelta(hours=CHAT_SESSION_RETENTION_HOURS)
    stale_sessions = [
        session_key
        for session_key, session in _chat_sessions.items()
        if session.get("created_at") is None or session["created_at"] < cutoff
    ]
    for session_key in stale_sessions:
        _chat_sessions.pop(session_key, None)

    stale_counters = [
        streamer
        for streamer, (_, window_start) in _minimax_hourly_counts.items()
        if window_start < (now or datetime.now(UTC)) - timedelta(hours=1)
    ]
    for streamer in stale_counters:
        _minimax_hourly_counts.pop(streamer, None)


def _chat_session_key(streamer: str, analysis_id: int | str) -> str:
    return f"{streamer}_{analysis_id}"


def _plan_ai_model(streamer: str) -> str | None:
    snapshot = resolve_plan_snapshot_for_login(streamer)
    entitlements = set(snapshot.get("entitlements") or [])
    if "analytics.ai_full" in entitlements:
        return AI_MODEL_OPUS
    if "analytics.ai_mini" in entitlements:
        return AI_MODEL_MINIMAX
    return None


class _AnalyticsAIMixin:
    """Plan-aware mixin for AI-powered stream analytics and follow-up chat."""

    # ------------------------------------------------------------------
    #  GET /twitch/api/v2/ai/analysis
    #  Parameter: streamer (required), days (optional, default 30)
    #  Auth: admin / localhost (earlysalty gets admin via _TWITCH_ADMIN_LOGINS)
    # ------------------------------------------------------------------

    async def _api_v2_ai_analysis(self, request: web.Request) -> web.Response:
        """Deep stream analytics analysis via Claude Opus (admin only)."""
        try:
            return await self._api_v2_ai_analysis_inner(request)
        except web.HTTPException:
            raise
        except Exception:
            log.exception("Unhandled exception in _api_v2_ai_analysis")
            return analytics_internal_error_response(
                error="KI-Analyse konnte nicht geladen werden.",
                code="ai_analysis_failed",
            )

    async def _api_v2_ai_analysis_inner(self, request: web.Request) -> web.Response:
        self._require_v2_auth(request)
        _cleanup_ai_chat_state()

        streamer = request.query.get("streamer", "").strip().lower() or None
        if not streamer:
            return web.json_response({"error": "streamer parameter required"}, status=400)

        if streamer in _in_progress_analyses:
            return web.json_response(
                {"error": "Analyse läuft bereits für diesen Streamer. Bitte warte bis sie abgeschlossen ist."},
                status=409,
            )

        try:
            days = min(max(int(request.query.get("days", "30")), 7), 365)
        except (ValueError, TypeError):
            days = 30

        game_filter = request.query.get("game_filter", "all").strip().lower()
        if game_filter not in ("deadlock", "all"):
            game_filter = "all"

        user_context = str(request.query.get("user_context", "") or "").strip()
        if len(user_context) > MAX_USER_CONTEXT_CHARS:
            return web.json_response(
                {
                    "error": f"user_context darf maximal {MAX_USER_CONTEXT_CHARS} Zeichen lang sein",
                },
                status=400,
            )

        auth_level = self._get_auth_level(request)
        if auth_level in ("localhost", "admin"):
            ai_model = AI_MODEL_OPUS
        else:
            ai_model = _plan_ai_model(streamer)
            if ai_model is None:
                return web.json_response(
                    {
                        "error": "plan_required",
                        "required_entitlements": ["analytics.ai_mini", "analytics.ai_full"],
                        "required_plans": [
                            "analysis_dashboard",
                            "analytics_trial",
                            "bundle_analysis_raid_boost",
                            "raid_boost",
                        ],
                    },
                    status=403,
                )

        since = (datetime.now(UTC) - timedelta(days=days)).isoformat()

        _in_progress_analyses.add(streamer)
        try:
            return await self._run_ai_analysis(
                streamer,
                days,
                since,
                game_filter,
                ai_model=ai_model,
                user_context=user_context,
            )
        finally:
            _in_progress_analyses.discard(streamer)

    async def _run_ai_analysis(
        self,
        streamer: str,
        days: int,
        since: str,
        game_filter: str,
        *,
        ai_model: str,
        user_context: str = "",
    ) -> web.Response:
        # Step 1: collect analytics context from DB
        try:
            ctx = self._collect_ai_context(streamer, days, since, game_filter)
        except Exception:
            log.exception("Error collecting AI context")
            return analytics_internal_error_response(
                error="Analyse-Daten konnten nicht gesammelt werden.",
                code="ai_context_collection_failed",
            )

        # Step 2: call configured AI model
        try:
            points = await self._call_ai_analysis(
                ai_model,
                streamer,
                days,
                ctx,
                game_filter,
                user_context=user_context,
            )
        except RuntimeError:
            return analytics_internal_error_response(
                error="KI-Service ist aktuell nicht verfuegbar.",
                code="ai_service_unavailable",
                status=503,
            )
        except Exception as exc:
            log.exception("Error calling AI analysis model %s", ai_model)
            err_str = str(exc)
            if "credit balance is too low" in err_str:
                return web.json_response(
                    {"error": "Kein Guthaben auf dem Anthropic-Konto. Bitte auf console.anthropic.com/billing Credits kaufen."},
                    status=503,
                )
            return analytics_internal_error_response(
                error="KI-Analyse konnte nicht abgeschlossen werden.",
                code="ai_analysis_failed",
            )

        generated_at = datetime.now(UTC)

        # Step 3: persist to DB (best-effort, never blocks the response)
        record_id: int | None = None
        try:
            with storage.transaction() as conn:
                _ensure_ai_table(conn)
                row = conn.execute(
                    """
                    INSERT INTO ai_analyses (streamer, days, model, generated_at, data_snapshot, points)
                    VALUES (%s, %s, %s, %s, %s::jsonb, %s::jsonb)
                    RETURNING id
                    """,
                    (
                        streamer,
                        days,
                        CLAUDE_MODEL if ai_model == AI_MODEL_OPUS else MINIMAX_MODEL,
                        generated_at,
                        json.dumps(ctx.get("summary", {})),
                        json.dumps(points),
                    ),
                ).fetchone()
                record_id = int(row[0]) if row else None
        except Exception:
            log.warning("Failed to persist AI analysis to DB", exc_info=True)

        session_key = None
        follow_ups_remaining = 0
        if record_id is not None:
            session_key = _chat_session_key(streamer, record_id)
            session = {
                "model": ai_model,
                "streamer": streamer,
                "analysis_id": record_id,
                "days": days,
                "game_filter": game_filter,
                "user_context": user_context,
                "ctx": ctx,
                "points": points,
                "history": [],
                "follow_up_count": 0,
                "created_at": generated_at,
            }
            _chat_sessions[session_key] = session
            follow_ups_remaining, _ = self._remaining_follow_ups(streamer, session)

        # Build response body
        try:
            return web.json_response({
                "id": record_id,
                "streamer": streamer,
                "days": days,
                "gameFilter": game_filter,
                "model": ai_model,
                "sessionKey": session_key,
                "followUpsRemaining": follow_ups_remaining,
                "generatedAt": generated_at.isoformat(),
                "points": points,
                "dataSnapshot": ctx.get("summary", {}),
            })
        except Exception:
            log.exception("JSON serialization error in _api_v2_ai_analysis")
            return analytics_internal_error_response(
                error="KI-Analyse konnte nicht serialisiert werden.",
                code="ai_analysis_serialization_failed",
            )

    def _collect_ai_context(
        self, streamer: str, days: int, since: str, game_filter: str = "all"
    ) -> dict:
        """Collect comprehensive analytics data for Opus context."""
        # SQL-Fragment das auf alle Haupt-Queries angewendet wird
        gf_sql = "AND had_deadlock_in_session = true" if game_filter == "deadlock" else ""

        with storage.readonly_connection() as conn:
            # 1. Overview KPIs
            ov = conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
                f"""
                SELECT
                    COUNT(*) AS stream_count,
                    ROUND((SUM(duration_seconds) / 3600.0)::numeric, 1) AS total_hours,
                    ROUND(AVG(avg_viewers)::numeric, 1) AS avg_viewers,
                    MAX(peak_viewers) AS peak_viewers,
                    COALESCE(SUM(
                        CASE WHEN follower_delta > 0 THEN follower_delta ELSE 0 END
                    ), 0) AS followers_gained,
                    ROUND((AVG(retention_10m) * 100)::numeric, 1) AS avg_retention_10m,
                    ROUND((AVG(dropoff_pct) * 100)::numeric, 1) AS avg_dropoff_pct,
                    ROUND(AVG(COALESCE(unique_chatters, 0))::numeric, 0) AS avg_chatters
                FROM twitch_stream_sessions
                WHERE LOWER(streamer_login) = %s
                  AND started_at >= %s
                  AND ended_at IS NOT NULL
                  {gf_sql}
                """,
                (streamer, since),
            ).fetchone()

            # 2. Recent sessions (last 20, newest first)
            sessions_rows = conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
                f"""
                SELECT
                    started_at::date,
                    stream_title,
                    ROUND((duration_seconds / 3600.0)::numeric, 2) AS hours,
                    ROUND(avg_viewers::numeric, 1),
                    peak_viewers,
                    ROUND((retention_10m * 100)::numeric, 1),
                    ROUND((dropoff_pct * 100)::numeric, 1),
                    COALESCE(unique_chatters, 0),
                    COALESCE(follower_delta, 0)
                FROM twitch_stream_sessions
                WHERE LOWER(streamer_login) = %s
                  AND started_at >= %s
                  AND ended_at IS NOT NULL
                  {gf_sql}
                ORDER BY started_at DESC
                LIMIT 20
                """,
                (streamer, since),
            ).fetchall()

            # 3. Weekday performance (sorted by avg viewers)
            weekday_rows = conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
                f"""
                SELECT
                    EXTRACT(DOW FROM started_at)::int AS dow,
                    COUNT(*) AS streams,
                    ROUND(AVG(avg_viewers)::numeric, 1),
                    ROUND(AVG(peak_viewers)::numeric, 1)
                FROM twitch_stream_sessions
                WHERE LOWER(streamer_login) = %s
                  AND started_at >= %s
                  AND ended_at IS NOT NULL
                  {gf_sql}
                GROUP BY dow
                ORDER BY AVG(avg_viewers) DESC
                """,
                (streamer, since),
            ).fetchall()

            # 4. Best 5 sessions by avg viewers
            best_rows = conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
                f"""
                SELECT
                    COALESCE(stream_title, ''), avg_viewers, peak_viewers,
                    ROUND((retention_10m * 100)::numeric, 1), started_at::date
                FROM twitch_stream_sessions
                WHERE LOWER(streamer_login) = %s
                  AND started_at >= %s
                  AND ended_at IS NOT NULL
                  {gf_sql}
                ORDER BY avg_viewers DESC NULLS LAST
                LIMIT 5
                """,
                (streamer, since),
            ).fetchall()

            # 5. Worst 5 sessions by avg viewers
            worst_rows = conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
                f"""
                SELECT
                    COALESCE(stream_title, ''), avg_viewers, peak_viewers,
                    ROUND((retention_10m * 100)::numeric, 1), started_at::date
                FROM twitch_stream_sessions
                WHERE LOWER(streamer_login) = %s
                  AND started_at >= %s
                  AND ended_at IS NOT NULL
                  {gf_sql}
                ORDER BY avg_viewers ASC NULLS LAST
                LIMIT 5
                """,
                (streamer, since),
            ).fetchall()

            # 6. Game/category breakdown from exp_sessions (best-effort)
            game_rows = []
            try:
                game_rows = conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
                    f"""
                    SELECT
                        COALESCE(game_name, 'Unbekannt') AS game,
                        COUNT(*) AS sessions,
                        ROUND(AVG(avg_viewers)::numeric, 1),
                        MAX(peak_viewers),
                        ROUND(AVG(duration_min)::numeric, 1)
                    FROM exp_sessions
                    WHERE LOWER(streamer) = %s
                      AND started_at >= %s
                      AND ended_at IS NOT NULL
                      {"AND LOWER(game_name) = 'deadlock'" if game_filter == "deadlock" else ""}
                    GROUP BY game_name
                    ORDER BY AVG(avg_viewers) DESC
                    LIMIT 10
                    """,
                    (streamer, since),
                ).fetchall()
            except Exception:
                pass  # exp_sessions might not be populated yet

            # 7. Weekly follower trend
            trend_rows = conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
                f"""
                SELECT
                    DATE_TRUNC('week', started_at)::date AS week_start,
                    COUNT(*) AS streams,
                    SUM(CASE WHEN follower_delta > 0 THEN follower_delta ELSE 0 END)
                FROM twitch_stream_sessions
                WHERE LOWER(streamer_login) = %s
                  AND started_at >= %s
                  AND ended_at IS NOT NULL
                  {gf_sql}
                GROUP BY week_start
                ORDER BY week_start
                """,
                (streamer, since),
            ).fetchall()

            # 8. Deadlock-spezifische KPIs (zum Vergleich mit All-Games-Übersicht)
            deadlock_ov = conn.execute(
                """
                SELECT
                    COUNT(*) AS session_count,
                    ROUND((SUM(duration_seconds) / 3600.0)::numeric, 1) AS total_hours,
                    ROUND(AVG(avg_viewers)::numeric, 1) AS avg_viewers,
                    MAX(peak_viewers) AS peak_viewers,
                    COALESCE(SUM(
                        CASE WHEN follower_delta > 0 THEN follower_delta ELSE 0 END
                    ), 0) AS followers_gained
                FROM twitch_stream_sessions
                WHERE LOWER(streamer_login) = %s
                  AND started_at >= %s
                  AND ended_at IS NOT NULL
                  AND had_deadlock_in_session = true
                """,
                (streamer, since),
            ).fetchone()

            # 9. Per-Game Breakdown aus sessions (alle gespielten Kategorien)
            game_session_rows = conn.execute(
                """
                SELECT
                    COALESCE(game_name, 'Unbekannt') AS game,
                    COUNT(*) AS sessions,
                    ROUND(AVG(avg_viewers)::numeric, 1) AS avg_viewers,
                    MAX(peak_viewers) AS peak_viewers,
                    ROUND((SUM(duration_seconds) / 3600.0)::numeric, 1) AS total_hours,
                    COALESCE(SUM(
                        CASE WHEN follower_delta > 0 THEN follower_delta ELSE 0 END
                    ), 0) AS followers_gained,
                    SUM(samples) AS total_samples,
                    MAX(started_at)::date AS last_played
                FROM twitch_stream_sessions
                WHERE LOWER(streamer_login) = %s
                  AND started_at >= %s
                  AND ended_at IS NOT NULL
                GROUP BY game_name
                ORDER BY COUNT(*) DESC, AVG(avg_viewers) DESC NULLS LAST
                LIMIT 15
                """,
                (streamer, since),
            ).fetchall()

        def _s(v, default=0):
            return v if v is not None else default

        return {
            "summary": {
                "streamCount": int(_s(ov[0])),
                "totalHours": float(_s(ov[1])),
                "avgViewers": float(_s(ov[2])),
                "peakViewers": int(_s(ov[3])),
                "followersGained": int(_s(ov[4])),
                "avgRetention10m": float(_s(ov[5])),
                "avgDropoffPct": float(_s(ov[6])),
                "avgChatters": int(_s(ov[7])),
            },
            "recentSessions": [
                {
                    "date": str(r[0]),
                    "title": (str(r[1]) if r[1] else "")[:60],
                    "hours": float(_s(r[2])),
                    "avgViewers": float(_s(r[3])),
                    "peakViewers": int(_s(r[4])),
                    "retention10m": float(_s(r[5])),
                    "dropoffPct": float(_s(r[6])),
                    "chatters": int(_s(r[7])),
                    "followerDelta": int(_s(r[8])),
                }
                for r in sessions_rows
            ],
            "weekdayPerformance": [
                {
                    "day": _DOW_NAMES[int(w[0])] if 0 <= int(w[0]) <= 6 else "?",
                    "streams": int(w[1]),
                    "avgViewers": float(_s(w[2])),
                    "avgPeak": float(_s(w[3])),
                }
                for w in weekday_rows
            ],
            "bestSessions": [
                {
                    "title": (str(s[0]) if s[0] else "")[:60],
                    "avgViewers": float(_s(s[1])),
                    "peakViewers": int(_s(s[2])),
                    "retention10m": float(_s(s[3])),
                    "date": str(s[4]),
                }
                for s in best_rows
            ],
            "worstSessions": [
                {
                    "title": (str(s[0]) if s[0] else "")[:60],
                    "avgViewers": float(_s(s[1])),
                    "peakViewers": int(_s(s[2])),
                    "retention10m": float(_s(s[3])),
                    "date": str(s[4]),
                }
                for s in worst_rows
            ],
            "gamePerformance": [
                {
                    "game": str(g[0]),
                    "sessions": int(g[1]),
                    "avgViewers": float(_s(g[2])),
                    "peakViewers": int(_s(g[3])),
                    "avgDurationMin": float(_s(g[4])),
                }
                for g in game_rows
            ],
            "weeklyTrend": [
                {
                    "week": str(t[0]),
                    "streams": int(t[1]),
                    "followersGained": int(_s(t[2])),
                }
                for t in trend_rows
            ],
            "deadlockSummary": {
                "sessionCount": int(_s(deadlock_ov[0])),
                "totalHours": float(_s(deadlock_ov[1])),
                "avgViewers": float(_s(deadlock_ov[2])),
                "peakViewers": int(_s(deadlock_ov[3])),
                "followersGained": int(_s(deadlock_ov[4])),
            },
            "gameBreakdown": [
                {
                    "game": str(g[0]),
                    "sessions": int(g[1]),
                    "avgViewers": float(_s(g[2])),
                    "peakViewers": int(_s(g[3])),
                    "totalHours": float(_s(g[4])),
                    "followersGained": int(_s(g[5])),
                    "totalSamples": int(_s(g[6])),
                    # hasFullData=False bedeutet: Sessions ohne Viewer-Sampling –
                    # avg_viewers/peak nur Initialwert, nicht aussagekräftig
                    "hasFullData": int(_s(g[6])) > 2,
                    "lastPlayed": str(g[7]),
                }
                for g in game_session_rows
            ],
        }

    def _build_ai_analysis_prompt(
        self,
        streamer: str,
        days: int,
        ctx: dict,
        game_filter: str = "all",
        *,
        user_context: str = "",
    ) -> str:
        s = ctx["summary"]
        mode_label = "Nur Deadlock-Sessions" if game_filter == "deadlock" else "Alle gespielten Kategorien"

        game_section = ctx.get("gamePerformance", [])
        if not game_section:
            game_section = [{"note": "Keine Kategorie-Daten vorhanden (exp_sessions leer)"}]

        game_breakdown = ctx.get("gameBreakdown", [])
        deadlock_summary = ctx.get("deadlockSummary", {})

        dl = deadlock_summary
        multi_game_lines = [
            f"Deadlock (gesamt): {dl.get('sessionCount', 0)} Sessions | "
            f"{dl.get('totalHours', 0)}h | Ø {dl.get('avgViewers', 0)} Viewer | "
            f"Peak {dl.get('peakViewers', 0)} | +{dl.get('followersGained', 0)} Follower",
        ]
        for g in game_breakdown:
            quality = "" if g["hasFullData"] else " (Viewer-Daten unvollständig)"
            multi_game_lines.append(
                f"  {g['game']}: {g['sessions']} Sessions | {g['totalHours']}h | "
                f"Ø {g['avgViewers']} Viewer | Peak {g['peakViewers']} | "
                f"+{g['followersGained']} Follower | zuletzt {g['lastPlayed']}{quality}"
            )
        multi_game_section = "\n".join(multi_game_lines) if multi_game_lines else "Keine Daten"

        prompt = f"""Du bist ein Experte für Twitch-Streaming-Analytik und Wachstumsstrategie.

Analysiere die Streaming-Daten des Kanals **{streamer}** (letzte {days} Tage, Modus: {mode_label}) und erstelle einen TIEFEN, DATEN-BASIERTEN 10-Punkte-Verbesserungsplan.

REGELN:
- Referenziere IMMER konkrete Zahlen aus den Daten
- Keine generischen Ratschläge
- Erkläre das WARUM hinter jedem Pattern
- Priorisiere nach maximalem Impact (#1 = wichtigster Hebel)
- Zeige sowohl Chancen als auch Risiken auf

=== KPI ÜBERSICHT ===
Streams: {s['streamCount']} | Gesamtzeit: {s['totalHours']}h
Ø Viewer: {s['avgViewers']} | Peak: {s['peakViewers']}
Follower gewonnen: +{s['followersGained']}
Ø 10-Min-Retention: {s['avgRetention10m']}% | Ø Dropoff: {s['avgDropoffPct']}%
Ø Aktive Chatter: {s['avgChatters']}

=== TOP 5 STREAMS (Ø Viewer) ===
{json.dumps(ctx.get('bestSessions', []), ensure_ascii=False)}

=== SCHWÄCHSTE 5 STREAMS ===
{json.dumps(ctx.get('worstSessions', []), ensure_ascii=False)}

=== LETZTE 20 SESSIONS ===
{json.dumps(ctx.get('recentSessions', []), ensure_ascii=False)}

=== WOCHENTAG-PERFORMANCE ===
{json.dumps(ctx.get('weekdayPerformance', []), ensure_ascii=False)}

=== KATEGORIEN-PERFORMANCE ===
{json.dumps(game_section, ensure_ascii=False)}

=== ALLE GESTREAMTEN SPIELE (inkl. Nicht-Deadlock) ===
{multi_game_section}

=== WÖCHENTLICHER FOLLOWER-TREND ===
{json.dumps(ctx.get('weeklyTrend', []), ensure_ascii=False)}

---
Antworte NUR als JSON Array mit exakt 10 Objekten. Kein Markdown, kein Text außerhalb des JSON.

[
  {{
    "number": 1,
    "priority": "kritisch",
    "title": "Titel (max 8 Wörter)",
    "analysis": "Tiefenanalyse 3-5 Sätze mit konkreten Zahlen aus den Daten.",
    "action": "Konkrete Handlungsempfehlung: Was, wann, wie oft, wie messen.",
    "expectedImpact": "Realistischer erwarteter Effekt basierend auf den Daten."
  }}
]

Gültige priority-Werte: "kritisch", "hoch", "mittel"
Punkte 1-3: kritisch | Punkte 4-7: hoch | Punkte 8-10: mittel

DATENHINWEIS: Spiele mit "(Viewer-Daten unvollständig)" haben kein Viewer-Sampling –
avg_viewers/peak dort sind Initialwerte bei Stream-Start, nicht repräsentativ.
Vollständige Viewer-Metriken nur für Einträge ohne diesen Hinweis verwenden."""

        if user_context:
            prompt += (
                "\n\n=== STREAMER-KONTEXT ===\n"
                f"Der Streamer hat folgende eigene Eindrücke/Fragen mitgegeben: {user_context}"
            )
        return prompt

    @staticmethod
    def _extract_text_response(value: Any) -> str:
        if value is None:
            return ""
        if isinstance(value, str):
            return value.strip()
        if isinstance(value, list):
            parts: list[str] = []
            for item in value:
                if isinstance(item, str):
                    parts.append(item)
                    continue
                text = getattr(item, "text", None)
                if text:
                    parts.append(str(text))
                    continue
                if isinstance(item, dict):
                    if item.get("type") == "text" and item.get("text"):
                        parts.append(str(item["text"]))
                    elif item.get("content"):
                        parts.append(str(item["content"]))
            return "\n".join(part for part in parts if part).strip()
        text = getattr(value, "text", None)
        if text:
            return str(text).strip()
        return str(value).strip()

    def _parse_ai_analysis_points(self, raw: str) -> list[dict]:
        """Parse and salvage the structured JSON array returned by the model."""
        raw = raw.strip()

        # Strip markdown code fences if present
        if raw.startswith("```"):
            lines = raw.splitlines()
            lines = [ln for ln in lines if not ln.strip().startswith("```")]
            raw = "\n".join(lines).strip()

        # 1) Direct parse – works when Claude returns perfect JSON
        try:
            points = json.loads(raw)
            if isinstance(points, list):
                return points
        except json.JSONDecodeError:
            pass

        # 2) Proper bracket extraction – handles preamble/trailing text AND ] inside strings
        extracted = _extract_json_array(raw)
        if extracted:
            try:
                points = json.loads(extracted)
                if isinstance(points, list):
                    log.info("Extracted complete JSON array from Claude response")
                    return points
            except json.JSONDecodeError:
                pass

        # 3) Truncation salvage – response cut off mid-array; collect complete objects
        #    Scan for depth-1 object boundaries using proper string-aware tracking.
        array_start = raw.find("[")
        if array_start != -1:
            depth = 0
            in_string = False
            escape_next = False
            obj_start: int | None = None
            salvaged: list[str] = []
            for i, ch in enumerate(raw[array_start:], array_start):
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
                if ch == "{":
                    if depth == 0:
                        obj_start = i
                    depth += 1
                elif ch == "}":
                    depth -= 1
                    if depth == 0 and obj_start is not None:
                        salvaged.append(raw[obj_start : i + 1])
                        obj_start = None
                elif ch == "]" and depth == 0:
                    break  # array closed – shouldn't reach here after step 2
            if salvaged:
                try:
                    points = json.loads("[" + ",".join(salvaged) + "]")
                    if isinstance(points, list) and points:
                        log.warning("Response truncated; salvaged %d/%d points", len(points), 10)
                        return points
                except json.JSONDecodeError:
                    pass

        log.warning("AI model returned unparseable response (first 300 chars): %s", raw[:300])
        return []

    async def _call_ai_analysis(
        self,
        ai_model: str,
        streamer: str,
        days: int,
        ctx: dict,
        game_filter: str = "all",
        *,
        user_context: str = "",
    ) -> list[dict]:
        prompt = self._build_ai_analysis_prompt(
            streamer,
            days,
            ctx,
            game_filter,
            user_context=user_context,
        )

        if ai_model == AI_MODEL_OPUS:
            client = _get_async_client()
            msg = await client.messages.create(
                model=CLAUDE_MODEL,
                max_tokens=60000,
                messages=[{"role": "user", "content": prompt}],
            )
            raw = self._extract_text_response(getattr(msg, "content", None))
            return self._parse_ai_analysis_points(raw)

        if ai_model == AI_MODEL_MINIMAX:
            client = _get_minimax_client()
            completion = await client.chat.completions.create(
                model=MINIMAX_MODEL,
                messages=[{"role": "user", "content": prompt}],
                temperature=0.5,
                max_tokens=60000,
            )
            raw = ""
            choices = getattr(completion, "choices", None) or []
            if choices:
                raw = self._extract_text_response(getattr(choices[0].message, "content", ""))
            return self._parse_ai_analysis_points(raw)

        raise RuntimeError(f"Unsupported AI model: {ai_model}")

    def _build_chat_system_prompt(self, session: dict[str, Any]) -> str:
        analysis_payload = {
            "summary": session.get("ctx", {}).get("summary", {}),
            "recentSessions": session.get("ctx", {}).get("recentSessions", []),
            "weekdayPerformance": session.get("ctx", {}).get("weekdayPerformance", []),
            "bestSessions": session.get("ctx", {}).get("bestSessions", []),
            "worstSessions": session.get("ctx", {}).get("worstSessions", []),
            "gamePerformance": session.get("ctx", {}).get("gamePerformance", []),
            "weeklyTrend": session.get("ctx", {}).get("weeklyTrend", []),
            "deadlockSummary": session.get("ctx", {}).get("deadlockSummary", {}),
            "gameBreakdown": session.get("ctx", {}).get("gameBreakdown", []),
            "analysisPoints": session.get("points", []),
            "userContext": session.get("user_context", ""),
            "gameFilter": session.get("game_filter", "all"),
            "days": session.get("days", 30),
            "streamer": session.get("streamer", ""),
        }
        return (
            "Du beantwortest Rueckfragen zu einer bereits erstellten Twitch-KI-Analyse. "
            "Nutze nur den folgenden Analysekontext und die bisherige Unterhaltung. "
            "Antworte praezise, konkret und datenbasiert. Erfinde keine zusaetzlichen Kennzahlen.\n\n"
            "=== ANALYSEKONTEXT ===\n"
            f"{json.dumps(analysis_payload, ensure_ascii=False)}"
        )

    async def _call_ai_chat(self, session: dict[str, Any], message: str) -> str:
        system_prompt = self._build_chat_system_prompt(session)
        history = session.get("history", [])
        messages = [
            {"role": "system", "content": system_prompt},
            *[
                {"role": entry["role"], "content": entry["content"]}
                for entry in history
                if entry.get("role") in {"user", "assistant"} and entry.get("content")
            ],
            {"role": "user", "content": message},
        ]

        if session.get("model") == AI_MODEL_OPUS:
            client = _get_async_client()
            msg = await client.messages.create(
                model=CLAUDE_MODEL,
                max_tokens=4000,
                system=system_prompt,
                messages=[
                    {"role": entry["role"], "content": entry["content"]}
                    for entry in history
                    if entry.get("role") in {"user", "assistant"} and entry.get("content")
                ] + [{"role": "user", "content": message}],
            )
            return self._extract_text_response(getattr(msg, "content", None))

        client = _get_minimax_client()
        completion = await client.chat.completions.create(
            model=MINIMAX_MODEL,
            messages=messages,
            temperature=0.5,
            max_tokens=4000,
        )
        choices = getattr(completion, "choices", None) or []
        if not choices:
            return ""
        return self._extract_text_response(getattr(choices[0].message, "content", ""))

    def _remaining_follow_ups(self, streamer: str, session: dict[str, Any]) -> tuple[int, int | None]:
        if _DDC_PENTEST_DISABLE_RATE_LIMITS:
            return 10**9, None
        if session.get("model") == AI_MODEL_OPUS:
            remaining = max(0, OPUS_SESSION_FOLLOW_UP_LIMIT - int(session.get("follow_up_count", 0) or 0))
            return remaining, None

        now = datetime.now(UTC)
        count, window_start = _minimax_hourly_counts.get(streamer, (0, now))
        if now - window_start >= timedelta(hours=1):
            count = 0
            window_start = now
            _minimax_hourly_counts[streamer] = (count, window_start)
        remaining = max(0, MINIMAX_HOURLY_FOLLOW_UP_LIMIT - int(count))
        reset_ts = int((window_start + timedelta(hours=1)).timestamp())
        return remaining, reset_ts

    def _consume_follow_up(self, streamer: str, session: dict[str, Any]) -> tuple[int, int | None]:
        if _DDC_PENTEST_DISABLE_RATE_LIMITS:
            return self._remaining_follow_ups(streamer, session)
        if session.get("model") == AI_MODEL_OPUS:
            session["follow_up_count"] = int(session.get("follow_up_count", 0) or 0) + 1
            return self._remaining_follow_ups(streamer, session)

        now = datetime.now(UTC)
        count, window_start = _minimax_hourly_counts.get(streamer, (0, now))
        if now - window_start >= timedelta(hours=1):
            count = 0
            window_start = now
        count += 1
        _minimax_hourly_counts[streamer] = (count, window_start)
        return self._remaining_follow_ups(streamer, session)

    async def _api_v2_ai_chat(self, request: web.Request) -> web.Response:
        try:
            return await self._api_v2_ai_chat_inner(request)
        except web.HTTPException:
            raise
        except Exception:
            log.exception("Unhandled exception in _api_v2_ai_chat")
            return analytics_internal_error_response(
                error="KI-Chat konnte nicht geladen werden.",
                code="ai_chat_failed",
            )

    async def _api_v2_ai_chat_inner(self, request: web.Request) -> web.Response:
        self._require_v2_auth(request)
        _cleanup_ai_chat_state()

        try:
            payload = await request.json()
        except Exception:
            return web.json_response({"error": "invalid_json"}, status=400)

        streamer = str(payload.get("streamer") or "").strip().lower()
        if not streamer:
            return web.json_response({"error": "streamer required"}, status=400)

        try:
            analysis_id = int(payload.get("analysis_id"))
        except (TypeError, ValueError):
            return web.json_response({"error": "analysis_id required"}, status=400)

        message = str(payload.get("message") or "").strip()
        if not message:
            return web.json_response({"error": "message required"}, status=400)

        auth_level = self._get_auth_level(request)
        if auth_level not in ("localhost", "admin") and _plan_ai_model(streamer) is None:
            return web.json_response(
                {
                    "error": "plan_required",
                    "required_entitlements": ["analytics.ai_mini", "analytics.ai_full"],
                },
                status=403,
            )

        session_key = _chat_session_key(streamer, analysis_id)
        session = _chat_sessions.get(session_key)
        if not session:
            return web.json_response({"error": "chat_session_not_found"}, status=404)

        remaining_before, reset_ts = self._remaining_follow_ups(streamer, session)
        if remaining_before <= 0:
            if session.get("model") == AI_MODEL_MINIMAX:
                retry_after = max(0, int(reset_ts or 0) - int(datetime.now(UTC).timestamp()))
                return web.json_response(
                    {
                        "error": "follow_up_limit_reached",
                        "retry_after": retry_after,
                        "rateLimitReset": reset_ts,
                    },
                    status=429,
                )
            return web.json_response(
                {"error": "follow_up_limit_reached", "followUpsRemaining": 0},
                status=429,
            )

        reply = await self._call_ai_chat(session, message)
        now_iso = datetime.now(UTC).isoformat()
        history = session.setdefault("history", [])
        history.append({"role": "user", "content": message, "timestamp": now_iso})
        history.append({"role": "assistant", "content": reply, "timestamp": now_iso})
        remaining_after, reset_ts = self._consume_follow_up(streamer, session)

        response_payload: dict[str, Any] = {
            "message": reply,
            "followUpsRemaining": remaining_after,
        }
        if session.get("model") == AI_MODEL_MINIMAX and reset_ts is not None:
            response_payload["rateLimitReset"] = reset_ts
        return web.json_response(response_payload)

    # ------------------------------------------------------------------
    #  GET /twitch/api/v2/ai/history
    #  Parameter: streamer (required), limit (optional, default 20)
    # ------------------------------------------------------------------

    async def _api_v2_ai_history(self, request: web.Request) -> web.Response:
        """Return past AI analyses for a streamer (newest first)."""
        self._require_v2_auth(request)
        _cleanup_ai_chat_state()

        streamer = request.query.get("streamer", "").strip().lower() or None
        if not streamer:
            return web.json_response({"error": "streamer parameter required"}, status=400)

        auth_level = self._get_auth_level(request)
        if auth_level not in ("localhost", "admin") and _plan_ai_model(streamer) is None:
            return web.json_response(
                {
                    "error": "plan_required",
                    "required_entitlements": ["analytics.ai_mini", "analytics.ai_full"],
                },
                status=403,
            )

        try:
            limit = min(max(int(request.query.get("limit", "20")), 1), 50)
        except (ValueError, TypeError):
            limit = 20

        try:
            with storage.readonly_connection() as conn:
                _ensure_ai_table(conn)
                rows = conn.execute(
                    """
                    SELECT id, streamer, days, model, generated_at, data_snapshot, points
                    FROM ai_analyses
                    WHERE streamer = %s
                    ORDER BY generated_at DESC
                    LIMIT %s
                    """,
                    (streamer, limit),
                ).fetchall()

            def _count(pts: list, priority: str) -> int:
                return sum(1 for p in pts if p.get("priority") == priority)

            result = []
            for row in rows:
                pts = row[6] if isinstance(row[6], list) else json.loads(row[6] or "[]")
                snap = row[5] if isinstance(row[5], dict) else json.loads(row[5] or "{}")
                generated_at = row[4]
                model_name = str(row[3] or "")
                model_alias = AI_MODEL_OPUS if "claude" in model_name else AI_MODEL_MINIMAX
                result.append({
                    "id": int(row[0]),
                    "streamer": str(row[1]),
                    "days": int(row[2]),
                    "model": model_alias,
                    "generatedAt": generated_at.isoformat() if hasattr(generated_at, "isoformat") else str(generated_at),
                    "dataSnapshot": snap,
                    "points": pts,
                    "kritischCount": _count(pts, "kritisch"),
                    "hochCount": _count(pts, "hoch"),
                    "mittelCount": _count(pts, "mittel"),
                })

            return web.json_response(result)
        except Exception:
            log.exception("Error in ai_history API")
            return analytics_internal_error_response(
                error="KI-Analyse-Historie konnte nicht geladen werden.",
                code="ai_history_load_failed",
            )

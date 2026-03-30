"""Synchronous read helpers for admin streamer endpoints.

This module keeps the heavy query assembly and row-to-payload mapping out of
the async HTTP handlers in ``api_admin.py``. Callers should wrap the public
functions in ``asyncio.to_thread(...)``.
"""

from __future__ import annotations

from datetime import UTC, datetime
from typing import Any

from ..core.twitch_login import normalize_twitch_login
from ..dashboard.live.live import _REQUIRED_SCOPES as _ADMIN_REQUIRED_SCOPES
from ..storage import pg as storage

_ADMIN_STREAMER_VIEW_ACTIVE = "active"
_ADMIN_STREAMER_VIEW_ARCHIVED = "archived"
_ADMIN_STREAMER_VIEW_DEPARTNERED = "departnered"
_ADMIN_STREAMER_VIEW_NON_PARTNER = "non_partner"
_ADMIN_STREAMER_VIEW_ALL = "all"
_ADMIN_STREAMER_VIEWS = frozenset(
    {
        _ADMIN_STREAMER_VIEW_ACTIVE,
        _ADMIN_STREAMER_VIEW_ARCHIVED,
        _ADMIN_STREAMER_VIEW_DEPARTNERED,
        _ADMIN_STREAMER_VIEW_NON_PARTNER,
        _ADMIN_STREAMER_VIEW_ALL,
    }
)


def _row_get_value(row: Any, key: str, index: int, default: Any = None) -> Any:
    if row is None:
        return default
    if hasattr(row, "get"):
        return row.get(key, default)
    values = tuple(row)
    return values[index] if index < len(values) else default


def _safe_int(value: Any, default: int = 0) -> int:
    try:
        return int(value)
    except (TypeError, ValueError):
        return default


def _coerce_utc_datetime(value: Any) -> datetime | None:
    if value is None:
        return None
    if isinstance(value, datetime):
        parsed = value
    else:
        text = str(value).strip()
        if not text:
            return None
        normalized = f"{text[:-1]}+00:00" if text.endswith("Z") else text
        try:
            parsed = datetime.fromisoformat(normalized)
        except ValueError:
            return None
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=UTC)
    return parsed.astimezone(UTC)


def _json_safe_datetime(value: Any) -> str | None:
    parsed = _coerce_utc_datetime(value)
    if parsed is not None:
        return parsed.isoformat()
    if value is None:
        return None
    text = str(value).strip()
    return text or None


def _admin_partner_state_cte_sql() -> str:
    return """
                    , partner_state AS (
                        SELECT
                            twitch_login,
                            twitch_user_id,
                            require_discord_link,
                            discord_user_id,
                            discord_display_name,
                            is_on_discord,
                            manual_partner_opt_out,
                            created_at,
                            archived_at,
                            raid_bot_enabled,
                            silent_ban,
                            silent_raid,
                            is_monitored_only,
                            is_verified,
                            is_partner_active,
                            live_ping_enabled,
                            status
                        FROM (
                            SELECT
                                s.*,
                                ROW_NUMBER() OVER (
                                    PARTITION BY LOWER(s.twitch_login)
                                    ORDER BY
                                        CASE
                                            WHEN s.status = 'active' THEN 0
                                            ELSE 1
                                        END,
                                        CASE
                                            WHEN s.created_at IS NULL AND s.archived_at IS NULL THEN 1
                                            ELSE 0
                                        END,
                                        CASE
                                            WHEN s.created_at IS NOT NULL THEN s.created_at
                                            ELSE s.archived_at
                                        END DESC,
                                        CASE WHEN s.archived_at IS NULL THEN 1 ELSE 0 END,
                                        s.archived_at DESC,
                                        LOWER(s.twitch_login) ASC
                                ) AS rn
                            FROM twitch_partners_all_state s
                            WHERE COALESCE(TRIM(s.twitch_login), '') <> ''
                        ) ranked_partner_state
                        WHERE rn = 1
                    )
    """


def _admin_partner_live_state_cte_sql(
    *,
    source_table: str = "twitch_partners_all_state",
    active_only: bool = True,
) -> str:
    active_filter = "WHERE s.status = 'active'" if active_only else ""
    return """
                    , partner_live_state AS (
                        SELECT
                            partner_login,
                            twitch_user_id,
                            streamer_login,
                            is_live,
                            last_seen_at,
                            last_viewer_count,
                            active_session_id,
                            last_started_at,
                            last_game
                        FROM (
                            SELECT
                                s.twitch_login AS partner_login,
                                l.twitch_user_id,
                                l.streamer_login,
                                l.is_live,
                                l.last_seen_at,
                                l.last_viewer_count,
                                l.active_session_id,
                                l.last_started_at,
                                l.last_game,
                                ROW_NUMBER() OVER (
                                    PARTITION BY LOWER(s.twitch_login)
                                    ORDER BY
                                        CASE
                                            WHEN NULLIF(TRIM(COALESCE(s.twitch_user_id, '')), '') IS NOT NULL
                                                 AND NULLIF(TRIM(COALESCE(l.twitch_user_id, '')), '') IS NOT NULL
                                                 AND LOWER(TRIM(s.twitch_user_id)) = LOWER(TRIM(l.twitch_user_id))
                                            THEN 0
                                            WHEN LOWER(COALESCE(l.twitch_user_id, ''))
                                                 = LOWER(COALESCE(l.streamer_login, ''))
                                            THEN 2
                                            ELSE 1
                                        END,
                                        CASE WHEN COALESCE(l.is_live, 0) = 1 THEN 0 ELSE 1 END,
                                        CASE
                                            WHEN l.last_seen_at IS NULL AND l.last_started_at IS NULL THEN 1
                                            ELSE 0
                                        END,
                                        CASE
                                            WHEN l.last_seen_at IS NOT NULL THEN l.last_seen_at
                                            ELSE l.last_started_at
                                        END DESC
                                ) AS rn
                            FROM {source_table} s
                            LEFT JOIN twitch_live_state l
                                ON (
                                    NULLIF(TRIM(COALESCE(s.twitch_user_id, '')), '') IS NOT NULL
                                    AND NULLIF(TRIM(COALESCE(l.twitch_user_id, '')), '') IS NOT NULL
                                    AND LOWER(TRIM(s.twitch_user_id)) = LOWER(TRIM(l.twitch_user_id))
                                )
                                OR LOWER(s.twitch_login) = LOWER(l.streamer_login)
                            {active_filter}
                        ) ranked_partner_live_state
                        WHERE rn = 1
                    )
    """.format(source_table=source_table, active_filter=active_filter)


def _admin_partner_oauth_cte_sql(*, source_table: str = "partner_state") -> str:
    return """
                    , partner_oauth AS (
                        SELECT
                            partner_login,
                            scopes,
                            needs_reauth,
                            raid_enabled,
                            authorized_at
                        FROM (
                            SELECT
                                s.twitch_login AS partner_login,
                                a.scopes,
                                a.needs_reauth,
                                a.raid_enabled,
                                a.authorized_at,
                                ROW_NUMBER() OVER (
                                    PARTITION BY LOWER(s.twitch_login)
                                    ORDER BY
                                        CASE
                                            WHEN NULLIF(TRIM(COALESCE(s.twitch_user_id, '')), '') IS NOT NULL
                                                 AND NULLIF(TRIM(COALESCE(a.twitch_user_id, '')), '') IS NOT NULL
                                                 AND LOWER(TRIM(s.twitch_user_id)) = LOWER(TRIM(a.twitch_user_id))
                                            THEN 0
                                            WHEN LOWER(COALESCE(a.twitch_login, ''))
                                                 = LOWER(s.twitch_login)
                                            THEN 1
                                            ELSE 2
                                        END,
                                        CASE WHEN a.authorized_at IS NULL THEN 1 ELSE 0 END,
                                        a.authorized_at DESC
                                ) AS rn
                            FROM {source_table} s
                            LEFT JOIN twitch_raid_auth a
                                ON (
                                    NULLIF(TRIM(COALESCE(s.twitch_user_id, '')), '') IS NOT NULL
                                    AND NULLIF(TRIM(COALESCE(a.twitch_user_id, '')), '') IS NOT NULL
                                    AND LOWER(TRIM(s.twitch_user_id)) = LOWER(TRIM(a.twitch_user_id))
                                )
                                OR LOWER(COALESCE(a.twitch_login, '')) = LOWER(s.twitch_login)
                        ) ranked_oauth
                        WHERE rn = 1
                    )
    """.format(source_table=source_table)


def _admin_last_stream_session_cte_sql() -> str:
    return """
                    , last_stream_session AS (
                        SELECT
                            LOWER(streamer_login) AS streamer_login,
                            MAX(COALESCE(ended_at, started_at)) AS last_stream_at
                        FROM twitch_stream_sessions
                        GROUP BY LOWER(streamer_login)
                    )
    """


def _admin_scope_snapshot(scopes_raw: Any, needs_reauth: Any) -> dict[str, Any]:
    granted_scopes = sorted(
        {
            str(scope or "").strip().lower()
            for scope in str(scopes_raw or "").split()
            if str(scope or "").strip()
        }
    )
    granted_scope_set = set(granted_scopes)
    missing_scopes = [
        scope for scope in _ADMIN_REQUIRED_SCOPES if scope not in granted_scope_set
    ]
    needs_reauth_bool = bool(needs_reauth)
    connected = bool(granted_scopes)
    if needs_reauth_bool:
        status = "reauth"
    elif not connected:
        status = "missing"
    elif missing_scopes:
        status = "partial"
    else:
        status = "connected"
    return {
        "connected": connected,
        "needsReauth": needs_reauth_bool,
        "status": status,
        "grantedScopes": granted_scopes,
        "missingScopes": missing_scopes,
    }


def _admin_partner_status(
    *,
    status: Any,
    archived_at: Any,
    manual_partner_opt_out: bool,
) -> str:
    if manual_partner_opt_out:
        return "non_partner"
    status_text = str(status or "").strip().lower()
    if status_text == "archived" or archived_at:
        return "archived"
    if status_text == "departnered":
        return "departnered"
    return "active" if status_text == "active" else "non_partner"


def _admin_streamer_list_row(row: Any) -> dict[str, Any]:
    login = str(_row_get_value(row, "twitch_login", 0, "") or "").strip().lower()
    archived_at = _row_get_value(row, "archived_at", 5, None)
    is_live = bool(_row_get_value(row, "is_live", 16, 0))
    verified = bool(_row_get_value(row, "is_verified", 14, 0))
    manual_partner_opt_out = bool(_row_get_value(row, "manual_partner_opt_out", 8, 0))
    partner_status = _admin_partner_status(
        status=_row_get_value(row, "status", 9, None),
        archived_at=archived_at,
        manual_partner_opt_out=manual_partner_opt_out,
    )
    scope_snapshot = _admin_scope_snapshot(
        _row_get_value(row, "scopes", 22, ""),
        _row_get_value(row, "needs_reauth", 23, 0),
    )
    status = (
        "non_partner"
        if partner_status == "non_partner"
        else "departnered"
        if partner_status == "departnered"
        else "archived"
        if partner_status == "archived"
        else "live"
        if is_live
        else "verified"
        if verified
        else "offline"
    )
    return {
        "login": login,
        "displayName": str(_row_get_value(row, "discord_display_name", 3, "") or login).strip()
        or login,
        "twitchUserId": str(_row_get_value(row, "twitch_user_id", 1, "") or "").strip() or None,
        "discordUserId": str(_row_get_value(row, "discord_user_id", 2, "") or "").strip() or None,
        "discordDisplayName": str(_row_get_value(row, "discord_display_name", 3, "") or "").strip()
        or None,
        "verified": verified,
        "archived": bool(archived_at),
        "archivedAt": _json_safe_datetime(archived_at),
        "createdAt": _json_safe_datetime(_row_get_value(row, "created_at", 4, None)),
        "isLive": is_live,
        "isOnDiscord": bool(_row_get_value(row, "is_on_discord", 7, 0)),
        "manualPartnerOptOut": manual_partner_opt_out,
        "partnerStatus": partner_status,
        "viewerCount": _safe_int(_row_get_value(row, "last_viewer_count", 18, 0), default=0),
        "activeSessionId": _row_get_value(row, "active_session_id", 19, None),
        "lastSeenAt": _json_safe_datetime(_row_get_value(row, "last_seen_at", 17, None)),
        "lastGame": _row_get_value(row, "last_game", 20, None),
        "lastStreamAt": _json_safe_datetime(_row_get_value(row, "last_stream_at", 21, None)),
        "planId": str(
            _row_get_value(row, "manual_plan_id", 28, "")
            or _row_get_value(row, "billing_plan_id", 31, "")
            or ""
        ).strip()
        or None,
        "billingStatus": str(_row_get_value(row, "billing_status", 32, "") or "").strip() or None,
        "oauthConnected": bool(scope_snapshot["connected"]),
        "oauthNeedsReauth": bool(scope_snapshot["needsReauth"]),
        "oauthStatus": str(scope_snapshot["status"]),
        "grantedScopes": list(scope_snapshot["grantedScopes"]),
        "missingScopes": list(scope_snapshot["missingScopes"]),
        "oauthAuthorizedAt": _json_safe_datetime(_row_get_value(row, "authorized_at", 24, None)),
        "promoDisabled": bool(_row_get_value(row, "promo_disabled", 25, 0)),
        "notes": str(_row_get_value(row, "manual_plan_notes", 30, "") or "").strip() or None,
        "status": status,
    }


def _admin_streamer_detail_payload(row: Any, stats_row: Any, sessions: list[Any], login: str) -> dict[str, Any]:
    session_payload = []
    for session in sessions:
        duration_seconds = _safe_int(_row_get_value(session, "duration_seconds", 7, 0), default=0)
        session_payload.append(
            {
                "sessionId": _row_get_value(session, "id", 0, None),
                "startedAt": _json_safe_datetime(_row_get_value(session, "started_at", 1, None)),
                "endedAt": _json_safe_datetime(_row_get_value(session, "ended_at", 2, None)),
                "title": _row_get_value(session, "stream_title", 3, None),
                "category": _row_get_value(session, "game_name", 4, None),
                "averageViewers": _row_get_value(session, "avg_viewers", 5, None),
                "peakViewers": _row_get_value(session, "peak_viewers", 6, None),
                "watchTimeHours": round(duration_seconds / 3600.0, 2),
                "followerDelta": _row_get_value(session, "follower_delta", 8, None),
            }
        )

    total_duration_seconds = _safe_int(
        _row_get_value(stats_row, "total_duration_seconds", 1, 0) if stats_row else 0,
        default=0,
    )
    archived_at = _row_get_value(row, "archived_at", 5, None)
    manual_partner_opt_out = bool(_row_get_value(row, "manual_partner_opt_out", 8, 0))
    scope_snapshot = _admin_scope_snapshot(
        _row_get_value(row, "scopes", 23, ""),
        _row_get_value(row, "needs_reauth", 24, 0),
    )
    partner_status = _admin_partner_status(
        status=_row_get_value(row, "status", 16, None),
        archived_at=archived_at,
        manual_partner_opt_out=manual_partner_opt_out,
    )
    return {
        "login": login,
        "displayName": str(_row_get_value(row, "discord_display_name", 3, "") or login).strip()
        or login,
        "twitchUserId": str(_row_get_value(row, "twitch_user_id", 1, "") or "").strip() or None,
        "verified": bool(_row_get_value(row, "is_verified", 13, 0)),
        "archived": bool(archived_at),
        "archivedAt": _json_safe_datetime(archived_at),
        "createdAt": _json_safe_datetime(_row_get_value(row, "created_at", 4, None)),
        "isLive": bool(_row_get_value(row, "is_live", 17, 0)),
        "partnerStatus": partner_status,
        "planId": str(
            _row_get_value(row, "manual_plan_id", 32, "")
            or _row_get_value(row, "billing_plan_id", 35, "")
            or _row_get_value(row, "plan_name", 27, "")
            or ""
        ).strip()
        or None,
        "stats": {
            "totalSessions": _safe_int(_row_get_value(stats_row, "total_sessions", 0, 0), default=0)
            if stats_row
            else 0,
            "totalWatchHours": round(total_duration_seconds / 3600.0, 2),
            "averageViewers": round(float(_row_get_value(stats_row, "avg_viewers", 2, 0.0) or 0.0), 2)
            if stats_row
            else 0.0,
            "peakViewers": _safe_int(_row_get_value(stats_row, "peak_viewers", 3, 0), default=0)
            if stats_row
            else 0,
            "followerDelta": _safe_int(_row_get_value(stats_row, "follower_delta", 4, 0), default=0)
            if stats_row
            else 0,
            "viewerCount": _safe_int(_row_get_value(row, "last_viewer_count", 19, 0), default=0),
            "lastSeenAt": _json_safe_datetime(_row_get_value(row, "last_seen_at", 18, None)),
            "lastStartedAt": _json_safe_datetime(_row_get_value(row, "last_started_at", 21, None)),
            "lastGame": _row_get_value(row, "last_game", 22, None),
        },
        "sessions": session_payload,
        "settings": {
            "isOnDiscord": bool(_row_get_value(row, "is_on_discord", 7, 0)),
            "manualPartnerOptOut": manual_partner_opt_out,
            "livePingEnabled": bool(_row_get_value(row, "live_ping_enabled", 14, 1)),
            "oauthConnected": bool(scope_snapshot["connected"]),
            "oauthNeedsReauth": bool(scope_snapshot["needsReauth"]),
            "oauthStatus": str(scope_snapshot["status"]),
            "grantedScopes": list(scope_snapshot["grantedScopes"]),
            "missingScopes": list(scope_snapshot["missingScopes"]),
            "oauthAuthorizedAt": _json_safe_datetime(_row_get_value(row, "authorized_at", 25, None)),
            "promoDisabled": bool(_row_get_value(row, "promo_disabled", 28, 0)),
            "promoMessage": _row_get_value(row, "promo_message", 29, None),
            "raidBoostEnabled": bool(_row_get_value(row, "raid_boost_enabled", 30, 0)),
            "notes": str(_row_get_value(row, "notes", 31, "") or "").strip() or None,
            "manualPlanId": str(_row_get_value(row, "manual_plan_id", 32, "") or "").strip() or None,
            "manualPlanExpiresAt": _json_safe_datetime(
                _row_get_value(row, "manual_plan_expires_at", 33, None)
            ),
            "manualPlanNotes": str(_row_get_value(row, "manual_plan_notes", 34, "") or "").strip()
            or None,
        },
        "oauth": {
            "connected": bool(scope_snapshot["connected"]),
            "needsReauth": bool(scope_snapshot["needsReauth"]),
            "status": str(scope_snapshot["status"]),
            "grantedScopes": list(scope_snapshot["grantedScopes"]),
            "missingScopes": list(scope_snapshot["missingScopes"]),
            "authorizedAt": _json_safe_datetime(_row_get_value(row, "authorized_at", 25, None)),
            "raidEnabled": bool(_row_get_value(row, "oauth_raid_enabled", 26, 0)),
        },
    }


def _build_streamer_list_query(view: str) -> tuple[str, str]:
    if view not in _ADMIN_STREAMER_VIEWS:
        raise ValueError(f"unsupported streamer view: {view}")
    where_clause = {
        _ADMIN_STREAMER_VIEW_ACTIVE: (
            "COALESCE(s.status, 'departnered') = 'active' "
            "AND s.archived_at IS NULL "
            "AND COALESCE(s.manual_partner_opt_out, 0) = 0"
        ),
        _ADMIN_STREAMER_VIEW_ARCHIVED: (
            "COALESCE(s.manual_partner_opt_out, 0) = 0 "
            "AND ("
            "    (COALESCE(s.status, 'departnered') = 'active' AND s.archived_at IS NOT NULL) "
            "    OR COALESCE(s.status, '') = 'archived'"
            ")"
        ),
        _ADMIN_STREAMER_VIEW_DEPARTNERED: (
            "COALESCE(s.manual_partner_opt_out, 0) = 0 "
            "AND COALESCE(s.status, 'departnered') = 'departnered'"
        ),
        _ADMIN_STREAMER_VIEW_NON_PARTNER: "COALESCE(s.manual_partner_opt_out, 0) = 1",
        _ADMIN_STREAMER_VIEW_ALL: "1 = 1",
    }[view]
    sql = f"""
                    WITH latest_billing AS (
                        SELECT
                            customer_reference,
                            plan_id,
                            status,
                            updated_at,
                            ROW_NUMBER() OVER (
                                PARTITION BY LOWER(customer_reference)
                                ORDER BY updated_at DESC
                            ) AS rn
                        FROM twitch_billing_subscriptions
                    )
                    {_admin_partner_state_cte_sql()}
                    {_admin_partner_live_state_cte_sql(source_table="partner_state", active_only=False)}
                    {_admin_partner_oauth_cte_sql(source_table="partner_state")}
                    {_admin_last_stream_session_cte_sql()}
                    SELECT
                        s.twitch_login,
                        s.twitch_user_id,
                        s.discord_user_id,
                        s.discord_display_name,
                        s.created_at,
                        s.archived_at,
                        s.require_discord_link,
                        s.is_on_discord,
                        s.manual_partner_opt_out,
                        s.status,
                        s.raid_bot_enabled,
                        s.silent_ban,
                        s.silent_raid,
                        s.is_monitored_only,
                        COALESCE(s.is_verified, 0) AS is_verified,
                        COALESCE(s.is_partner_active, 0) AS is_partner_active,
                        COALESCE(pls.is_live, 0) AS is_live,
                        pls.last_seen_at,
                        pls.last_viewer_count,
                        pls.active_session_id,
                        pls.last_game,
                        lss.last_stream_at,
                        po.scopes,
                        po.needs_reauth,
                        po.authorized_at,
                        sp.promo_disabled,
                        sp.promo_message,
                        sp.raid_boost_enabled,
                        sp.manual_plan_id,
                        sp.manual_plan_expires_at,
                        sp.manual_plan_notes,
                        lb.plan_id AS billing_plan_id,
                        lb.status AS billing_status,
                        lb.updated_at AS billing_updated_at
                    FROM partner_state s
                    LEFT JOIN partner_live_state pls
                        ON LOWER(pls.partner_login) = LOWER(s.twitch_login)
                    LEFT JOIN partner_oauth po
                        ON LOWER(po.partner_login) = LOWER(s.twitch_login)
                    LEFT JOIN last_stream_session lss
                        ON lss.streamer_login = LOWER(s.twitch_login)
                    LEFT JOIN streamer_plans sp
                        ON LOWER(sp.twitch_login) = LOWER(s.twitch_login)
                    LEFT JOIN latest_billing lb
                        ON LOWER(lb.customer_reference) = LOWER(s.twitch_login)
                       AND lb.rn = 1
                    WHERE {where_clause}
                    ORDER BY
                        CASE
                            WHEN COALESCE(s.manual_partner_opt_out, 0) = 1 THEN 3
                            WHEN COALESCE(s.status, 'departnered') = 'active' AND s.archived_at IS NULL THEN 0
                            WHEN COALESCE(s.status, 'departnered') IN ('active', 'archived') THEN 1
                            ELSE 2
                        END,
                        CASE WHEN COALESCE(pls.is_live, 0) = 1 THEN 0 ELSE 1 END,
                        LOWER(s.twitch_login) ASC
    """
    return sql, where_clause


def load_admin_streamers(view: str) -> dict[str, Any]:
    sql, _where_clause = _build_streamer_list_query(view)
    with storage.readonly_connection() as conn:
        rows = conn.execute(sql).fetchall()
    items = [_admin_streamer_list_row(row) for row in rows]
    return {"items": items, "count": len(items), "view": view}


def load_admin_streamer_detail(login: str) -> dict[str, Any] | None:
    normalized_login = normalize_twitch_login(login)
    if not normalized_login:
        return None

    sql = f"""
                    WITH latest_billing AS (
                        SELECT
                            customer_reference,
                            plan_id,
                            status,
                            updated_at,
                            ROW_NUMBER() OVER (
                                PARTITION BY LOWER(customer_reference)
                                ORDER BY updated_at DESC
                            ) AS rn
                        FROM twitch_billing_subscriptions
                    )
                    {_admin_partner_state_cte_sql()}
                    {_admin_partner_live_state_cte_sql(source_table="partner_state", active_only=False)}
                    {_admin_partner_oauth_cte_sql(source_table="partner_state")}
                    SELECT
                        s.twitch_login,
                        s.twitch_user_id,
                        s.discord_user_id,
                        s.discord_display_name,
                        s.created_at,
                        s.archived_at,
                        s.require_discord_link,
                        s.is_on_discord,
                        s.manual_partner_opt_out,
                        s.raid_bot_enabled,
                        s.silent_ban,
                        s.silent_raid,
                        s.is_monitored_only,
                        COALESCE(s.is_verified, 0) AS is_verified,
                        COALESCE(s.is_partner_active, 0) AS is_partner_active,
                        COALESCE(s.live_ping_enabled, 1) AS live_ping_enabled,
                        s.status,
                        COALESCE(pls.is_live, 0) AS is_live,
                        pls.last_seen_at,
                        pls.last_viewer_count,
                        pls.active_session_id,
                        pls.last_started_at,
                        pls.last_game,
                        po.scopes,
                        po.needs_reauth,
                        po.raid_enabled AS oauth_raid_enabled,
                        po.authorized_at,
                        sp.plan_name,
                        sp.promo_disabled,
                        sp.promo_message,
                        sp.raid_boost_enabled,
                        sp.notes,
                        sp.manual_plan_id,
                        sp.manual_plan_expires_at,
                        sp.manual_plan_notes,
                        lb.plan_id AS billing_plan_id,
                        lb.status AS billing_status,
                        lb.updated_at AS billing_updated_at
                    FROM partner_state s
                    LEFT JOIN partner_live_state pls
                        ON LOWER(pls.partner_login) = LOWER(s.twitch_login)
                    LEFT JOIN partner_oauth po
                        ON LOWER(po.partner_login) = LOWER(s.twitch_login)
                    LEFT JOIN streamer_plans sp
                        ON LOWER(sp.twitch_login) = LOWER(s.twitch_login)
                    LEFT JOIN latest_billing lb
                        ON LOWER(lb.customer_reference) = LOWER(s.twitch_login)
                       AND lb.rn = 1
                    WHERE LOWER(s.twitch_login) = LOWER(%s)
                    LIMIT 1
    """

    with storage.readonly_connection() as conn:
        row = conn.execute(sql, (normalized_login,)).fetchone()
        if row is None:
            return None
        stats_row = conn.execute(
            """
                    SELECT
                        COUNT(*) AS total_sessions,
                        COALESCE(SUM(duration_seconds), 0) AS total_duration_seconds,
                        COALESCE(AVG(avg_viewers), 0) AS avg_viewers,
                        COALESCE(MAX(peak_viewers), 0) AS peak_viewers,
                        COALESCE(SUM(follower_delta), 0) AS follower_delta
                    FROM twitch_stream_sessions
                    WHERE LOWER(streamer_login) = LOWER(%s)
            """,
            (normalized_login,),
        ).fetchone()
        sessions = conn.execute(
            """
                    SELECT
                        id,
                        started_at,
                        ended_at,
                        stream_title,
                        game_name,
                        avg_viewers,
                        peak_viewers,
                        duration_seconds,
                        follower_delta
                    FROM twitch_stream_sessions
                    WHERE LOWER(streamer_login) = LOWER(%s)
                    ORDER BY started_at DESC
                    LIMIT 10
            """,
            (normalized_login,),
        ).fetchall()

    return _admin_streamer_detail_payload(row, stats_row, list(sessions), normalized_login)


def _admin_database_row_count(conn: Any, table_name: str) -> int | None:
    try:
        row = conn.execute(f"SELECT COUNT(*) AS total FROM {table_name}").fetchone()
    except Exception:
        return None
    if row is None:
        return None
    return _safe_int(_row_get_value(row, "total", 0, None), default=0)


def _admin_database_table_size_bytes(conn: Any, table_name: str) -> int | None:
    try:
        row = conn.execute(
            f"SELECT pg_total_relation_size('{table_name}') AS size_bytes"
        ).fetchone()
    except Exception:
        return None
    if row is None:
        return None
    return _safe_int(_row_get_value(row, "size_bytes", 0, None), default=0)


def _admin_database_size_bytes(conn: Any) -> int | None:
    try:
        row = conn.execute("SELECT pg_database_size(current_database()) AS size_bytes").fetchone()
    except Exception:
        return None
    if row is None:
        return None
    return _safe_int(_row_get_value(row, "size_bytes", 0, None), default=0)


def load_admin_database_overview(tables: tuple[str, ...]) -> dict[str, Any]:
    table_rows: list[dict[str, Any]] = []
    with storage.readonly_connection() as conn:
        database_size_bytes = _admin_database_size_bytes(conn)
        for table_name in tables:
            row_count = _admin_database_row_count(conn, table_name)
            if row_count is None:
                continue
            table_rows.append(
                {
                    "table": table_name,
                    "rowCount": row_count,
                    "sizeBytes": _admin_database_table_size_bytes(conn, table_name),
                }
            )
    return {
        "databaseSizeBytes": database_size_bytes,
        "tables": table_rows,
    }


__all__ = [
    "load_admin_database_overview",
    "load_admin_streamer_detail",
    "load_admin_streamers",
]

from __future__ import annotations

import asyncio
import logging
from collections import deque
from dataclasses import dataclass
from datetime import UTC, datetime, timedelta
from pathlib import Path
from typing import Any, Callable
from urllib.parse import urlencode

from ...logging_setup import log_path
from ...storage import pg as storage

log = logging.getLogger("TwitchStreams.AnalyticsV2")


@dataclass(frozen=True)
class InternalHomeServiceConfig:
    required_scopes: tuple[str, ...]
    partner_status_active: str
    ban_reason_keywords: tuple[str, ...]
    service_warning_log_filename: str
    service_warning_max_scan_lines: int
    service_warning_max_events: int
    autoban_log_filename: str
    autoban_max_scan_lines: int
    autoban_max_events: int
    activity_max_events: int
    login_url: str
    discord_connect_url: str


def internal_home_keyword_clause(
    column_expr: str,
    *,
    config: InternalHomeServiceConfig,
) -> tuple[str, list[str]]:
    if not config.ban_reason_keywords:
        return "1=0", []
    like_parts = [
        f"LOWER(COALESCE({column_expr}, '')) LIKE %s"
        for _ in config.ban_reason_keywords
    ]
    like_params = [f"%{keyword}%" for keyword in config.ban_reason_keywords]
    return f"({' OR '.join(like_parts)})", like_params


def internal_home_iso(value: Any) -> str:
    if value is None:
        return ""
    if hasattr(value, "isoformat"):
        return str(value.isoformat())
    return str(value)


def internal_home_parse_iso_datetime(value: Any) -> datetime | None:
    if value is None:
        return None
    text = str(value).strip()
    if not text:
        return None
    normalized = text.replace("Z", "+00:00")
    try:
        parsed = datetime.fromisoformat(normalized)
    except ValueError:
        return None
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=UTC)
    return parsed.astimezone(UTC)


def internal_home_parse_prefixed_int(token: str, prefix: str) -> int | None:
    normalized = str(token or "").strip()
    if not normalized.lower().startswith(prefix.lower()):
        return None
    raw_value = normalized[len(prefix):].strip()
    if raw_value in {"", "-"}:
        return None
    try:
        return int(raw_value)
    except ValueError:
        return None


def internal_home_service_warning_log_candidates(
    *,
    config: InternalHomeServiceConfig,
) -> tuple[Path, ...]:
    project_path = log_path(config.service_warning_log_filename)
    legacy_cwd_path = Path("logs") / config.service_warning_log_filename
    candidates = dict.fromkeys([project_path, legacy_cwd_path])
    return tuple(candidates)


def internal_home_autoban_log_candidates(
    *,
    config: InternalHomeServiceConfig,
) -> tuple[Path, ...]:
    project_root = Path(__file__).resolve().parents[3]
    project_path = log_path(config.autoban_log_filename)
    legacy_cwd_path = Path("logs") / config.autoban_log_filename
    sibling_path = project_root.parent / "Deadlock" / "logs" / config.autoban_log_filename
    candidates = dict.fromkeys([project_path, legacy_cwd_path, sibling_path])
    return tuple(candidates)


def internal_home_service_warning_title(severity_code: str) -> str:
    normalized = str(severity_code or "").strip().upper()
    if normalized == "ESCALATED_TIMEOUT":
        return "Service-Pitch eskaliert (Timeout)"
    if normalized == "WARNING_STRONG":
        return "Service-Pitch Warnung (stark)"
    if normalized == "WARNING_PUBLIC":
        return "Service-Pitch Warnung"
    if normalized == "HINT":
        return "Service-Pitch Hinweis"
    return "Service-Pitch Ereignis"


def internal_home_service_warning_severity(severity_code: str) -> str:
    normalized = str(severity_code or "").strip().upper()
    if normalized == "ESCALATED_TIMEOUT":
        return "critical"
    if normalized in {"WARNING_STRONG", "WARNING_PUBLIC"}:
        return "warning"
    if normalized == "HINT":
        return "info"
    return "warning"


def parse_internal_home_service_warning_line(ctx: Any, raw_line: str) -> dict[str, Any] | None:
    line = str(raw_line or "").strip()
    if not line:
        return None
    parts = line.split("\t", 10)
    if len(parts) < 10:
        return None
    if len(parts) == 10:
        parts.append("")

    timestamp_raw = parts[0].strip()
    severity_code = parts[1].strip().upper()
    channel_login = parts[2].strip().lower()
    chatter_login = parts[3].strip().lower()
    chatter_id = parts[4].strip()
    age_days = ctx._internal_home_parse_prefixed_int(parts[5], "age_days=")
    follower_count = ctx._internal_home_parse_prefixed_int(parts[6], "followers=")
    score = ctx._internal_home_parse_prefixed_int(parts[7], "score=")
    message_count = ctx._internal_home_parse_prefixed_int(parts[8], "msgs=")
    reasons_text = parts[9].strip()
    content_text = parts[10].strip()

    parsed_ts = ctx._internal_home_parse_iso_datetime(timestamp_raw)
    timestamp = (
        parsed_ts.isoformat()
        if parsed_ts is not None
        else ctx._internal_home_iso(timestamp_raw)
    )

    metric_parts: list[str] = []
    if score is not None:
        metric_parts.append(f"Score {score}")
    if message_count is not None:
        metric_parts.append(f"Msgs {message_count}")
    if age_days is not None and age_days >= 0:
        metric_parts.append(f"Account {age_days}d")
    if follower_count is not None:
        metric_parts.append(f"Followers {follower_count}")
    metric = " | ".join(metric_parts)

    reason = "" if reasons_text in {"", "-"} else reasons_text
    description_parts: list[str] = []
    if reason:
        description_parts.append(f"Signale: {reason}")
    if content_text:
        description_parts.append(f"Nachricht: {content_text}")
    description = " | ".join(description_parts)

    chatter_label = f"@{chatter_login}" if chatter_login and chatter_login != "-" else "Unbekannt"
    summary_parts: list[str] = [chatter_label]
    if metric:
        summary_parts.append(metric)
    summary = " | ".join(summary_parts)

    return {
        "type": "service_pitch_warning",
        "event_type": "service_pitch_warning",
        "timestamp": timestamp,
        "target_login": "" if chatter_login == "-" else chatter_login,
        "target_id": "" if chatter_id == "-" else chatter_id,
        "actor_login": channel_login,
        "status_label": f"[{severity_code or 'WARNING'}]",
        "title": ctx._internal_home_service_warning_title(severity_code),
        "summary": summary,
        "description": description,
        "reason": reason,
        "metric": metric,
        "severity": ctx._internal_home_service_warning_severity(severity_code),
        "source": "service_warning_log",
    }


def load_internal_home_service_warning_events(
    ctx: Any,
    *,
    streamer_login: str,
    since_date: str,
    max_events: int,
    config: InternalHomeServiceConfig,
) -> list[dict[str, Any]]:
    channel_key = str(streamer_login or "").strip().lower()
    if not channel_key:
        return []

    selected_log_path: Path | None = None
    for candidate in ctx._internal_home_service_warning_log_candidates():
        try:
            if candidate.exists():
                selected_log_path = candidate
                break
        except OSError:
            continue
    if selected_log_path is None:
        return []

    since_dt = ctx._internal_home_parse_iso_datetime(since_date)
    recent_lines: deque[str] = deque(maxlen=config.service_warning_max_scan_lines)
    try:
        with selected_log_path.open("r", encoding="utf-8", errors="replace") as handle:
            for line in handle:
                if line:
                    recent_lines.append(line.rstrip("\n"))
    except OSError:
        log.debug(
            "Could not read service warning log for internal-home: %s",
            selected_log_path,
            exc_info=True,
        )
        return []

    events: list[dict[str, Any]] = []
    for raw_line in reversed(recent_lines):
        parsed = ctx._parse_internal_home_service_warning_line(raw_line)
        if not parsed:
            continue

        severity_label = str(parsed.get("status_label") or "").upper()
        if "HINT" in severity_label:
            continue

        event_channel = str(parsed.get("actor_login") or "").strip().lower()
        if event_channel != channel_key:
            continue

        if since_dt is not None:
            event_dt = ctx._internal_home_parse_iso_datetime(parsed.get("timestamp"))
            if event_dt is None or event_dt < since_dt:
                continue

        events.append(parsed)
        if len(events) >= int(max_events):
            break

    return events


def parse_internal_home_autoban_line(ctx: Any, raw_line: str) -> dict[str, Any] | None:
    line = str(raw_line or "").strip()
    if not line:
        return None
    parts = line.split("\t", 6)
    if len(parts) < 6:
        return None
    if len(parts) == 6:
        parts.append("")

    timestamp_raw = parts[0].strip()
    status_raw = parts[1].strip()
    channel_login = parts[2].strip().lower()
    chatter_login = parts[3].strip().lower()
    chatter_id = parts[4].strip()
    reason_text = parts[5].strip()
    content_text = parts[6].strip()

    normalized_status = status_raw.strip().strip("[]").upper()
    if normalized_status != "BANNED":
        return None

    parsed_ts = ctx._internal_home_parse_iso_datetime(timestamp_raw)
    timestamp = (
        parsed_ts.isoformat()
        if parsed_ts is not None
        else ctx._internal_home_iso(timestamp_raw)
    )

    reason = "" if reason_text in {"", "-"} else reason_text
    content = "" if content_text in {"", "-"} else content_text
    target_login = "" if chatter_login in {"", "-"} else chatter_login
    target_id = "" if chatter_id in {"", "-"} else chatter_id
    status_label = (
        status_raw
        if status_raw.startswith("[") and status_raw.endswith("]")
        else "[BANNED]"
    )

    summary_parts: list[str] = []
    if reason:
        summary_parts.append(reason)
    if content:
        summary_parts.append(content)
    if channel_login:
        summary_parts.append(f"Mod: @{channel_login}")
    summary = " | ".join(summary_parts) if summary_parts else "Ban ausgeführt"

    description_parts: list[str] = []
    if reason:
        description_parts.append(f"Signale: {reason}")
    if content:
        description_parts.append(f"Nachricht: {content}")
    description = " | ".join(description_parts)

    return {
        "type": "ban",
        "event_type": "ban",
        "timestamp": timestamp,
        "target_login": target_login,
        "target_id": target_id,
        "moderator_login": channel_login,
        "actor_login": channel_login,
        "reason": reason,
        "status_label": status_label,
        "title": f"Ban gegen @{target_login}" if target_login else "Ban ausgeführt",
        "summary": summary,
        "description": description,
        "severity": "warning",
        "source": "autoban_log",
    }


def load_internal_home_autoban_events(
    ctx: Any,
    *,
    streamer_login: str,
    since_date: str,
    max_events: int,
    config: InternalHomeServiceConfig,
) -> list[dict[str, Any]]:
    channel_key = str(streamer_login or "").strip().lower()
    if not channel_key:
        return []

    selected_log_path: Path | None = None
    for candidate in ctx._internal_home_autoban_log_candidates():
        try:
            if candidate.exists():
                selected_log_path = candidate
                break
        except OSError:
            continue
    if selected_log_path is None:
        return []

    since_dt = ctx._internal_home_parse_iso_datetime(since_date)
    recent_lines: deque[str] = deque(maxlen=config.autoban_max_scan_lines)
    try:
        with selected_log_path.open("r", encoding="utf-8", errors="replace") as handle:
            for line in handle:
                if line:
                    recent_lines.append(line.rstrip("\n"))
    except OSError:
        log.debug(
            "Could not read autoban log for internal-home: %s",
            selected_log_path,
            exc_info=True,
        )
        return []

    events: list[dict[str, Any]] = []
    for raw_line in reversed(recent_lines):
        parsed = ctx._parse_internal_home_autoban_line(raw_line)
        if not parsed:
            continue

        event_channel = str(
            parsed.get("actor_login") or parsed.get("moderator_login") or ""
        ).strip().lower()
        if event_channel != channel_key:
            continue

        if since_dt is not None:
            event_dt = ctx._internal_home_parse_iso_datetime(parsed.get("timestamp"))
            if event_dt is None or event_dt < since_dt:
                continue

        events.append(parsed)
        if len(events) >= int(max_events):
            break

    return events


def internal_home_identity_block(
    *,
    twitch_login: str,
    twitch_user_id: str,
) -> tuple[str, str, bool]:
    resolved_login = twitch_login
    resolved_user_id = twitch_user_id
    discord_connected = False

    with storage.readonly_connection() as conn:
        identity_row = conn.execute(
            """
            SELECT
                LOWER(twitch_login),
                COALESCE(twitch_user_id, ''),
                CASE
                    WHEN COALESCE(is_on_discord, 0) = 1 THEN 1
                    WHEN COALESCE(discord_user_id, '') <> '' THEN 1
                    ELSE 0
                END AS discord_connected
            FROM twitch_streamer_identities
            WHERE (COALESCE(%s, '') != '' AND LOWER(twitch_login) = %s)
               OR (COALESCE(%s, '') != '' AND twitch_user_id = %s)
            ORDER BY CASE
                WHEN (COALESCE(%s, '') != '' AND LOWER(twitch_login) = %s) THEN 0
                ELSE 1
            END
            LIMIT 1
            """,
            (
                twitch_login,
                twitch_login,
                twitch_user_id,
                twitch_user_id,
                twitch_login,
                twitch_login,
            ),
        ).fetchone()
        if identity_row:
            resolved_login = str(identity_row[0] or resolved_login or "").strip().lower()
            resolved_user_id = str(identity_row[1] or resolved_user_id or "").strip()
            discord_connected = bool(identity_row[2])

    return resolved_login, resolved_user_id, discord_connected


def internal_home_oauth_status_from_conn(
    conn: Any,
    *,
    resolved_login: str,
    resolved_user_id: str,
    config: InternalHomeServiceConfig,
    scope_snapshot_builder: Callable[[Any, Any], dict[str, Any]],
) -> dict[str, Any]:
    granted_scopes: list[str] = []
    missing_scopes: list[str] = []
    oauth_needs_reauth = False
    oauth_status = "missing"

    if resolved_login:
        oauth_row = conn.execute(
            """
            SELECT scopes, needs_reauth
            FROM twitch_raid_auth
            WHERE (%s != '' AND TRIM(COALESCE(twitch_user_id, '')) = %s)
               OR (%s != '' AND LOWER(COALESCE(twitch_login, '')) = LOWER(%s))
            ORDER BY CASE
                WHEN (%s != '' AND TRIM(COALESCE(twitch_user_id, '')) = %s) THEN 0
                ELSE 1
            END
            LIMIT 1
            """,
            (
                resolved_user_id,
                resolved_user_id,
                resolved_login,
                resolved_login,
                resolved_user_id,
                resolved_user_id,
            ),
        ).fetchone()
        if oauth_row:
            scope_snapshot = scope_snapshot_builder(oauth_row[0], oauth_row[1])
            granted_scopes = list(scope_snapshot["granted_scopes"])
            missing_scopes = list(scope_snapshot["missing_scopes"])
            oauth_needs_reauth = bool(scope_snapshot["needs_reauth"])
            oauth_status = str(scope_snapshot["status"])
        else:
            missing_scopes = list(config.required_scopes)

    return {
        "granted_scopes": granted_scopes,
        "missing_scopes": missing_scopes,
        "oauth_needs_reauth": oauth_needs_reauth,
        "oauth_status": oauth_status,
    }


def internal_home_kpis_and_recent_from_conn(
    ctx: Any,
    conn: Any,
    *,
    since_date: str,
    resolved_login: str,
) -> dict[str, Any]:
    streams_count = 0
    avg_viewers = 0.0
    follower_delta = 0
    recent_streams: list[dict[str, Any]] = []

    if not resolved_login:
        return {
            "streams_count": streams_count,
            "avg_viewers": avg_viewers,
            "follower_delta": follower_delta,
            "recent_streams": recent_streams,
        }

    kpi_row = conn.execute(
        """
        SELECT
            COUNT(*) AS streams_count,
            COALESCE(AVG(s.avg_viewers), 0) AS avg_viewers,
            COALESCE(SUM(CASE
                WHEN s.follower_delta IS NOT NULL
                     AND NOT (s.followers_end = 0 AND s.followers_start > 0)
                THEN s.follower_delta
                ELSE 0
            END), 0) AS follower_delta
        FROM twitch_stream_sessions s
        WHERE s.started_at >= %s
          AND s.ended_at IS NOT NULL
          AND LOWER(s.streamer_login) = %s
        """,
        (since_date, resolved_login),
    ).fetchone()
    if kpi_row:
        streams_count = int(kpi_row[0] or 0)
        avg_viewers = float(kpi_row[1] or 0.0)
        follower_delta = int(kpi_row[2] or 0)

    recent_rows = conn.execute(
        """
        SELECT
            s.started_at,
            s.ended_at,
            s.duration_seconds,
            s.avg_viewers,
            s.peak_viewers,
            CASE
                WHEN s.follower_delta IS NOT NULL
                     AND NOT (s.followers_end = 0 AND s.followers_start > 0)
                THEN s.follower_delta
                ELSE 0
            END AS follower_delta,
            s.stream_title
        FROM twitch_stream_sessions s
        WHERE s.started_at >= %s
          AND s.ended_at IS NOT NULL
          AND LOWER(s.streamer_login) = %s
        ORDER BY s.started_at DESC
        LIMIT 5
        """,
        (since_date, resolved_login),
    ).fetchall()
    for row in recent_rows:
        started_iso = ctx._internal_home_iso(row[0])
        recent_streams.append(
            {
                "date": started_iso[:10] if started_iso else "",
                "started_at": started_iso,
                "ended_at": ctx._internal_home_iso(row[1]),
                "duration_seconds": int(row[2] or 0),
                "avg_viewers": round(float(row[3] or 0.0), 1),
                "peak_viewers": int(row[4] or 0),
                "follower_delta": int(row[5] or 0),
                "title": str(row[6] or ""),
            }
        )

    return {
        "streams_count": streams_count,
        "avg_viewers": avg_viewers,
        "follower_delta": follower_delta,
        "recent_streams": recent_streams,
    }


def internal_home_ban_events_from_conn(
    ctx: Any,
    conn: Any,
    *,
    since_date: str,
    resolved_user_id: str,
) -> dict[str, Any]:
    bot_bans_keyword_count = 0
    ban_events: list[dict[str, Any]] = []
    if not resolved_user_id:
        return {
            "bot_bans_keyword_count": bot_bans_keyword_count,
            "ban_events": ban_events,
        }

    ban_clause, ban_params = ctx._internal_home_keyword_clause("b.reason")
    ban_count_row = conn.execute(
        f"""
        SELECT COUNT(*)
        FROM twitch_ban_events b
        WHERE b.received_at >= %s
          AND b.twitch_user_id = %s
          AND LOWER(COALESCE(b.event_type, '')) = 'ban'
          AND {ban_clause}
        """,
        (since_date, resolved_user_id, *ban_params),
    ).fetchone()
    bot_bans_keyword_count = int((ban_count_row[0] if ban_count_row else 0) or 0)

    ban_event_rows = conn.execute(
        """
        SELECT b.received_at, b.target_login, b.target_id, b.moderator_login, b.reason
        FROM twitch_ban_events b
        WHERE b.received_at >= %s
          AND b.twitch_user_id = %s
          AND LOWER(COALESCE(b.event_type, '')) = 'ban'
        ORDER BY b.received_at DESC
        LIMIT 20
        """,
        (since_date, resolved_user_id),
    ).fetchall()
    for row in ban_event_rows:
        target_login = str(row[1] or "").strip()
        moderator_login = str(row[3] or "").strip()
        reason_text = str(row[4] or "").strip()
        summary_parts: list[str] = []
        if reason_text:
            summary_parts.append(reason_text)
        if moderator_login:
            summary_parts.append(f"Mod: @{moderator_login}")
        ban_events.append(
            {
                "type": "ban",
                "event_type": "ban",
                "timestamp": ctx._internal_home_iso(row[0]),
                "target_login": target_login,
                "target_id": str(row[2] or ""),
                "moderator_login": moderator_login,
                "reason": reason_text,
                "status_label": "[BANNED]",
                "title": f"Ban gegen @{target_login}" if target_login else "Ban ausgeführt",
                "summary": " | ".join(summary_parts) if summary_parts else "Ban ausgeführt",
                "severity": "warning",
            }
        )

    return {
        "bot_bans_keyword_count": bot_bans_keyword_count,
        "ban_events": ban_events,
    }


def internal_home_raid_events_from_conn(
    ctx: Any,
    conn: Any,
    *,
    since_date: str,
    resolved_login: str,
    resolved_user_id: str,
) -> list[dict[str, Any]]:
    raid_events: list[dict[str, Any]] = []
    if not (resolved_user_id or resolved_login):
        return raid_events

    raid_rows = conn.execute(
        """
        SELECT
            r.executed_at,
            r.to_broadcaster_login,
            r.to_broadcaster_id,
            r.viewer_count,
            r.reason,
            r.success
        FROM twitch_raid_history r
        WHERE r.executed_at >= %s
          AND (
              (COALESCE(%s, '') != '' AND r.from_broadcaster_id = %s)
              OR (COALESCE(%s, '') != '' AND LOWER(r.from_broadcaster_login) = %s)
          )
        ORDER BY r.executed_at DESC
        LIMIT 10
        """,
        (
            since_date,
            resolved_user_id,
            resolved_user_id,
            resolved_login,
            resolved_login,
        ),
    ).fetchall()
    for row in raid_rows:
        raid_events.append(
            {
                "type": "raid_history",
                "timestamp": ctx._internal_home_iso(row[0]),
                "target_login": str(row[1] or ""),
                "target_id": str(row[2] or ""),
                "viewer_count": int(row[3] or 0),
                "reason": str(row[4] or ""),
                "success": bool(row[5]) if row[5] is not None else True,
                "status_label": "[RAID]",
            }
        )
    return raid_events


def internal_home_chat_count_from_conn(
    conn: Any,
    *,
    resolved_login: str,
    started_at: str,
    ended_at: str,
) -> int | None:
    chat_row = conn.execute(
        """
        SELECT COUNT(*) FROM twitch_chat_messages
        WHERE LOWER(streamer_login) = LOWER(%s)
          AND message_ts >= %s AND message_ts <= %s
        """,
        (resolved_login, started_at, ended_at),
    ).fetchone()
    if not chat_row:
        return None
    return int(chat_row[0])


def internal_home_core_sequential(
    ctx: Any,
    *,
    since_date: str,
    resolved_login: str,
    resolved_user_id: str,
    config: InternalHomeServiceConfig,
) -> dict[str, Any]:
    granted_scopes: list[str] = []
    missing_scopes: list[str] = []
    oauth_needs_reauth = False
    oauth_status = "missing"
    streams_count = 0
    avg_viewers = 0.0
    follower_delta = 0
    bot_bans_keyword_count = 0
    recent_streams: list[dict[str, Any]] = []
    raid_events: list[dict[str, Any]] = []
    bot_events: list[dict[str, Any]] = []
    last_stream: dict[str, Any] | None = None
    access_state = {
        "partner_status": config.partner_status_active,
        "technical_pause_reason": None,
        "operational_state": None,
        "token_error_grace_expires_at": None,
        "token_error_error_count": 0,
        "analytics_access_allowed": True,
    }

    with storage.readonly_connection() as conn:
        try:
            access_state = ctx._dashboard_access_state_from_conn(
                conn,
                twitch_login=resolved_login,
                twitch_user_id=resolved_user_id,
            )
        except Exception:
            log.exception("Error loading internal-home access-state block")

        if resolved_login:
            try:
                oauth_data = ctx._internal_home_oauth_status_from_conn(
                    conn,
                    resolved_login=resolved_login,
                    resolved_user_id=resolved_user_id,
                )
                granted_scopes = list(oauth_data["granted_scopes"])
                missing_scopes = list(oauth_data["missing_scopes"])
                oauth_needs_reauth = bool(oauth_data["oauth_needs_reauth"])
                oauth_status = str(oauth_data["oauth_status"])
            except Exception:
                log.exception("Error loading internal-home oauth-status block")

            try:
                kpi_data = ctx._internal_home_kpis_and_recent_from_conn(
                    conn,
                    since_date=since_date,
                    resolved_login=resolved_login,
                )
                streams_count = int(kpi_data["streams_count"] or 0)
                avg_viewers = float(kpi_data["avg_viewers"] or 0.0)
                follower_delta = int(kpi_data["follower_delta"] or 0)
                recent_streams = list(kpi_data["recent_streams"] or [])
            except Exception:
                log.exception("Error loading internal-home kpis/recent-streams block")

        if resolved_user_id:
            try:
                ban_data = ctx._internal_home_ban_events_from_conn(
                    conn,
                    since_date=since_date,
                    resolved_user_id=resolved_user_id,
                )
                bot_bans_keyword_count = int(ban_data["bot_bans_keyword_count"] or 0)
                bot_events.extend(list(ban_data["ban_events"] or []))
            except Exception:
                log.exception("Error loading internal-home ban-events block")

        if resolved_user_id or resolved_login:
            try:
                raid_events = ctx._internal_home_raid_events_from_conn(
                    conn,
                    since_date=since_date,
                    resolved_login=resolved_login,
                    resolved_user_id=resolved_user_id,
                )
                bot_events.extend(raid_events)
            except Exception:
                log.exception("Error loading internal-home raid-events block")

        if recent_streams:
            ls = recent_streams[0]
            chat_count = None
            try:
                chat_count = ctx._internal_home_chat_count_from_conn(
                    conn,
                    resolved_login=resolved_login,
                    started_at=str(ls.get("started_at") or ""),
                    ended_at=str(ls.get("ended_at") or ""),
                )
            except Exception:
                log.exception("Error loading internal-home chat-count block")
            last_stream = {**ls, "chat_messages": chat_count}

    return {
        "granted_scopes": granted_scopes,
        "missing_scopes": missing_scopes,
        "oauth_needs_reauth": oauth_needs_reauth,
        "oauth_status": oauth_status,
        "streams_count": streams_count,
        "avg_viewers": avg_viewers,
        "follower_delta": follower_delta,
        "bot_bans_keyword_count": bot_bans_keyword_count,
        "recent_streams": recent_streams,
        "raid_events": raid_events,
        "bot_events": bot_events,
        "last_stream": last_stream,
        "access_state": access_state,
    }


def internal_home_health_score(
    *,
    resolved_login: str,
    health_score_builder: Callable[[str, Any], dict[str, Any] | None],
) -> dict[str, Any] | None:
    if not resolved_login:
        return None
    with storage.readonly_connection() as conn:
        return health_score_builder(resolved_login, conn)


def internal_home_week_comparison(
    *,
    resolved_login: str,
    week_comparison_builder: Callable[[str, Any], dict[str, Any] | None],
) -> dict[str, Any] | None:
    if not resolved_login:
        return None
    with storage.readonly_connection() as conn:
        return week_comparison_builder(resolved_login, conn)


def internal_home_live_status(
    ctx: Any,
    *,
    resolved_login: str,
    resolved_user_id: str,
) -> dict[str, Any] | None:
    if not resolved_login and not resolved_user_id:
        return None
    with storage.readonly_connection() as conn:
        row = conn.execute(
            """
            SELECT
                COALESCE(is_live, 0),
                last_started_at,
                last_seen_at,
                last_title,
                last_game,
                COALESCE(last_viewer_count, 0)
            FROM twitch_live_state
            WHERE (COALESCE(%s, '') != '' AND twitch_user_id = %s)
               OR (COALESCE(%s, '') != '' AND LOWER(streamer_login) = LOWER(%s))
            ORDER BY CASE
                WHEN (COALESCE(%s, '') != '' AND twitch_user_id = %s) THEN 0
                ELSE 1
            END
            LIMIT 1
            """,
            (
                resolved_user_id,
                resolved_user_id,
                resolved_login,
                resolved_login,
                resolved_user_id,
                resolved_user_id,
            ),
        ).fetchone()
    if not row:
        return {
            "is_live": False,
            "viewer_count": 0,
            "started_at": None,
            "last_seen_at": None,
            "title": None,
            "game": None,
        }
    is_live = bool(int(row[0] or 0))
    return {
        "is_live": is_live,
        "viewer_count": int(row[5] or 0),
        "started_at": ctx._internal_home_iso(row[1]) if is_live else None,
        "last_seen_at": ctx._internal_home_iso(row[2]),
        "title": str(row[3] or "") or None,
        "game": str(row[4] or "") or None,
    }


def internal_home_result_or_default(result: Any, *, block_name: str, default: Any) -> Any:
    if isinstance(result, BaseException):
        try:
            raise result
        except BaseException:
            log.exception("Error loading internal-home %s block", block_name)
        return default
    return result


async def build_internal_home_payload(
    ctx: Any,
    *,
    twitch_login: str,
    twitch_user_id: str,
    display_name: str,
    days: int,
    config: InternalHomeServiceConfig,
) -> dict[str, Any]:
    generated_at = datetime.now(UTC).isoformat()
    since_date = (datetime.now(UTC) - timedelta(days=days)).isoformat()
    resolved_login, resolved_user_id, discord_connected = await asyncio.to_thread(
        ctx._internal_home_identity_block,
        twitch_login=twitch_login,
        twitch_user_id=twitch_user_id,
    )

    (
        core_result,
        health_result,
        week_result,
        autoban_result,
        service_warning_result,
        live_result,
    ) = await asyncio.gather(
        asyncio.to_thread(
            ctx._internal_home_core_sequential,
            since_date=since_date,
            resolved_login=resolved_login,
            resolved_user_id=resolved_user_id,
        ),
        asyncio.to_thread(
            ctx._internal_home_health_score,
            resolved_login=resolved_login,
        ),
        asyncio.to_thread(
            ctx._internal_home_week_comparison,
            resolved_login=resolved_login,
        ),
        (
            asyncio.to_thread(
                ctx._load_internal_home_autoban_events,
                streamer_login=resolved_login,
                since_date=since_date,
            )
            if resolved_login
            else asyncio.sleep(0, result=[])
        ),
        (
            asyncio.to_thread(
                ctx._load_internal_home_service_warning_events,
                streamer_login=resolved_login,
                since_date=since_date,
            )
            if resolved_login
            else asyncio.sleep(0, result=[])
        ),
        asyncio.to_thread(
            ctx._internal_home_live_status,
            resolved_login=resolved_login,
            resolved_user_id=resolved_user_id,
        ),
        return_exceptions=True,
    )

    core_data = ctx._internal_home_result_or_default(
        core_result,
        block_name="core",
        default={
            "granted_scopes": [],
            "missing_scopes": [],
            "oauth_needs_reauth": False,
            "oauth_status": "missing",
            "streams_count": 0,
            "avg_viewers": 0.0,
            "follower_delta": 0,
            "bot_bans_keyword_count": 0,
            "recent_streams": [],
            "raid_events": [],
            "bot_events": [],
            "last_stream": None,
            "access_state": {
                "partner_status": config.partner_status_active,
                "technical_pause_reason": None,
                "operational_state": None,
                "token_error_grace_expires_at": None,
                "token_error_error_count": 0,
                "analytics_access_allowed": True,
            },
        },
    )
    granted_scopes = list(core_data["granted_scopes"] or [])
    missing_scopes = list(core_data["missing_scopes"] or [])
    oauth_needs_reauth = bool(core_data["oauth_needs_reauth"])
    oauth_status = str(core_data["oauth_status"] or "missing")
    streams_count = int(core_data["streams_count"] or 0)
    avg_viewers = float(core_data["avg_viewers"] or 0.0)
    follower_delta = int(core_data["follower_delta"] or 0)
    bot_bans_keyword_count = int(core_data["bot_bans_keyword_count"] or 0)
    recent_streams = list(core_data["recent_streams"] or [])
    raid_events = list(core_data["raid_events"] or [])
    bot_events = list(core_data["bot_events"] or [])
    last_stream = core_data["last_stream"] if isinstance(core_data, dict) else None
    access_state = dict(core_data.get("access_state") or {})
    partner_status = str(access_state.get("partner_status") or config.partner_status_active)
    can_access_analytics = bool(access_state.get("analytics_access_allowed", True))

    health_score = ctx._internal_home_result_or_default(
        health_result,
        block_name="health-score",
        default=None,
    )
    week_comparison = ctx._internal_home_result_or_default(
        week_result,
        block_name="week-comparison",
        default=None,
    )
    autoban_events = list(
        ctx._internal_home_result_or_default(
            autoban_result,
            block_name="autoban-events",
            default=[],
        )
        or []
    )
    bot_events.extend(autoban_events)
    service_warning_events = list(
        ctx._internal_home_result_or_default(
            service_warning_result,
            block_name="service-warning-events",
            default=[],
        )
        or []
    )
    bot_events.extend(service_warning_events)

    live_status = ctx._internal_home_result_or_default(
        live_result,
        block_name="live-status",
        default=None,
    )

    bot_events.sort(key=lambda item: item.get("timestamp") or "", reverse=True)
    bot_events = bot_events[: config.activity_max_events]

    overview_query: dict[str, Any] = {"days": days}
    if resolved_login:
        overview_query["streamer"] = resolved_login

    oauth_reconnect_url = "/twitch/raid/auth" if resolved_login else config.login_url

    return {
        "profile": {
            "twitch_login": resolved_login,
            "twitch_user_id": resolved_user_id,
            "display_name": display_name or resolved_login,
        },
        "status": {
            "authenticated": True,
            "streamer_bound": bool(resolved_login or resolved_user_id),
            "period_days": days,
            "oauth": {
                "connected": bool(granted_scopes),
                "status": oauth_status,
                "needs_reauth": oauth_needs_reauth,
                "granted_scopes": granted_scopes,
                "missing_scopes": missing_scopes,
                "reconnect_url": oauth_reconnect_url,
                "profile_url": "/twitch/dashboard",
                "last_checked_at": generated_at,
            },
            "discord": {
                "connected": discord_connected,
                "status": "connected" if discord_connected else "missing",
                "connect_url": config.discord_connect_url,
                "last_checked_at": generated_at,
            },
            "raid_status": {
                "state": "active",
                "read_only": True,
            },
            "partner": {
                "status": partner_status,
                "technical_pause_reason": access_state.get("technical_pause_reason"),
                "operational_state": access_state.get("operational_state"),
                "token_error_grace_expires_at": access_state.get(
                    "token_error_grace_expires_at"
                ),
                "token_error_error_count": int(
                    access_state.get("token_error_error_count") or 0
                ),
            },
            "access": {
                "landing": True,
                "analytics": can_access_analytics,
            },
        },
        "kpis": {
            "streams_count": streams_count,
            "avg_viewers": round(avg_viewers, 1),
            "follower_delta": follower_delta,
            "bot_bans_keyword_count": bot_bans_keyword_count,
        },
        "recent_streams": recent_streams,
        "last_stream_summary": last_stream,
        "health_score": health_score,
        "week_comparison": week_comparison,
        "live_status": live_status,
        "bot_impact": {
            "events": bot_events,
            "summary": {
                "ban_keyword_hits_30d": bot_bans_keyword_count,
                "recent_raid_events": len(raid_events),
                "recent_autoban_events": len(autoban_events),
                "recent_service_warnings": len(service_warning_events),
            },
            "note": (
                "Raid automation is active in read-only mode. "
                "Bot impact events are informational and no write action is triggered here."
            ),
        },
        "bot_activity": {
            "events": bot_events,
        },
        "links": {
            "dashboard": "/twitch/dashboard",
            "dashboard_v2": "/analyse" if can_access_analytics else "/twitch/dashboard",
            "raid_history": "/twitch/raid/history",
            "raid_requirements": "/twitch/raid/requirements",
            "billing": "/twitch/abbo",
            "oauth_reconnect": oauth_reconnect_url,
            "profile_status": "/twitch/dashboard",
            "discord_connect": config.discord_connect_url,
            "internal_home_api": f"/twitch/api/v2/internal-home?{urlencode({'days': days})}",
            "overview_api": f"/twitch/api/v2/overview?{urlencode(overview_query)}",
        },
        "generated_at": generated_at,
    }

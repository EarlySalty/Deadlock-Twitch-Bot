"""Admin-only analytics API endpoints for the Twitch admin dashboard."""

from __future__ import annotations

import asyncio
import logging
import os
import platform
import re
import time
from collections import deque
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

from aiohttp import web

from ..app_keys import (
    ANALYTICS_DB_FINGERPRINT_ERROR_KEY,
    ANALYTICS_DB_FINGERPRINT_KEY,
    ANALYTICS_DB_FINGERPRINT_MISMATCH_KEY,
    INTERNAL_API_ANALYTICS_DB_FINGERPRINT_KEY,
)
from ..dashboard.affiliate.affiliate_pii import AffiliatePII
from ..dashboard.affiliate.gutschrift import AffiliateGutschriftService
from ..dashboard.live.live import (
    _CRITICAL_SCOPES as _ADMIN_CRITICAL_SCOPES,
    _REQUIRED_SCOPES as _ADMIN_REQUIRED_SCOPES,
    _SCOPE_COLUMN_LABELS as _ADMIN_SCOPE_COLUMN_LABELS,
)
from ..logging_setup import log_path, logs_dir
from ..promo_mode import (
    evaluate_global_promo_mode,
    load_global_promo_mode,
    save_global_promo_mode,
    validate_global_promo_mode_config,
)
from ..storage import pg as storage

LOGIN_RE = re.compile(r"^[A-Za-z0-9_]{3,25}$")
_ERROR_LOG_MAX_SCAN_LINES = 4000
_ERROR_LOG_MAX_RETURNED = 200
_RAW_CHAT_LAG_WARNING_SECONDS = 900
_ADMIN_MANAGED_SCOPE_ACTIVE = "active"
_ADMIN_MANAGED_SCOPE_ALL = "all"
_ADMIN_MANAGED_SCOPES = frozenset({_ADMIN_MANAGED_SCOPE_ACTIVE, _ADMIN_MANAGED_SCOPE_ALL})
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
_AFFILIATE_REVENUE_STATUSES: tuple[str, ...] = ("pending", "transferred")
_AFFILIATE_REVENUE_STATUS_PLACEHOLDERS = ", ".join(
    ["%s"] * len(_AFFILIATE_REVENUE_STATUSES)
)
_DATABASE_STATS_TABLES: tuple[str, ...] = (
    "twitch_streamers",
    "twitch_live_state",
    "twitch_stream_sessions",
    "twitch_stats_tracked",
    "twitch_stats_category",
    "streamer_plans",
    "twitch_billing_subscriptions",
    "affiliate_accounts",
    "twitch_eventsub_capacity_snapshot",
    "dashboard_sessions",
)
_LOG_HEADER_SECRET_RE = re.compile(
    r"(?i)\b(authorization\s*[:=]\s*(?:bearer|basic)\s+)([^\s,;]+)"
)
_LOG_COOKIE_RE = re.compile(r"(?i)\b((?:set-cookie|cookie)\s*[:=]\s*)([^\r\n]+)")
_LOG_KEY_VALUE_SECRET_RE = re.compile(
    r"(?i)\b("
    r"access[_-]?token|refresh[_-]?token|id[_-]?token|csrf[_-]?token|"
    r"client[_-]?secret|api[_-]?key|apikey|session(?:id)?|password|secret"
    r")(\s*[:=]\s*)(\"[^\"]+\"|'[^']+'|[^\s,;]+)"
)
_LOG_QUOTED_KEY_VALUE_SECRET_RE = re.compile(
    r"(?i)((?:\"|')("
    r"access[_-]?token|refresh[_-]?token|id[_-]?token|csrf[_-]?token|"
    r"client[_-]?secret|api[_-]?key|apikey|session(?:id)?|password|secret"
    r")(?:\"|')\s*[:=]\s*)(\"[^\"]*\"|'[^']*'|[^\s,;]+)"
)
_LOG_QUERY_SECRET_RE = re.compile(
    r"(?i)\b("
    r"access[_-]?token|refresh[_-]?token|id[_-]?token|csrf[_-]?token|"
    r"client[_-]?secret|api[_-]?key|apikey|session(?:id)?|password|secret"
    r")=([^&\s]+)"
)
_LOG_JWT_RE = re.compile(r"\beyJ[a-zA-Z0-9_-]{8,}\.[a-zA-Z0-9._-]{8,}\.[a-zA-Z0-9._-]{8,}\b")
_LOG_OAUTH_TOKEN_RE = re.compile(r"\boauth:[a-zA-Z0-9]{12,}\b")


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


def _normalize_login(raw_value: str) -> str | None:
    login = str(raw_value or "").strip().lower()
    if not LOGIN_RE.fullmatch(login):
        return None
    return login


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
    """Deduplicate partner state rows so one canonical row exists per login."""
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
    """Rank live-state rows so each partner resolves to one canonical row."""
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
                        ) ranked_live_state
                        WHERE rn = 1
                    )
    """.format(source_table=source_table, active_filter=active_filter)


def _admin_partner_oauth_cte_sql(*, source_table: str = "partner_state") -> str:
    """Resolve at most one OAuth row per canonical partner login."""
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


def _fetch_raw_chat_health_snapshot(conn: Any) -> dict[str, Any]:
    live_row = conn.execute(
        """
        SELECT
            h.streamer_login,
            h.last_raw_chat_message_at,
            h.last_raw_chat_insert_ok_at,
            h.last_raw_chat_insert_error_at,
            h.last_raw_chat_error,
            h.updated_at
        FROM twitch_raw_chat_ingest_health h
        JOIN twitch_live_state ls
          ON LOWER(ls.streamer_login) = LOWER(h.streamer_login)
        WHERE COALESCE(ls.is_live, 0) = 1
        ORDER BY COALESCE(
            h.last_raw_chat_message_at,
            h.last_raw_chat_insert_ok_at,
            h.last_raw_chat_insert_error_at,
            h.updated_at
        ) ASC NULLS LAST
        LIMIT 1
        """
    ).fetchone()
    row = live_row
    is_live_scope = live_row is not None
    if row is None:
        row = conn.execute(
            """
            SELECT
                streamer_login,
                last_raw_chat_message_at,
                last_raw_chat_insert_ok_at,
                last_raw_chat_insert_error_at,
                last_raw_chat_error,
                updated_at
            FROM twitch_raw_chat_ingest_health
            ORDER BY COALESCE(
                updated_at,
                last_raw_chat_message_at,
                last_raw_chat_insert_ok_at,
                last_raw_chat_insert_error_at
            ) DESC NULLS LAST
            LIMIT 1
            """
        ).fetchone()

    streamer_login = _row_get_value(row, "streamer_login", 0, None) if row else None
    last_message_at = _row_get_value(row, "last_raw_chat_message_at", 1, None) if row else None
    last_insert_ok_at = (
        _row_get_value(row, "last_raw_chat_insert_ok_at", 2, None) if row else None
    )
    last_insert_error_at = (
        _row_get_value(row, "last_raw_chat_insert_error_at", 3, None) if row else None
    )
    last_error = str(_row_get_value(row, "last_raw_chat_error", 4, "") or "").strip() or None
    updated_at = _row_get_value(row, "updated_at", 5, None) if row else None

    signal_ts = max(
        (
            dt
            for dt in (
                _coerce_utc_datetime(last_message_at),
                _coerce_utc_datetime(last_insert_ok_at),
                _coerce_utc_datetime(last_insert_error_at),
                _coerce_utc_datetime(updated_at),
            )
            if dt is not None
        ),
        default=None,
    )
    raw_chat_lag_seconds = None
    if signal_ts is not None:
        raw_chat_lag_seconds = max(
            0,
            int((datetime.now(UTC) - signal_ts).total_seconds()),
        )

    return {
        "streamerLogin": streamer_login,
        "lastMessageAt": last_message_at,
        "lastInsertOkAt": last_insert_ok_at,
        "lastInsertErrorAt": last_insert_error_at,
        "lastError": last_error,
        "rawChatLagSeconds": raw_chat_lag_seconds,
        "isLiveScope": is_live_scope,
    }


_admin_log = logging.getLogger("analytics.admin")


def _admin_500(exc: Exception) -> web.Response:
    """Return generic 500 without leaking internal details."""
    _admin_log.exception("Admin API error: %s", type(exc).__name__)
    return web.json_response({"error": "Internal server error"}, status=500)


class _AnalyticsAdminMixin:
    """Admin-only endpoints for the `/twitch/api/admin/*` namespace."""

    def _register_v2_admin_api_routes(self, router: web.UrlDispatcher) -> None:
        router.add_get("/twitch/api/admin/streamers", self._api_admin_streamers)
        router.add_get("/twitch/api/admin/streamers/{login}", self._api_admin_streamer_detail)
        router.add_get("/twitch/api/admin/system/health", self._api_admin_system_health)
        router.add_get("/twitch/api/admin/system/oauth-scopes", self._api_admin_system_oauth_scopes)
        router.add_get("/twitch/api/admin/system/eventsub", self._api_admin_system_eventsub)
        router.add_get("/twitch/api/admin/system/database", self._api_admin_system_database)
        router.add_get("/twitch/api/admin/system/errors", self._api_admin_system_errors)
        router.add_get("/twitch/api/admin/config/overview", self._api_admin_config_overview)
        router.add_post("/twitch/api/admin/config/promo", self._api_admin_config_promo)
        router.add_post("/twitch/api/admin/config/raids", self._api_admin_config_raids)
        router.add_post("/twitch/api/admin/config/chat", self._api_admin_config_chat)
        router.add_get(
            "/twitch/api/admin/billing/subscriptions",
            self._api_admin_billing_subscriptions,
        )
        router.add_get(
            "/twitch/api/admin/billing/affiliates",
            self._api_admin_billing_affiliates,
        )
        # Affiliate management endpoints
        router.add_get(
            "/twitch/api/admin/affiliates/gutschriften",
            self._api_admin_affiliate_gutschriften,
        )
        router.add_get(
            "/twitch/api/admin/affiliates/gutschriften/{gutschrift_id}/pdf",
            self._api_admin_affiliate_gutschrift_pdf,
        )
        router.add_post(
            "/twitch/api/admin/affiliates/generate-gutschriften",
            self._api_admin_affiliate_generate_gutschriften,
        )
        router.add_get(
            "/twitch/api/admin/affiliates/stats",
            self._api_admin_affiliate_stats,
        )
        router.add_get(
            "/twitch/api/admin/affiliates",
            self._api_admin_affiliates_list,
        )
        router.add_get(
            "/twitch/api/admin/affiliates/{login}/gutschriften",
            self._api_admin_affiliate_gutschriften_for_login,
        )
        router.add_get(
            "/twitch/api/admin/affiliates/{login}",
            self._api_admin_affiliate_detail,
        )
        router.add_post(
            "/twitch/api/admin/affiliates/{login}/toggle",
            self._api_admin_affiliate_toggle,
        )

    @staticmethod
    def _admin_auth_error(request: web.Request, checker: Any) -> web.Response | None:
        if callable(checker):
            return checker(request)
        return web.json_response({"error": "admin_required", "required": "admin"}, status=403)

    @staticmethod
    def _admin_actor_label(request: web.Request, getter: Any) -> str:
        if not callable(getter):
            return "admin"
        try:
            session = getter(request) or {}
        except Exception:
            session = {}
        user_id = str(session.get("user_id") or "").strip()
        if user_id.isdigit():
            return f"discord:{user_id}"
        return "admin"

    async def _admin_json_body(self, request: web.Request) -> dict[str, Any]:
        cache_key = "_admin_json_body"
        cached = request.get(cache_key)
        if isinstance(cached, dict):
            return cached
        try:
            payload = await request.json()
        except Exception:
            payload = {}
        if not isinstance(payload, dict):
            payload = {}
        request[cache_key] = payload
        return payload

    async def _admin_extract_csrf(self, request: web.Request) -> tuple[str, dict[str, Any]]:
        payload = await self._admin_json_body(request)
        header = str(request.headers.get("X-CSRF-Token") or "").strip()
        if header:
            return header, payload
        return str(payload.get("csrf_token") or "").strip(), payload

    def _admin_verify_csrf(self, request: web.Request, provided_token: str) -> bool:
        verifier = getattr(self, "_csrf_verify_token", None)
        if not callable(verifier):
            return False
        try:
            return bool(verifier(request, provided_token))
        except Exception:
            return False

    @staticmethod
    def _admin_is_missing_schema_error(exc: Exception) -> bool:
        normalized = str(exc).strip().lower()
        return any(
            marker in normalized
            for marker in (
                "does not exist",
                "no such table",
                "undefined table",
                "no such column",
                "undefined column",
            )
        )

    def _admin_affiliate_prepare_conn(self, conn: Any) -> None:
        ensure_tables = getattr(self, "_affiliate_ensure_tables", None)
        if callable(ensure_tables):
            ensure_tables(conn)

    def _admin_affiliate_load_pii(self, conn: Any, login: str) -> dict[str, Any]:
        try:
            return AffiliatePII.load_pii(conn, login)
        except Exception as exc:
            if self._admin_is_missing_schema_error(exc):
                return {
                    "full_name": "",
                    "email": "",
                    "address_line1": "",
                    "address_city": "",
                    "address_zip": "",
                    "address_country": "DE",
                    "tax_id": "",
                    "vat_id": "",
                    "ust_status": "unknown",
                    "updated_at": None,
                }
            raise

    @staticmethod
    def _admin_affiliate_gutschrift_download_path(gutschrift_id: int) -> str | None:
        if gutschrift_id <= 0:
            return None
        return f"/twitch/api/admin/affiliates/gutschriften/{gutschrift_id}/pdf"

    def _admin_affiliate_gutschriften_summary(
        self,
        conn: Any,
        *,
        affiliate_login: str | None = None,
    ) -> dict[str, Any]:
        where_clause = ""
        params: list[Any] = []
        if affiliate_login:
            where_clause = "WHERE affiliate_twitch_login = %s"
            params.append(affiliate_login)

        try:
            row = conn.execute(
                f"""
                SELECT
                    COUNT(*) AS total_gutschriften,
                    COALESCE(SUM(gross_amount_cents), 0) AS total_gutschrift_amount_cents,
                    COALESCE(
                        SUM(
                            CASE
                                WHEN pdf_generated_at IS NOT NULL AND email_sent_at IS NULL
                                THEN 1
                                ELSE 0
                            END
                        ),
                        0
                    ) AS pending_email_gutschriften,
                    MAX(pdf_generated_at) AS last_generated_at,
                    MAX(email_sent_at) AS last_emailed_at
                FROM affiliate_gutschriften
                {where_clause}
                """,
                params,
            ).fetchone()
        except Exception as exc:
            if self._admin_is_missing_schema_error(exc):
                return {
                    "total_gutschriften": 0,
                    "total_gutschrift_amount_cents": 0,
                    "total_gutschrift_amount": 0.0,
                    "pending_email_gutschriften": 0,
                    "last_generated_at": None,
                    "last_emailed_at": None,
                }
            raise

        total_gutschrift_amount_cents = _safe_int(
            _row_get_value(row, "total_gutschrift_amount_cents", 1, 0),
            default=0,
        )
        return {
            "total_gutschriften": _safe_int(
                _row_get_value(row, "total_gutschriften", 0, 0),
                default=0,
            ),
            "total_gutschrift_amount_cents": total_gutschrift_amount_cents,
            "total_gutschrift_amount": round(total_gutschrift_amount_cents / 100.0, 2),
            "pending_email_gutschriften": _safe_int(
                _row_get_value(row, "pending_email_gutschriften", 2, 0),
                default=0,
            ),
            "last_generated_at": _row_get_value(row, "last_generated_at", 3, None),
            "last_emailed_at": _row_get_value(row, "last_emailed_at", 4, None),
        }

    @staticmethod
    def _admin_mask_secret(raw_value: Any) -> str:
        value = str(raw_value or "")
        if not value:
            return "[redacted]"
        return f"[redacted:{min(len(value), 999)}]"

    @classmethod
    def _admin_sanitize_log_text(cls, raw_value: Any, *, max_length: int) -> str:
        text = str(raw_value or "").strip()
        if not text:
            return ""

        sanitized = _LOG_HEADER_SECRET_RE.sub(
            lambda match: f"{match.group(1)}{cls._admin_mask_secret(match.group(2))}",
            text,
        )
        sanitized = _LOG_COOKIE_RE.sub(
            lambda match: f"{match.group(1)}{cls._admin_mask_secret(match.group(2))}",
            sanitized,
        )
        sanitized = _LOG_QUOTED_KEY_VALUE_SECRET_RE.sub(
            lambda match: f"{match.group(1)}{cls._admin_mask_secret(match.group(3))}",
            sanitized,
        )
        sanitized = _LOG_KEY_VALUE_SECRET_RE.sub(
            lambda match: f"{match.group(1)}{match.group(2)}{cls._admin_mask_secret(match.group(3))}",
            sanitized,
        )
        sanitized = _LOG_QUERY_SECRET_RE.sub(
            lambda match: f"{match.group(1)}={cls._admin_mask_secret(match.group(2))}",
            sanitized,
        )
        sanitized = _LOG_JWT_RE.sub(cls._admin_mask_secret("[jwt]"), sanitized)
        sanitized = _LOG_OAUTH_TOKEN_RE.sub(cls._admin_mask_secret("[oauth-token]"), sanitized)
        return sanitized[:max_length]

    @staticmethod
    def _admin_normalize_bool(value: Any) -> bool | None:
        if isinstance(value, bool):
            return value
        if isinstance(value, (int, float)) and value in {0, 1}:
            return bool(value)
        if isinstance(value, str):
            normalized = value.strip().lower()
            if normalized in {"1", "true", "yes", "on"}:
                return True
            if normalized in {"0", "false", "no", "off"}:
                return False
        return None

    @staticmethod
    def _admin_parse_scope(raw_value: Any) -> str | None:
        if raw_value is None or str(raw_value).strip() == "":
            return _ADMIN_MANAGED_SCOPE_ACTIVE
        normalized = str(raw_value).strip().lower()
        if normalized not in _ADMIN_MANAGED_SCOPES:
            return None
        return normalized

    @staticmethod
    def _admin_scope_filter_sql(scope: str) -> str:
        if scope == _ADMIN_MANAGED_SCOPE_ALL:
            return "1=1"
        return "status = 'active'"

    @staticmethod
    def _admin_parse_streamer_view(raw_value: Any) -> str | None:
        if raw_value is None or str(raw_value).strip() == "":
            return _ADMIN_STREAMER_VIEW_ACTIVE
        normalized = str(raw_value).strip().lower()
        if normalized not in _ADMIN_STREAMER_VIEWS:
            return None
        return normalized

    @staticmethod
    def _admin_streamer_view_filter_sql(view: str) -> str:
        if view == _ADMIN_STREAMER_VIEW_ARCHIVED:
            return (
                "COALESCE(s.manual_partner_opt_out, 0) = 0 "
                "AND ("
                "    (COALESCE(s.status, 'departnered') = 'active' AND s.archived_at IS NOT NULL) "
                "    OR COALESCE(s.status, '') = 'archived'"
                ")"
            )
        if view == _ADMIN_STREAMER_VIEW_DEPARTNERED:
            return (
                "COALESCE(s.manual_partner_opt_out, 0) = 0 "
                "AND COALESCE(s.status, 'departnered') = 'departnered'"
            )
        if view == _ADMIN_STREAMER_VIEW_NON_PARTNER:
            return "COALESCE(s.manual_partner_opt_out, 0) = 1"
        if view == _ADMIN_STREAMER_VIEW_ALL:
            return "1=1"
        return (
            "COALESCE(s.status, 'departnered') = 'active' "
            "AND s.archived_at IS NULL "
            "AND COALESCE(s.manual_partner_opt_out, 0) = 0"
        )

    @staticmethod
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
        oauth_connected = bool(granted_scopes)
        if needs_reauth_bool:
            oauth_status = "reauth"
        elif not oauth_connected:
            oauth_status = "missing"
        elif missing_scopes:
            oauth_status = "partial"
        else:
            oauth_status = "connected"
        return {
            "connected": oauth_connected,
            "status": oauth_status,
            "needsReauth": needs_reauth_bool,
            "grantedScopes": granted_scopes,
            "missingScopes": missing_scopes,
        }

    @staticmethod
    def _admin_partner_status(
        *,
        status: Any,
        archived_at: Any,
        manual_partner_opt_out: Any,
    ) -> str:
        if bool(manual_partner_opt_out):
            return "non_partner"
        normalized_status = str(status or "").strip().lower()
        if normalized_status == "departnered":
            return "departnered"
        if normalized_status == "archived" or bool(archived_at):
            return "archived"
        return "active"

    @classmethod
    def _admin_load_streamer_config_snapshots(
        cls,
        conn: Any,
        *,
        scope: str = _ADMIN_MANAGED_SCOPE_ACTIVE,
    ) -> tuple[dict[str, Any], dict[str, Any]]:
        normalized_scope = cls._admin_parse_scope(scope) or _ADMIN_MANAGED_SCOPE_ACTIVE
        where_clause = cls._admin_scope_filter_sql(normalized_scope)
        row = conn.execute(
            f"""
            SELECT
                COUNT(*) AS total_managed_streamers,
                COUNT(*) FILTER (WHERE raid_bot_enabled = 1) AS raid_bot_enabled_count,
                COUNT(*) FILTER (WHERE COALESCE(live_ping_enabled, 1) = 1) AS live_ping_enabled_count,
                COUNT(*) FILTER (WHERE silent_ban = 1) AS silent_ban_count,
                COUNT(*) FILTER (WHERE silent_raid = 1) AS silent_raid_count
            FROM twitch_partners
            WHERE {where_clause}
            """
        ).fetchone()
        total_managed_streamers = _safe_int(
            _row_get_value(row, "total_managed_streamers", 0, 0),
            default=0,
        )
        raid_bot_enabled_count = _safe_int(
            _row_get_value(row, "raid_bot_enabled_count", 1, 0),
            default=0,
        )
        live_ping_enabled_count = _safe_int(
            _row_get_value(row, "live_ping_enabled_count", 2, 0),
            default=0,
        )
        silent_ban_count = _safe_int(
            _row_get_value(row, "silent_ban_count", 3, 0),
            default=0,
        )
        silent_raid_count = _safe_int(
            _row_get_value(row, "silent_raid_count", 4, 0),
            default=0,
        )
        raid_snapshot = {
            "managedScope": normalized_scope,
            "scope": normalized_scope,
            "totalManagedStreamers": total_managed_streamers,
            "raidBotEnabledCount": raid_bot_enabled_count,
            "livePingEnabledCount": live_ping_enabled_count,
            "allRaidBotEnabled": total_managed_streamers > 0
            and raid_bot_enabled_count == total_managed_streamers,
            "allLivePingEnabled": total_managed_streamers > 0
            and live_ping_enabled_count == total_managed_streamers,
        }
        chat_snapshot = {
            "managedScope": normalized_scope,
            "scope": normalized_scope,
            "totalManagedStreamers": total_managed_streamers,
            "silentBanCount": silent_ban_count,
            "silentRaidCount": silent_raid_count,
            "allSilentBan": total_managed_streamers > 0
            and silent_ban_count == total_managed_streamers,
            "allSilentRaid": total_managed_streamers > 0
            and silent_raid_count == total_managed_streamers,
        }
        return raid_snapshot, chat_snapshot

    @staticmethod
    def _admin_eventsub_transport(value: Any) -> str:
        if isinstance(value, dict):
            method = str(value.get("method") or "").strip().lower()
            if method:
                return method
        return str(value or "").strip().lower()

    @staticmethod
    def _admin_error_log_candidates() -> tuple[Path, ...]:
        candidates = [
            log_path("twitch_bot.log"),
            log_path("twitch_dashboard.log"),
            log_path("twitch_service_warnings.log"),
            log_path("twitch_autobans.log"),
        ]
        try:
            for candidate in logs_dir().glob("*.log"):
                candidates.append(candidate)
        except OSError:
            pass
        return tuple(dict.fromkeys(candidates))

    @staticmethod
    def _admin_error_log_entry(source: str, line_number: int, raw_line: str) -> dict[str, Any] | None:
        line = str(raw_line or "").strip()
        if not line:
            return None
        upper_line = line.upper()
        if not any(token in upper_line for token in ("ERROR", "CRITICAL", "TRACEBACK", "EXCEPTION")):
            return None

        timestamp = ""
        level = ""
        message = line
        parts = line.split(" - ", 3)
        if len(parts) == 4:
            timestamp = str(parts[0]).strip()
            level = str(parts[2]).strip()
            message = str(parts[3]).strip() or line
        elif len(parts) >= 2:
            timestamp = str(parts[0]).strip()
            message = str(parts[-1]).strip() or line

        sanitized_message = _AnalyticsAdminMixin._admin_sanitize_log_text(
            message,
            max_length=1200,
        )
        sanitized_context = _AnalyticsAdminMixin._admin_sanitize_log_text(
            line,
            max_length=2000,
        )
        return {
            "id": f"{source}:{line_number}",
            "timestamp": timestamp or None,
            "level": level or None,
            "source": source,
            "message": sanitized_message or "[redacted]",
            "context": sanitized_context or sanitized_message or "[redacted]",
        }

    def _load_admin_error_log_entries(self) -> list[dict[str, Any]]:
        entries: list[dict[str, Any]] = []
        for candidate in self._admin_error_log_candidates():
            try:
                if not candidate.exists():
                    continue
            except OSError:
                continue
            recent_lines: deque[tuple[int, str]] = deque(maxlen=_ERROR_LOG_MAX_SCAN_LINES)
            try:
                with candidate.open("r", encoding="utf-8", errors="replace") as handle:
                    for index, line in enumerate(handle, start=1):
                        recent_lines.append((index, line.rstrip("\n")))
            except OSError:
                continue

            for line_number, line in reversed(recent_lines):
                entry = self._admin_error_log_entry(candidate.name, line_number, line)
                if entry is not None:
                    entries.append(entry)
                    if len(entries) >= _ERROR_LOG_MAX_RETURNED:
                        return entries
        return entries

    @staticmethod
    def _admin_database_row_count(conn: Any, table_name: str) -> int | None:
        try:
            row = conn.execute(f"SELECT COUNT(*) AS total FROM {table_name}").fetchone()
        except Exception:
            return None
        if row is None:
            return None
        return _safe_int(_row_get_value(row, "total", 0, None), default=0)

    @staticmethod
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

    @staticmethod
    def _admin_database_size_bytes(conn: Any) -> int | None:
        try:
            row = conn.execute("SELECT pg_database_size(current_database()) AS size_bytes").fetchone()
        except Exception:
            return None
        if row is None:
            return None
        return _safe_int(_row_get_value(row, "size_bytes", 0, None), default=0)

    async def _api_admin_streamers(self, request: web.Request) -> web.Response:
        auth_error = self._admin_auth_error(request, getattr(self, "_require_v2_admin_api", None))
        if auth_error is not None:
            return auth_error

        view = self._admin_parse_streamer_view(request.query.get("view"))
        if view is None:
            return web.json_response(
                {"error": "invalid_view", "supported": sorted(_ADMIN_STREAMER_VIEWS)},
                status=400,
            )
        where_clause = self._admin_streamer_view_filter_sql(view)

        try:
            with storage.transaction() as conn:
                rows = conn.execute(
                    f"""
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
                ).fetchall()
        except Exception as exc:
            return _admin_500(exc)

        payload = []
        for row in rows:
            login = str(_row_get_value(row, "twitch_login", 0, "") or "").strip().lower()
            archived_at = _row_get_value(row, "archived_at", 5, None)
            archived = bool(archived_at)
            is_live = bool(_row_get_value(row, "is_live", 16, 0))
            verified = bool(_row_get_value(row, "is_verified", 14, 0))
            manual_partner_opt_out = bool(_row_get_value(row, "manual_partner_opt_out", 8, 0))
            partner_status = self._admin_partner_status(
                status=_row_get_value(row, "status", 9, None),
                archived_at=archived_at,
                manual_partner_opt_out=manual_partner_opt_out,
            )
            scope_snapshot = self._admin_scope_snapshot(
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
            payload.append(
                {
                    "login": login,
                    "displayName": str(
                        _row_get_value(row, "discord_display_name", 3, "") or login
                    ).strip()
                    or login,
                    "twitchUserId": str(_row_get_value(row, "twitch_user_id", 1, "") or "").strip()
                    or None,
                    "discordUserId": str(_row_get_value(row, "discord_user_id", 2, "") or "").strip()
                    or None,
                    "discordDisplayName": str(
                        _row_get_value(row, "discord_display_name", 3, "") or ""
                    ).strip()
                    or None,
                    "verified": verified,
                    "archived": archived,
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
                    "lastStreamAt": _json_safe_datetime(
                        _row_get_value(row, "last_stream_at", 21, None)
                    ),
                    "planId": str(
                        _row_get_value(row, "manual_plan_id", 28, "")
                        or _row_get_value(row, "billing_plan_id", 31, "")
                        or ""
                    ).strip()
                    or None,
                    "billingStatus": str(_row_get_value(row, "billing_status", 32, "") or "").strip()
                    or None,
                    "oauthConnected": bool(scope_snapshot["connected"]),
                    "oauthNeedsReauth": bool(scope_snapshot["needsReauth"]),
                    "oauthStatus": str(scope_snapshot["status"]),
                    "grantedScopes": list(scope_snapshot["grantedScopes"]),
                    "missingScopes": list(scope_snapshot["missingScopes"]),
                    "oauthAuthorizedAt": _json_safe_datetime(
                        _row_get_value(row, "authorized_at", 24, None)
                    ),
                    "promoDisabled": bool(_row_get_value(row, "promo_disabled", 25, 0)),
                    "notes": str(_row_get_value(row, "manual_plan_notes", 30, "") or "").strip()
                    or None,
                    "status": status,
                }
            )
        return web.json_response({"items": payload, "count": len(payload), "view": view})

    async def _api_admin_streamer_detail(self, request: web.Request) -> web.Response:
        auth_error = self._admin_auth_error(request, getattr(self, "_require_v2_admin_api", None))
        if auth_error is not None:
            return auth_error

        login = _normalize_login(request.match_info.get("login", ""))
        if not login:
            return web.json_response({"error": "invalid_login"}, status=400)

        try:
            with storage.transaction() as conn:
                row = conn.execute(
                    f"""
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
                    """,
                    (login,),
                ).fetchone()
                if row is None:
                    return web.json_response({"error": "not_found"}, status=404)

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
                    (login,),
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
                    (login,),
                ).fetchall()
        except Exception as exc:
            return _admin_500(exc)

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
        scope_snapshot = self._admin_scope_snapshot(
            _row_get_value(row, "scopes", 23, ""),
            _row_get_value(row, "needs_reauth", 24, 0),
        )
        partner_status = self._admin_partner_status(
            status=_row_get_value(row, "status", 16, None),
            archived_at=archived_at,
            manual_partner_opt_out=manual_partner_opt_out,
        )
        payload = {
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
                "lastStartedAt": _json_safe_datetime(
                    _row_get_value(row, "last_started_at", 21, None)
                ),
                "lastGame": _row_get_value(row, "last_game", 22, None),
            },
            "settings": {
                "requireDiscordLink": bool(_row_get_value(row, "require_discord_link", 6, 0)),
                "isOnDiscord": bool(_row_get_value(row, "is_on_discord", 7, 0)),
                "discordUserId": str(_row_get_value(row, "discord_user_id", 2, "") or "").strip() or None,
                "discordDisplayName": str(
                    _row_get_value(row, "discord_display_name", 3, "") or ""
                ).strip()
                or None,
                "manualPartnerOptOut": manual_partner_opt_out,
                "raidBotEnabled": bool(_row_get_value(row, "raid_bot_enabled", 9, 0)),
                "silentBan": bool(_row_get_value(row, "silent_ban", 10, 0)),
                "silentRaid": bool(_row_get_value(row, "silent_raid", 11, 0)),
                "isMonitoredOnly": bool(_row_get_value(row, "is_monitored_only", 12, 0)),
                "livePingEnabled": bool(_row_get_value(row, "live_ping_enabled", 15, 1)),
                "oauthConnected": bool(scope_snapshot["connected"]),
                "oauthStatus": str(scope_snapshot["status"]),
                "oauthNeedsReauth": bool(scope_snapshot["needsReauth"]),
                "oauthRaidEnabled": bool(_row_get_value(row, "oauth_raid_enabled", 25, 0)),
                "oauthAuthorizedAt": _json_safe_datetime(
                    _row_get_value(row, "authorized_at", 26, None)
                ),
                "grantedScopes": list(scope_snapshot["grantedScopes"]),
                "missingScopes": list(scope_snapshot["missingScopes"]),
                "promoDisabled": bool(_row_get_value(row, "promo_disabled", 28, 0)),
                "promoMessage": _row_get_value(row, "promo_message", 29, None),
                "raidBoostEnabled": bool(_row_get_value(row, "raid_boost_enabled", 30, 0)),
                "notes": _row_get_value(row, "notes", 31, None),
                "manualPlanId": _row_get_value(row, "manual_plan_id", 32, None),
                "manualPlanExpiresAt": _json_safe_datetime(
                    _row_get_value(row, "manual_plan_expires_at", 33, None)
                ),
                "manualPlanNotes": _row_get_value(row, "manual_plan_notes", 34, None),
                "billingStatus": _row_get_value(row, "billing_status", 36, None),
                "billingUpdatedAt": _json_safe_datetime(
                    _row_get_value(row, "billing_updated_at", 37, None)
                ),
            },
            "sessions": session_payload,
            "recentActivity": session_payload[:5],
        }
        return web.json_response(payload)

    async def _api_admin_system_oauth_scopes(self, request: web.Request) -> web.Response:
        auth_error = self._admin_auth_error(request, getattr(self, "_require_v2_admin_api", None))
        if auth_error is not None:
            return auth_error

        try:
            with storage.transaction() as conn:
                rows = conn.execute(
                    f"""
                    WITH auth_rows AS (
                        SELECT
                            ROW_NUMBER() OVER (
                                ORDER BY
                                    CASE WHEN authorized_at IS NULL THEN 1 ELSE 0 END,
                                    authorized_at DESC,
                                    LOWER(COALESCE(NULLIF(TRIM(twitch_login), ''), '')),
                                    LOWER(COALESCE(NULLIF(TRIM(twitch_user_id), ''), ''))
                            ) AS auth_row_id,
                            twitch_login,
                            twitch_user_id,
                            scopes,
                            needs_reauth,
                            authorized_at
                        FROM twitch_raid_auth
                    )
                    {_admin_partner_state_cte_sql()}
                    , ranked_auth_matches AS (
                        SELECT
                            a.auth_row_id,
                            a.twitch_login,
                            a.twitch_user_id,
                            a.scopes,
                            a.needs_reauth,
                            a.authorized_at,
                            s.twitch_login AS partner_login,
                            s.discord_display_name,
                            s.archived_at,
                            s.manual_partner_opt_out,
                            s.status,
                            ROW_NUMBER() OVER (
                                PARTITION BY a.auth_row_id
                                ORDER BY
                                    CASE
                                        WHEN NULLIF(TRIM(COALESCE(a.twitch_user_id, '')), '') IS NOT NULL
                                             AND NULLIF(TRIM(COALESCE(s.twitch_user_id, '')), '') IS NOT NULL
                                             AND LOWER(TRIM(a.twitch_user_id)) = LOWER(TRIM(s.twitch_user_id))
                                        THEN 0
                                        WHEN LOWER(COALESCE(a.twitch_login, '')) = LOWER(s.twitch_login)
                                        THEN 1
                                        ELSE 2
                                    END,
                                    CASE
                                        WHEN COALESCE(s.manual_partner_opt_out, 0) = 1 THEN 3
                                        WHEN COALESCE(s.status, 'departnered') = 'active' AND s.archived_at IS NULL THEN 0
                                        WHEN COALESCE(s.status, 'departnered') IN ('active', 'archived') THEN 1
                                        ELSE 2
                                    END,
                                    CASE
                                        WHEN s.created_at IS NULL AND s.archived_at IS NULL THEN 1
                                        ELSE 0
                                    END,
                                    CASE
                                        WHEN s.created_at IS NOT NULL THEN s.created_at
                                        ELSE s.archived_at
                                    END DESC,
                                    LOWER(COALESCE(s.twitch_login, '')) ASC
                            ) AS rn
                        FROM auth_rows a
                        LEFT JOIN partner_state s
                            ON (
                                NULLIF(TRIM(COALESCE(a.twitch_user_id, '')), '') IS NOT NULL
                                AND NULLIF(TRIM(COALESCE(s.twitch_user_id, '')), '') IS NOT NULL
                                AND LOWER(TRIM(a.twitch_user_id)) = LOWER(TRIM(s.twitch_user_id))
                            )
                            OR LOWER(COALESCE(a.twitch_login, '')) = LOWER(s.twitch_login)
                    )
                    SELECT
                        auth_row_id,
                        COALESCE(
                            NULLIF(TRIM(partner_login), ''),
                            NULLIF(TRIM(twitch_login), ''),
                            NULLIF(TRIM(twitch_user_id), '')
                        ) AS effective_login,
                        twitch_login,
                        twitch_user_id,
                        scopes,
                        needs_reauth,
                        authorized_at,
                        partner_login,
                        discord_display_name,
                        archived_at,
                        manual_partner_opt_out,
                        status
                    FROM ranked_auth_matches
                    WHERE rn = 1
                    ORDER BY
                        LOWER(
                            COALESCE(
                                NULLIF(TRIM(partner_login), ''),
                                NULLIF(TRIM(twitch_login), ''),
                                NULLIF(TRIM(twitch_user_id), '')
                            )
                        ) ASC,
                        auth_row_id ASC
                    """
                ).fetchall()
        except Exception as exc:
            if self._admin_is_missing_schema_error(exc):
                return web.json_response(
                    {
                        "requiredScopes": list(_ADMIN_REQUIRED_SCOPES),
                        "criticalScopes": sorted(_ADMIN_CRITICAL_SCOPES),
                        "labels": dict(_ADMIN_SCOPE_COLUMN_LABELS),
                        "summary": {
                            "totalAuthorized": 0,
                            "fullScopeCount": 0,
                            "missingScopeCount": 0,
                        },
                        "items": [],
                    }
                )
            return _admin_500(exc)

        payload_rows: list[dict[str, Any]] = []
        total_authorized = 0
        full_scope_count = 0
        for row in rows:
            login = str(_row_get_value(row, "effective_login", 1, "") or "").strip().lower()
            if not login:
                continue
            total_authorized += 1
            scope_snapshot = self._admin_scope_snapshot(
                _row_get_value(row, "scopes", 4, ""),
                _row_get_value(row, "needs_reauth", 5, 0),
            )
            if (
                scope_snapshot["connected"]
                and not scope_snapshot["missingScopes"]
                and not scope_snapshot["needsReauth"]
            ):
                full_scope_count += 1
            partner_status = self._admin_partner_status(
                status=_row_get_value(row, "status", 11, None),
                archived_at=_row_get_value(row, "archived_at", 9, None),
                manual_partner_opt_out=_row_get_value(row, "manual_partner_opt_out", 10, 0),
            )
            payload_rows.append(
                {
                    "login": login,
                    "displayName": str(_row_get_value(row, "discord_display_name", 8, "") or login)
                    .strip()
                    or login,
                    "partnerStatus": partner_status,
                    "archivedAt": _json_safe_datetime(_row_get_value(row, "archived_at", 9, None)),
                    "oauthStatus": scope_snapshot["status"],
                    "oauthNeedsReauth": bool(scope_snapshot["needsReauth"]),
                    "grantedScopes": list(scope_snapshot["grantedScopes"]),
                    "missingScopes": list(scope_snapshot["missingScopes"]),
                }
            )

        return web.json_response(
            {
                "requiredScopes": list(_ADMIN_REQUIRED_SCOPES),
                "criticalScopes": sorted(_ADMIN_CRITICAL_SCOPES),
                "labels": dict(_ADMIN_SCOPE_COLUMN_LABELS),
                "summary": {
                    "totalAuthorized": total_authorized,
                    "fullScopeCount": full_scope_count,
                    "missingScopeCount": max(0, total_authorized - full_scope_count),
                },
                "items": payload_rows,
            }
        )

    async def _api_admin_system_health(self, request: web.Request) -> web.Response:
        auth_error = self._admin_auth_error(request, getattr(self, "_require_v2_admin_api", None))
        if auth_error is not None:
            return auth_error

        memory_bytes = None
        memory_rss_bytes = None
        uptime_seconds = None
        process_id = os.getpid()
        try:
            import psutil  # type: ignore

            process = psutil.Process(process_id)
            mem_info = process.memory_info()
            memory_bytes = int(getattr(mem_info, "rss", 0) or 0)
            memory_rss_bytes = memory_bytes
            uptime_seconds = max(0, int(time.time() - float(process.create_time())))
        except Exception:
            runtime_started_at = getattr(self, "_admin_runtime_started_at", None)
            if not runtime_started_at:
                runtime_started_at = time.time()
                setattr(self, "_admin_runtime_started_at", runtime_started_at)
            uptime_seconds = max(0, int(time.time() - float(runtime_started_at)))

        last_tick_at = None
        raw_chat_snapshot = {
            "streamerLogin": None,
            "lastMessageAt": None,
            "lastInsertOkAt": None,
            "lastInsertErrorAt": None,
            "lastError": None,
            "rawChatLagSeconds": None,
            "isLiveScope": False,
        }
        try:
            with storage.transaction() as conn:
                row = conn.execute(
                    """
                    SELECT MAX(COALESCE(last_seen_at, last_started_at)) AS last_tick_at
                    FROM twitch_live_state
                    """
                ).fetchone()
                last_tick_at = _row_get_value(row, "last_tick_at", 0, None) if row else None
                try:
                    raw_chat_snapshot = _fetch_raw_chat_health_snapshot(conn)
                except Exception:
                    raw_chat_snapshot = {
                        "streamerLogin": None,
                        "lastMessageAt": None,
                        "lastInsertOkAt": None,
                        "lastInsertErrorAt": None,
                        "lastError": None,
                        "rawChatLagSeconds": None,
                        "isLiveScope": False,
                    }
        except Exception:
            last_tick_at = None

        last_tick_age_seconds = None
        parsed_last_tick = _coerce_utc_datetime(last_tick_at)
        if parsed_last_tick is not None:
            last_tick_age_seconds = max(
                0,
                int((datetime.now(UTC) - parsed_last_tick).total_seconds()),
            )

        analytics_db_fingerprint = str(
            request.app.get(ANALYTICS_DB_FINGERPRINT_KEY) or storage.analytics_db_fingerprint()
        ).strip() or None
        internal_analytics_db_fingerprint = (
            str(request.app.get(INTERNAL_API_ANALYTICS_DB_FINGERPRINT_KEY) or "").strip() or None
        )
        analytics_db_fingerprint_mismatch = bool(
            request.app.get(ANALYTICS_DB_FINGERPRINT_MISMATCH_KEY)
        )
        analytics_db_fingerprint_error = (
            str(request.app.get(ANALYTICS_DB_FINGERPRINT_ERROR_KEY) or "").strip() or None
        )

        service_warnings: list[dict[str, Any]] = []
        if analytics_db_fingerprint_mismatch:
            service_warnings.append(
                {
                    "level": "error",
                    "code": "analytics_db_fingerprint_mismatch",
                    "message": (
                        "Dashboard und Bot-Service zeigen auf unterschiedliche Analytics-Datenbanken."
                    ),
                    "timestamp": datetime.now(UTC).isoformat(),
                    "analyticsDbFingerprint": analytics_db_fingerprint,
                    "internalAnalyticsDbFingerprint": internal_analytics_db_fingerprint,
                }
            )
        elif analytics_db_fingerprint_error:
            service_warnings.append(
                {
                    "level": "warning",
                    "code": "analytics_db_fingerprint_check_failed",
                    "message": analytics_db_fingerprint_error,
                    "timestamp": datetime.now(UTC).isoformat(),
                }
            )

        raw_chat_lag_seconds = raw_chat_snapshot.get("rawChatLagSeconds")
        raw_chat_last_error = raw_chat_snapshot.get("lastError")
        if raw_chat_snapshot.get("isLiveScope") and isinstance(raw_chat_lag_seconds, int):
            if raw_chat_lag_seconds >= _RAW_CHAT_LAG_WARNING_SECONDS:
                service_warnings.append(
                    {
                        "level": "warning",
                        "code": "raw_chat_lag_high",
                        "message": (
                            "Roh-Chat-Ingestion ist für einen live überwachten Kanal verzögert."
                        ),
                        "timestamp": raw_chat_snapshot.get("lastMessageAt")
                        or raw_chat_snapshot.get("lastInsertOkAt")
                        or raw_chat_snapshot.get("lastInsertErrorAt"),
                        "streamerLogin": raw_chat_snapshot.get("streamerLogin"),
                        "rawChatLagSeconds": raw_chat_lag_seconds,
                    }
                )
        if raw_chat_last_error:
            service_warnings.append(
                {
                    "level": "warning",
                    "code": "raw_chat_insert_error",
                    "message": f"Letzter Roh-Chat-Insert-Fehler: {raw_chat_last_error}",
                    "timestamp": raw_chat_snapshot.get("lastInsertErrorAt"),
                    "streamerLogin": raw_chat_snapshot.get("streamerLogin"),
                }
            )

        return web.json_response(
            {
                "uptimeSeconds": uptime_seconds,
                "memoryBytes": memory_bytes,
                "memoryRssBytes": memory_rss_bytes,
                "pythonVersion": platform.python_version(),
                "processId": process_id,
                "lastTickAt": last_tick_at,
                "lastTickAgeSeconds": last_tick_age_seconds,
                "rawChatLagSeconds": raw_chat_snapshot.get("rawChatLagSeconds"),
                "rawChatLagStreamer": raw_chat_snapshot.get("streamerLogin"),
                "rawChatLastMessageAt": raw_chat_snapshot.get("lastMessageAt"),
                "rawChatLastInsertOkAt": raw_chat_snapshot.get("lastInsertOkAt"),
                "rawChatLastInsertErrorAt": raw_chat_snapshot.get("lastInsertErrorAt"),
                "rawChatLastError": raw_chat_snapshot.get("lastError"),
                "analyticsDbFingerprint": analytics_db_fingerprint,
                "internalAnalyticsDbFingerprint": internal_analytics_db_fingerprint,
                "analyticsDbFingerprintMismatch": analytics_db_fingerprint_mismatch,
                "serviceWarnings": service_warnings,
            }
        )

    async def _api_admin_system_eventsub(self, request: web.Request) -> web.Response:
        auth_error = self._admin_auth_error(request, getattr(self, "_require_v2_admin_api", None))
        if auth_error is not None:
            return auth_error

        overview_getter = getattr(self, "_get_eventsub_capacity_overview", None)
        overview: dict[str, Any] = {}
        if callable(overview_getter):
            try:
                overview = await overview_getter(hours=24)
            except Exception:
                overview = {}

        current = overview.get("current") if isinstance(overview, dict) else {}
        subscriptions = list(overview.get("active_subscriptions") or []) if isinstance(overview, dict) else []
        websocket_status = "inactive"
        if subscriptions:
            transports = {
                self._admin_eventsub_transport(item.get("transport"))
                for item in subscriptions
                if isinstance(item, dict)
            }
            if "websocket" in transports:
                websocket_status = "connected"
            elif "webhook" in transports:
                websocket_status = "webhook"

        return web.json_response(
            {
                "websocketStatus": websocket_status,
                "websocketSessionId": getattr(self, "_eventsub_session_id", None),
                "websocketConnectedAt": None,
                "websocketReconnectedAt": None,
                "activeSubscriptionCount": len(subscriptions),
                "capacity": {
                    "used": _safe_int((current or {}).get("used_slots"), default=0)
                    if isinstance(current, dict)
                    else 0,
                    "max": _safe_int((current or {}).get("total_slots"), default=0)
                    if isinstance(current, dict)
                    else 0,
                    "remaining": max(
                        0,
                        _safe_int((current or {}).get("headroom_slots"), default=0),
                    )
                    if isinstance(current, dict)
                    else 0,
                    "lastSnapshotAt": overview.get("last_snapshot_at") if isinstance(overview, dict) else None,
                },
                "subscriptions": subscriptions,
                "transportMode": websocket_status,
                "raw": overview,
            }
        )

    async def _api_admin_system_database(self, request: web.Request) -> web.Response:
        auth_error = self._admin_auth_error(request, getattr(self, "_require_v2_admin_api", None))
        if auth_error is not None:
            return auth_error

        tables: list[dict[str, Any]] = []
        database_size_bytes = None
        try:
            with storage.transaction() as conn:
                database_size_bytes = self._admin_database_size_bytes(conn)
                for table_name in _DATABASE_STATS_TABLES:
                    row_count = self._admin_database_row_count(conn, table_name)
                    if row_count is None:
                        continue
                    tables.append(
                        {
                            "table": table_name,
                            "rowCount": row_count,
                            "sizeBytes": self._admin_database_table_size_bytes(conn, table_name),
                        }
                    )
        except Exception as exc:
            return _admin_500(exc)

        return web.json_response(
            {
                "databaseSizeBytes": database_size_bytes,
                "tables": tables,
            }
        )

    async def _api_admin_system_errors(self, request: web.Request) -> web.Response:
        auth_error = self._admin_auth_error(request, getattr(self, "_require_v2_admin_api", None))
        if auth_error is not None:
            return auth_error

        try:
            page = max(1, int(request.query.get("page", "1")))
        except ValueError:
            page = 1
        try:
            page_size = min(100, max(1, int(request.query.get("page_size", "25"))))
        except ValueError:
            page_size = 25

        entries = self._load_admin_error_log_entries()
        total = len(entries)
        start = (page - 1) * page_size
        end = start + page_size
        return web.json_response(
            {
                "page": page,
                "pageSize": page_size,
                "total": total,
                "hasMore": end < total,
                "entries": entries[start:end],
            }
        )

    async def _api_admin_config_overview(self, request: web.Request) -> web.Response:
        auth_error = self._admin_auth_error(request, getattr(self, "_require_v2_admin_api", None))
        if auth_error is not None:
            return auth_error

        promo_config: dict[str, Any] = {}
        raid_snapshot: dict[str, Any] = {}
        chat_snapshot: dict[str, Any] = {}
        csrf_token = ""
        csrf_getter = getattr(self, "_csrf_get_token", None)
        csrf_generator = getattr(self, "_csrf_generate_token", None)
        if callable(csrf_getter):
            try:
                csrf_token = str(csrf_getter(request) or "")
            except Exception:
                csrf_token = ""
        if not csrf_token and callable(csrf_generator):
            try:
                csrf_token = str(csrf_generator(request) or "")
            except Exception:
                csrf_token = ""

        scope = self._admin_parse_scope(request.query.get("scope"))
        if scope is None:
            return web.json_response(
                {
                    "error": "invalid_scope",
                    "message": "scope muss 'active' oder 'all' sein.",
                },
                status=400,
            )
        try:
            with storage.transaction() as conn:
                promo_config = evaluate_global_promo_mode(load_global_promo_mode(conn))
                raid_snapshot, chat_snapshot = self._admin_load_streamer_config_snapshots(
                    conn,
                    scope=scope,
                )
        except Exception as exc:
            return _admin_500(exc)

        return web.json_response(
            {
                "promo": promo_config,
                "raids": raid_snapshot,
                "chat": chat_snapshot,
                "announcements": promo_config.get("config", {}) if isinstance(promo_config, dict) else {},
                "csrfToken": csrf_token or None,
                "csrf_token": csrf_token or None,
            }
        )

    async def _api_admin_config_promo(self, request: web.Request) -> web.Response:
        auth_error = self._admin_auth_error(request, getattr(self, "_require_v2_admin_api", None))
        if auth_error is not None:
            return auth_error

        csrf_token, payload = await self._admin_extract_csrf(request)
        if not self._admin_verify_csrf(request, csrf_token):
            return web.json_response({"error": "invalid_csrf"}, status=403)

        normalized, issues = validate_global_promo_mode_config(payload)
        if issues:
            return web.json_response(
                {
                    "error": "validation_failed",
                    "validation": issues,
                },
                status=400,
            )

        actor_label = self._admin_actor_label(request, getattr(self, "_get_discord_admin_session", None))
        try:
            with storage.transaction() as conn:
                saved = save_global_promo_mode(conn, config=normalized, updated_by=actor_label)
                evaluation = evaluate_global_promo_mode(saved)
        except ValueError as exc:
            return web.json_response({"error": str(exc)}, status=400)
        except Exception as exc:
            return _admin_500(exc)

        return web.json_response({"ok": True, "config": saved, "evaluation": evaluation})

    async def _api_admin_config_raids(self, request: web.Request) -> web.Response:
        auth_error = self._admin_auth_error(request, getattr(self, "_require_v2_admin_api", None))
        if auth_error is not None:
            return auth_error

        csrf_token, payload = await self._admin_extract_csrf(request)
        if not self._admin_verify_csrf(request, csrf_token):
            return web.json_response({"error": "invalid_csrf"}, status=403)

        raid_bot_enabled = self._admin_normalize_bool(payload.get("raid_bot_enabled"))
        live_ping_enabled = self._admin_normalize_bool(payload.get("live_ping_enabled"))
        if raid_bot_enabled is None or live_ping_enabled is None:
            return web.json_response(
                {
                    "error": "validation_failed",
                    "validation": [
                        {
                            "path": "raid_bot_enabled",
                            "message": "raid_bot_enabled muss boolean sein.",
                        },
                        {
                            "path": "live_ping_enabled",
                            "message": "live_ping_enabled muss boolean sein.",
                        },
                    ],
                },
                status=400,
            )

        scope = self._admin_parse_scope(payload.get("scope"))
        if scope is None:
            return web.json_response(
                {
                    "error": "invalid_scope",
                    "message": "scope muss 'active' oder 'all' sein.",
                },
                status=400,
            )
        where_clause = self._admin_scope_filter_sql(scope)
        actor_label = self._admin_actor_label(request, getattr(self, "_get_discord_admin_session", None))
        updated_at = datetime.now(UTC).isoformat()

        try:
            with storage.transaction() as conn:
                target_count = storage.bulk_update_partner_flags(
                    conn,
                    scope=scope,
                    raid_bot_enabled=raid_bot_enabled,
                    live_ping_enabled=live_ping_enabled,
                )
                updated_count = target_count
                raid_snapshot, chat_snapshot = self._admin_load_streamer_config_snapshots(
                    conn,
                    scope=scope,
                )
        except Exception as exc:
            return _admin_500(exc)

        return web.json_response(
            {
                "ok": True,
                "scope": scope,
                "updatedAt": updated_at,
                "updatedBy": actor_label,
                "targetCount": target_count,
                "updatedCount": updated_count,
                "raids": {
                    **raid_snapshot,
                    "raidBotEnabled": raid_bot_enabled,
                    "livePingEnabled": live_ping_enabled,
                },
                "chat": chat_snapshot,
            }
        )

    async def _api_admin_config_chat(self, request: web.Request) -> web.Response:
        auth_error = self._admin_auth_error(request, getattr(self, "_require_v2_admin_api", None))
        if auth_error is not None:
            return auth_error

        csrf_token, payload = await self._admin_extract_csrf(request)
        if not self._admin_verify_csrf(request, csrf_token):
            return web.json_response({"error": "invalid_csrf"}, status=403)

        silent_ban = self._admin_normalize_bool(payload.get("silent_ban"))
        silent_raid = self._admin_normalize_bool(payload.get("silent_raid"))
        if silent_ban is None or silent_raid is None:
            return web.json_response(
                {
                    "error": "validation_failed",
                    "validation": [
                        {
                            "path": "silent_ban",
                            "message": "silent_ban muss boolean sein.",
                        },
                        {
                            "path": "silent_raid",
                            "message": "silent_raid muss boolean sein.",
                        },
                    ],
                },
                status=400,
            )

        scope = self._admin_parse_scope(payload.get("scope"))
        if scope is None:
            return web.json_response(
                {
                    "error": "invalid_scope",
                    "message": "scope muss 'active' oder 'all' sein.",
                },
                status=400,
            )
        where_clause = self._admin_scope_filter_sql(scope)
        actor_label = self._admin_actor_label(request, getattr(self, "_get_discord_admin_session", None))
        updated_at = datetime.now(UTC).isoformat()

        try:
            with storage.transaction() as conn:
                target_count = storage.bulk_update_partner_flags(
                    conn,
                    scope=scope,
                    silent_ban=silent_ban,
                    silent_raid=silent_raid,
                )
                updated_count = target_count
                raid_snapshot, chat_snapshot = self._admin_load_streamer_config_snapshots(
                    conn,
                    scope=scope,
                )
        except Exception as exc:
            return _admin_500(exc)

        return web.json_response(
            {
                "ok": True,
                "scope": scope,
                "updatedAt": updated_at,
                "updatedBy": actor_label,
                "targetCount": target_count,
                "updatedCount": updated_count,
                "raids": raid_snapshot,
                "chat": {
                    **chat_snapshot,
                    "silentBan": silent_ban,
                    "silentRaid": silent_raid,
                },
            }
        )

    async def _api_admin_billing_subscriptions(self, request: web.Request) -> web.Response:
        auth_error = self._admin_auth_error(request, getattr(self, "_require_v2_admin_api", None))
        if auth_error is not None:
            return auth_error

        try:
            with storage.transaction() as conn:
                rows = conn.execute(
                    """
                    SELECT
                        b.customer_reference,
                        b.plan_id,
                        b.status,
                        b.current_period_start,
                        b.current_period_end,
                        b.updated_at,
                        b.canceled_at,
                        b.ended_at,
                        sp.manual_plan_id,
                        sp.manual_plan_expires_at
                    FROM twitch_billing_subscriptions b
                    LEFT JOIN streamer_plans sp
                        ON LOWER(sp.twitch_login) = LOWER(b.customer_reference)
                    ORDER BY b.updated_at DESC
                    """
                ).fetchall()
        except Exception as exc:
            return _admin_500(exc)

        payload = [
            {
                "login": str(_row_get_value(row, "customer_reference", 0, "") or "").strip().lower()
                or None,
                "customerReference": _row_get_value(row, "customer_reference", 0, None),
                "planId": _row_get_value(row, "plan_id", 1, None),
                "status": _row_get_value(row, "status", 2, None),
                "trialEndsAt": None,
                "currentPeriodEnd": _row_get_value(row, "current_period_end", 4, None),
                "updatedAt": _row_get_value(row, "updated_at", 5, None),
                "manualPlanId": _row_get_value(row, "manual_plan_id", 8, None),
                "manualPlanExpiresAt": _row_get_value(row, "manual_plan_expires_at", 9, None),
            }
            for row in rows
        ]
        return web.json_response({"items": payload, "count": len(payload)})

    async def _api_admin_billing_affiliates(self, request: web.Request) -> web.Response:
        auth_error = self._admin_auth_error(request, getattr(self, "_require_v2_admin_api", None))
        if auth_error is not None:
            return auth_error

        try:
            with storage.transaction() as conn:
                rows = conn.execute(
                    """
                    SELECT
                        twitch_login,
                        email,
                        stripe_account_id,
                        stripe_connect_status,
                        commission_rate,
                        updated_at,
                        created_at
                    FROM affiliate_accounts
                    ORDER BY COALESCE(updated_at, created_at) DESC
                    """
                ).fetchall()
        except Exception as exc:
            return _admin_500(exc)

        payload = []
        for row in rows:
            payload.append(
                {
                    "twitchLogin": _row_get_value(row, "twitch_login", 0, None),
                    "stripeAccountId": _row_get_value(row, "stripe_account_id", 2, None),
                    "status": _row_get_value(row, "stripe_connect_status", 3, None),
                    "payoutEmail": _row_get_value(row, "email", 1, None),
                    "commissionRate": _row_get_value(row, "commission_rate", 4, None),
                    "updatedAt": _row_get_value(row, "updated_at", 5, None)
                    or _row_get_value(row, "created_at", 6, None),
                }
            )
        return web.json_response({"items": payload, "count": len(payload)})

    # ------------------------------------------------------------------ #
    # Affiliate management endpoints                                      #
    # ------------------------------------------------------------------ #

    async def _api_admin_affiliates_list(self, request: web.Request) -> web.Response:
        """List all affiliates with claims and provision totals."""
        auth_error = self._admin_auth_error(request, getattr(self, "_require_v2_admin_api", None))
        if auth_error is not None:
            return auth_error

        try:
            with storage.transaction() as conn:
                self._admin_affiliate_prepare_conn(conn)
                try:
                    rows = conn.execute(
                        f"""
                        SELECT
                            a.twitch_login,
                            a.display_name,
                            a.is_active,
                            a.created_at,
                            COALESCE(claim_stats.total_claims, 0)       AS total_claims,
                            COALESCE(comm_stats.total_provision, 0)     AS total_provision,
                            claim_stats.last_claim_at,
                            COALESCE(pii.ust_status, 'unknown')         AS ust_status,
                            CASE WHEN pii.twitch_login IS NOT NULL THEN 1 ELSE 0 END AS has_pii
                        FROM affiliate_accounts a
                        LEFT JOIN affiliate_pii pii
                          ON pii.twitch_login = a.twitch_login
                        LEFT JOIN (
                            SELECT
                                affiliate_twitch_login,
                                COUNT(*) AS total_claims,
                                MAX(claimed_at) AS last_claim_at
                            FROM affiliate_streamer_claims
                            GROUP BY affiliate_twitch_login
                        ) claim_stats ON claim_stats.affiliate_twitch_login = a.twitch_login
                        LEFT JOIN (
                            SELECT
                                affiliate_twitch_login,
                                SUM(
                                    CASE
                                        WHEN status IN ({_AFFILIATE_REVENUE_STATUS_PLACEHOLDERS})
                                        THEN commission_cents
                                        ELSE 0
                                    END
                                ) AS total_provision
                            FROM affiliate_commissions
                            GROUP BY affiliate_twitch_login
                        ) comm_stats ON comm_stats.affiliate_twitch_login = a.twitch_login
                        ORDER BY a.created_at DESC
                        """,
                        [*_AFFILIATE_REVENUE_STATUSES],
                    ).fetchall()
                except Exception as exc:
                    if not self._admin_is_missing_schema_error(exc):
                        raise
                    rows = conn.execute(
                        f"""
                        SELECT
                            a.twitch_login,
                            a.display_name,
                            a.is_active,
                            a.created_at,
                            COALESCE(claim_stats.total_claims, 0)       AS total_claims,
                            COALESCE(comm_stats.total_provision, 0)     AS total_provision,
                            claim_stats.last_claim_at,
                            'unknown'                                   AS ust_status,
                            0                                           AS has_pii
                        FROM affiliate_accounts a
                        LEFT JOIN (
                            SELECT
                                affiliate_twitch_login,
                                COUNT(*) AS total_claims,
                                MAX(claimed_at) AS last_claim_at
                            FROM affiliate_streamer_claims
                            GROUP BY affiliate_twitch_login
                        ) claim_stats ON claim_stats.affiliate_twitch_login = a.twitch_login
                        LEFT JOIN (
                            SELECT
                                affiliate_twitch_login,
                                SUM(
                                    CASE
                                        WHEN status IN ({_AFFILIATE_REVENUE_STATUS_PLACEHOLDERS})
                                        THEN commission_cents
                                        ELSE 0
                                    END
                                ) AS total_provision
                            FROM affiliate_commissions
                            GROUP BY affiliate_twitch_login
                        ) comm_stats ON comm_stats.affiliate_twitch_login = a.twitch_login
                        ORDER BY a.created_at DESC
                        """,
                        [*_AFFILIATE_REVENUE_STATUSES],
                    ).fetchall()
        except Exception as exc:
            if self._admin_is_missing_schema_error(exc):
                return web.json_response({"affiliates": []})
            return _admin_500(exc)

        affiliates = []
        for row in rows:
            total_provision_cents = _safe_int(
                _row_get_value(row, "total_provision", 5, 0), default=0
            )
            affiliates.append(
                {
                    "login": str(
                        _row_get_value(row, "twitch_login", 0, "") or ""
                    ).strip(),
                    "display_name": _row_get_value(row, "display_name", 1, None),
                    "active": bool(
                        _safe_int(_row_get_value(row, "is_active", 2, 1), default=1)
                    ),
                    "total_claims": _safe_int(
                        _row_get_value(row, "total_claims", 4, 0), default=0
                    ),
                    "total_provision": round(total_provision_cents / 100.0, 2),
                    "created_at": _row_get_value(row, "created_at", 3, None),
                    "last_claim_at": _row_get_value(row, "last_claim_at", 6, None),
                    "ust_status": str(
                        _row_get_value(row, "ust_status", 7, "unknown") or "unknown"
                    ).strip()
                    or "unknown",
                    "has_pii": bool(_safe_int(_row_get_value(row, "has_pii", 8, 0), default=0)),
                }
            )
        return web.json_response({"affiliates": affiliates})

    async def _api_admin_affiliate_gutschriften(self, request: web.Request) -> web.Response:
        """List all affiliate gutschriften for admins."""
        auth_error = self._admin_auth_error(request, getattr(self, "_require_v2_admin_api", None))
        if auth_error is not None:
            return auth_error

        try:
            with storage.transaction() as conn:
                self._admin_affiliate_prepare_conn(conn)
                try:
                    rows = conn.execute(
                        """
                        SELECT
                            g.id,
                            g.affiliate_twitch_login,
                            g.period_year,
                            g.period_month,
                            g.gutschrift_number,
                            g.net_amount_cents,
                            g.vat_amount_cents,
                            g.gross_amount_cents,
                            g.commission_ids,
                            g.affiliate_ust_status,
                            g.email_error,
                            g.pdf_generated_at,
                            g.email_sent_at,
                            g.created_at,
                            CASE WHEN g.pdf_blob IS NOT NULL THEN 1 ELSE NULL END AS pdf_blob,
                            a.display_name,
                            a.is_active,
                            COALESCE(pii.ust_status, 'unknown') AS ust_status,
                            CASE WHEN pii.twitch_login IS NOT NULL THEN 1 ELSE 0 END AS has_pii
                        FROM affiliate_gutschriften g
                        JOIN affiliate_accounts a
                          ON a.twitch_login = g.affiliate_twitch_login
                        LEFT JOIN affiliate_pii pii
                          ON pii.twitch_login = g.affiliate_twitch_login
                        ORDER BY g.period_year DESC, g.period_month DESC, g.id DESC
                        """
                    ).fetchall()
                except Exception as exc:
                    if not self._admin_is_missing_schema_error(exc):
                        raise
                    rows = conn.execute(
                        """
                        SELECT
                            g.id,
                            g.affiliate_twitch_login,
                            g.period_year,
                            g.period_month,
                            g.gutschrift_number,
                            g.net_amount_cents,
                            g.vat_amount_cents,
                            g.gross_amount_cents,
                            g.commission_ids,
                            g.affiliate_ust_status,
                            g.email_error,
                            g.pdf_generated_at,
                            g.email_sent_at,
                            g.created_at,
                            CASE WHEN g.pdf_blob IS NOT NULL THEN 1 ELSE NULL END AS pdf_blob,
                            a.display_name,
                            a.is_active,
                            'unknown' AS ust_status,
                            0 AS has_pii
                        FROM affiliate_gutschriften g
                        JOIN affiliate_accounts a
                          ON a.twitch_login = g.affiliate_twitch_login
                        ORDER BY g.period_year DESC, g.period_month DESC, g.id DESC
                        """
                    ).fetchall()
        except Exception as exc:
            if self._admin_is_missing_schema_error(exc):
                return web.json_response({"gutschriften": [], "count": 0})
            return _admin_500(exc)

        documents = []
        for row in rows:
            payload = dict(AffiliateGutschriftService._row_to_metadata(row))
            row_id = _safe_int(payload.get("id"), default=0)
            payload["download_path"] = self._admin_affiliate_gutschrift_download_path(row_id)
            payload["affiliate_login"] = str(
                _row_get_value(row, "affiliate_twitch_login", 1, "") or ""
            ).strip()
            payload["display_name"] = _row_get_value(row, "display_name", 15, None)
            payload["active"] = bool(_safe_int(_row_get_value(row, "is_active", 16, 1), default=1))
            payload["ust_status"] = str(
                _row_get_value(row, "ust_status", 17, "unknown") or "unknown"
            ).strip() or "unknown"
            payload["has_pii"] = bool(_safe_int(_row_get_value(row, "has_pii", 18, 0), default=0))
            documents.append(payload)
        return web.json_response({"gutschriften": documents, "count": len(documents)})

    async def _api_admin_affiliate_gutschriften_for_login(
        self,
        request: web.Request,
    ) -> web.Response:
        """List all gutschriften for one affiliate."""
        auth_error = self._admin_auth_error(request, getattr(self, "_require_v2_admin_api", None))
        if auth_error is not None:
            return auth_error

        login = _normalize_login(request.match_info.get("login", ""))
        if not login:
            return web.json_response({"error": "invalid_login"}, status=400)

        try:
            with storage.transaction() as conn:
                self._admin_affiliate_prepare_conn(conn)
                account_row = conn.execute(
                    """
                    SELECT twitch_login, display_name, is_active, created_at, updated_at
                    FROM affiliate_accounts
                    WHERE twitch_login = %s
                    """,
                    (login,),
                ).fetchone()
                if not account_row:
                    return web.json_response({"error": "not_found"}, status=404)

                pii = self._admin_affiliate_load_pii(conn, login)
                readiness = AffiliateGutschriftService.build_readiness(pii)
                summary = self._admin_affiliate_gutschriften_summary(
                    conn,
                    affiliate_login=login,
                )
                try:
                    documents = AffiliateGutschriftService.list_for_affiliate(conn, login)
                except Exception as exc:
                    if not self._admin_is_missing_schema_error(exc):
                        raise
                    documents = []
        except Exception as exc:
            if self._admin_is_missing_schema_error(exc):
                return web.json_response({"error": "not_found"}, status=404)
            return _admin_500(exc)

        items = []
        for document in documents:
            payload = dict(document)
            row_id = _safe_int(payload.get("id"), default=0)
            payload["download_path"] = self._admin_affiliate_gutschrift_download_path(row_id)
            items.append(payload)

        affiliate = {
            "login": str(_row_get_value(account_row, "twitch_login", 0, "") or "").strip(),
            "display_name": _row_get_value(account_row, "display_name", 1, None),
            "active": bool(_safe_int(_row_get_value(account_row, "is_active", 2, 1), default=1)),
            "created_at": _row_get_value(account_row, "created_at", 3, None),
            "updated_at": _row_get_value(account_row, "updated_at", 4, None),
        }
        return web.json_response(
            {
                "affiliate": affiliate,
                "ust_status": str(pii.get("ust_status") or "unknown"),
                "readiness": readiness,
                "gutschriften_summary": summary,
                "gutschriften": items,
            }
        )

    async def _api_admin_affiliate_gutschrift_pdf(self, request: web.Request) -> web.Response:
        """Download a stored affiliate gutschrift PDF without ownership checks."""
        auth_error = self._admin_auth_error(request, getattr(self, "_require_v2_admin_api", None))
        if auth_error is not None:
            return auth_error

        try:
            gutschrift_id = int(request.match_info.get("gutschrift_id", "0"))
        except (TypeError, ValueError):
            return web.json_response({"error": "invalid_gutschrift_id"}, status=400)
        if gutschrift_id <= 0:
            return web.json_response({"error": "invalid_gutschrift_id"}, status=400)

        try:
            with storage.transaction() as conn:
                self._admin_affiliate_prepare_conn(conn)
                row = conn.execute(
                    """
                    SELECT affiliate_twitch_login
                    FROM affiliate_gutschriften
                    WHERE id = %s
                    """,
                    (gutschrift_id,),
                ).fetchone()
                if not row:
                    return web.json_response({"error": "not_found"}, status=404)

                affiliate_login = _normalize_login(
                    _row_get_value(row, "affiliate_twitch_login", 0, "")
                )
                if not affiliate_login:
                    return web.json_response({"error": "not_found"}, status=404)

                resolved = AffiliateGutschriftService.get_pdf(
                    conn,
                    affiliate_login=affiliate_login,
                    gutschrift_id=gutschrift_id,
                )
        except Exception as exc:
            if self._admin_is_missing_schema_error(exc):
                return web.json_response({"error": "not_found"}, status=404)
            return _admin_500(exc)

        if resolved is None:
            return web.json_response({"error": "not_found"}, status=404)

        metadata, pdf_bytes = resolved
        filename = str(
            metadata.get("gutschrift_number") or f"gutschrift-{gutschrift_id}"
        ).replace('"', "")
        return web.Response(
            body=pdf_bytes,
            content_type="application/pdf",
            headers={
                "Content-Disposition": f'attachment; filename="{filename}.pdf"',
            },
        )

    async def _api_admin_affiliate_generate_gutschriften(
        self,
        request: web.Request,
    ) -> web.Response:
        """Trigger affiliate gutschrift generation as admin."""
        auth_error = self._admin_auth_error(request, getattr(self, "_require_v2_admin_api", None))
        if auth_error is not None:
            return auth_error

        csrf_token, payload = await self._admin_extract_csrf(request)
        if not self._admin_verify_csrf(request, csrf_token):
            return web.json_response({"error": "invalid_csrf"}, status=403)

        affiliate_login = _normalize_login(
            payload.get("affiliate_login")
            or payload.get("twitch_login")
            or payload.get("login")
            or ""
        )
        raw_login = str(
            payload.get("affiliate_login")
            or payload.get("twitch_login")
            or payload.get("login")
            or ""
        ).strip()
        if raw_login and not affiliate_login:
            return web.json_response({"error": "invalid_login"}, status=400)

        force = str(payload.get("force") or "").strip().lower() in {"1", "true", "yes", "on"}
        year_raw = payload.get("year")
        month_raw = payload.get("month")
        year = None
        month = None
        if year_raw not in (None, "") or month_raw not in (None, ""):
            if year_raw in (None, "") or month_raw in (None, ""):
                return web.json_response({"error": "invalid_period"}, status=400)
            try:
                year = int(year_raw)
                month = int(month_raw)
            except (TypeError, ValueError):
                return web.json_response({"error": "invalid_period"}, status=400)
            if year < 2000 or month < 1 or month > 12:
                return web.json_response({"error": "invalid_period"}, status=400)

        runner = getattr(self, "_affiliate_run_gutschrift_job", None)
        if not callable(runner):
            return web.json_response({"error": "gutschrift_job_unavailable"}, status=500)

        try:
            result = await asyncio.to_thread(
                runner,
                affiliate_login=affiliate_login or None,
                year=year,
                month=month,
                force=force,
            )
        except Exception as exc:
            return _admin_500(exc)
        return web.json_response(
            {
                "ok": True,
                "results": list(result.get("results") or []),
            }
        )

    async def _api_admin_affiliate_detail(self, request: web.Request) -> web.Response:
        """Get detailed info for a specific affiliate."""
        auth_error = self._admin_auth_error(request, getattr(self, "_require_v2_admin_api", None))
        if auth_error is not None:
            return auth_error

        login = _normalize_login(request.match_info.get("login", ""))
        if not login:
            return web.json_response({"error": "invalid_login"}, status=400)

        try:
            with storage.transaction() as conn:
                self._admin_affiliate_prepare_conn(conn)
                # Fetch affiliate account
                acct_row = conn.execute(
                    """
                    SELECT
                        twitch_login, display_name, is_active, created_at,
                        email, stripe_connect_status, stripe_account_id, updated_at
                    FROM affiliate_accounts
                    WHERE twitch_login = %s
                    """,
                    (login,),
                ).fetchone()

                if not acct_row:
                    return web.json_response({"error": "not_found"}, status=404)

                # Fetch claims with commission aggregates
                claim_rows = conn.execute(
                    f"""
                    SELECT
                        c.id,
                        c.claimed_streamer_login,
                        c.claimed_at,
                        COALESCE(SUM(co.commission_cents), 0) AS commission_cents,
                        COUNT(co.id) AS commission_count
                    FROM affiliate_streamer_claims c
                    LEFT JOIN affiliate_commissions co
                        ON co.affiliate_twitch_login = c.affiliate_twitch_login
                        AND co.streamer_login = c.claimed_streamer_login
                        AND co.status IN ({_AFFILIATE_REVENUE_STATUS_PLACEHOLDERS})
                    WHERE c.affiliate_twitch_login = %s
                    GROUP BY c.id, c.claimed_streamer_login, c.claimed_at
                    ORDER BY c.claimed_at DESC
                    """,
                    (*_AFFILIATE_REVENUE_STATUSES, login),
                ).fetchall()

                claim_stats_row = conn.execute(
                    """
                    SELECT
                        COUNT(*) AS total_claims
                    FROM affiliate_streamer_claims
                    WHERE affiliate_twitch_login = %s
                    """,
                    (login,),
                ).fetchone()

                commission_stats_row = conn.execute(
                    f"""
                    SELECT
                        COALESCE(SUM(commission_cents), 0) AS total_provision,
                        COUNT(DISTINCT streamer_login) AS active_customers
                    FROM affiliate_commissions
                    WHERE affiliate_twitch_login = %s
                      AND status IN ({_AFFILIATE_REVENUE_STATUS_PLACEHOLDERS})
                    """,
                    (login, *_AFFILIATE_REVENUE_STATUSES),
                ).fetchone()

                pii = self._admin_affiliate_load_pii(conn, login)
                readiness = AffiliateGutschriftService.build_readiness(pii)
                gutschriften_summary = self._admin_affiliate_gutschriften_summary(
                    conn,
                    affiliate_login=login,
                )
        except Exception as exc:
            if self._admin_is_missing_schema_error(exc):
                return web.json_response({"error": "not_found"}, status=404)
            return _admin_500(exc)

        stripe_id = str(_row_get_value(acct_row, "stripe_account_id", 6, "") or "")
        masked_stripe = (
            f"{stripe_id[:8]}...{stripe_id[-4:]}" if len(stripe_id) > 12 else stripe_id
        )

        affiliate = {
            "login": str(_row_get_value(acct_row, "twitch_login", 0, "") or "").strip(),
            "display_name": _row_get_value(acct_row, "display_name", 1, None),
            "active": bool(
                _safe_int(_row_get_value(acct_row, "is_active", 2, 1), default=1)
            ),
            "created_at": _row_get_value(acct_row, "created_at", 3, None),
            "email": _row_get_value(acct_row, "email", 4, None),
            "stripe_connect_status": _row_get_value(acct_row, "stripe_connect_status", 5, None),
            "stripe_account_id": masked_stripe or None,
            "updated_at": _row_get_value(acct_row, "updated_at", 7, None),
        }

        claims = [
            {
                "id": _safe_int(_row_get_value(r, "id", 0, 0), default=0),
                "customer_login": str(
                    _row_get_value(r, "claimed_streamer_login", 1, "") or ""
                ).strip(),
                "claimed_at": _row_get_value(r, "claimed_at", 2, None),
                "commission_cents": _safe_int(
                    _row_get_value(r, "commission_cents", 3, 0), default=0
                ),
                "commission_count": _safe_int(
                    _row_get_value(r, "commission_count", 4, 0), default=0
                ),
            }
            for r in claim_rows
        ]

        total_claims = _safe_int(
            _row_get_value(claim_stats_row, "total_claims", 0, 0), default=0
        )
        total_provision_cents = _safe_int(
            _row_get_value(commission_stats_row, "total_provision", 0, 0), default=0
        )
        stats = {
            "total_claims": total_claims,
            "total_provision": round(total_provision_cents / 100.0, 2),
            "avg_provision": round((total_provision_cents / max(total_claims, 1)) / 100.0, 2)
            if total_claims > 0
            else 0.0,
            "active_customers": _safe_int(
                _row_get_value(commission_stats_row, "active_customers", 1, 0), default=0
            ),
        }

        return web.json_response({
            "affiliate": affiliate,
            "claims": claims,
            "stats": stats,
            "ust_status": str(pii.get("ust_status") or "unknown"),
            "pii_readiness": readiness,
            "gutschriften_summary": {
                "count": gutschriften_summary["total_gutschriften"],
                "total_gross_cents": gutschriften_summary["total_gutschrift_amount_cents"],
            },
        })

    async def _api_admin_affiliate_toggle(self, request: web.Request) -> web.Response:
        """Toggle affiliate active status."""
        auth_error = self._admin_auth_error(request, getattr(self, "_require_v2_admin_api", None))
        if auth_error is not None:
            return auth_error

        csrf_token, _payload = await self._admin_extract_csrf(request)
        if not self._admin_verify_csrf(request, csrf_token):
            return web.json_response({"error": "invalid_csrf"}, status=403)

        login = _normalize_login(request.match_info.get("login", ""))
        if not login:
            return web.json_response({"error": "invalid_login"}, status=400)

        try:
            revenue_status_placeholders = ", ".join(
                ["%s"] * len(_AFFILIATE_REVENUE_STATUSES)
            )
            with storage.transaction() as conn:
                row = conn.execute(
                    "SELECT is_active FROM affiliate_accounts WHERE twitch_login = %s",
                    (login,),
                ).fetchone()
                if not row:
                    return web.json_response({"error": "not_found"}, status=404)

                current = _safe_int(_row_get_value(row, "is_active", 0, 1), default=1)
                new_status = 0 if current else 1
                now = datetime.now(UTC).isoformat()

                conn.execute(
                    """
                    UPDATE affiliate_accounts
                    SET is_active = %s, updated_at = %s
                    WHERE twitch_login = %s
                    """,
                    (new_status, now, login),
                )
        except Exception as exc:
            normalized = str(exc).strip().lower()
            if any(m in normalized for m in ("does not exist", "no such table", "undefined table")):
                return web.json_response({"error": "not_found"}, status=404)
            return _admin_500(exc)

        return web.json_response({"login": login, "active": bool(new_status)})

    async def _api_admin_affiliate_stats(self, request: web.Request) -> web.Response:
        """Aggregated affiliate program stats."""
        auth_error = self._admin_auth_error(request, getattr(self, "_require_v2_admin_api", None))
        if auth_error is not None:
            return auth_error

        month_start_iso = (
            datetime.now(UTC)
            .replace(day=1, hour=0, minute=0, second=0, microsecond=0)
            .isoformat()
        )
        try:
            with storage.transaction() as conn:
                self._admin_affiliate_prepare_conn(conn)
                acct_row = conn.execute(
                    """
                    SELECT
                        COUNT(*)                                    AS total_affiliates,
                        COALESCE(
                            SUM(CASE WHEN is_active = 1 THEN 1 ELSE 0 END),
                            0
                        ) AS active_affiliates
                    FROM affiliate_accounts
                    """
                ).fetchone()

                claim_row = conn.execute(
                    """
                    SELECT
                        COUNT(*) AS total_claims,
                        COALESCE(SUM(CASE WHEN claimed_at >= %s THEN 1 ELSE 0 END), 0)
                            AS this_month_claims
                    FROM affiliate_streamer_claims
                    """,
                    (month_start_iso,),
                ).fetchone()

                comm_row = conn.execute(
                    f"""
                    SELECT
                        COALESCE(SUM(commission_cents), 0) AS total_provision,
                        COALESCE(
                            SUM(
                                CASE
                                    WHEN created_at >= %s
                                     AND status IN ({_AFFILIATE_REVENUE_STATUS_PLACEHOLDERS})
                                    THEN commission_cents
                                    ELSE 0
                                END
                            ),
                            0
                        ) AS this_month_provision
                    FROM affiliate_commissions
                    WHERE status IN ({_AFFILIATE_REVENUE_STATUS_PLACEHOLDERS})
                    """,
                    (month_start_iso, *_AFFILIATE_REVENUE_STATUSES, *_AFFILIATE_REVENUE_STATUSES),
                ).fetchone()
                gutschrift_summary = self._admin_affiliate_gutschriften_summary(conn)
        except Exception as exc:
            if self._admin_is_missing_schema_error(exc):
                return web.json_response({
                    "total_affiliates": 0,
                    "active_affiliates": 0,
                    "total_claims": 0,
                    "total_provision": 0.0,
                    "this_month_claims": 0,
                    "this_month_provision": 0.0,
                    "total_gutschriften": 0,
                    "total_gutschrift_amount": 0.0,
                    "pending_email_gutschriften": 0,
                })
            return _admin_500(exc)

        total_provision_cents = _safe_int(
            _row_get_value(comm_row, "total_provision", 0, 0), default=0
        )
        this_month_provision_cents = _safe_int(
            _row_get_value(comm_row, "this_month_provision", 1, 0), default=0
        )

        return web.json_response({
            "total_affiliates": _safe_int(
                _row_get_value(acct_row, "total_affiliates", 0, 0), default=0
            ),
            "active_affiliates": _safe_int(
                _row_get_value(acct_row, "active_affiliates", 1, 0), default=0
            ),
            "total_claims": _safe_int(
                _row_get_value(claim_row, "total_claims", 0, 0), default=0
            ),
            "total_provision": round(total_provision_cents / 100.0, 2),
            "this_month_claims": _safe_int(
                _row_get_value(claim_row, "this_month_claims", 1, 0), default=0
            ),
            "this_month_provision": round(this_month_provision_cents / 100.0, 2),
            "total_gutschriften": gutschrift_summary["total_gutschriften"],
            "total_gutschrift_amount": gutschrift_summary["total_gutschrift_amount"],
            "pending_email_gutschriften": gutschrift_summary["pending_email_gutschriften"],
        })


__all__ = ["_AnalyticsAdminMixin"]

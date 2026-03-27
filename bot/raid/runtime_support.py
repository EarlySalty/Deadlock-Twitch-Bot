from __future__ import annotations

import logging
from datetime import UTC, datetime
from typing import Any

from ..storage import insert_observability_event


log = logging.getLogger("TwitchStreams.RaidManager")


def build_analytics_followers_runtime_state(bot: Any) -> dict[str, object]:
    chat_bot = getattr(bot, "chat_bot", None)
    token_mgr = getattr(chat_bot, "_token_manager", None) if chat_bot is not None else None
    return {
        "chat_bot_available": bool(chat_bot),
        "bot_token_manager_available": bool(token_mgr),
        "raid_session_available": bool(bot.session),
    }


def log_analytics_followers_decision(
    bot: Any,
    *,
    flow_id: str,
    flow: str,
    login: str,
    target_id: str | None,
    decision: str,
    reason: str,
    request_attempted: object,
    request_result: str,
    http_status: int | None,
    scope_state: dict[str, object],
    runtime_state: dict[str, object],
    level: int = logging.INFO,
    insert_observability_event_fn=insert_observability_event,
    **extra_fields: object,
) -> None:
    payload = {
        "flow_id": str(flow_id or "").strip() or None,
        "flow": str(flow or "").strip().lower() or "followers",
        "login": str(login or "").strip().lower() or None,
        "target_id": str(target_id or "").strip() or None,
        "decision": str(decision or "").strip() or "unknown",
        "reason": str(reason or "").strip() or "unknown",
        "request_attempted": request_attempted,
        "request_result": str(request_result or "").strip() or "unknown",
        "http_status": int(http_status) if http_status is not None else None,
        "scope_state": scope_state,
        "runtime_state": runtime_state,
        **extra_fields,
    }
    bot._last_analytics_followers_diagnostic = payload
    log.log(level, "analytics_decision %s", bot._format_raid_observability_fields(**payload))
    insert_observability_event_fn(
        flow_type="analytics",
        flow_id=str(payload.get("flow_id") or ""),
        entity_login=str(payload.get("login") or ""),
        entity_id=str(payload.get("target_id") or ""),
        step="terminal_decision",
        decision=str(payload.get("decision") or "unknown"),
        details=payload,
    )


async def resolve_bot_oauth_context(bot: Any) -> tuple[str | None, str | None, set[str]]:
    """Resolve bot OAuth token + bot id + scopes (best-effort)."""
    token_mgr = None
    chat_bot = getattr(bot, "chat_bot", None)
    if chat_bot is not None:
        token_mgr = getattr(chat_bot, "_token_manager", None)
    if token_mgr is None:
        cog = getattr(bot, "_cog", None)
        token_mgr = getattr(cog, "_bot_token_manager", None) if cog is not None else None
    if token_mgr is None:
        return None, None, set()

    try:
        token, bot_id = await token_mgr.get_valid_token()
    except Exception:
        return None, None, set()

    token = str(token or "").strip()
    if token.lower().startswith("oauth:"):
        token = token[6:]
    resolved_bot_id = str(bot_id or getattr(token_mgr, "bot_id", "") or "").strip() or None
    scopes = {
        str(scope).strip().lower()
        for scope in (getattr(token_mgr, "scopes", None) or set())
        if str(scope).strip()
    }
    return token or None, resolved_bot_id, scopes


def warn_user_scope_fallback_once(
    bot: Any,
    *,
    area: str,
    subject: str,
) -> None:
    subject_key = str(subject or "").strip().lower() or "<unknown>"
    key = (str(area or "").strip().lower(), subject_key)
    if key in bot._user_scope_fallback_warned:
        return
    bot._user_scope_fallback_warned.add(key)
    log.warning(
        "RaidBot: nutze Legacy-Broadcaster-Token fuer %s (%s). "
        "Der Bot-Token sollte diesen Pfad uebernehmen.",
        area,
        subject or "<unknown>",
    )


def clear_user_scope_fallback_warning(
    bot: Any,
    *,
    area: str,
    subject: str,
) -> None:
    subject_key = str(subject or "").strip().lower() or "<unknown>"
    key = (str(area or "").strip().lower(), subject_key)
    bot._user_scope_fallback_warned.discard(key)


async def get_followers_total_result_with_legacy_fallback(
    api: Any,
    user_id: str,
    *,
    user_token: str | None = None,
) -> dict[str, object]:
    result_getter = getattr(api, "get_followers_total_result", None)
    if callable(result_getter):
        return await result_getter(user_id, user_token=user_token)
    legacy_total = await api.get_followers_total(user_id, user_token=user_token)
    return {
        "ok": legacy_total is not None,
        "data": legacy_total,
        "http_status": 200 if legacy_total is not None else None,
        "error_code": None if legacy_total is not None else "legacy_none_result",
        "request_attempted": True,
    }


def row_value(row: Any, key: str, index: int, default: Any = None) -> Any:
    if row is None:
        return default
    try:
        if hasattr(row, "keys"):
            return row[key]
        return row[index]
    except Exception:
        return default


def safe_int(value: object, default: int = 0) -> int:
    try:
        if value is None:
            return default
        return int(value)
    except (TypeError, ValueError):
        return default


def parse_datetime(value: object) -> datetime | None:
    if value is None:
        return None
    text = str(value).strip()
    if not text:
        return None
    try:
        parsed = datetime.fromisoformat(text.replace("Z", "+00:00"))
    except Exception:
        return None
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=UTC)
    return parsed.astimezone(UTC)


def create_twitch_api(bot: Any, *, session: Any = None) -> Any | None:
    session = session or bot.session
    if session is None:
        return None
    try:
        from ..api.twitch_api import TwitchAPI
    except Exception:
        return None
    return TwitchAPI(
        bot.auth_manager.client_id,
        bot.auth_manager.client_secret,
        session=session,
    )

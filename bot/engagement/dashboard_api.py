"""JSON-API-Endpoints für das React-Dashboard (/twitch/api/v2/engagement/*).

Mountet sich auf den bestehenden v2-Server (api_overview.py / AnalyticsApiV2-Mixin).
Auth: identisch zu anderen v2-Endpoints — `server._check_v2_auth(request)` +
`server._get_dashboard_auth_session(request)` für User-ID-Extraktion.

Permission-Model:
- Super-Mod (twitch_admin_roles): sieht und togglet ALLE Channels.
- Normaler User: sieht und togglet nur seinen eigenen Channel (twitch_login).
"""

from __future__ import annotations

import asyncio
import json
import logging
from datetime import datetime, timezone
from typing import Any

from aiohttp import web

from bot.engagement.admin import is_super_mod
from bot.storage.pg import query_all, query_one, transaction

log = logging.getLogger("TwitchStreams.Engagement.DashboardAPI")


# === DB helpers (sync, wrapped in to_thread) ===

def _sync_load_settings_all() -> list[tuple]:
    return query_all(
        """
        SELECT channel_login, enabled, steam_id, persona_override, tabu_topics,
               enabled_at, enabled_by, updated_at
        FROM twitch_engagement_settings
        ORDER BY channel_login
        """
    )


def _sync_load_settings_one(channel_login: str):
    return query_one(
        """
        SELECT channel_login, enabled, steam_id, persona_override, tabu_topics,
               enabled_at, enabled_by, updated_at
        FROM twitch_engagement_settings
        WHERE channel_login = %s
        """,
        [channel_login],
    )


def _sync_load_log(channel_login: str, limit: int) -> list[tuple]:
    return query_all(
        """
        SELECT decision, response_text, model, prompt_tokens, completion_tokens,
               cost_usd_estimate, latency_ms, ts
        FROM twitch_engagement_log
        WHERE channel_login = %s
        ORDER BY ts DESC
        LIMIT %s
        """,
        [channel_login, limit],
    )


def _sync_update_settings(
    *,
    channel_login: str,
    enabled: bool | None,
    steam_id: str | None,
    persona_override: str | None,
    tabu_topics: list[str] | None,
    actor_id: str | None,
) -> None:
    with transaction() as conn:
        existing = conn.execute(
            "SELECT 1 FROM twitch_engagement_settings WHERE channel_login = %s",
            [channel_login],
        ).fetchone()
        if existing is None:
            conn.execute(
                """
                INSERT INTO twitch_engagement_settings
                    (channel_login, enabled, steam_id, persona_override, tabu_topics,
                     enabled_at, enabled_by, updated_at)
                VALUES (%s, %s, %s, %s, %s,
                        CASE WHEN %s THEN NOW() ELSE NULL END, %s, NOW())
                """,
                [
                    channel_login,
                    bool(enabled) if enabled is not None else False,
                    steam_id,
                    persona_override,
                    tabu_topics or [],
                    bool(enabled) if enabled is not None else False,
                    actor_id,
                ],
            )
            return
        sets: list[str] = []
        args: list[Any] = []
        if enabled is not None:
            sets.append("enabled = %s")
            args.append(bool(enabled))
            if enabled:
                sets.append("enabled_at = NOW()")
                sets.append("enabled_by = COALESCE(%s, enabled_by)")
                args.append(actor_id)
        if steam_id is not None:
            sets.append("steam_id = %s")
            args.append((steam_id or "").strip() or None)
        if persona_override is not None:
            sets.append("persona_override = %s")
            args.append((persona_override or "").strip() or None)
        if tabu_topics is not None:
            sets.append("tabu_topics = %s")
            args.append(tabu_topics)
        if not sets:
            return
        sets.append("updated_at = NOW()")
        args.append(channel_login)
        conn.execute(
            f"UPDATE twitch_engagement_settings SET {', '.join(sets)} WHERE channel_login = %s",
            args,
        )


# === Helpers ===

def _extract_session_user(session: dict | None) -> tuple[str | None, str | None]:
    if not session:
        return None, None
    user_id: str | None = None
    for key in ("twitch_user_id", "user_id", "id"):
        v = session.get(key)
        if v:
            user_id = str(v)
            break
    login: str | None = None
    for key in ("twitch_login", "username", "login", "display_name"):
        v = session.get(key)
        if v:
            login = str(v).strip().lower() or None
            if login:
                break
    return user_id, login


def _iso(ts: datetime | None) -> str | None:
    if ts is None:
        return None
    if ts.tzinfo is None:
        ts = ts.replace(tzinfo=timezone.utc)
    return ts.isoformat()


def _serialize_settings(row) -> dict:
    (
        channel_login,
        enabled,
        steam_id,
        persona_override,
        tabu_topics,
        enabled_at,
        enabled_by,
        updated_at,
    ) = row
    return {
        "channelLogin": channel_login,
        "enabled": bool(enabled),
        "steamId": steam_id,
        "personaOverride": persona_override,
        "tabuTopics": list(tabu_topics or []),
        "enabledAt": _iso(enabled_at),
        "enabledBy": enabled_by,
        "updatedAt": _iso(updated_at),
    }


def _serialize_log(entry) -> dict:
    decision, response_text, model, ptok, ctok, cost, latency, ts = entry
    return {
        "decision": decision,
        "responseText": response_text,
        "model": model,
        "promptTokens": ptok,
        "completionTokens": ctok,
        "costUsdEstimate": float(cost) if cost is not None else None,
        "latencyMs": latency,
        "ts": _iso(ts),
    }


def _json(payload: dict, status: int = 200) -> web.Response:
    return web.Response(
        status=status,
        text=json.dumps(payload, ensure_ascii=False),
        content_type="application/json",
        charset="utf-8",
    )


def _err(status: int, message: str, **extra) -> web.Response:
    return _json({"error": message, **extra}, status=status)


# === Handlers ===

async def _resolve_actor(server, request):
    if not server._check_v2_auth(request):
        return None, None, False, _err(401, "Authentication required.")
    session = server._get_dashboard_auth_session(request) or {}
    actor_id, actor_login = _extract_session_user(session)
    # localhost und admin Auth-Levels gelten als super_mod (gleiche Semantik wie
    # in anderen v2-Endpoints, plus DB-Lookup für 'super_mod'-Rolle).
    auth_level = ""
    get_level = getattr(server, "_get_auth_level", None)
    if callable(get_level):
        try:
            auth_level = str(get_level(request) or "")
        except Exception:
            auth_level = ""
    admin = auth_level in ("localhost", "admin") or await is_super_mod(actor_id)
    return actor_id, actor_login, admin, None


async def _handle_get_settings(server, request: web.Request) -> web.Response:
    actor_id, actor_login, admin, err = await _resolve_actor(server, request)
    if err is not None:
        return err

    channel = (request.query.get("channel") or "").strip().lower() or None

    if channel:
        if not admin and channel != actor_login:
            return _err(403, "Du darfst nur deinen eigenen Channel sehen.")
        row = await asyncio.to_thread(_sync_load_settings_one, channel)
        settings_list = [_serialize_settings(row)] if row else []
    elif admin:
        rows = await asyncio.to_thread(_sync_load_settings_all)
        settings_list = [_serialize_settings(r) for r in rows]
    elif actor_login:
        row = await asyncio.to_thread(_sync_load_settings_one, actor_login)
        settings_list = [_serialize_settings(row)] if row else []
    else:
        settings_list = []

    return _json(
        {
            "settings": settings_list,
            "isSuperMod": admin,
            "actorLogin": actor_login,
        }
    )


async def _handle_post_toggle(server, request: web.Request) -> web.Response:
    actor_id, actor_login, admin, err = await _resolve_actor(server, request)
    if err is not None:
        return err

    try:
        payload = await request.json()
    except json.JSONDecodeError:
        return _err(400, "Invalid JSON body.")

    channel = str(payload.get("channelLogin") or "").strip().lower()
    enabled = payload.get("enabled")
    if not channel or not isinstance(enabled, bool):
        return _err(400, "channelLogin (str) und enabled (bool) erforderlich.")
    if not admin and channel != actor_login:
        return _err(403, "Du darfst nur deinen eigenen Channel toggeln.")

    try:
        await asyncio.to_thread(
            _sync_update_settings,
            channel_login=channel,
            enabled=enabled,
            steam_id=None,
            persona_override=None,
            tabu_topics=None,
            actor_id=actor_id,
        )
    except Exception:
        log.exception("engagement toggle failed for %s", channel)
        return _err(500, "Update fehlgeschlagen.")

    row = await asyncio.to_thread(_sync_load_settings_one, channel)
    return _json({"settings": _serialize_settings(row) if row else None})


async def _handle_post_update(server, request: web.Request) -> web.Response:
    actor_id, actor_login, admin, err = await _resolve_actor(server, request)
    if err is not None:
        return err

    try:
        payload = await request.json()
    except json.JSONDecodeError:
        return _err(400, "Invalid JSON body.")

    channel = str(payload.get("channelLogin") or "").strip().lower()
    if not channel:
        return _err(400, "channelLogin erforderlich.")
    if not admin and channel != actor_login:
        return _err(403, "Du darfst nur deinen eigenen Channel verändern.")

    # Semantik: ein Feld im payload → schreiben (leer/null → NULL/[] in DB).
    # Feld nicht im payload → nicht anfassen.
    update_kwargs: dict[str, Any] = {}

    if "steamId" in payload:
        raw = payload["steamId"]
        if raw is None:
            update_kwargs["steam_id"] = ""  # empty marker → "" or None → NULL
        elif isinstance(raw, str):
            update_kwargs["steam_id"] = raw.strip()
        else:
            return _err(400, "steamId muss string oder null sein.")

    if "personaOverride" in payload:
        raw = payload["personaOverride"]
        if raw is None:
            update_kwargs["persona_override"] = ""
        elif isinstance(raw, str):
            update_kwargs["persona_override"] = raw.strip()
        else:
            return _err(400, "personaOverride muss string oder null sein.")

    if "tabuTopics" in payload:
        raw = payload["tabuTopics"]
        if raw is None:
            update_kwargs["tabu_topics"] = []
        elif isinstance(raw, list):
            update_kwargs["tabu_topics"] = [
                str(t).strip() for t in raw if str(t).strip()
            ]
        else:
            return _err(400, "tabuTopics muss array oder null sein.")

    try:
        await asyncio.to_thread(
            _sync_update_settings,
            channel_login=channel,
            enabled=None,
            steam_id=update_kwargs.get("steam_id"),
            persona_override=update_kwargs.get("persona_override"),
            tabu_topics=update_kwargs.get("tabu_topics"),
            actor_id=actor_id,
        )
    except Exception:
        log.exception("engagement update failed for %s", channel)
        return _err(500, "Update fehlgeschlagen.")

    row = await asyncio.to_thread(_sync_load_settings_one, channel)
    return _json({"settings": _serialize_settings(row) if row else None})


async def _handle_get_log(server, request: web.Request) -> web.Response:
    actor_id, actor_login, admin, err = await _resolve_actor(server, request)
    if err is not None:
        return err

    channel = (request.query.get("channel") or "").strip().lower()
    if not channel:
        return _err(400, "channel query-param erforderlich.")
    if not admin and channel != actor_login:
        return _err(403, "Du darfst nur deinen eigenen Log sehen.")

    try:
        limit = int(request.query.get("limit") or "25")
    except ValueError:
        limit = 25
    limit = max(1, min(limit, 200))

    entries = await asyncio.to_thread(_sync_load_log, channel, limit)
    return _json(
        {
            "channelLogin": channel,
            "entries": [_serialize_log(e) for e in entries],
        }
    )


async def _handle_sender_auth_start(server, request: web.Request) -> web.Response:
    """Admin-only: erzeugt den Authorize-Link für den Engagement-Sende-Account."""
    _actor_id, _actor_login, admin, err = await _resolve_actor(server, request)
    if err is not None:
        return err
    if not admin:
        return _err(403, "Nur Admins dürfen den Sende-Account autorisieren.")
    try:
        from bot.engagement import sender_auth
        url = await asyncio.to_thread(sender_auth.build_authorize_url)
    except Exception as exc:
        log.exception("engagement sender-auth: Link-Erzeugung fehlgeschlagen")
        return _err(500, f"Link-Erzeugung fehlgeschlagen: {type(exc).__name__}")
    return _json(
        {
            "authorizeUrl": url,
            "senderLogin": sender_auth.SENDER_LOGIN,
            "hint": (
                "In einem separaten Browser/Inkognito als der Sende-Account einloggen, "
                "dann diesen Link öffnen und Authorize klicken."
            ),
        }
    )


async def _handle_sender_auth_callback(server, request: web.Request) -> web.Response:
    """Öffentlicher OAuth-Callback (Sicherheit über den State-Token)."""
    code = request.query.get("code") or ""
    state = request.query.get("state") or ""
    error = request.query.get("error") or ""

    def _page(title: str, body: str, status: int = 200) -> web.Response:
        html = (
            "<!doctype html><html><head><meta charset='utf-8'>"
            f"<title>{title}</title></head><body style='font-family:sans-serif;max-width:560px;margin:40px auto'>"
            f"<h2>{title}</h2><p>{body}</p></body></html>"
        )
        return web.Response(status=status, text=html, content_type="text/html", charset="utf-8")

    if error:
        return _page("Autorisierung abgebrochen", f"Twitch meldete: {error}", status=400)
    if not code or not state:
        return _page("Ungültige Anfrage", "Code oder State fehlt.", status=400)

    try:
        from bot.engagement import sender_auth
        result = await sender_auth.handle_callback(code, state)
    except Exception as exc:
        log.exception("engagement sender-auth callback fehlgeschlagen")
        return _page("Autorisierung fehlgeschlagen", f"{type(exc).__name__}: {exc}", status=400)

    return _page(
        "Sende-Account verbunden ✓",
        f"Der Engagement-Account <b>{result.get('login')}</b> ist jetzt autorisiert. "
        "Du kannst dieses Fenster schließen.",
    )


def register_engagement_v2_routes(router: web.UrlDispatcher, server: Any) -> None:
    """Mountet alle JSON-Endpoints auf den v2-Server (api_overview.py setup)."""

    async def _get_settings(request: web.Request) -> web.Response:
        return await _handle_get_settings(server, request)

    async def _post_toggle(request: web.Request) -> web.Response:
        return await _handle_post_toggle(server, request)

    async def _post_update(request: web.Request) -> web.Response:
        return await _handle_post_update(server, request)

    async def _get_log(request: web.Request) -> web.Response:
        return await _handle_get_log(server, request)

    async def _sender_auth_start(request: web.Request) -> web.Response:
        return await _handle_sender_auth_start(server, request)

    async def _sender_auth_callback(request: web.Request) -> web.Response:
        return await _handle_sender_auth_callback(server, request)

    router.add_get("/twitch/api/v2/engagement/settings", _get_settings)
    router.add_post("/twitch/api/v2/engagement/toggle", _post_toggle)
    router.add_post("/twitch/api/v2/engagement/update", _post_update)
    router.add_get("/twitch/api/v2/engagement/log", _get_log)
    # Engagement-Sende-Account onboarding (getrennt vom normalen Streamer-OAuth)
    router.add_get("/twitch/api/v2/engagement/sender-auth", _sender_auth_start)
    # Callback auf beiden Pfaden: der engagement-Namespace ist bereits durch Caddy
    # geroutet; /callback/engagement-sender wird zusätzlich akzeptiert, falls dieser
    # in der Twitch-App registriert ist (braucht dann eine Caddy-Pfad-Freigabe).
    router.add_get("/twitch/api/v2/engagement/sender-callback", _sender_auth_callback)
    router.add_get("/callback/engagement-sender", _sender_auth_callback)

"""Standalone dashboard module for configurable go-live announcements."""

from __future__ import annotations

import html
import json
import re
from datetime import UTC, datetime
from typing import Any

import discord
from aiohttp import web

from ... import storage as _storage
from ...core.constants import log
from ...live_announce.template import (
    LiveAnnouncementConfig,
    build_template_context,
    render_announcement_payload,
    validate_live_announcement_config,
)

_MAX_PREVIEW_CONFIG_CHARS = 50_000
_ROLE_NAME_SAFE_RE = re.compile(r"[^A-Za-z0-9 _-]+")
SUPPORTED_PLACEHOLDERS: tuple[str, ...] = (
    "channel",
    "url",
    "rolle",
    "title",
    "viewer_count",
    "started_at",
    "language",
    "tags",
    "uptime",
    "game",
)


def _default_live_announcement_config() -> dict[str, Any]:
    return {
        "content": "{rolle} **{channel}** ist live! Schau ueber den Button unten rein.",
        "mentions": {"enabled": True, "role_id": ""},
        "embed": {
            "color": "#9146ff",
            "author": {
                "enabled": True,
                "name": "LIVE: {channel}",
                "icon_mode": "twitch_logo",
                "link_to_channel": True,
            },
            "title": "{channel} ist LIVE in {game}!",
            "title_link_enabled": True,
            "description_mode": "stream_title",
            "description": "{title}",
            "shorten": False,
            "fields": [
                {"name": "Viewer", "value": "{viewer_count}", "inline": True},
                {"name": "Kategorie", "value": "{game}", "inline": True},
            ],
            "thumbnail": {"mode": "none", "custom_url": ""},
            "image": {
                "use_stream_thumbnail": True,
                "custom_url": "",
                "format": "16:9",
                "cache_buster": True,
            },
            "footer": {
                "text": "Auf Twitch ansehen fuer mehr Action!",
                "icon_mode": "none",
                "timestamp_mode": "started_at",
            },
        },
        "button": {"enabled": True, "label": "Auf Twitch ansehen", "url_template": "{url}"},
        "allowed_editor_role_ids": [],
    }


def _deep_merge(dst: dict[str, Any], src: dict[str, Any]) -> dict[str, Any]:
    out = json.loads(json.dumps(dst))
    for key, value in src.items():
        if isinstance(value, dict) and isinstance(out.get(key), dict):
            out[key] = _deep_merge(out.get(key) or {}, value)
        else:
            out[key] = value
    return out


def _parse_config_json(raw: str | None) -> dict[str, Any]:
    text = str(raw or "").strip()
    if not text:
        return _default_live_announcement_config()
    try:
        parsed = json.loads(text)
    except Exception:
        return _default_live_announcement_config()
    if not isinstance(parsed, dict):
        return _default_live_announcement_config()
    return _deep_merge(_default_live_announcement_config(), parsed)


def _to_template_config(cfg: dict[str, Any]) -> dict[str, Any]:
    if not isinstance(cfg, dict):
        return {}
    if "embed" not in cfg:
        return cfg

    embed = cfg.get("embed") if isinstance(cfg.get("embed"), dict) else {}
    author = embed.get("author") if isinstance(embed.get("author"), dict) else {}
    footer = embed.get("footer") if isinstance(embed.get("footer"), dict) else {}
    fields = embed.get("fields") if isinstance(embed.get("fields"), list) else []
    image = embed.get("image") if isinstance(embed.get("image"), dict) else {}
    thumbnail = embed.get("thumbnail") if isinstance(embed.get("thumbnail"), dict) else {}
    mentions = cfg.get("mentions") if isinstance(cfg.get("mentions"), dict) else {}
    button = cfg.get("button") if isinstance(cfg.get("button"), dict) else {}
    mention_role_id = str(mentions.get("role_id") or "").strip()
    static_ping_role_ids = [int(mention_role_id)] if mention_role_id.isdigit() else []

    normalized_fields: list[dict[str, Any]] = []
    for field in fields:
        if not isinstance(field, dict):
            continue
        normalized_fields.append(
            {
                "name_template": str(field.get("name") or ""),
                "value_template": str(field.get("value") or ""),
                "inline": bool(field.get("inline", True)),
            }
        )

    image_mode = "stream_thumbnail"
    if not bool(image.get("use_stream_thumbnail", True)):
        image_mode = "custom" if str(image.get("custom_url") or "").strip() else "none"

    return {
        "content_template": str(cfg.get("content") or "").replace("{rolle}", "{mention_role}"),
        "color": embed.get("color", "#9146ff"),
        "author": {
            "name_template": str(author.get("name") or "LIVE: {channel}"),
            "icon_mode": (
                "twitch"
                if str(author.get("icon_mode") or "").strip().lower() in {"twitch_logo", "twitch"}
                else str(author.get("icon_mode") or "none").strip().lower()
            ),
            "link_to_stream": bool(author.get("link_to_channel", True)),
        },
        "title_template": str(embed.get("title") or "{channel} ist LIVE in {game}!"),
        "title_link_to_stream": bool(embed.get("title_link_enabled", True)),
        "description_mode": str(embed.get("description_mode") or "stream_title"),
        "description_template": str(embed.get("description") or "{title}"),
        "short_description": bool(embed.get("shorten", False)),
        "fields": normalized_fields,
        "images": {
            "thumbnail_mode": (
                "custom"
                if str(thumbnail.get("mode") or "").strip().lower() == "custom_url"
                else str(thumbnail.get("mode") or "none").strip().lower()
            ),
            "thumbnail_url_template": str(thumbnail.get("custom_url") or ""),
            "image_mode": image_mode,
            "image_url_template": str(image.get("custom_url") or ""),
            "image_ratio": str(image.get("format") or "16:9"),
            "cache_buster": bool(image.get("cache_buster", True)),
        },
        "footer": {
            "text_template": str(footer.get("text") or ""),
            "icon_mode": (
                "twitch"
                if str(footer.get("icon_mode") or "").strip().lower() in {"twitch_logo", "twitch"}
                else "none"
            ),
            "timestamp_mode": str(footer.get("timestamp_mode") or "started_at"),
        },
        "button": {
            "label_template": str(button.get("label") or "Auf Twitch ansehen"),
            "url_template": "{url}",
            "force_stream_url": True,
        },
        "mentions": {
            "use_streamer_ping_role": bool(mentions.get("enabled", True)),
            "streamer_ping_role_name_template": "{channel} LIVE PING",
            "allowed_editor_role_ids": [
                int(role_id)
                for role_id in (cfg.get("allowed_editor_role_ids") or [])
                if str(role_id).isdigit()
            ],
            "static_ping_role_ids": static_ping_role_ids,
            "allow_everyone": False,
        },
    }


def _validate_config_dict(cfg: dict[str, Any]) -> list[dict[str, str]]:
    try:
        config_obj = LiveAnnouncementConfig.from_dict(_to_template_config(cfg))
    except Exception:
        return [{"path": "config", "message": "Config konnte nicht verarbeitet werden."}]
    try:
        raw_errors = list(validate_live_announcement_config(config_obj))
    except Exception:
        return [{"path": "config", "message": "Config konnte nicht validiert werden."}]
    return [{"path": "config", "message": str(err)} for err in raw_errors]


class DashboardLiveAnnouncementMixin:
    """Routes + UI for the configurable Go-Live builder."""

    async def live_announcement_page(self, request: web.Request) -> web.StreamResponse:
        self._la_require_auth(request)
        auth_level = self._la_auth_level(request)
        is_admin = auth_level in {"admin", "localhost"}
        session = self._la_session(request)
        session_login = str((session or {}).get("twitch_login") or "").strip().lower()
        streamer = await self._la_resolve_streamer(
            request=request,
            session_login=session_login,
            is_admin=is_admin,
        )
        if not streamer:
            return web.Response(text="Kein Streamer verfuegbar.", status=404)

        row = self._la_load(streamer)
        role_state = await self._la_ensure_streamer_ping_role(
            streamer_login=streamer,
            create_if_missing=False,
        )
        streamer_ping_role_id = self._la_coerce_role_id(role_state.get("role_id"))
        preview = self._la_preview_payload(
            streamer,
            row["config"],
            streamer_ping_role_id=streamer_ping_role_id,
        )
        csrf = self._la_csrf_generate(request)
        streamers = self._la_list_streamers(session_login=session_login, is_admin=is_admin)
        page = self._la_render_page(
            streamer_login=streamer,
            streamers=streamers,
            csrf_token=csrf,
            auth_level=auth_level,
            config=row["config"],
            allowed_editor_role_ids=row["allowed_editor_role_ids"],
            preview=preview,
            streamer_ping_role_id=streamer_ping_role_id,
            streamer_ping_role_name=str(role_state.get("role_name") or "") or None,
            role_status_message=str(role_state.get("message") or ""),
        )
        return web.Response(text=page, content_type="text/html")  # lgtm[py/reflective-xss]

    async def api_live_announcement_config(self, request: web.Request) -> web.StreamResponse:
        self._la_require_auth(request)
        auth_level = self._la_auth_level(request)
        is_admin = auth_level in {"admin", "localhost"}
        session = self._la_session(request)
        session_login = str((session or {}).get("twitch_login") or "").strip().lower()
        streamer = await self._la_resolve_streamer(
            request=request,
            session_login=session_login,
            is_admin=is_admin,
        )
        if not streamer:
            return web.json_response({"error": "streamer_not_found"}, status=404)

        row = self._la_load(streamer)
        role_state = await self._la_ensure_streamer_ping_role(
            streamer_login=streamer,
            create_if_missing=False,
        )
        streamer_ping_role_id = self._la_coerce_role_id(role_state.get("role_id"))
        return web.json_response(
            {
                "streamer_login": streamer,
                "auth_level": auth_level,
                "is_admin": is_admin,
                "config": row["config"],
                "allowed_editor_role_ids": row["allowed_editor_role_ids"],
                "streamer_ping_role_id": streamer_ping_role_id,
                "streamer_ping_role_name": str(role_state.get("role_name") or ""),
                "role_status_message": str(role_state.get("message") or ""),
                "validation": _validate_config_dict(row["config"]),
            }
        )

    async def api_live_announcement_save_config(self, request: web.Request) -> web.StreamResponse:
        self._la_require_auth(request)
        csrf = await self._la_csrf_extract(request)
        if not self._la_csrf_verify(request, csrf):
            return web.json_response({"error": "invalid_csrf"}, status=403)

        auth_level = self._la_auth_level(request)
        is_admin = auth_level in {"admin", "localhost"}
        session = self._la_session(request)
        session_login = str((session or {}).get("twitch_login") or "").strip().lower()

        body = await self._la_json_body(request)
        streamer = await self._la_resolve_streamer(
            request=request,
            session_login=session_login,
            is_admin=is_admin,
            requested_streamer=str(body.get("streamer_login") or "").strip(),
        )
        if not streamer:
            return web.json_response({"error": "streamer_not_found"}, status=404)
        if not is_admin and session_login and streamer != session_login:
            return web.json_response(
                {
                    "error": "forbidden_streamer_scope",
                    "message": "Partner duerfen nur den eigenen Streamer konfigurieren.",
                },
                status=403,
            )

        raw_cfg = body.get("config")
        if not isinstance(raw_cfg, dict):
            return web.json_response({"error": "invalid_config_payload"}, status=400)
        cfg = _deep_merge(_default_live_announcement_config(), raw_cfg)
        cfg["content"] = self._la_sanitize_disallowed_mentions_text(cfg.get("content"))
        issues = _validate_config_dict(cfg)
        if issues:
            return web.json_response({"error": "validation_failed", "validation": issues}, status=400)

        current_row = self._la_load(streamer)
        existing_role_ids = self._la_parse_role_ids(current_row.get("allowed_editor_role_ids"))
        can_edit, reason = await self._la_can_edit(
            request=request,
            streamer_login=streamer,
            session_login=session_login,
            is_admin=is_admin,
            allowed_editor_role_ids=existing_role_ids,
        )
        if not can_edit:
            return web.json_response({"error": "editor_role_required", "message": reason}, status=403)

        role_ids = self._la_parse_role_ids(body.get("allowed_editor_role_ids"))
        actor = self._la_actor_label(request)
        self._la_save(streamer, cfg, role_ids, actor)
        role_state = await self._la_ensure_streamer_ping_role(
            streamer_login=streamer,
            create_if_missing=True,
        )
        streamer_ping_role_id = self._la_coerce_role_id(role_state.get("role_id"))
        preview = self._la_preview_payload(
            streamer,
            cfg,
            streamer_ping_role_id=streamer_ping_role_id,
        )
        return web.json_response(
            {
                "ok": True,
                "streamer_login": streamer,
                "updated_by": actor,
                "preview": preview,
                "streamer_ping_role_id": streamer_ping_role_id,
                "streamer_ping_role_name": str(role_state.get("role_name") or ""),
                "role_status_message": str(role_state.get("message") or ""),
                "validation": [],
            }
        )

    async def api_live_announcement_test_send(self, request: web.Request) -> web.StreamResponse:
        self._la_require_auth(request)
        csrf = await self._la_csrf_extract(request)
        if not self._la_csrf_verify(request, csrf):
            return web.json_response({"error": "invalid_csrf"}, status=403)

        auth_level = self._la_auth_level(request)
        is_admin = auth_level in {"admin", "localhost"}
        session = self._la_session(request)
        session_login = str((session or {}).get("twitch_login") or "").strip().lower()

        body = await self._la_json_body(request)
        streamer = await self._la_resolve_streamer(
            request=request,
            session_login=session_login,
            is_admin=is_admin,
            requested_streamer=str(body.get("streamer_login") or "").strip(),
        )
        if not streamer:
            return web.json_response({"error": "streamer_not_found"}, status=404)

        cfg = _deep_merge(
            _default_live_announcement_config(),
            body.get("config") if isinstance(body.get("config"), dict) else self._la_load(streamer)["config"],
        )
        role_state = await self._la_ensure_streamer_ping_role(
            streamer_login=streamer,
            create_if_missing=False,
        )
        streamer_ping_role_id = self._la_coerce_role_id(role_state.get("role_id"))
        preview = self._la_preview_payload(
            streamer,
            cfg,
            streamer_ping_role_id=streamer_ping_role_id,
        )

        dm_user_id = self._la_dm_target_user_id(request, streamer_login=streamer)
        if not dm_user_id:
            return web.json_response(
                {
                    "error": "dm_target_not_found",
                    "message": "Kein Discord-User fuer den Testversand gefunden.",
                    "preview": preview,
                    "streamer_ping_role_id": streamer_ping_role_id,
                    "streamer_ping_role_name": str(role_state.get("role_name") or ""),
                },
                status=404,
            )

        sent, message = await self._la_send_test_dm(dm_user_id, streamer, preview)
        return web.json_response(
            {
                "ok": sent,
                "message": message,
                "preview": preview,
                "streamer_ping_role_id": streamer_ping_role_id,
                "streamer_ping_role_name": str(role_state.get("role_name") or ""),
            },
            status=(200 if sent else 503),
        )

    async def api_live_announcement_preview(self, request: web.Request) -> web.StreamResponse:
        self._la_require_auth(request)
        auth_level = self._la_auth_level(request)
        is_admin = auth_level in {"admin", "localhost"}
        session = self._la_session(request)
        session_login = str((session or {}).get("twitch_login") or "").strip().lower()
        streamer = await self._la_resolve_streamer(
            request=request,
            session_login=session_login,
            is_admin=is_admin,
        )
        if not streamer:
            return web.json_response({"error": "streamer_not_found"}, status=404)

        cfg = self._la_load(streamer)["config"]
        raw_cfg = str(request.query.get("config") or "").strip()
        if raw_cfg:
            if len(raw_cfg) > _MAX_PREVIEW_CONFIG_CHARS:
                return web.json_response({"error": "config_too_large"}, status=413)
            try:
                parsed = json.loads(raw_cfg)
            except Exception:
                return web.json_response({"error": "invalid_config_json"}, status=400)
            if isinstance(parsed, dict):
                cfg = _deep_merge(_default_live_announcement_config(), parsed)

        streamer_entry = self._la_load_streamer_entry(streamer)
        streamer_ping_role_id = self._la_coerce_role_id(streamer_entry.get("live_ping_role_id"))
        preview = self._la_preview_payload(
            streamer,
            cfg,
            streamer_ping_role_id=streamer_ping_role_id,
        )
        issues = _validate_config_dict(cfg)
        return web.json_response(
            {
                "streamer_login": streamer,
                "streamer_ping_role_id": streamer_ping_role_id,
                "streamer_ping_role_name": "",
                "preview": preview,
                "validation": issues,
            }
        )

    def _la_require_auth(self, request: web.Request) -> None:
        require_token = getattr(self, "_require_token", None)
        if callable(require_token):
            require_token(request)
        else:
            checker = getattr(self, "_check_v2_auth", None)
            if not (callable(checker) and checker(request)):
                raise web.HTTPUnauthorized(text="missing or invalid authentication")
        # Disallow token-only API access; the builder must run in an authenticated dashboard session.
        # Keep localhost/noauth developer workflow usable.
        if not isinstance(self._la_session(request), dict):
            if self._la_auth_level(request) != "localhost":
                raise web.HTTPUnauthorized(text="dashboard session required")

    def _la_auth_level(self, request: web.Request) -> str:
        getter = getattr(self, "_get_auth_level", None)
        if callable(getter):
            try:
                return str(getter(request) or "none")
            except Exception:
                pass
        return "none"

    def _la_session(self, request: web.Request) -> dict[str, Any] | None:
        admin_getter = getattr(self, "_get_discord_admin_session", None)
        if callable(admin_getter):
            try:
                admin = admin_getter(request)
            except Exception:
                admin = None
            if isinstance(admin, dict):
                copied = dict(admin)
                copied.setdefault("auth_type", "discord_admin")
                return copied
        partner_getter = getattr(self, "_get_dashboard_auth_session", None)
        if callable(partner_getter):
            try:
                partner = partner_getter(request)
            except Exception:
                partner = None
            if isinstance(partner, dict):
                return partner
        return None

    async def _la_resolve_streamer(
        self,
        *,
        request: web.Request,
        session_login: str,
        is_admin: bool,
        requested_streamer: str = "",
    ) -> str:
        candidate = requested_streamer or str(request.query.get("streamer") or "").strip()
        normalized = ""
        normalizer = getattr(self, "_normalize_login", None)
        if callable(normalizer):
            try:
                normalized = str(normalizer(candidate) or "")
            except Exception:
                normalized = ""
        if not normalized:
            normalized = candidate.strip().lower()

        if not normalized:
            if not is_admin:
                return self._la_active_partner_login(session_login)
            streamers = self._la_list_streamers(session_login=session_login, is_admin=is_admin)
            return streamers[0] if streamers else ""

        if not is_admin:
            session_partner = self._la_active_partner_login(session_login)
            if not session_partner:
                return ""
            if normalized != session_partner:
                return session_partner
            return session_partner

        return self._la_active_partner_login(normalized)

    def _la_active_partner_login(self, login: str) -> str:
        normalized = str(login or "").strip().lower()
        if not normalized:
            return ""
        try:
            with _storage.readonly_connection() as conn:
                row = _storage.load_active_partner(conn, twitch_login=normalized)
        except Exception:
            safe_login = normalized.replace("\r", "\\r").replace("\n", "\\n")
            log.debug("Could not resolve active partner login for %s", safe_login, exc_info=True)
            return ""
        if not row:
            return ""
        if hasattr(row, "keys"):
            return str(row.get("twitch_login") or normalized).strip().lower()
        return str((row[2] if len(row) > 2 else normalized) or normalized).strip().lower()

    def _la_ensure_storage(self) -> None:
        if getattr(self, "_live_announcement_storage_ready", False):
            return
        with _storage.transaction() as conn:
            conn.execute(
                """
                CREATE TABLE IF NOT EXISTS twitch_live_announcement_configs (
                    streamer_login TEXT PRIMARY KEY,
                    config_json TEXT NOT NULL,
                    allowed_editor_role_ids TEXT,
                    updated_at TEXT NOT NULL,
                    updated_by TEXT
                )
                """
            )
        self._live_announcement_storage_ready = True

    def _la_list_streamers(self, *, session_login: str, is_admin: bool) -> list[str]:
        if not is_admin and session_login:
            active_login = self._la_active_partner_login(session_login)
            return [active_login] if active_login else []
        self._la_ensure_storage()
        try:
            with _storage.readonly_connection() as conn:
                rows = conn.execute(
                    "SELECT twitch_login FROM twitch_streamers_partner_state "
                    "WHERE COALESCE(is_partner_active, 0) = 1 ORDER BY twitch_login"
                ).fetchall()
            streamers = [str(row[0] or "").strip().lower() for row in rows if row and row[0]]
        except Exception:
            streamers = []
        return streamers

    def _la_load(self, streamer_login: str) -> dict[str, Any]:
        self._la_ensure_storage()
        login = str(streamer_login or "").strip().lower()
        fallback = {"config": _default_live_announcement_config(), "allowed_editor_role_ids": []}
        if not login:
            return fallback

        try:
            with _storage.readonly_connection() as conn:
                row = conn.execute(
                    """
                    SELECT config_json, allowed_editor_role_ids
                      FROM twitch_live_announcement_configs
                     WHERE LOWER(streamer_login) = LOWER(%s)
                     LIMIT 1
                    """,
                    (login,),
                ).fetchone()
        except Exception:
            safe_login = login.replace("\r", "\\r").replace("\n", "\\n")
            log.debug("Could not load live announcement config for %s", safe_login, exc_info=True)
            row = None

        if not row:
            return fallback

        cfg_raw = row[0] if not hasattr(row, "keys") else row["config_json"]
        role_raw = row[1] if not hasattr(row, "keys") else row["allowed_editor_role_ids"]
        return {
            "config": _parse_config_json(str(cfg_raw or "")),
            "allowed_editor_role_ids": self._la_parse_role_ids(role_raw),
        }

    def _la_save(
        self,
        streamer_login: str,
        cfg: dict[str, Any],
        allowed_editor_role_ids: list[int],
        actor: str,
    ) -> None:
        self._la_ensure_storage()
        now_iso = datetime.now(tz=UTC).isoformat(timespec="seconds")
        cfg_json = json.dumps(cfg, ensure_ascii=True, separators=(",", ":"))
        roles_json = json.dumps(allowed_editor_role_ids, ensure_ascii=True, separators=(",", ":"))
        with _storage.transaction() as conn:
            conn.execute(
                """
                INSERT INTO twitch_live_announcement_configs (
                    streamer_login, config_json, allowed_editor_role_ids, updated_at, updated_by
                ) VALUES (%s, %s, %s, %s, %s)
                ON CONFLICT (streamer_login) DO UPDATE SET
                    config_json = EXCLUDED.config_json,
                    allowed_editor_role_ids = EXCLUDED.allowed_editor_role_ids,
                    updated_at = EXCLUDED.updated_at,
                    updated_by = EXCLUDED.updated_by
                """,
                (streamer_login.lower(), cfg_json, roles_json, now_iso, actor),
            )

    @staticmethod
    def _la_parse_role_ids(raw: Any) -> list[int]:
        if isinstance(raw, list):
            payload = raw
        else:
            text = str(raw or "").strip()
            if not text:
                return []
            try:
                payload = json.loads(text)
            except Exception:
                return []
        if not isinstance(payload, list):
            return []

        out: list[int] = []
        seen: set[int] = set()
        for item in payload:
            text = str(item or "").strip()
            if not text.isdigit():
                continue
            role_id = int(text)
            if role_id <= 0 or role_id in seen:
                continue
            out.append(role_id)
            seen.add(role_id)
        return out

    @staticmethod
    def _la_coerce_role_id(value: Any) -> int | None:
        text = str(value or "").strip()
        if not text.isdigit():
            return None
        try:
            parsed = int(text)
        except Exception:
            return None
        return parsed if parsed > 0 else None

    @staticmethod
    def _la_is_live_ping_enabled(value: Any) -> bool:
        if isinstance(value, str):
            return value.strip().lower() not in {"0", "false", "no", "off"}
        return bool(value)

    @staticmethod
    def _la_sanitize_live_ping_role_name(login: str) -> str:
        cleaned = _ROLE_NAME_SAFE_RE.sub("", str(login or "").strip())
        cleaned = " ".join(cleaned.split())
        if not cleaned:
            cleaned = "STREAMER"
        return f"{cleaned.upper()} LIVE PING"[:100]

    @staticmethod
    def _la_sanitize_disallowed_mentions_text(value: Any) -> str:
        sanitized = str(value or "")
        sanitized = re.sub(r"@everyone", "@\u200beveryone", sanitized, flags=re.IGNORECASE)
        sanitized = re.sub(r"@here", "@\u200bhere", sanitized, flags=re.IGNORECASE)
        return sanitized

    def _la_load_streamer_entry(self, streamer_login: str) -> dict[str, Any]:
        login = str(streamer_login or "").strip().lower()
        fallback: dict[str, Any] = {
            "twitch_login": login,
            "discord_user_id": None,
            "live_ping_role_id": None,
            "live_ping_enabled": 1,
        }
        if not login:
            return fallback
        try:
            with _storage.readonly_connection() as conn:
                row = _storage.load_active_partner(conn, twitch_login=login)
        except Exception:
            safe_login = login.replace("\r", "\\r").replace("\n", "\\n")
            log.debug(
                "Could not load streamer entry for live ping role sync (%s)",
                safe_login,
                exc_info=True,
            )
            return fallback
        if not row:
            return fallback
        if hasattr(row, "keys"):
            return {
                "twitch_login": str(row.get("twitch_login") or login).strip().lower(),
                "discord_user_id": row.get("discord_user_id"),
                "live_ping_role_id": row.get("live_ping_role_id") if "live_ping_role_id" in row.keys() else None,
                "live_ping_enabled": row.get("live_ping_enabled", 1) if "live_ping_enabled" in row.keys() else 1,
            }
        # Fallback: positionale Zugriffe basierend auf der bekannten Query-Struktur
        # (twitch_login=Index 2, live_ping_role_id=Index 16, live_ping_enabled=Index 17,
        #  discord_user_id=Index 21)
        return {
            "twitch_login": str(row[2] or login).strip().lower(),
            "discord_user_id": row[21] if len(row) > 21 else None,
            "live_ping_role_id": row[16] if len(row) > 16 else None,
            "live_ping_enabled": row[17] if len(row) > 17 else 1,
        }

    def _la_persist_streamer_ping_role_id(self, streamer_login: str, role_id: int) -> None:
        login = str(streamer_login or "").strip().lower()
        if not login or role_id <= 0:
            return
        try:
            with _storage.transaction() as conn:
                _storage.set_partner_live_ping_settings(
                    conn,
                    twitch_login=login,
                    live_ping_role_id=role_id,
                )
        except Exception:
            safe_login = login.replace("\r", "\\r").replace("\n", "\\n")
            log.debug("Could not persist live_ping_role_id for %s", safe_login, exc_info=True)

    async def _la_ensure_streamer_ping_role(
        self,
        *,
        streamer_login: str,
        create_if_missing: bool,
    ) -> dict[str, Any]:
        login = str(streamer_login or "").strip().lower()
        if not login:
            return {"role_id": None, "role_name": None, "created": False, "message": "Kein Streamer gewaehlt."}

        entry = self._la_load_streamer_entry(login)
        existing_role_id = self._la_coerce_role_id(entry.get("live_ping_role_id"))
        if not self._la_is_live_ping_enabled(entry.get("live_ping_enabled", 1)):
            return {
                "role_id": existing_role_id,
                "role_name": None,
                "created": False,
                "message": "Live-Ping ist fuer diesen Streamer deaktiviert.",
            }

        bot = self._la_discord_bot()
        if bot is None:
            return {
                "role_id": existing_role_id,
                "role_name": None,
                "created": False,
                "message": "Discord-Bot nicht verbunden. Rolle wird beim naechsten Live-Event erstellt.",
            }

        guilds = list(getattr(bot, "guilds", []) or [])
        guild = guilds[0] if guilds else None
        if guild is None:
            return {
                "role_id": existing_role_id,
                "role_name": None,
                "created": False,
                "message": "Discord-Guild nicht verfuegbar. Rolle kann aktuell nicht synchronisiert werden.",
            }

        role = guild.get_role(existing_role_id) if existing_role_id else None
        role_name = self._la_sanitize_live_ping_role_name(login)
        if role is None:
            role = discord.utils.get(guild.roles, name=role_name)

        created = False
        if role is None and create_if_missing:
            try:
                role = await guild.create_role(
                    name=role_name,
                    reason=f"Auto-created Twitch live ping role for {login}",
                    mentionable=True,
                )
                created = True
            except Exception:
                safe_login = login.replace("\r", "\\r").replace("\n", "\\n")
                log.debug(
                    "Could not create live ping role for %s via dashboard",
                    safe_login,
                    exc_info=True,
                )
                role = None

        if role is None:
            return {
                "role_id": existing_role_id,
                "role_name": None,
                "created": False,
                "message": "Keine Ping-Rolle gefunden. Sie wird beim Live-Post automatisch erstellt.",
            }

        role_id = int(getattr(role, "id", 0) or 0) or None
        if role_id and role_id != existing_role_id:
            self._la_persist_streamer_ping_role_id(login, role_id)

        if role is not None and not bool(getattr(role, "mentionable", False)):
            try:
                await role.edit(
                    mentionable=True,
                    reason=f"Enable mentions for Twitch live ping role ({login})",
                )
            except Exception:
                safe_login = login.replace("\r", "\\r").replace("\n", "\\n")
                log.debug(
                    "Could not set live ping role mentionable for %s",
                    safe_login,
                    exc_info=True,
                )

        discord_user_id = self._la_coerce_role_id(entry.get("discord_user_id"))
        if role is not None and discord_user_id:
            member = guild.get_member(discord_user_id)
            if member is None:
                try:
                    member = await guild.fetch_member(discord_user_id)
                except Exception:
                    member = None
            if member is not None and role not in getattr(member, "roles", []):
                try:
                    await member.add_roles(role, reason=f"Live ping role mapping for {login}")
                except Exception:
                    log.debug(
                        "Could not assign live ping role in dashboard for member=%s",
                        discord_user_id,
                        exc_info=True,
                    )

        if created:
            message = f"Ping-Rolle automatisch erstellt: {role.name}"
        else:
            message = f"Ping-Rolle aktiv: {role.name}"
        return {"role_id": role_id, "role_name": role.name if role else None, "created": created, "message": message}

    def _la_csrf_generate(self, request: web.Request) -> str:
        generator = getattr(self, "_csrf_ensure_token", None)
        if callable(generator):
            try:
                return str(generator(request) or "")
            except Exception:
                pass
        return ""

    async def _la_csrf_extract(self, request: web.Request) -> str:
        header = str(request.headers.get("X-CSRF-Token") or "").strip()
        if header:
            return header
        body = await self._la_json_body(request)
        return str(body.get("csrf_token") or "").strip()

    def _la_csrf_verify(self, request: web.Request, token: str) -> bool:
        verifier = getattr(self, "_csrf_verify_token", None)
        if not callable(verifier):
            return False
        try:
            return bool(verifier(request, token))
        except Exception:
            return False

    async def _la_json_body(self, request: web.Request) -> dict[str, Any]:
        try:
            data = await request.json()
        except Exception:
            data = {}
        return data if isinstance(data, dict) else {}

    async def _la_can_edit(
        self,
        *,
        request: web.Request,
        streamer_login: str,
        session_login: str,
        is_admin: bool,
        allowed_editor_role_ids: list[int],
    ) -> tuple[bool, str]:
        if is_admin:
            return True, "admin"
        if not session_login:
            return False, "Partner-Session ohne Twitch-Login."
        if session_login and streamer_login != session_login:
            return False, "Nur eigener Streamer erlaubt."
        if not allowed_editor_role_ids:
            return True, "ok"
        user_id = self._la_dm_target_user_id(request, streamer_login=streamer_login)
        if not user_id:
            return False, "Keine Discord-ID fuer Rollencheck gefunden."
        member_roles = await self._la_member_role_ids(user_id)
        if member_roles.intersection(set(allowed_editor_role_ids)):
            return True, "ok"
        return False, "Dir fehlt eine erlaubte Editor-Rolle."

    def _la_actor_label(self, request: web.Request) -> str:
        level = self._la_auth_level(request)
        if level in {"admin", "localhost"}:
            uid = self._la_dm_target_user_id(request, streamer_login="")
            return f"discord:{uid}" if uid else "admin"
        session = self._la_session(request)
        login = str((session or {}).get("twitch_login") or "").strip().lower()
        return f"twitch:{login}" if login else level

    def _la_dm_target_user_id(self, request: web.Request, *, streamer_login: str) -> int | None:
        session = self._la_session(request) or {}
        uid = str(session.get("user_id") or "").strip()
        if uid.isdigit():
            return int(uid)
        if not streamer_login:
            return None
        try:
            with _storage.readonly_connection() as conn:
                row = _storage.load_active_partner(conn, twitch_login=streamer_login)
        except Exception:
            row = None
        if row and hasattr(row, "keys"):
            value = row.get("discord_user_id")
        elif row:
            value = row[2] if len(row) > 2 and len(row) < 10 else (row[21] if len(row) > 21 else "")
        else:
            value = ""
        text = str(value or "").strip()
        return int(text) if text.isdigit() else None

    async def _la_member_role_ids(self, user_id: int) -> set[int]:
        bot = self._la_discord_bot()
        if bot is None:
            return set()
        role_ids: set[int] = set()
        for guild in list(getattr(bot, "guilds", []) or []):
            member = guild.get_member(user_id)
            if member is None:
                try:
                    member = await guild.fetch_member(user_id)
                except Exception:
                    member = None
            if member is None:
                continue
            for role in getattr(member, "roles", []) or []:
                role_id = getattr(role, "id", None)
                if role_id:
                    role_ids.add(int(role_id))
        return role_ids

    def _la_discord_bot(self) -> Any | None:
        resolver = getattr(self, "_dashboard_discord_bot", None)
        if not callable(resolver):
            return None
        return resolver()

    async def _la_send_test_dm(
        self,
        user_id: int,
        streamer_login: str,
        preview_payload: dict[str, Any],
    ) -> tuple[bool, str]:
        bot = self._la_discord_bot()
        if bot is None:
            return False, "Discord Bot ist nicht verfuegbar."
        try:
            user = bot.get_user(user_id)
            if user is None:
                user = await bot.fetch_user(user_id)
        except Exception:
            user = None
        if user is None:
            return False, "Discord User konnte nicht geladen werden."

        content = str(preview_payload.get("content") or "").strip()
        embed_payload = (
            preview_payload.get("embed")
            if isinstance(preview_payload.get("embed"), dict)
            else {}
        )
        button_payload = (
            preview_payload.get("button")
            if isinstance(preview_payload.get("button"), dict)
            else {}
        )

        embed = None
        if embed_payload:
            embed_color_raw = embed_payload.get("color")
            try:
                embed_color = int(embed_color_raw or 0x9146FF)
            except Exception:
                embed_color = 0x9146FF
            embed = discord.Embed(
                title=str(embed_payload.get("title") or "").strip()[:256] or None,
                description=str(embed_payload.get("description") or "").strip()[:4096] or None,
                color=embed_color,
            )
            author_payload = (
                embed_payload.get("author")
                if isinstance(embed_payload.get("author"), dict)
                else {}
            )
            author_name = str(author_payload.get("name") or "").strip()
            if author_name and bool(author_payload.get("enabled", True)):
                embed.set_author(
                    name=author_name[:256],
                    icon_url=str(author_payload.get("icon_url") or "").strip() or None,
                    url=str(author_payload.get("url") or "").strip() or None,
                )
            for field in embed_payload.get("fields") or []:
                if not isinstance(field, dict):
                    continue
                name = str(field.get("name") or "").strip()
                value = str(field.get("value") or "").strip()
                if not name or not value:
                    continue
                embed.add_field(
                    name=name[:256],
                    value=value[:1024],
                    inline=bool(field.get("inline", True)),
                )
            thumb_payload = (
                embed_payload.get("thumbnail")
                if isinstance(embed_payload.get("thumbnail"), dict)
                else {}
            )
            thumb_url = str(thumb_payload.get("url") or "").strip()
            if thumb_url:
                embed.set_thumbnail(url=thumb_url)
            image_payload = (
                embed_payload.get("image")
                if isinstance(embed_payload.get("image"), dict)
                else {}
            )
            image_url = str(image_payload.get("url") or "").strip()
            if image_url:
                embed.set_image(url=image_url)
            footer_payload = (
                embed_payload.get("footer")
                if isinstance(embed_payload.get("footer"), dict)
                else {}
            )
            footer_text = str(footer_payload.get("text") or "").strip()
            footer_with_label = "TEST-Preview"
            if footer_text:
                footer_with_label = f"{footer_text} | TEST-Preview"
            embed.set_footer(
                text=footer_with_label[:2048],
                icon_url=str(footer_payload.get("icon_url") or "").strip() or None,
            )

        view = None
        if bool(button_payload.get("enabled", True)):
            button_url = str(button_payload.get("url") or "").strip()
            if button_url:
                view = discord.ui.View(timeout=180)
                view.add_item(
                    discord.ui.Button(
                        style=discord.ButtonStyle.link,
                        label=str(button_payload.get("label") or "Auf Twitch ansehen")[:80],
                        url=button_url,
                    )
                )

        try:
            await user.send(
                content=content or f"**TEST-Preview** fuer `{streamer_login}`",
                embed=embed,
                view=view,
                allowed_mentions=discord.AllowedMentions.none(),
            )
        except Exception:
            return False, "DM konnte nicht gesendet werden."
        return True, "Test-DM mit Embed gesendet."

    def _la_preview_payload(
        self,
        streamer_login: str,
        cfg: dict[str, Any],
        *,
        streamer_ping_role_id: int | None = None,
    ) -> dict[str, Any]:
        normalized_login = str(streamer_login or "").strip().lower() or "streamer"
        sample_stream = {
            "user_login": normalized_login,
            "user_name": normalized_login.capitalize() if normalized_login else "Streamer",
            "title": "Ranked Grind bis Top 100",
            "viewer_count": 42,
            "started_at": datetime.now(tz=UTC).isoformat(timespec="seconds"),
            "language": "de",
            "game_name": "Deadlock",
            "tags": ["Deadlock", "Community"],
            "thumbnail_url": f"https://static-cdn.jtvnw.net/previews-ttv/live_user_{normalized_login}-{{width}}x{{height}}.jpg",
            "profile_image_url": "https://static-cdn.jtvnw.net/jtv_user_pictures/xarth/404_user_70x70.png",
        }
        mentions = cfg.get("mentions") if isinstance(cfg.get("mentions"), dict) else {}
        mention_role = ""
        if bool(mentions.get("enabled", True)) and streamer_ping_role_id:
            mention_role = f"<@&{streamer_ping_role_id}>"
        else:
            role_id = str(mentions.get("role_id") or "").strip()
            if role_id.isdigit():
                mention_role = f"<@&{role_id}>"
        context = build_template_context(normalized_login, sample_stream)
        context["mention_role"] = mention_role
        context["rolle"] = mention_role
        context["url"] = f"https://www.twitch.tv/{normalized_login}"
        config_obj = LiveAnnouncementConfig.from_dict(_to_template_config(cfg))
        payload = render_announcement_payload(config_obj, context)
        payload["content"] = self._la_sanitize_disallowed_mentions_text(payload.get("content"))
        embed_cfg = cfg.get("embed") if isinstance(cfg.get("embed"), dict) else {}
        author_cfg = embed_cfg.get("author") if isinstance(embed_cfg.get("author"), dict) else {}
        payload.setdefault("embed", {}).setdefault("author", {})
        payload["embed"]["author"]["enabled"] = bool(author_cfg.get("enabled", True))
        payload.setdefault("button", {})
        payload["button"]["enabled"] = bool((cfg.get("button") or {}).get("enabled", True))
        payload["button"]["label"] = str((cfg.get("button") or {}).get("label") or payload["button"].get("label") or "Auf Twitch ansehen")
        embed_color = payload.get("embed", {}).get("color")
        if isinstance(embed_color, int):
            payload.setdefault("embed", {})["color"] = embed_color
        payload["meta"] = {
            "placeholders": list(SUPPORTED_PLACEHOLDERS),
            "sample_stream": sample_stream,
        }
        payload["validation"] = _validate_config_dict(cfg)
        return payload

    def _la_render_page(
        self,
        *,
        streamer_login: str,
        streamers: list[str],
        csrf_token: str,
        auth_level: str,
        config: dict[str, Any],
        allowed_editor_role_ids: list[int],
        preview: dict[str, Any],
        streamer_ping_role_id: int | None,
        streamer_ping_role_name: str | None,
        role_status_message: str,
    ) -> str:
        options = "".join(
            f"<option value='{html.escape(login, quote=True)}' {'selected' if login == streamer_login else ''}>{html.escape(login)}</option>"
            for login in streamers
        )
        initial = json.dumps(
            {
                "streamer_login": streamer_login,
                "auth_level": auth_level,
                "csrf_token": csrf_token,
                "config": config,
                "allowed_editor_role_ids": allowed_editor_role_ids,
                "streamer_ping_role_id": streamer_ping_role_id,
                "streamer_ping_role_name": streamer_ping_role_name or "",
                "role_status_message": role_status_message,
                "preview": preview,
                "placeholders": list(SUPPORTED_PLACEHOLDERS),
            },
            ensure_ascii=True,
        )
        initial = initial.replace("</", "<\\/")
        return f"""<!doctype html>
<html lang='de'>
<head>
  <meta charset='utf-8'>
  <meta name='viewport' content='width=device-width,initial-scale=1'>
  <title>Live Announcement Builder</title>
  <style>
    @import url('https://fonts.googleapis.com/css2?family=Manrope:wght@400;500;600;700&family=Sora:wght@500;600;700&display=swap');
    :root{{
      color-scheme:dark;
      --bg:#07151d;--card:#102635;--card2:#0f2230;
      --bd:rgba(194,221,240,.14);--bd-strong:rgba(194,221,240,.3);
      --txt:#e9f1f7;--muted:#9bb3c5;
      --primary:#ff7a18;--primary-hover:#ff8d39;
      --accent:#10b7ad;--accent-hover:#1dd4ca;
      --ok:#2ecc71;--err:#ff6b5e;
      --discord-bg:#313338;--discord-card:#2b2d31;
      --discord-embed:#1f2023;--discord-link:#00a8fc;
      --discord-btn:#5865f2;--discord-btn-hover:#6d77ff;
    }}
    *{{box-sizing:border-box}}
    body{{margin:0;padding:20px;color:var(--txt);font-family:"Manrope","Segoe UI",sans-serif;
      background:radial-gradient(1200px 520px at 90% -10%,rgba(255,122,24,.18),transparent 65%),
        radial-gradient(900px 460px at 12% -20%,rgba(16,183,173,.24),transparent 60%),
        linear-gradient(160deg,#07151d,#081a24 55%,#0a202c);min-height:100vh}}
    .page-hdr{{display:flex;justify-content:space-between;align-items:flex-start;gap:16px;flex-wrap:wrap;margin-bottom:16px}}
    .page-hdr .ey{{text-transform:uppercase;letter-spacing:.14em;font-size:11px;color:var(--muted);font-weight:700}}
    .page-hdr h1{{margin:2px 0 0;font-family:"Sora",sans-serif;font-size:22px;letter-spacing:-.02em}}
    .page-hdr .sub{{margin:4px 0 0;font-size:13px;color:var(--muted)}}
    .hdr-actions{{display:flex;gap:8px;align-items:center;flex-wrap:wrap}}
    .layout{{display:grid;grid-template-columns:1.1fr .9fr;gap:14px;align-items:start}}
    @media(max-width:1100px){{.layout{{grid-template-columns:1fr}}.preview-panel{{order:-1}}}}
    .builder-stack{{display:grid;gap:10px}}
    .block{{border:1px solid var(--bd);border-radius:12px;background:linear-gradient(160deg,rgba(13,34,47,.78),rgba(8,27,39,.78));box-shadow:inset 0 1px 0 rgba(255,255,255,.04);overflow:hidden;transition:border-color .2s}}
    .block[data-expanded="true"]{{border-color:rgba(16,183,173,.25)}}
    .block-header{{display:flex;align-items:center;gap:10px;padding:12px 14px;cursor:pointer;user-select:none;transition:background .15s}}
    .block-header:hover{{background:rgba(255,255,255,.03)}}
    .block-icon{{font-size:18px;flex-shrink:0}}
    .block-title{{flex:1;font-family:"Sora",sans-serif;font-size:14px;font-weight:600;color:#d8e5ef}}
    .block-badge{{padding:2px 8px;border-radius:999px;font-size:11px;font-weight:700;border:1px solid rgba(16,183,173,.3);background:rgba(16,183,173,.12);color:var(--accent)}}
    .block-badge.off{{background:rgba(255,255,255,.05);color:var(--muted);border-color:var(--bd)}}
    .block-tools{{display:flex;gap:4px}}
    .move-btn{{border:1px solid var(--bd);background:rgba(17,43,59,.9);color:#d8e5ef;border-radius:8px;width:28px;height:28px;cursor:pointer;font-weight:700;font-size:12px;display:flex;align-items:center;justify-content:center}}
    .move-btn:hover:not(:disabled){{border-color:var(--accent);color:#d6fffb}}
    .move-btn:disabled{{opacity:.4;cursor:not-allowed}}
    .chevron{{color:var(--muted);font-size:12px;transition:transform .25s ease;flex-shrink:0}}
    .block[data-expanded="true"] .chevron{{transform:rotate(180deg)}}
    .block-body{{max-height:0;opacity:0;overflow:hidden;transition:max-height .35s ease,opacity .2s ease,padding .25s ease;padding:0 14px}}
    .block[data-expanded="true"] .block-body{{max-height:2200px;opacity:1;padding:0 14px 14px}}
    .toggle{{display:inline-flex;align-items:center;gap:8px;cursor:pointer;font-size:13px;color:var(--muted)}}
    .toggle input{{position:absolute;opacity:0;width:0;height:0}}
    .toggle-track{{width:36px;height:20px;background:rgba(194,221,240,.2);border-radius:999px;position:relative;transition:background .2s;flex-shrink:0}}
    .toggle-track::after{{content:"";position:absolute;top:2px;left:2px;width:16px;height:16px;background:#c5d4df;border-radius:999px;transition:transform .2s,background .2s}}
    .toggle input:checked+.toggle-track{{background:var(--accent)}}
    .toggle input:checked+.toggle-track::after{{transform:translateX(16px);background:#fff}}
    .sub-tabs{{display:flex;gap:3px;margin-bottom:12px;background:rgba(7,21,29,.6);border:1px solid var(--bd);border-radius:8px;padding:3px}}
    .sub-tab{{flex:1;padding:7px 12px;border-radius:6px;border:none;background:transparent;color:var(--muted);font-weight:600;font-size:13px;font-family:inherit;cursor:pointer;transition:all .15s}}
    .sub-tab.active{{background:rgba(16,183,173,.15);color:var(--accent)}}
    .sub-tab:hover:not(.active){{color:var(--txt)}}
    .sub-pane{{display:none}}.sub-pane.active{{display:block}}
    .color-row{{display:flex;align-items:center;gap:8px;flex-wrap:wrap}}
    .color-native{{width:40px;height:36px;border:1px solid var(--bd);border-radius:8px;padding:2px;cursor:pointer;background:transparent}}
    .color-native::-webkit-color-swatch-wrapper{{padding:0}}
    .color-native::-webkit-color-swatch{{border:none;border-radius:6px}}
    .color-presets{{display:flex;gap:6px;align-items:center}}
    .color-dot{{width:22px;height:22px;border-radius:999px;border:2px solid transparent;cursor:pointer;transition:border-color .15s,transform .1s}}
    .color-dot:hover{{transform:scale(1.15)}}.color-dot.active{{border-color:#fff}}
    .role-card{{display:flex;align-items:center;gap:10px;padding:10px 12px;background:rgba(16,183,173,.06);border:1px solid rgba(16,183,173,.2);border-radius:10px}}
    .role-dot{{width:10px;height:10px;border-radius:999px;background:var(--accent);flex-shrink:0}}
    .role-dot.inactive{{background:var(--muted)}}
    .role-info{{flex:1;min-width:0}}
    .role-name{{font-weight:600;color:var(--accent);font-size:14px}}
    .role-id{{font-family:monospace;font-size:12px;color:var(--muted)}}
    .token-badge{{display:inline-flex;padding:2px 8px;border-radius:4px;background:rgba(255,255,255,.08);color:var(--accent);font-family:monospace;font-size:12px;font-weight:600}}
    input,textarea,select{{width:100%;border:1px solid var(--bd);background:var(--card2);color:var(--txt);border-radius:10px;padding:9px 10px;font:inherit}}
    input:focus,textarea:focus,select:focus{{outline:none;border-color:var(--accent);box-shadow:0 0 0 2px rgba(16,183,173,.16)}}
    input[readonly]{{color:var(--muted);background:rgba(7,21,29,.7)}}
    textarea{{min-height:80px;resize:vertical}}
    .form-grid{{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:8px}}
    @media(max-width:780px){{.form-grid{{grid-template-columns:1fr}}}}
    .form-label{{display:flex;flex-direction:column;gap:4px;font-size:13px;color:var(--muted)}}
    .form-label span{{font-weight:600}}
    h3{{margin:10px 0 7px;font-size:13px;font-family:"Sora","Manrope",sans-serif;letter-spacing:.02em;color:#d8e5ef}}
    .btn{{border:1px solid transparent;border-radius:10px;padding:9px 12px;font-weight:700;cursor:pointer;text-decoration:none;font-family:inherit;font-size:13px;transition:transform .14s,border-color .14s,background .14s}}
    .btn:hover{{transform:translateY(-1px)}}
    .btn.primary{{background:linear-gradient(135deg,var(--primary),#ff9c4f);color:#3b1500}}
    .btn.primary:hover{{background:linear-gradient(135deg,var(--primary-hover),#ffb06d)}}
    .btn.warn{{background:linear-gradient(135deg,var(--accent),#6ae0d8);color:#022e2b}}
    .btn.ghost{{background:rgba(17,43,59,.85);color:#d1e4f3;border-color:var(--bd);text-decoration:none}}
    .btn.ghost:hover{{border-color:var(--bd-strong)}}
    .btn.sm{{padding:6px 9px;font-size:12px}}
    .actions{{display:flex;align-items:center;gap:8px;flex-wrap:wrap}}
    .pill{{display:inline-flex;align-items:center;border-radius:999px;padding:5px 9px;background:rgba(16,183,173,.12);color:#c6f4f1;border:1px solid var(--bd);font-size:12px;font-weight:700;margin:2px;cursor:pointer}}
    .pill:hover{{border-color:var(--accent);background:rgba(16,183,173,.2)}}
    .field-row{{display:grid;grid-template-columns:minmax(100px,.9fr) minmax(140px,1.3fr) auto auto auto auto;gap:8px;align-items:center;margin-bottom:6px;border:1px solid var(--bd);border-radius:10px;padding:8px;background:rgba(8,23,32,.74)}}
    .field-row .btn{{padding:7px 9px;border-radius:8px;font-size:12px}}
    .field-row label{{display:inline-flex;align-items:center;gap:6px;color:var(--muted);font-size:12px}}
    .field-row label input{{width:14px;height:14px;margin:0}}
    @media(max-width:860px){{.field-row{{grid-template-columns:1fr}}}}
    .media-group{{border:1px solid var(--bd);border-radius:10px;padding:10px;background:rgba(8,23,32,.54);margin-bottom:8px}}
    .media-group h4{{margin:0 0 8px;font-size:12px;font-weight:700;color:var(--accent);text-transform:uppercase;letter-spacing:.06em}}
    .validation{{display:grid;gap:6px;margin-top:8px}}
    .validation .item{{border:1px solid rgba(255,107,94,.45);background:rgba(255,107,94,.12);border-radius:10px;padding:8px;color:#ffc6c0;font-size:12px}}
    .status{{min-height:1.3em;margin-top:4px;font-size:13px;color:var(--muted);font-weight:600}}
    .status.ok{{color:var(--ok)}}.status.err{{color:var(--err)}}
    .editor-section{{margin-top:10px;padding-top:10px;border-top:1px solid var(--bd)}}
    .small{{font-size:12px;color:var(--muted)}}
    .preview-panel{{position:sticky;top:20px}}
    .preview-card{{background:linear-gradient(160deg,rgba(16,38,53,.92),rgba(10,30,42,.92));border:1px solid var(--bd);border-radius:16px;padding:14px;box-shadow:0 10px 30px rgba(0,0,0,.26)}}
    .preview-card h3{{margin:0 0 10px;font-family:"Sora",sans-serif;font-size:14px;color:#d8e5ef}}
    .preview{{background:var(--discord-bg);border-radius:12px;overflow:hidden}}
    .discord-channel-bar{{display:flex;align-items:center;gap:6px;padding:10px 14px;background:var(--discord-card);border-bottom:1px solid rgba(255,255,255,.06);font-size:14px;font-weight:600;color:#fff}}
    .discord-channel-bar .hash{{color:var(--muted);font-size:18px;font-weight:400}}
    .preview-body{{padding:12px}}
    .discord-head{{display:flex;gap:10px;align-items:flex-start}}
    .discord-avatar{{width:40px;height:40px;border-radius:999px;border:1px solid rgba(255,255,255,.2);object-fit:cover}}
    .discord-meta{{display:flex;gap:8px;align-items:baseline;flex-wrap:wrap}}
    .discord-name{{font-weight:700;color:#fff;font-size:14px}}
    .discord-time{{color:#b4b7bd;font-size:12px}}
    .embed{{margin-top:8px;background:var(--discord-embed);border-left:4px solid var(--accent);border-radius:4px;padding:10px 10px 10px 12px;position:relative;display:grid;gap:8px}}
    .pv-author{{display:flex;align-items:center;gap:6px;font-size:12px;font-weight:700;color:#fff}}
    .pv-author img{{width:18px;height:18px;border-radius:999px;object-fit:cover;display:none}}
    .pv-title{{color:var(--discord-link);text-decoration:none;font-size:16px;font-weight:700}}
    .pv-title.no-link{{color:#fff;pointer-events:none}}
    .pv-title:hover{{text-decoration:underline}}
    .pv-desc{{color:#dbdee1;font-size:14px;white-space:pre-wrap;line-height:1.45}}
    .pv-fields{{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:8px}}
    .pv-field{{min-width:0}}.pv-field.full{{grid-column:1/-1}}
    .pv-field-name{{font-size:12px;font-weight:700;color:#fff;margin-bottom:2px}}
    .pv-field-value{{font-size:13px;color:#dbdee1;white-space:pre-wrap;word-break:break-word}}
    .pv-image{{width:100%;max-height:260px;object-fit:cover;border-radius:8px;border:1px solid rgba(255,255,255,.1);display:none}}
    .pv-thumb{{position:absolute;top:10px;right:10px;width:76px;height:76px;border-radius:8px;border:1px solid rgba(255,255,255,.1);object-fit:cover;display:none}}
    .pv-footer{{display:flex;align-items:center;gap:6px;color:#b4b7bd;font-size:12px;flex-wrap:wrap}}
    .pv-footer img{{width:16px;height:16px;border-radius:999px;object-fit:cover;display:none}}
    #pvBtn{{display:inline-flex;margin-top:8px;padding:9px 12px;border-radius:10px;background:var(--discord-btn);color:#fff;text-decoration:none;font-weight:700}}
    #pvBtn:hover{{background:var(--discord-btn-hover)}}
    .pv-highlight{{outline:2px solid var(--accent);outline-offset:2px;border-radius:4px;box-shadow:0 0 12px rgba(16,183,173,.3);transition:outline-color .2s,box-shadow .2s}}
    @media(max-width:780px){{.pv-fields{{grid-template-columns:1fr}}.pv-thumb{{position:static;width:96px;height:96px}}.page-hdr{{flex-direction:column}}.hdr-actions{{width:100%}}}}
  </style>
</head>
<body>
  <div class='page-hdr'>
    <div>
      <div class='ey'>Go-Live Builder</div>
      <h1>Discord Announcement Designer</h1>
      <div class='sub'>Rolle automatisch, Nachricht anpassen, Preview direkt sehen.</div>
    </div>
    <div class='hdr-actions'>
      <select id='streamerSelect' style='width:auto;min-width:140px;'>{options}</select>
      <button class='btn warn' id='testBtn' type='button'>Test per DM</button>
      <button class='btn primary' id='saveBtn' type='button'>Speichern</button>
      <a class='btn ghost' href='/twitch/dashboard?streamer={html.escape(streamer_login, quote=True)}'>Zurueck</a>
    </div>
  </div>
  <div class='layout'>
    <section>
      <div id='builderBlocks' class='builder-stack'>
        <div class='block' data-block='ping' data-expanded='false' style='border-color:rgba(16,183,173,.2)'>
          <div class='block-header'>
            <span class='block-icon'>&#128276;</span>
            <span class='block-title'>Ping-Rolle</span>
            <span class='block-badge' id='pingBadge'>Aktiv</span>
            <div class='block-tools'>
              <button type='button' class='move-btn' data-move='up'>&#8593;</button>
              <button type='button' class='move-btn' data-move='down'>&#8595;</button>
            </div>
            <span class='chevron'>&#9662;</span>
          </div>
          <div class='block-body'>
            <div class='role-card'>
              <span class='role-dot' id='roleDot'></span>
              <div class='role-info'>
                <div class='role-name' id='pingRoleName'>&mdash;</div>
                <div class='role-id'>ID: <span id='pingRoleIdText' style='cursor:text;user-select:all'></span></div>
              </div>
              <input type='hidden' id='pingRoleId' readonly>
              <button type='button' class='btn ghost sm' id='copyRoleIdBtn' title='Rolle-ID kopieren'>Kopieren</button>
            </div>
            <div id='roleStatus' style='margin-top:8px;padding:8px 10px;border-radius:10px;border:1px solid var(--bd);background:rgba(16,183,173,.08);color:#c6f4f1;font-size:13px'></div>
            <div style='margin-top:10px;display:flex;flex-direction:column;gap:8px'>
              <label class='toggle'>
                <input id='mentionsEnabled' type='checkbox'>
                <span class='toggle-track'></span>
                Rolle erwaehnen <span class='token-badge'>{{{{rolle}}}}</span>
              </label>
              <div class='small' style='line-height:1.5'>
                User tragen sich ueber das <strong>Self-Role-Menue</strong> oder per <strong>Bot-Command</strong> ein.
                Die Rolle wird automatisch erstellt und verwaltet.
              </div>
            </div>
            <div class='editor-section'>
              <div class='small' style='font-weight:600;margin-bottom:4px'>Editor-Rollen (optional)</div>
              <input id='editorRoles' placeholder='Discord Role IDs (kommagetrennt)'>
              <div class='small' style='margin-top:4px'>Wer diese Konfiguration bearbeiten darf.</div>
            </div>
          </div>
        </div>
        <div class='block' data-block='message' data-expanded='true'>
          <div class='block-header'>
            <span class='block-icon'>&#128172;</span>
            <span class='block-title'>Nachricht</span>
            <div class='block-tools'>
              <button type='button' class='move-btn' data-move='up'>&#8593;</button>
              <button type='button' class='move-btn' data-move='down'>&#8595;</button>
            </div>
            <span class='chevron'>&#9662;</span>
          </div>
          <div class='block-body'>
            <div id='placeholderPills' style='margin-bottom:8px'></div>
            <textarea id='contentTpl' placeholder='Nachrichtentext ueber dem Embed (Platzhalter: {{{{channel}}}}, {{{{rolle}}}}, ...)'></textarea>
          </div>
        </div>
        <div class='block' data-block='embed' data-expanded='true'>
          <div class='block-header'>
            <span class='block-icon'>&#127912;</span>
            <span class='block-title'>Embed-Designer</span>
            <div class='block-tools'>
              <button type='button' class='move-btn' data-move='up'>&#8593;</button>
              <button type='button' class='move-btn' data-move='down'>&#8595;</button>
            </div>
            <span class='chevron'>&#9662;</span>
          </div>
          <div class='block-body'>
            <div class='sub-tabs'>
              <button type='button' class='sub-tab active' data-tab='appearance'>Aussehen</button>
              <button type='button' class='sub-tab' data-tab='content'>Inhalt</button>
            </div>
            <div class='sub-pane active' data-pane='appearance'>
              <h3>Embed-Farbe</h3>
              <div class='color-row'>
                <input type='color' id='embedColorPicker' class='color-native' value='#10b7ad'>
                <input id='embedColor' placeholder='#10b7ad' style='width:110px'>
                <div class='color-presets'>
                  <span class='color-dot' data-color='#9146ff' style='background:#9146ff' title='Twitch Lila'></span>
                  <span class='color-dot' data-color='#10b7ad' style='background:#10b7ad' title='Teal'></span>
                  <span class='color-dot' data-color='#ff7a18' style='background:#ff7a18' title='Orange'></span>
                  <span class='color-dot' data-color='#2ecc71' style='background:#2ecc71' title='Gruen'></span>
                  <span class='color-dot' data-color='#e74c3c' style='background:#e74c3c' title='Rot'></span>
                </div>
              </div>
              <h3>Author</h3>
              <div class='form-grid'>
                <div class='form-label'>
                  <span>Author-Name</span>
                  <input id='authorName' placeholder='LIVE: {{{{channel}}}}'>
                </div>
                <div class='form-label'>
                  <span>Icon</span>
                  <select id='authorIconMode'>
                    <option value='twitch_logo'>Twitch Logo</option>
                    <option value='channel_avatar'>Kanal Avatar</option>
                    <option value='none'>Kein Icon</option>
                  </select>
                </div>
              </div>
              <div style='display:flex;gap:16px;flex-wrap:wrap;margin-top:8px'>
                <label class='toggle'><input id='authorEnabled' type='checkbox'><span class='toggle-track'></span> Author anzeigen</label>
                <label class='toggle'><input id='authorLinkEnabled' type='checkbox'><span class='toggle-track'></span> Auf Kanal verlinken</label>
              </div>
            </div>
            <div class='sub-pane' data-pane='content'>
              <div class='form-grid'>
                <div class='form-label'>
                  <span>Embed-Titel</span>
                  <input id='embedTitle' placeholder='{{{{channel}}}} ist LIVE in Deadlock!'>
                </div>
                <div class='form-label'>
                  <span>Beschreibungs-Modus</span>
                  <select id='descMode'>
                    <option value='stream_title'>Auto Streamtitel</option>
                    <option value='custom'>Custom Text</option>
                    <option value='custom_plus_title'>Custom + Streamtitel</option>
                  </select>
                </div>
              </div>
              <div class='form-label' style='margin-top:8px'>
                <span>Beschreibung</span>
                <textarea id='embedDesc' placeholder='Beschreibung'></textarea>
              </div>
              <div style='display:flex;gap:16px;flex-wrap:wrap;margin-top:8px'>
                <label class='toggle'><input id='titleLinkEnabled' type='checkbox'><span class='toggle-track'></span> Titel als Link</label>
                <label class='toggle'><input id='shortenEnabled' type='checkbox'><span class='toggle-track'></span> Texte kuerzen</label>
              </div>
            </div>
          </div>
        </div>
        <div class='block' data-block='button' data-expanded='false'>
          <div class='block-header'>
            <span class='block-icon'>&#128279;</span>
            <span class='block-title'>Button</span>
            <span class='block-badge' id='buttonBadge'>Aktiv</span>
            <div class='block-tools'>
              <button type='button' class='move-btn' data-move='up'>&#8593;</button>
              <button type='button' class='move-btn' data-move='down'>&#8595;</button>
            </div>
            <span class='chevron'>&#9662;</span>
          </div>
          <div class='block-body'>
            <label class='toggle' style='margin-bottom:8px'>
              <input id='buttonEnabled' type='checkbox'>
              <span class='toggle-track'></span>
              Button anzeigen
            </label>
            <div class='form-grid'>
              <div class='form-label'>
                <span>Button Text</span>
                <input id='buttonLabel' placeholder='Auf Twitch ansehen'>
              </div>
              <div class='form-label'>
                <span>URL (automatisch)</span>
                <input id='buttonUrl' value='{{{{url}}}}' readonly>
              </div>
            </div>
          </div>
        </div>
        <div class='block' data-block='advanced' data-expanded='false'>
          <div class='block-header'>
            <span class='block-icon'>&#128451;</span>
            <span class='block-title'>Felder &amp; Medien</span>
            <div class='block-tools'>
              <button type='button' class='move-btn' data-move='up'>&#8593;</button>
              <button type='button' class='move-btn' data-move='down'>&#8595;</button>
            </div>
            <span class='chevron'>&#9662;</span>
          </div>
          <div class='block-body'>
            <h3>Felder</h3>
            <div id='fieldsWrap'></div>
            <div class='actions' style='margin-top:6px'>
              <button class='btn ghost sm' type='button' id='addFieldBtn'>+ Feld</button>
              <button class='btn ghost sm' type='button' id='presetBtn'>Preset: Viewer + Kategorie</button>
              <button class='btn ghost sm' type='button' id='presetMetaBtn'>Preset: Start + Sprache + Tags</button>
            </div>
            <div class='media-group' style='margin-top:12px'>
              <h4>Thumbnail</h4>
              <div class='form-grid'>
                <div class='form-label'>
                  <span>Modus</span>
                  <select id='thumbMode'>
                    <option value='none'>Aus</option>
                    <option value='channel_avatar'>Kanal Avatar</option>
                    <option value='custom_url'>Custom URL</option>
                  </select>
                </div>
                <div class='form-label'>
                  <span>Custom URL</span>
                  <input id='thumbUrl' placeholder='https://...'>
                </div>
              </div>
            </div>
            <div class='media-group'>
              <h4>Stream-Bild</h4>
              <div class='form-grid'>
                <label class='toggle'><input id='useStreamImage' type='checkbox'><span class='toggle-track'></span> Stream-Thumbnail verwenden</label>
                <div class='form-label'>
                  <span>Custom Image URL</span>
                  <input id='imageUrl' placeholder='https://...'>
                </div>
                <div class='form-label'>
                  <span>Format</span>
                  <select id='imageFormat'><option value='16:9'>16:9</option><option value='4:3'>4:3</option></select>
                </div>
                <label class='toggle'><input id='imageCb' type='checkbox'><span class='toggle-track'></span> Cache-Buster</label>
              </div>
            </div>
            <div class='media-group'>
              <h4>Footer</h4>
              <div class='form-grid'>
                <div class='form-label'>
                  <span>Footer Text</span>
                  <input id='footerText' placeholder='Footer Text'>
                </div>
                <div class='form-label'>
                  <span>Zeitstempel</span>
                  <select id='footerTs'><option value='started_at'>Startzeit</option><option value='now'>Jetzt</option><option value='none'>Aus</option></select>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
      <div class='validation' id='validation'></div>
      <div class='status' id='status'></div>
    </section>
    <aside class='preview-panel'>
      <div class='preview-card'>
        <h3>Live Preview</h3>
        <div class='preview'>
          <div class='discord-channel-bar'>
            <span class='hash'>#</span> go-live-announcements
          </div>
          <div class='preview-body'>
            <div class='discord-head'>
              <img id='pvBotAvatar' class='discord-avatar' alt='Bot Avatar'>
              <div style='min-width:0;width:100%'>
                <div class='discord-meta'>
                  <span class='discord-name'>Deadlock Bot</span>
                  <span id='pvMsgTime' class='discord-time'>Heute um --:--</span>
                </div>
                <div id='pvContent' class='pv-desc'>(leer)</div>
              </div>
            </div>
            <div class='embed' id='pvEmbed'>
              <img id='pvThumb' class='pv-thumb' alt='Thumbnail'>
              <div id='pvAuthorWrap' class='pv-author'>
                <img id='pvAuthorIcon' alt='Author Icon'>
                <a id='pvAuthor' class='pv-title no-link' href='#' target='_blank' rel='noopener noreferrer'>LIVE</a>
              </div>
              <a id='pvTitle' class='pv-title no-link' href='#' target='_blank' rel='noopener noreferrer'></a>
              <div id='pvDesc' class='pv-desc'></div>
              <img id='pvImage' class='pv-image' alt='Stream Preview'>
              <div id='pvFields' class='pv-fields'></div>
              <div id='pvFooter' class='pv-footer'>
                <img id='pvFooterIcon' alt='Footer Icon'>
                <span id='pvFooterText'></span>
                <span id='pvFooterTime'></span>
              </div>
            </div>
            <a id='pvBtn' href='#' target='_blank' rel='noopener noreferrer'>Auf Twitch ansehen</a>
          </div>
        </div>
      </div>
    </aside>
  </div>
  <script>
    const ST = {initial};
    const DEFAULT_BLOCK_ORDER = ['ping','message','embed','button','advanced'];
    const S = {{
      streamer: ST.streamer_login || '',
      csrf: ST.csrf_token || '',
      config: structuredClone(ST.config || {{}}),
      allowedRoles: [...(ST.allowed_editor_role_ids || [])],
      streamerPingRoleId: Number(ST.streamer_ping_role_id || 0) || null,
      streamerPingRoleName: String(ST.streamer_ping_role_name || ''),
      roleStatusMessage: String(ST.role_status_message || ''),
      preview: ST.preview || {{}},
      blockOrder: [],
      timer: null,
      activeBlock: null
    }};
    const E = (id) => document.getElementById(id);
    const esc = (v) => String(v||'').replaceAll('&','&amp;').replaceAll('<','&lt;').replaceAll('>','&gt;').replaceAll('"','&quot;').replaceAll("'",'&#39;');
    const WATCH_IDS = ['editorRoles','mentionsEnabled','contentTpl','embedColor','authorName','authorIconMode','authorEnabled','authorLinkEnabled','embedTitle','descMode','embedDesc','titleLinkEnabled','shortenEnabled','thumbMode','thumbUrl','useStreamImage','imageUrl','imageFormat','imageCb','footerText','footerTs','buttonEnabled','buttonLabel'];
    const BOT_AVATAR = 'https://static-cdn.jtvnw.net/jtv_user_pictures/2f6f9be7-41f7-4fd1-8ca8-13213e63ed05-profile_image-300x300.png';

    function toHexColor(raw) {{
      if (typeof raw === 'number') return '#' + raw.toString(16).padStart(6,'0');
      const t = String(raw||'').trim();
      if (t.startsWith('#') && t.length === 7) return t;
      if (/^[0-9a-fA-F]{{6}}$/.test(t)) return '#' + t;
      return '#10b7ad';
    }}
    function formatTime(iso) {{
      if (!iso) return '';
      const d = new Date(iso);
      if (Number.isNaN(d.getTime())) return '';
      return d.toLocaleString('de-DE', {{ day:'2-digit', month:'2-digit', hour:'2-digit', minute:'2-digit' }});
    }}
    function setImage(node, url, fb) {{
      const p = String(url||'').trim(), f = String(fb||'').trim();
      let tried = false;
      const apply = (s) => {{ node.style.display = s ? 'block' : 'none'; node.src = s || ''; }};
      node.onerror = () => {{ if (!tried && f) {{ tried = true; apply(f); return; }} node.style.display = 'none'; }};
      if (p) {{ apply(p); return; }}
      if (f) {{ tried = true; apply(f); return; }}
      apply('');
    }}
    function norm() {{
      S.config.mentions = S.config.mentions || {{}};
      S.config.embed = S.config.embed || {{}};
      S.config.embed.author = S.config.embed.author || {{}};
      S.config.embed.thumbnail = S.config.embed.thumbnail || {{}};
      S.config.embed.image = S.config.embed.image || {{}};
      S.config.embed.footer = S.config.embed.footer || {{}};
      S.config.button = S.config.button || {{}};
      S.config.ui = S.config.ui || {{}};
      S.config.embed.fields = Array.isArray(S.config.embed.fields) ? S.config.embed.fields : [];
      S.config.button.url_template = '{{url}}';
      S.blockOrder = normalizeBlockOrder(S.config.ui.block_order);
    }}
    function normalizeBlockOrder(raw) {{
      const src = Array.isArray(raw) ? raw : [], out = [];
      src.forEach((v) => {{ const id = String(v||'').trim(); if (!DEFAULT_BLOCK_ORDER.includes(id) || out.includes(id)) return; out.push(id); }});
      DEFAULT_BLOCK_ORDER.forEach((id) => {{ if (!out.includes(id)) out.push(id); }});
      return out;
    }}
    function roleIdsCsv(t) {{ return String(t||'').split(',').map(v=>v.trim()).filter(v=>/^\\d+$/.test(v)).map(v=>Number(v)); }}

    function applyBlockOrder() {{
      const c = E('builderBlocks');
      if (!c) return;
      S.blockOrder = normalizeBlockOrder(S.blockOrder);
      S.blockOrder.forEach((id) => {{ const n = c.querySelector(`.block[data-block="${{id}}"]`); if (n) c.appendChild(n); }});
      S.blockOrder.forEach((id, i) => {{
        const n = c.querySelector(`.block[data-block="${{id}}"]`);
        if (!n) return;
        const up = n.querySelector('[data-move="up"]'), dn = n.querySelector('[data-move="down"]');
        if (up) up.disabled = i <= 0;
        if (dn) dn.disabled = i >= S.blockOrder.length - 1;
      }});
    }}
    function moveBlock(id, dir) {{
      const i = S.blockOrder.indexOf(id); if (i < 0) return;
      const n = dir === 'up' ? i - 1 : i + 1;
      if (n < 0 || n >= S.blockOrder.length) return;
      [S.blockOrder[i], S.blockOrder[n]] = [S.blockOrder[n], S.blockOrder[i]];
      applyBlockOrder(); onMutate();
    }}
    function bindBlockMoveButtons() {{
      document.querySelectorAll('#builderBlocks .block').forEach((node) => {{
        const id = String(node.getAttribute('data-block') || '');
        node.querySelectorAll('[data-move]').forEach((btn) => {{
          btn.addEventListener('click', (e) => {{ e.stopPropagation(); moveBlock(id, String(btn.getAttribute('data-move') || '')); }});
        }});
      }});
    }}

    function toggleBlock(blockId, forceOpen) {{
      document.querySelectorAll('#builderBlocks .block').forEach((node) => {{
        const id = node.getAttribute('data-block');
        if (id === blockId) {{
          const isOpen = node.getAttribute('data-expanded') === 'true';
          node.setAttribute('data-expanded', forceOpen !== undefined ? String(forceOpen) : String(!isOpen));
        }}
      }});
      highlightPreview(blockId);
    }}
    function initAccordion() {{
      document.querySelectorAll('#builderBlocks .block-header').forEach((hdr) => {{
        hdr.addEventListener('click', (e) => {{
          if (e.target.closest('.block-tools') || e.target.closest('.move-btn')) return;
          const block = hdr.closest('.block');
          const id = block.getAttribute('data-block');
          toggleBlock(id);
        }});
      }});
    }}

    function initSubTabs() {{
      document.querySelectorAll('.sub-tab').forEach((tab) => {{
        tab.addEventListener('click', () => {{
          const pane = tab.getAttribute('data-tab');
          const parent = tab.closest('.block-body');
          parent.querySelectorAll('.sub-tab').forEach(t => t.classList.toggle('active', t === tab));
          parent.querySelectorAll('.sub-pane').forEach(p => p.classList.toggle('active', p.getAttribute('data-pane') === pane));
        }});
      }});
    }}

    function initColorPicker() {{
      const picker = E('embedColorPicker');
      const hex = E('embedColor');
      if (!picker || !hex) return;
      picker.addEventListener('input', () => {{ hex.value = picker.value; syncColorDots(picker.value); onMutate(); }});
      hex.addEventListener('input', () => {{
        const v = toHexColor(hex.value);
        picker.value = v;
        syncColorDots(v);
      }});
      document.querySelectorAll('.color-dot').forEach((dot) => {{
        dot.addEventListener('click', () => {{
          const c = dot.getAttribute('data-color');
          hex.value = c; picker.value = c;
          syncColorDots(c); onMutate();
        }});
      }});
    }}
    function syncColorDots(active) {{
      document.querySelectorAll('.color-dot').forEach((d) => {{
        d.classList.toggle('active', d.getAttribute('data-color') === active);
      }});
    }}

    function highlightPreview(blockId) {{
      S.activeBlock = blockId;
      document.querySelectorAll('.pv-highlight').forEach(el => el.classList.remove('pv-highlight'));
      const map = {{
        'message': ['pvContent'],
        'embed': ['pvEmbed'],
        'button': ['pvBtn'],
        'advanced': ['pvFields'],
        'ping': ['pvContent']
      }};
      const ids = map[blockId] || [];
      ids.forEach((id) => {{ const el = E(id); if (el) el.classList.add('pv-highlight'); }});
    }}
    function initFocusTracking() {{
      document.addEventListener('focusin', (e) => {{
        const block = e.target.closest('.block');
        if (!block) return;
        const id = block.getAttribute('data-block');
        if (id) highlightPreview(id);
      }});
    }}

    function updateBadges() {{
      const pb = E('pingBadge'), bb = E('buttonBadge');
      if (pb) {{ const on = E('mentionsEnabled')?.checked; pb.textContent = on ? 'Aktiv' : 'Aus'; pb.className = 'block-badge' + (on ? '' : ' off'); }}
      if (bb) {{ const on = E('buttonEnabled')?.checked; bb.textContent = on ? 'Aktiv' : 'Aus'; bb.className = 'block-badge' + (on ? '' : ' off'); }}
    }}

    function renderRoleInfo() {{
      const rid = S.streamerPingRoleId ? String(S.streamerPingRoleId) : '';
      const rn = S.streamerPingRoleName || '';
      const nameEl = E('pingRoleName');
      const idTextEl = E('pingRoleIdText');
      const hiddenEl = E('pingRoleId');
      if (nameEl) {{ nameEl.textContent = rn || (rid ? 'ID: ' + rid : 'Noch nicht erstellt'); nameEl.style.color = rn ? 'var(--accent)' : 'var(--muted)'; }}
      if (idTextEl) {{ idTextEl.textContent = rid; }}
      if (hiddenEl) {{ hiddenEl.value = rid; }}
      const dot = E('roleDot');
      if (dot) {{ dot.className = 'role-dot' + (rid ? '' : ' inactive'); }}
      const copyBtn = E('copyRoleIdBtn');
      if (copyBtn) {{ copyBtn.style.display = rid ? '' : 'none'; }}
      E('roleStatus').textContent = S.roleStatusMessage || (rid ? 'Ping-Rolle verknuepft (ID: ' + rid + ').' : 'Wird automatisch beim ersten Go-Live erstellt.');
    }}

    function fillForm() {{
      norm();
      E('mentionsEnabled').checked = Boolean(S.config.mentions.enabled);
      E('editorRoles').value = (S.allowedRoles || []).join(',');
      E('contentTpl').value = S.config.content || '';
      const color = toHexColor(S.config.embed.color || '#9146ff');
      E('embedColor').value = color;
      const picker = E('embedColorPicker');
      if (picker) picker.value = color;
      syncColorDots(color);
      E('authorEnabled').checked = Boolean(S.config.embed.author.enabled);
      E('authorName').value = S.config.embed.author.name || '';
      E('authorIconMode').value = S.config.embed.author.icon_mode || 'twitch_logo';
      E('authorLinkEnabled').checked = Boolean(S.config.embed.author.link_to_channel);
      E('embedTitle').value = S.config.embed.title || '';
      E('descMode').value = S.config.embed.description_mode || 'stream_title';
      E('embedDesc').value = S.config.embed.description || '';
      E('titleLinkEnabled').checked = Boolean(S.config.embed.title_link_enabled);
      E('shortenEnabled').checked = Boolean(S.config.embed.shorten);
      E('thumbMode').value = S.config.embed.thumbnail.mode || 'none';
      E('thumbUrl').value = S.config.embed.thumbnail.custom_url || '';
      E('useStreamImage').checked = Boolean(S.config.embed.image.use_stream_thumbnail);
      E('imageUrl').value = S.config.embed.image.custom_url || '';
      E('imageFormat').value = S.config.embed.image.format || '16:9';
      E('imageCb').checked = Boolean(S.config.embed.image.cache_buster);
      E('footerText').value = S.config.embed.footer.text || '';
      E('footerTs').value = S.config.embed.footer.timestamp_mode || 'started_at';
      E('buttonEnabled').checked = Boolean(S.config.button.enabled);
      E('buttonLabel').value = S.config.button.label || '';
      applyBlockOrder();
      renderRoleInfo();
      renderFields();
      renderPlaceholderPills();
      updateBadges();
    }}

    function readForm() {{
      norm();
      S.config.content = E('contentTpl').value;
      S.config.mentions.enabled = Boolean(E('mentionsEnabled').checked);
      S.config.mentions.role_id = '';
      S.allowedRoles = roleIdsCsv(E('editorRoles').value);
      S.config.allowed_editor_role_ids = S.allowedRoles;
      S.config.embed.color = E('embedColor').value.trim() || '#9146ff';
      S.config.embed.author.enabled = Boolean(E('authorEnabled').checked);
      S.config.embed.author.name = E('authorName').value;
      S.config.embed.author.icon_mode = E('authorIconMode').value;
      S.config.embed.author.link_to_channel = Boolean(E('authorLinkEnabled').checked);
      S.config.embed.title = E('embedTitle').value;
      S.config.embed.description_mode = E('descMode').value;
      S.config.embed.description = E('embedDesc').value;
      S.config.embed.title_link_enabled = Boolean(E('titleLinkEnabled').checked);
      S.config.embed.shorten = Boolean(E('shortenEnabled').checked);
      S.config.embed.thumbnail.mode = E('thumbMode').value;
      S.config.embed.thumbnail.custom_url = E('thumbUrl').value;
      S.config.embed.image.use_stream_thumbnail = Boolean(E('useStreamImage').checked);
      S.config.embed.image.custom_url = E('imageUrl').value;
      S.config.embed.image.format = E('imageFormat').value;
      S.config.embed.image.cache_buster = Boolean(E('imageCb').checked);
      S.config.embed.footer.text = E('footerText').value;
      S.config.embed.footer.timestamp_mode = E('footerTs').value;
      S.config.button.enabled = Boolean(E('buttonEnabled').checked);
      S.config.button.label = E('buttonLabel').value;
      S.config.button.url_template = '{{url}}';
      S.config.ui.block_order = [...S.blockOrder];
      S.config.embed.fields = collectFields();
      return S.config;
    }}

    function renderFields() {{
      const wrap = E('fieldsWrap'); wrap.innerHTML = '';
      (S.config.embed.fields || []).forEach((field, idx) => {{
        const row = document.createElement('div'); row.className = 'field-row';
        row.innerHTML = `<input data-name='${{idx}}' value='${{esc(field.name)}}' placeholder='Field Name'><input data-value='${{idx}}' value='${{esc(field.value)}}' placeholder='Field Value'><label class='small'><input data-inline='${{idx}}' type='checkbox' ${{field.inline ? 'checked' : ''}}> Inline</label><button class='btn ghost sm' type='button' data-up='${{idx}}'>&#9650;</button><button class='btn ghost sm' type='button' data-down='${{idx}}'>&#9660;</button><button class='btn ghost sm' type='button' data-remove='${{idx}}'>Entfernen</button>`;
        wrap.appendChild(row);
      }});
      wrap.querySelectorAll('[data-remove]').forEach((btn) => btn.addEventListener('click', () => {{ S.config.embed.fields.splice(Number(btn.getAttribute('data-remove')), 1); renderFields(); onMutate(); }}));
      wrap.querySelectorAll('[data-up]').forEach((btn) => btn.addEventListener('click', () => {{
        const i = Number(btn.getAttribute('data-up')); if (i <= 0) return;
        [S.config.embed.fields[i-1], S.config.embed.fields[i]] = [S.config.embed.fields[i], S.config.embed.fields[i-1]];
        renderFields(); onMutate();
      }}));
      wrap.querySelectorAll('[data-down]').forEach((btn) => btn.addEventListener('click', () => {{
        const i = Number(btn.getAttribute('data-down')); if (i >= S.config.embed.fields.length - 1) return;
        [S.config.embed.fields[i+1], S.config.embed.fields[i]] = [S.config.embed.fields[i], S.config.embed.fields[i+1]];
        renderFields(); onMutate();
      }}));
      wrap.querySelectorAll('input').forEach((inp) => inp.addEventListener('input', onMutate));
      wrap.querySelectorAll('input[type="checkbox"]').forEach((inp) => inp.addEventListener('change', onMutate));
    }}

    function collectFields() {{
      const rows = E('fieldsWrap').querySelectorAll('.field-row'), out = [];
      rows.forEach((row) => {{
        const n = (row.querySelector('[data-name]')?.value || '').trim();
        const v = (row.querySelector('[data-value]')?.value || '').trim();
        const inline = Boolean(row.querySelector('[data-inline]')?.checked);
        if (n && v) out.push({{ name: n, value: v, inline }});
      }});
      return out;
    }}

    function renderPlaceholderPills() {{
      const wrap = E('placeholderPills'); wrap.innerHTML = '';
      (ST.placeholders || []).forEach((ph) => {{
        const pill = document.createElement('button'); pill.type = 'button'; pill.className = 'pill';
        pill.textContent = '{{' + ph + '}}';
        pill.addEventListener('click', () => {{
          const a = document.activeElement;
          if (!(a instanceof HTMLInputElement || a instanceof HTMLTextAreaElement)) return;
          const token = '{{' + ph + '}}';
          const s = a.selectionStart ?? a.value.length, e = a.selectionEnd ?? a.value.length;
          a.value = a.value.slice(0, s) + token + a.value.slice(e);
          a.focus(); a.selectionStart = a.selectionEnd = s + token.length;
          onMutate();
        }});
        wrap.appendChild(pill);
      }});
    }}

    function renderValidation(items) {{
      const wrap = E('validation'); wrap.innerHTML = '';
      (items || []).forEach((item) => {{
        const el = document.createElement('div'); el.className = 'item';
        el.textContent = `${{item.path || 'config'}}: ${{item.message || 'ungueltig'}}`;
        wrap.appendChild(el);
      }});
    }}
    function setStatus(msg, isErr = false) {{
      const el = E('status'); el.textContent = msg || '';
      el.className = 'status' + (msg ? (isErr ? ' err' : ' ok') : '');
    }}

    function renderPreview() {{
      const p = S.preview || {{}};
      const embed = p.embed || {{}};
      const author = embed.author || {{}};
      const footer = embed.footer || {{}};
      const button = p.button || {{}};
      const fbStream = 'https://static-cdn.jtvnw.net/ttv-static/404_preview-640x360.jpg';
      const fbAvatar = 'https://static-cdn.jtvnw.net/jtv_user_pictures/xarth/404_user_70x70.png';
      E('pvBotAvatar').src = BOT_AVATAR;
      E('pvMsgTime').textContent = 'Heute um ' + new Date().toLocaleTimeString('de-DE', {{ hour:'2-digit', minute:'2-digit' }});
      E('pvContent').textContent = p.content || '(leer)';
      E('pvEmbed').style.borderLeftColor = toHexColor(embed.color);
      const authOn = author.enabled !== false && Boolean(author.name || author.icon_url);
      E('pvAuthorWrap').style.display = authOn ? 'flex' : 'none';
      const authLink = E('pvAuthor');
      authLink.textContent = author.name || '';
      authLink.href = author.url || '#';
      authLink.classList.toggle('no-link', !author.url);
      setImage(E('pvAuthorIcon'), author.icon_url || '', fbAvatar);
      const titleNode = E('pvTitle');
      const titleText = String(embed.title || '').trim();
      const titleUrl = String(embed.url || '').trim();
      if (!titleText) {{ titleNode.style.display = 'none'; }}
      else {{ titleNode.style.display = 'block'; titleNode.textContent = titleText; titleNode.href = titleUrl || '#'; titleNode.classList.toggle('no-link', !titleUrl); }}
      E('pvDesc').textContent = embed.description || '';
      const fWrap = E('pvFields'); fWrap.innerHTML = '';
      (embed.fields || []).forEach((f) => {{
        const d = document.createElement('div'); d.className = 'pv-field ' + (f.inline ? '' : 'full');
        const l = document.createElement('div'); l.className = 'pv-field-name'; l.textContent = String(f.name || '');
        const v = document.createElement('div'); v.className = 'pv-field-value'; v.textContent = String(f.value || '');
        d.appendChild(l); d.appendChild(v); fWrap.appendChild(d);
      }});
      setImage(E('pvImage'), (embed.image || {{}}).url || '', fbStream);
      setImage(E('pvThumb'), (embed.thumbnail || {{}}).url || '', fbAvatar);
      E('pvFooterText').textContent = footer.text || '';
      E('pvFooterTime').textContent = embed.timestamp ? ('• ' + formatTime(embed.timestamp)) : '';
      setImage(E('pvFooterIcon'), footer.icon_url || '', fbAvatar);
      E('pvFooter').style.display = (footer.text || footer.icon_url || embed.timestamp) ? 'flex' : 'none';
      const pvBtn = E('pvBtn');
      pvBtn.style.display = button.enabled !== false ? 'inline-flex' : 'none';
      pvBtn.textContent = button.label || 'Auf Twitch ansehen';
      pvBtn.href = button.url || '#';
      if (S.activeBlock) highlightPreview(S.activeBlock);
    }}

    async function fetchPreview() {{
      const cfg = readForm();
      updateBadges();
      const url = `/twitch/api/live-announcement/preview?streamer=${{encodeURIComponent(S.streamer)}}&config=${{encodeURIComponent(JSON.stringify(cfg))}}`;
      try {{
        const res = await fetch(url, {{ credentials:'same-origin' }});
        const data = await res.json();
        if (!res.ok) {{ setStatus(data.error || 'Preview fehlgeschlagen.', true); return; }}
        S.streamerPingRoleId = Number(data.streamer_ping_role_id || S.streamerPingRoleId || 0) || null;
        if (data.streamer_ping_role_name) S.streamerPingRoleName = String(data.streamer_ping_role_name);
        if (typeof data.role_status_message === 'string' && data.role_status_message) S.roleStatusMessage = data.role_status_message;
        renderRoleInfo();
        S.preview = data.preview || {{}};
        renderPreview();
        renderValidation(data.validation || []);
      }} catch (_) {{ setStatus('Preview Request fehlgeschlagen.', true); }}
    }}

    async function saveConfig() {{
      const cfg = readForm();
      setStatus('Speichere...');
      try {{
        const res = await fetch(`/twitch/api/live-announcement/config?streamer=${{encodeURIComponent(S.streamer)}}`, {{
          method:'POST', credentials:'same-origin',
          headers: {{ 'Content-Type':'application/json', 'X-CSRF-Token': S.csrf }},
          body: JSON.stringify({{ csrf_token: S.csrf, streamer_login: S.streamer, config: cfg, allowed_editor_role_ids: S.allowedRoles }})
        }});
        const data = await res.json();
        if (!res.ok) {{ renderValidation(data.validation || []); setStatus(data.message || data.error || 'Speichern fehlgeschlagen.', true); return; }}
        S.streamerPingRoleId = Number(data.streamer_ping_role_id || S.streamerPingRoleId || 0) || null;
        if (data.streamer_ping_role_name) S.streamerPingRoleName = String(data.streamer_ping_role_name);
        if (typeof data.role_status_message === 'string' && data.role_status_message) S.roleStatusMessage = data.role_status_message;
        renderRoleInfo();
        S.preview = data.preview || S.preview;
        renderPreview();
        renderValidation([]);
        setStatus('Gespeichert.');
      }} catch (_) {{ setStatus('Speichern fehlgeschlagen.', true); }}
    }}

    async function sendTestDm() {{
      const cfg = readForm();
      setStatus('Sende Test-DM...');
      try {{
        const res = await fetch(`/twitch/api/live-announcement/test?streamer=${{encodeURIComponent(S.streamer)}}`, {{
          method:'POST', credentials:'same-origin',
          headers: {{ 'Content-Type':'application/json', 'X-CSRF-Token': S.csrf }},
          body: JSON.stringify({{ csrf_token: S.csrf, streamer_login: S.streamer, config: cfg }})
        }});
        const data = await res.json();
        S.streamerPingRoleId = Number(data.streamer_ping_role_id || S.streamerPingRoleId || 0) || null;
        if (data.streamer_ping_role_name) S.streamerPingRoleName = String(data.streamer_ping_role_name);
        renderRoleInfo();
        setStatus(data.message || (data.ok ? 'Test versendet.' : 'Test fehlgeschlagen.'), !data.ok);
      }} catch (_) {{ setStatus('Test-DM fehlgeschlagen.', true); }}
    }}

    function onMutate() {{ if (S.timer) clearTimeout(S.timer); S.timer = setTimeout(fetchPreview, 190); }}

    function bindEvents() {{
      bindBlockMoveButtons();
      WATCH_IDS.forEach((id) => {{
        const node = E(id); if (!node) return;
        const evt = (node instanceof HTMLInputElement && node.type === 'checkbox') || node instanceof HTMLSelectElement ? 'change' : 'input';
        node.addEventListener(evt, onMutate);
      }});
      E('streamerSelect').addEventListener('change', () => {{ const n = E('streamerSelect').value; if (n) window.location.href = `/twitch/live-announcement?streamer=${{encodeURIComponent(n)}}`; }});
      E('addFieldBtn').addEventListener('click', () => {{ S.config.embed.fields.push({{ name:'Neues Feld', value:'{{title}}', inline:true }}); renderFields(); onMutate(); }});
      E('presetBtn').addEventListener('click', () => {{ S.config.embed.fields = [{{ name:'Viewer', value:'{{viewer_count}}', inline:true }}, {{ name:'Kategorie', value:'{{game}}', inline:true }}]; renderFields(); onMutate(); }});
      E('presetMetaBtn').addEventListener('click', () => {{ S.config.embed.fields = [{{ name:'Startzeit', value:'{{started_at}}', inline:true }}, {{ name:'Sprache', value:'{{language}}', inline:true }}, {{ name:'Tags', value:'{{tags}}', inline:false }}]; renderFields(); onMutate(); }});
      E('saveBtn').addEventListener('click', saveConfig);
      E('testBtn').addEventListener('click', sendTestDm);
      const copyBtn = E('copyRoleIdBtn');
      if (copyBtn) {{ copyBtn.addEventListener('click', () => {{
        const rid = E('pingRoleId').value;
        if (rid) {{ navigator.clipboard.writeText(rid).then(() => {{ copyBtn.textContent = 'Kopiert!'; setTimeout(() => {{ copyBtn.textContent = 'Kopieren'; }}, 1500); }}); }}
      }}); }}
    }}

    fillForm();
    bindEvents();
    initAccordion();
    initSubTabs();
    initColorPicker();
    initFocusTracking();
    renderPreview();
    renderValidation((ST.preview || {{}}).validation || []);
  </script>
</body>
</html>"""

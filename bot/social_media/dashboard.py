"""
Social Media Clip Dashboard - Web Interface.

Bietet UI für:
- Clip-Übersicht
- Upload-Management (TikTok, YouTube, Instagram)
- Analytics-Dashboard
"""

import html
import asyncio
import json
import ipaddress
import logging
import os
import re
import tempfile
import uuid
from datetime import UTC, datetime
from pathlib import Path
from urllib.parse import urlencode, urlsplit, urlunsplit

from aiohttp import web
from aiohttp.web_request import FileField

from .clip_manager import ClipManager
from .layout import DEFAULT_STREAMER_LAYOUT
from .layout import LayoutValidationError
from .layout import StreamerLayout
from .layout import get_clip_effective_layout
from .layout import get_streamer_layout
from .layout import set_clip_layout_override
from .layout import upsert_streamer_layout
from .retention import mark_clip_discarded
from .rendering import (
    render_social_media_dashboard,
    render_social_media_privacy,
    render_social_media_terms,
)
from .uploaders.video_processor import VideoProcessor
from ..storage import readonly_connection, transaction

log = logging.getLogger("TwitchStreams.SocialMediaDashboard")

try:
    import magic as _magic
except Exception:  # pragma: no cover - optional native dependency
    _magic = None


_UPLOAD_MAX_BYTES = 200 * 1024 * 1024
_UPLOAD_MAX_DURATION_SECONDS = 300
_UPLOAD_CHUNK_SIZE = 1024 * 1024
_SAFE_ID_RE = re.compile(r"^[A-Za-z0-9_-]+$")


def _sanitize_log_value(value: str | None) -> str:
    """Prevent CRLF log-forging via untrusted values."""
    if value is None:
        return "<none>"
    return str(value).replace("\r", "\\r").replace("\n", "\\n")


def _dashboard_url(**params: str) -> str:
    """Build internal dashboard URL with encoded query values."""
    if not params:
        return "/social-media"
    return f"/social-media?{urlencode(params)}"


class SocialMediaDashboard:
    """Web Dashboard für Social Media Clip Management."""

    def __init__(
        self,
        clip_manager: ClipManager,
        auth_checker=None,
        auth_session_getter=None,
        auth_level_getter=None,
        oauth_ready_checker=None,
        public_base_url: str | None = None,
    ):
        """
        Args:
            clip_manager: ClipManager instance
            auth_checker: Callable that checks authentication (from parent dashboard server)
            auth_session_getter: Callable that resolves dashboard OAuth session (dict)
            auth_level_getter: Callable that returns auth level (admin/partner/localhost/none)
            oauth_ready_checker: Callable that reports whether Twitch OAuth login is usable
            public_base_url: Trusted public dashboard base URL for OAuth callbacks
        """
        self.clip_manager = clip_manager
        self.auth_checker = auth_checker
        self.auth_session_getter = auth_session_getter
        self.auth_level_getter = auth_level_getter
        self.oauth_ready_checker = oauth_ready_checker
        self.public_base_url = str(public_base_url or "").strip()

    def _require_auth(self, request: web.Request) -> None:
        """Check authentication using parent dashboard's OAuth system."""
        # If no auth_checker provided, allow (backwards compat)
        if not self.auth_checker:
            return

        # Use parent's auth checker (supports Twitch OAuth, localhost, tokens)
        if not self.auth_checker(request):
            oauth_ready_checker = self.oauth_ready_checker
            if callable(oauth_ready_checker):
                try:
                    oauth_ready = bool(oauth_ready_checker())
                except Exception:
                    oauth_ready = True
                if not oauth_ready:
                    raise web.HTTPServiceUnavailable(
                        text=(
                            "Twitch OAuth ist aktuell nicht konfiguriert oder die Redirect-URI ist ungültig. "
                            "Bitte OAuth-Einstellungen prüfen."
                        )
                    )
            raise web.HTTPFound("/twitch/auth/login?next=%2Fsocial-media")

    def _get_auth_streamer_login(self, request: web.Request) -> str | None:
        """Return Twitch login from dashboard OAuth session when available."""
        getter = self.auth_session_getter
        if not callable(getter):
            return None
        try:
            session = getter(request)
        except Exception:
            log.debug("Failed to resolve dashboard session for social-media", exc_info=True)
            return None
        if not isinstance(session, dict):
            return None
        login = str(session.get("twitch_login") or "").strip().lower()
        return login or None

    def _get_auth_level(self, request: web.Request) -> str:
        getter = self.auth_level_getter
        if not callable(getter):
            return "unknown"
        try:
            raw_level = str(getter(request) or "").strip().lower()
        except Exception:
            log.debug("Failed to resolve auth level for social-media", exc_info=True)
            return "unknown"
        if raw_level in {"localhost", "admin", "partner", "none"}:
            return raw_level
        return "unknown"

    def _require_admin(self, request: web.Request) -> None:
        self._require_auth(request)
        if self._get_auth_level(request) not in {"admin", "localhost"}:
            raise web.HTTPForbidden(text="Admin access required")

    def _get_editor_user_id(self, request: web.Request) -> str | None:
        getter = self.auth_session_getter
        if not callable(getter):
            return None
        try:
            session = getter(request)
        except Exception:
            return None
        if not isinstance(session, dict):
            return None
        for key in ("discord_user_id", "user_id", "twitch_user_id"):
            value = str(session.get(key) or "").strip()
            if value:
                return value
        return None

    @staticmethod
    def _is_loopback_host(raw_host: str | None) -> bool:
        token = str(raw_host or "").strip().lower()
        if not token:
            return False
        if token.startswith("["):
            end = token.find("]")
            if end != -1:
                token = token[1:end]
        elif token.count(":") == 1:
            host_part, port_part = token.rsplit(":", 1)
            if port_part.isdigit():
                token = host_part
        if token == "localhost":
            return True
        try:
            return ipaddress.ip_address(token).is_loopback
        except ValueError:
            return False

    def _is_localhost_request(self, request: web.Request) -> bool:
        host_header = request.headers.get("Host") or request.host or ""
        if not self._is_loopback_host(host_header):
            return False

        remote = (request.remote or "").strip() if hasattr(request, "remote") else ""
        if remote and self._is_loopback_host(remote):
            return True

        transport = getattr(request, "transport", None)
        if transport is None:
            return False
        peer = transport.get_extra_info("peername")
        if isinstance(peer, tuple) and peer and self._is_loopback_host(str(peer[0]).strip()):
            return True
        if isinstance(peer, str) and self._is_loopback_host(peer.strip()):
            return True
        return False

    @staticmethod
    def _normalize_public_origin(raw_url: str | None) -> str | None:
        value = str(raw_url or "").strip()
        if not value:
            return None
        candidate = value if "://" in value else f"https://{value}"
        try:
            parsed = urlsplit(candidate)
        except Exception:
            return None
        scheme = str(parsed.scheme or "").strip().lower()
        host = str(parsed.hostname or "").strip().lower()
        if scheme not in {"http", "https"}:
            return None
        if not host or not parsed.netloc:
            return None
        if parsed.username or parsed.password:
            return None
        if scheme == "http" and host not in {"127.0.0.1", "localhost", "::1"}:
            return None
        return urlunsplit((scheme, parsed.netloc, "", "", "")).rstrip("/")

    def _oauth_public_origin(self, request: web.Request) -> str:
        configured = self._normalize_public_origin(self.public_base_url)
        if configured:
            return configured

        env_origin = self._normalize_public_origin(
            os.getenv("SOCIAL_MEDIA_PUBLIC_URL")
            or os.getenv("TWITCH_ADMIN_PUBLIC_URL")
            or os.getenv("MASTER_DASHBOARD_PUBLIC_URL")
            or "https://admin.deutsche-deadlock-community.de"
        )

        if env_origin:
            return env_origin

        if self._is_localhost_request(request):
            try:
                request_origin = self._normalize_public_origin(str(request.url.origin()))
            except Exception:
                request_origin = None
            if request_origin:
                return request_origin
        return "https://admin.deutsche-deadlock-community.de"

    def _resolve_streamer_scope(
        self,
        request: web.Request,
        requested_streamer: str | None = None,
        *,
        required: bool = False,
    ) -> str | None:
        """Resolve effective streamer with session-based ownership enforcement."""
        requested = str(requested_streamer or "").strip().lower()
        session_streamer = self._get_auth_streamer_login(request)
        auth_level = self._get_auth_level(request)

        if session_streamer:
            if requested and requested.lower() != session_streamer:
                safe_requested = _sanitize_log_value(requested)
                safe_session = _sanitize_log_value(session_streamer)
                log.warning(
                    "Blocked cross-account social-media access: requested=%s session=%s",
                    safe_requested,
                    safe_session,
                )
                raise web.HTTPForbidden(
                    text="Du kannst nur auf deinen eigenen Twitch-Account zugreifen."
                )
            return session_streamer

        if auth_level == "partner":
            safe_requested = _sanitize_log_value(requested or "<none>")
            log.warning(
                "Blocked token-only social-media scope access without session: requested=%s",
                safe_requested,
            )
            raise web.HTTPForbidden(
                text="Partner-Token benötigt für Social-Media einen Twitch-Login mit Session."
            )

        if required and not requested:
            raise web.HTTPBadRequest(text="streamer parameter required")

        return requested or None

    @staticmethod
    def _normalize_clip_id(raw_value) -> int | None:
        """Convert user-provided clip id into positive integer."""
        try:
            clip_id = int(raw_value)
        except (TypeError, ValueError):
            return None
        return clip_id if clip_id > 0 else None

    @staticmethod
    def _normalize_safe_slug(raw_value: str | None, field_name: str) -> str:
        value = str(raw_value or "").strip()
        if not value:
            raise web.HTTPBadRequest(text=f"{field_name} is required")
        if not _SAFE_ID_RE.fullmatch(value):
            raise web.HTTPBadRequest(text=f"{field_name} must match [A-Za-z0-9_-]+")
        return value

    @staticmethod
    def _serialize_layout(layout: StreamerLayout) -> dict:
        payload = layout.to_layout_json()
        payload["cam_enabled"] = layout.cam_enabled
        payload["mode"] = layout.mode
        return payload

    def _parse_layout_request(self, payload: dict) -> StreamerLayout:
        layout_payload = payload.get("layout")
        if layout_payload is None:
            raise LayoutValidationError("layout is required")
        return StreamerLayout.from_mapping(
            layout_payload,
            cam_enabled=payload.get("cam_enabled"),
            mode=payload.get("mode"),
        )

    def _ensure_streamer_exists(self, streamer_login: str) -> bool:
        with readonly_connection() as conn:
            row = conn.execute(
                """
                SELECT 1
                  FROM twitch_streamers
                 WHERE LOWER(twitch_login) = LOWER(%s)
                 LIMIT 1
                """,
                (streamer_login,),
            ).fetchone()
        return bool(row)

    def _load_clip_row(self, clip_db_id: int) -> dict | None:
        with readonly_connection() as conn:
            row = conn.execute(
                """
                SELECT id, clip_id, clip_url, clip_title, clip_thumbnail_url, streamer_login,
                       created_at, duration_seconds, view_count, game_name, status,
                       source_kind, upload_local_path, retention_until, discarded_at,
                       layout_override_json, uploaded_tiktok, uploaded_youtube, uploaded_instagram
                  FROM twitch_clips_social_media
                 WHERE id = %s
                 LIMIT 1
                """,
                (clip_db_id,),
            ).fetchone()
        return dict(row) if row else None

    def _serialize_clip_record(self, row: dict) -> dict:
        layout_override = row.get("layout_override_json")
        layout_override_payload = None
        if layout_override:
            if isinstance(layout_override, dict):
                layout_override_payload = layout_override
            else:
                layout_override_payload = json.loads(layout_override)
        effective_layout = get_clip_effective_layout(int(row["id"]))
        return {
            "clip_db_id": row["id"],
            "clip_id": row.get("clip_id"),
            "clip_url": row.get("clip_url"),
            "title": row.get("clip_title"),
            "thumbnail_url": row.get("clip_thumbnail_url"),
            "streamer_login": row.get("streamer_login"),
            "created_at": row.get("created_at"),
            "duration_seconds": row.get("duration_seconds"),
            "view_count": row.get("view_count"),
            "game_name": row.get("game_name"),
            "status": row.get("status"),
            "source_kind": row.get("source_kind", "twitch"),
            "upload_local_path": row.get("upload_local_path"),
            "retention_until": row.get("retention_until"),
            "discarded_at": row.get("discarded_at"),
            "platform_status": {
                "tiktok": bool(row.get("uploaded_tiktok")),
                "youtube": bool(row.get("uploaded_youtube")),
                "instagram": bool(row.get("uploaded_instagram")),
            },
            "layout_override": layout_override_payload,
            "effective_layout": self._serialize_layout(effective_layout),
        }

    async def _store_uploaded_mp4(self, file_field: FileField, target_path: Path) -> Path:
        target_path.parent.mkdir(parents=True, exist_ok=True)
        fd, tmp_path_raw = tempfile.mkstemp(
            prefix=f"{target_path.stem}-",
            suffix=".tmp",
            dir=str(target_path.parent),
        )
        size = 0
        tmp_path = Path(tmp_path_raw)
        try:
            with os.fdopen(fd, "wb") as handle:
                while True:
                    chunk = file_field.file.read(_UPLOAD_CHUNK_SIZE)
                    if not chunk:
                        break
                    size += len(chunk)
                    if size > _UPLOAD_MAX_BYTES:
                        raise web.HTTPRequestEntityTooLarge(
                            max_size=_UPLOAD_MAX_BYTES,
                            actual_size=size,
                        )
                    handle.write(chunk)
            return tmp_path
        except Exception:
            tmp_path.unlink(missing_ok=True)
            raise

    async def _validate_uploaded_mp4(self, temp_path: Path) -> float:
        mime_type = None
        if _magic is not None:
            try:
                mime_type = _magic.from_file(str(temp_path), mime=True)
            except Exception:
                mime_type = None
        if mime_type and mime_type not in {"video/mp4", "application/mp4"}:
            raise web.HTTPUnsupportedMediaType(text="Only MP4 uploads are supported")

        with temp_path.open("rb") as handle:
            header = handle.read(64)
        if b"ftyp" not in header:
            raise web.HTTPUnsupportedMediaType(text="Only MP4 uploads are supported")

        processor = VideoProcessor()
        try:
            video_info = await processor.get_video_info(str(temp_path))
        except Exception as exc:
            raise web.HTTPUnsupportedMediaType(text="Uploaded file is not a valid MP4 video") from exc
        duration = float(video_info.get("duration") or 0)
        if duration <= 0:
            raise web.HTTPBadRequest(text="Uploaded MP4 must have a positive duration")
        if duration > _UPLOAD_MAX_DURATION_SECONDS:
            raise web.HTTPBadRequest(text="Uploaded MP4 must be 300 seconds or shorter")
        return duration

    def _clip_owned_by_streamer(self, clip_id: int, streamer_login: str) -> bool:
        with readonly_connection() as conn:
            row = conn.execute(
                """
                SELECT 1
                FROM twitch_clips_social_media
                WHERE id = %s AND LOWER(streamer_login) = LOWER(%s)
                LIMIT 1
                """,
                (clip_id, streamer_login),
            ).fetchone()
        return bool(row)

    def _streamer_template_owned_by_streamer(self, template_id: int, streamer_login: str) -> bool:
        with readonly_connection() as conn:
            row = conn.execute(
                """
                SELECT 1
                FROM clip_templates_streamer
                WHERE id = %s AND LOWER(streamer_login) = LOWER(%s)
                LIMIT 1
                """,
                (template_id, streamer_login),
            ).fetchone()
        return bool(row)

    def _build_app(self) -> web.Application:
        """Build aiohttp app with routes."""
        app = web.Application()

        # HTML Pages
        app.router.add_get("/social-media", self.index)
        app.router.add_get("/terms", self.page_terms)
        app.router.add_get("/privacy", self.page_privacy)

        # API Endpoints
        app.router.add_get("/social-media/api/stats", self.api_stats)
        app.router.add_get("/social-media/api/clips", self.clips_list)
        app.router.add_post("/social-media/api/clips/upload", self.api_upload_clip)
        app.router.add_post("/social-media/api/upload", self.queue_upload)
        app.router.add_get("/social-media/api/analytics", self.analytics)
        app.router.add_get(
            "/social-media/api/admin/streamer-layout",
            self.api_admin_streamer_layout_get,
        )
        app.router.add_put(
            "/social-media/api/admin/streamer-layout",
            self.api_admin_streamer_layout_put,
        )
        app.router.add_get("/social-media/api/admin/clips", self.api_admin_clips)
        app.router.add_get("/social-media/api/admin/clips/{clip_db_id}", self.api_admin_clip_detail)
        app.router.add_put(
            "/social-media/api/admin/clips/{clip_db_id}/layout",
            self.api_admin_clip_layout_put,
        )
        app.router.add_post(
            "/social-media/api/admin/clips/{clip_db_id}/discard",
            self.api_admin_clip_discard,
        )

        # Template Management Endpoints
        app.router.add_get("/social-media/api/templates/global", self.api_templates_global)
        app.router.add_get("/social-media/api/templates/streamer", self.api_templates_streamer)
        app.router.add_post("/social-media/api/templates/streamer", self.api_create_template)
        app.router.add_post("/social-media/api/templates/apply", self.api_apply_template)

        # Batch Operations Endpoints
        app.router.add_post("/social-media/api/batch-upload", self.api_batch_upload)
        app.router.add_post("/social-media/api/mark-uploaded", self.api_mark_uploaded)

        # Clip Fetching Endpoints
        app.router.add_post("/social-media/api/fetch-clips", self.api_fetch_clips)
        app.router.add_get("/social-media/api/last-hashtags", self.api_last_hashtags)

        # OAuth & Platform Management Endpoints
        app.router.add_get("/social-media/oauth/start/{platform}", self.oauth_start)
        app.router.add_get("/social-media/oauth/callback", self.oauth_callback)
        app.router.add_get("/social-media/oauth/callback/{platform}", self.oauth_callback)
        app.router.add_post("/social-media/oauth/disconnect/{platform}", self.oauth_disconnect)
        app.router.add_get("/social-media/api/platforms/status", self.api_platforms_status)

        return app

    async def page_terms(self, request: web.Request) -> web.Response:
        """Public Terms of Service page (required for TikTok / platform OAuth apps)."""
        return web.Response(
            text=render_social_media_terms(),
            content_type="text/html",
            charset="utf-8",
        )

    async def page_privacy(self, request: web.Request) -> web.Response:
        """Public Privacy Policy page (required for TikTok / platform OAuth apps)."""
        return web.Response(
            text=render_social_media_privacy(),
            content_type="text/html",
            charset="utf-8",
        )

    async def index(self, request: web.Request) -> web.Response:
        """Main dashboard page with full template & batch upload UI."""
        self._require_auth(request)
        authenticated_streamer = self._resolve_streamer_scope(request)
        safe_streamer_label = html.escape(
            f"@{authenticated_streamer}" if authenticated_streamer else "nicht gesetzt"
        )
        safe_streamer_data = html.escape(authenticated_streamer or "", quote=True)

        return web.Response(
            text=render_social_media_dashboard(
                safe_streamer_label=safe_streamer_label,
                safe_streamer_data=safe_streamer_data,
            ),
            content_type="text/html",
            charset="utf-8",
        )

    async def api_stats(self, request: web.Request) -> web.Response:
        """Stats API endpoint for dashboard."""
        self._require_auth(request)

        streamer = self._resolve_streamer_scope(request, request.query.get("streamer"))
        summary = self.clip_manager.get_analytics_summary(streamer_login=streamer)

        return web.json_response(summary)

    async def clips_list(self, request: web.Request) -> web.Response:
        """Clips list API endpoint."""
        self._require_auth(request)

        try:
            limit = int(request.query.get("limit", "50"))
        except (TypeError, ValueError):
            return web.json_response(
                {
                    "error": "invalid_limit",
                    "allowed_range": [1, 200],
                },
                status=400,
            )
        if limit < 1 or limit > 200:
            return web.json_response(
                {
                    "error": "invalid_limit",
                    "allowed_range": [1, 200],
                },
                status=400,
            )

        streamer = self._resolve_streamer_scope(request, request.query.get("streamer"))
        status = request.query.get("status")

        clips = self.clip_manager.get_clips_for_dashboard(
            streamer_login=streamer,
            status=status,
            limit=limit,
        )

        return web.json_response(clips)

    async def queue_upload(self, request: web.Request) -> web.Response:
        """Queue upload API endpoint."""
        self._require_auth(request)

        data = await request.json()
        clip_id = self._normalize_clip_id(data.get("clip_id"))
        platforms = data.get("platforms", [])  # ['tiktok', 'youtube', 'instagram'] or 'all'

        if not clip_id:
            return web.json_response({"error": "clip_id required"}, status=400)

        streamer = self._resolve_streamer_scope(
            request,
            data.get("streamer") or request.query.get("streamer"),
        )
        if streamer and not self._clip_owned_by_streamer(clip_id, streamer):
            return web.json_response(
                {"error": "forbidden: clip does not belong to authenticated streamer"},
                status=403,
            )

        if platforms == "all":
            platforms = ["tiktok", "youtube", "instagram"]

        queued = []
        for platform in platforms:
            try:
                queue_id = self.clip_manager.queue_upload(
                    clip_db_id=clip_id,
                    platform=platform,
                    title=data.get("title"),
                    description=data.get("description"),
                    hashtags=data.get("hashtags"),
                    priority=data.get("priority", 0),
                )
                queued.append({"platform": platform, "queue_id": queue_id})
            except Exception:
                safe_platform = _sanitize_log_value(platform)
                log.exception("Failed to queue upload for platform=%s", safe_platform)
                queued.append({"platform": platform, "error": "queue_failed"})

        return web.json_response({"queued": queued})

    async def analytics(self, request: web.Request) -> web.Response:
        """Analytics dashboard."""
        self._require_auth(request)

        streamer = self._resolve_streamer_scope(request, request.query.get("streamer"))
        summary = self.clip_manager.get_analytics_summary(streamer_login=streamer)

        return web.json_response(summary)

    async def api_admin_streamer_layout_get(self, request: web.Request) -> web.Response:
        self._require_admin(request)
        streamer_login = self._normalize_safe_slug(
            request.query.get("streamer_login"),
            "streamer_login",
        ).lower()
        if not self._ensure_streamer_exists(streamer_login):
            return web.json_response({"error": "unknown_streamer"}, status=404)
        layout = get_streamer_layout(streamer_login) or DEFAULT_STREAMER_LAYOUT
        stored_layout = get_streamer_layout(streamer_login)

        updated_at = None
        updated_by = None
        if stored_layout:
            with readonly_connection() as conn:
                row = conn.execute(
                    """
                    SELECT updated_at, updated_by
                      FROM social_media_streamer_layout
                     WHERE LOWER(streamer_login) = LOWER(%s)
                     LIMIT 1
                    """,
                    (streamer_login,),
                ).fetchone()
            if row:
                updated_at = row["updated_at"] if hasattr(row, "keys") else row[0]
                updated_by = row["updated_by"] if hasattr(row, "keys") else row[1]

        return web.json_response(
            {
                "streamer_login": streamer_login,
                "layout": self._serialize_layout(layout),
                "cam_enabled": layout.cam_enabled,
                "mode": layout.mode,
                "is_default": stored_layout is None,
                "updated_at": updated_at,
                "updated_by": updated_by,
            }
        )

    async def api_admin_streamer_layout_put(self, request: web.Request) -> web.Response:
        self._require_admin(request)
        try:
            payload = await request.json()
            streamer_login = self._normalize_safe_slug(payload.get("streamer_login"), "streamer_login").lower()
            if not self._ensure_streamer_exists(streamer_login):
                return web.json_response({"error": "unknown_streamer"}, status=404)
            layout = self._parse_layout_request(payload)
            upsert_streamer_layout(
                streamer_login,
                layout,
                updated_by=self._get_editor_user_id(request),
            )
            return web.json_response(
                {
                    "streamer_login": streamer_login,
                    "layout": self._serialize_layout(layout),
                    "cam_enabled": layout.cam_enabled,
                    "mode": layout.mode,
                }
            )
        except LayoutValidationError as exc:
            return web.json_response({"error": "invalid_layout", "message": str(exc)}, status=400)

    async def api_admin_clips(self, request: web.Request) -> web.Response:
        self._require_admin(request)
        try:
            page = max(1, int(request.query.get("page", "1")))
            page_size = min(100, max(1, int(request.query.get("page_size", "20"))))
        except (TypeError, ValueError):
            return web.json_response({"error": "invalid_pagination"}, status=400)

        status = str(request.query.get("status") or "").strip().lower() or None
        streamer = str(request.query.get("streamer") or "").strip().lower() or None
        offset = (page - 1) * page_size

        where_clauses = ["1=1"]
        params: list[object] = []
        if streamer:
            where_clauses.append("LOWER(streamer_login) = LOWER(%s)")
            params.append(streamer)
        if status:
            if status == "discarded":
                where_clauses.append("discarded_at IS NOT NULL")
            else:
                where_clauses.append("LOWER(status) = LOWER(%s)")
                params.append(status)

        where_sql = " AND ".join(where_clauses)
        with readonly_connection() as conn:
            total_row = conn.execute(
                f"SELECT COUNT(*) AS total FROM twitch_clips_social_media WHERE {where_sql}",
                tuple(params),
            ).fetchone()
            rows = conn.execute(
                f"""
                SELECT id, clip_id, clip_url, clip_title, clip_thumbnail_url, streamer_login,
                       created_at, duration_seconds, view_count, game_name, status, source_kind,
                       upload_local_path, retention_until, discarded_at, layout_override_json,
                       uploaded_tiktok, uploaded_youtube, uploaded_instagram
                  FROM twitch_clips_social_media
                 WHERE {where_sql}
                 ORDER BY created_at DESC, id DESC
                 LIMIT %s OFFSET %s
                """,
                tuple([*params, page_size, offset]),
            ).fetchall()

        total = total_row["total"] if hasattr(total_row, "keys") else total_row[0]
        return web.json_response(
            {
                "items": [self._serialize_clip_record(dict(row)) for row in rows],
                "page": page,
                "page_size": page_size,
                "total": total,
            }
        )

    async def api_admin_clip_detail(self, request: web.Request) -> web.Response:
        self._require_admin(request)
        clip_db_id = self._normalize_clip_id(request.match_info.get("clip_db_id"))
        if not clip_db_id:
            return web.json_response({"error": "invalid_clip_db_id"}, status=400)
        row = self._load_clip_row(clip_db_id)
        if not row:
            return web.json_response({"error": "clip_not_found"}, status=404)
        return web.json_response(self._serialize_clip_record(row))

    async def api_admin_clip_layout_put(self, request: web.Request) -> web.Response:
        self._require_admin(request)
        clip_db_id = self._normalize_clip_id(request.match_info.get("clip_db_id"))
        if not clip_db_id:
            return web.json_response({"error": "invalid_clip_db_id"}, status=400)
        row = self._load_clip_row(clip_db_id)
        if not row:
            return web.json_response({"error": "clip_not_found"}, status=404)

        try:
            payload = await request.json()
            layout_payload = payload.get("layout")
            if layout_payload is None:
                set_clip_layout_override(clip_db_id, None)
                effective_layout = get_clip_effective_layout(clip_db_id)
                return web.json_response(
                    {
                        "clip_db_id": clip_db_id,
                        "layout_override": None,
                        "effective_layout": self._serialize_layout(effective_layout),
                    }
                )

            layout = StreamerLayout.from_mapping(layout_payload)
            set_clip_layout_override(clip_db_id, layout)
            return web.json_response(
                {
                    "clip_db_id": clip_db_id,
                    "layout_override": layout.to_override_json(),
                    "effective_layout": self._serialize_layout(get_clip_effective_layout(clip_db_id)),
                }
            )
        except LayoutValidationError as exc:
            return web.json_response({"error": "invalid_layout", "message": str(exc)}, status=400)

    async def api_admin_clip_discard(self, request: web.Request) -> web.Response:
        self._require_admin(request)
        clip_db_id = self._normalize_clip_id(request.match_info.get("clip_db_id"))
        if not clip_db_id:
            return web.json_response({"error": "invalid_clip_db_id"}, status=400)
        if not mark_clip_discarded(clip_db_id):
            return web.json_response({"error": "clip_not_found"}, status=404)
        row = self._load_clip_row(clip_db_id)
        if not row:
            return web.json_response({"clip_db_id": clip_db_id, "discarded": True})
        return web.json_response(self._serialize_clip_record(row))

    async def api_upload_clip(self, request: web.Request) -> web.Response:
        self._require_admin(request)
        post_data = await request.post()
        file_field = post_data.get("file")
        if not isinstance(file_field, FileField):
            return web.json_response({"error": "file is required"}, status=400)

        try:
            streamer_login = self._normalize_safe_slug(
                post_data.get("streamer_login"),
                "streamer_login",
            ).lower()
        except web.HTTPBadRequest as exc:
            return web.json_response({"error": "invalid_streamer_login", "message": exc.text}, status=400)

        if not self._ensure_streamer_exists(streamer_login):
            return web.json_response({"error": "unknown_streamer"}, status=404)

        raw_clip_id = post_data.get("clip_id")
        clip_id = (
            self._normalize_safe_slug(raw_clip_id, "clip_id")
            if raw_clip_id
            else uuid.uuid4().hex
        )
        title = str(post_data.get("title") or "").strip() or None
        upload_dir = Path("data/clips/uploads") / streamer_login
        final_path = upload_dir / f"{clip_id}.mp4"
        if final_path.exists():
            return web.json_response({"error": "duplicate_clip_id"}, status=409)

        temp_path = await self._store_uploaded_mp4(file_field, final_path)
        try:
            duration_seconds = await self._validate_uploaded_mp4(temp_path)
            os.replace(temp_path, final_path)
            clip_db_id, retention_until = self.clip_manager.register_manual_upload(
                clip_id=clip_id,
                streamer_login=streamer_login,
                title=title,
                local_path=str(final_path),
                duration_seconds=duration_seconds,
            )
        except ValueError as exc:
            temp_path.unlink(missing_ok=True)
            final_path.unlink(missing_ok=True)
            return web.json_response({"error": "duplicate_clip_id", "message": str(exc)}, status=409)
        except LookupError:
            temp_path.unlink(missing_ok=True)
            final_path.unlink(missing_ok=True)
            return web.json_response({"error": "unknown_streamer"}, status=404)
        except web.HTTPException:
            temp_path.unlink(missing_ok=True)
            raise
        except Exception:
            temp_path.unlink(missing_ok=True)
            final_path.unlink(missing_ok=True)
            log.exception("Failed to store uploaded clip")
            return web.json_response({"error": "upload_failed"}, status=500)

        return web.json_response(
            {
                "clip_db_id": clip_db_id,
                "clip_id": clip_id,
                "retention_until": retention_until,
            },
            status=201,
        )

    # ========== Template Management API ==========

    async def api_templates_global(self, request: web.Request) -> web.Response:
        """GET /api/templates/global - Get global templates."""
        self._require_auth(request)

        category = request.query.get("category")
        templates = self.clip_manager.get_global_templates(category=category)

        return web.json_response({"templates": templates})

    async def api_templates_streamer(self, request: web.Request) -> web.Response:
        """GET /api/templates/streamer - Get streamer templates."""
        self._require_auth(request)

        streamer = self._resolve_streamer_scope(
            request,
            request.query.get("streamer"),
            required=False,
        )

        templates = self.clip_manager.get_streamer_templates(streamer_login=streamer)

        return web.json_response({"templates": templates})

    async def api_create_template(self, request: web.Request) -> web.Response:
        """POST /api/templates/streamer - Create/Update streamer template."""
        self._require_auth(request)

        try:
            data = await request.json()

            streamer = self._resolve_streamer_scope(
                request,
                data.get("streamer"),
                required=True,
            )
            template_name = data.get("template_name")
            description = data.get("description")
            hashtags = data.get("hashtags", [])
            is_default = data.get("is_default", False)

            if not all([template_name, description]):
                return web.json_response(
                    {"error": "template_name and description are required"}, status=400
                )

            template_id = self.clip_manager.create_streamer_template(
                streamer_login=streamer,
                template_name=template_name,
                description_template=description,
                hashtags=hashtags,
                is_default=is_default,
            )

            return web.json_response(
                {
                    "success": True,
                    "template_id": template_id,
                    "message": "Template created/updated successfully",
                }
            )

        except web.HTTPException:
            raise
        except Exception:
            log.exception("Failed to create template")
            return web.json_response({"error": "template_create_failed"}, status=500)

    async def api_apply_template(self, request: web.Request) -> web.Response:
        """POST /api/templates/apply - Apply template to clip."""
        self._require_auth(request)

        try:
            data = await request.json()

            clip_id = self._normalize_clip_id(data.get("clip_id"))
            template_id = self._normalize_clip_id(data.get("template_id"))
            is_global = data.get("is_global", False)

            if not clip_id or not template_id:
                return web.json_response(
                    {"error": "clip_id and template_id are required"}, status=400
                )

            streamer = self._resolve_streamer_scope(
                request,
                data.get("streamer") or request.query.get("streamer"),
            )
            if streamer and not self._clip_owned_by_streamer(clip_id, streamer):
                return web.json_response(
                    {"error": "forbidden: clip does not belong to authenticated streamer"},
                    status=403,
                )
            if (
                streamer
                and not is_global
                and not self._streamer_template_owned_by_streamer(template_id, streamer)
            ):
                return web.json_response(
                    {"error": "forbidden: template does not belong to authenticated streamer"},
                    status=403,
                )

            success = self.clip_manager.apply_template_to_clip(
                clip_id=clip_id,
                template_id=template_id,
                is_global=is_global,
            )

            if success:
                return web.json_response(
                    {"success": True, "message": "Template applied successfully"}
                )
            else:
                return web.json_response({"error": "Failed to apply template"}, status=500)

        except web.HTTPException:
            raise
        except Exception:
            log.exception("Failed to apply template")
            return web.json_response({"error": "template_apply_failed"}, status=500)

    # ========== Batch Operations API ==========

    async def api_batch_upload(self, request: web.Request) -> web.Response:
        """POST /api/batch-upload - Batch upload all new clips."""
        self._require_auth(request)

        try:
            data = await request.json()

            streamer = self._resolve_streamer_scope(
                request,
                data.get("streamer"),
                required=True,
            )
            platforms = data.get("platforms", [])
            apply_default_template = data.get("apply_default_template", True)

            if not platforms:
                return web.json_response({"error": "platforms are required"}, status=400)

            stats = await self.clip_manager.batch_upload_all_new(
                streamer_login=streamer,
                platforms=platforms,
                apply_default_template=apply_default_template,
            )

            return web.json_response(
                {
                    "success": True,
                    "stats": stats,
                    "message": f"Queued {stats['queued']} clips, {stats['errors']} errors",
                }
            )

        except web.HTTPException:
            raise
        except Exception:
            log.exception("Failed to batch upload")
            return web.json_response({"error": "batch_upload_failed"}, status=500)

    async def api_mark_uploaded(self, request: web.Request) -> web.Response:
        """POST /api/mark-uploaded - Manually mark clip as uploaded."""
        self._require_auth(request)

        try:
            data = await request.json()

            clip_id = self._normalize_clip_id(data.get("clip_id"))
            platforms = data.get("platforms", [])

            if not clip_id or not platforms:
                return web.json_response(
                    {"error": "clip_id and platforms are required"}, status=400
                )

            streamer = self._resolve_streamer_scope(
                request,
                data.get("streamer") or request.query.get("streamer"),
            )
            if streamer and not self._clip_owned_by_streamer(clip_id, streamer):
                return web.json_response(
                    {"error": "forbidden: clip does not belong to authenticated streamer"},
                    status=403,
                )

            success = self.clip_manager.mark_clip_uploaded(
                clip_id=clip_id,
                platforms=platforms,
                manual=True,
            )

            if success:
                return web.json_response({"success": True, "message": "Clip marked as uploaded"})
            else:
                return web.json_response({"error": "Failed to mark clip as uploaded"}, status=500)

        except web.HTTPException:
            raise
        except Exception:
            log.exception("Failed to mark clip as uploaded")
            return web.json_response({"error": "mark_uploaded_failed"}, status=500)

    # ========== Clip Fetching API ==========

    async def api_fetch_clips(self, request: web.Request) -> web.Response:
        """POST /api/fetch-clips - Manually fetch clips for streamer."""
        self._require_auth(request)

        try:
            data = await request.json()

            streamer = self._resolve_streamer_scope(
                request,
                data.get("streamer"),
                required=True,
            )
            limit = data.get("limit", 20)
            days = data.get("days", 7)

            clips = await self.clip_manager.fetch_recent_clips(
                streamer_login=streamer,
                limit=limit,
                days=days,
            )

            return web.json_response(
                {
                    "success": True,
                    "clips_found": len(clips),
                    "message": f"Fetched {len(clips)} clips",
                }
            )

        except web.HTTPException:
            raise
        except ValueError as exc:
            log.warning("Manual clip fetch unavailable: %s", exc)
            return web.json_response(
                {
                    "error": "twitch_api_unavailable",
                    "message": "Clip-Fetch ist derzeit nicht verfügbar.",
                },
                status=503,
            )
        except Exception:
            log.exception("Failed to fetch clips")
            return web.json_response({"error": "fetch_clips_failed"}, status=500)

    async def api_last_hashtags(self, request: web.Request) -> web.Response:
        """GET /api/last-hashtags - Get last used hashtags."""
        self._require_auth(request)

        streamer = self._resolve_streamer_scope(
            request,
            request.query.get("streamer"),
            required=False,
        )

        hashtags = self.clip_manager.get_last_hashtags(streamer_login=streamer)

        return web.json_response({"hashtags": hashtags})

    # ========== OAuth & Platform Management ==========

    async def oauth_start(self, request: web.Request) -> web.Response:
        """Start OAuth flow for a platform."""
        self._require_auth(request)

        platform = request.match_info["platform"]
        streamer = self._resolve_streamer_scope(
            request,
            request.query.get("streamer"),
            required=False,
        )

        if platform not in ["tiktok", "youtube", "instagram"]:
            return web.Response(text="Invalid platform", status=400)

        from .oauth_manager import SocialMediaOAuthManager

        oauth_mgr = SocialMediaOAuthManager()

        try:
            redirect_uri = (
                f"{self._oauth_public_origin(request)}/social-media/oauth/callback/{platform}"
            )
            auth_url = oauth_mgr.generate_auth_url(platform, streamer, redirect_uri)

            return web.HTTPFound(auth_url)
        except Exception:
            log.exception("OAuth start failed")
            return web.HTTPFound(_dashboard_url(oauth_error="oauth_start_failed"))

    async def oauth_callback(self, request: web.Request) -> web.Response:
        """Handle OAuth callback from platform."""
        code = request.query.get("code")
        state = request.query.get("state")
        error = request.query.get("error")

        if error:
            safe_error = _sanitize_log_value(error)
            log.error("OAuth provider returned an error: %s", safe_error)
            return web.HTTPFound(_dashboard_url(oauth_error="provider_error"))

        if not code or not state:
            return web.Response(text="Missing code or state", status=400)

        from .oauth_manager import SocialMediaOAuthManager
        from .oauth_manager import OAuthStateValidationError
        from .oauth_manager import OAuthTokenExchangeError

        oauth_mgr = SocialMediaOAuthManager()
        expected_platform = str(request.match_info.get("platform") or "").strip().lower() or None
        callback_redirect_uri = (
            f"{self._oauth_public_origin(request)}{request.path}"
            if expected_platform
            else None
        )

        try:
            result = await oauth_mgr.handle_callback(
                code,
                state,
                expected_platform=expected_platform,
                expected_redirect_uri=callback_redirect_uri,
            )

            # Redirect back to dashboard with success message
            platform = result.get("platform", "unknown")
            if platform not in {"tiktok", "youtube", "instagram"}:
                platform = "unknown"
            return web.HTTPFound(_dashboard_url(oauth_success=platform))

        except OAuthStateValidationError:
            log.warning("OAuth callback validation failed")
            return web.HTTPFound(_dashboard_url(oauth_error="invalid_callback"))
        except OAuthTokenExchangeError:
            log.warning("OAuth token exchange failed", exc_info=True)
            return web.HTTPFound(_dashboard_url(oauth_error="token_exchange_failed"))
        except Exception:
            log.exception("OAuth callback failed")
            return web.HTTPFound(_dashboard_url(oauth_error="callback_failed"))

    async def oauth_disconnect(self, request: web.Request) -> web.Response:
        """Disconnect a platform."""
        self._require_auth(request)

        platform = request.match_info["platform"]
        streamer = self._resolve_streamer_scope(
            request,
            request.query.get("streamer"),
            required=False,
        )

        if platform not in ["tiktok", "youtube", "instagram"]:
            return web.json_response({"error": "Invalid platform"}, status=400)

        try:
            await asyncio.to_thread(self._disconnect_platform_sync, platform, streamer)

            safe_platform = _sanitize_log_value(platform)
            safe_streamer = _sanitize_log_value(streamer)
            log.info("Disconnected platform=%s, streamer=%s", safe_platform, safe_streamer)
            return web.json_response({"success": True})

        except Exception:
            log.exception("Failed to disconnect platform")
            return web.json_response({"error": "disconnect_failed"}, status=500)

    def _disconnect_platform_sync(self, platform: str, streamer: str | None) -> None:
        with transaction() as conn:
            conn.execute(
                """
                UPDATE social_media_platform_auth
                SET enabled = 0
                WHERE platform = %s
                  AND (streamer_login = %s OR (streamer_login IS NULL AND %s IS NULL))
                """,
                (platform, streamer, streamer),
            )

    async def api_platforms_status(self, request: web.Request) -> web.Response:
        """GET platform connection status."""
        self._require_auth(request)

        streamer = self._resolve_streamer_scope(
            request,
            request.query.get("streamer"),
            required=False,
        )

        from .credential_manager import SocialMediaCredentialManager

        cred_mgr = SocialMediaCredentialManager()

        try:
            platforms_status = await asyncio.to_thread(
                cred_mgr.get_all_platforms_status,
                streamer,
            )

            platforms = []
            for platform_name, status in platforms_status.items():
                platforms.append(
                    {
                        "platform": platform_name,
                        "connected": status["connected"],
                        "username": (
                            None
                            if streamer and status.get("uses_global_fallback")
                            else status.get("username")
                        ),
                        "user_id": (
                            None
                            if streamer and status.get("uses_global_fallback")
                            else status.get("user_id")
                        ),
                        "expires_at": status.get("expires_at"),
                        "expired": status.get("expired", False),
                        "uses_global_fallback": status.get("uses_global_fallback", False),
                    }
                )

            return web.json_response({"platforms": platforms})

        except Exception:
            log.exception("Failed to get platform status")
            return web.json_response({"error": "platform_status_failed"}, status=500)


def create_social_media_app(
    clip_manager: ClipManager,
    auth_checker=None,
    auth_session_getter=None,
    auth_level_getter=None,
    oauth_ready_checker=None,
    public_base_url: str | None = None,
) -> web.Application:
    """
    Create Social Media Dashboard aiohttp app.

    Args:
        clip_manager: ClipManager instance
        auth_checker: Callable that checks authentication (from parent dashboard server)
        auth_session_getter: Callable that resolves dashboard OAuth session
        auth_level_getter: Callable that resolves auth level for token/session enforcement
        oauth_ready_checker: Callable that reports whether Twitch OAuth login is usable
        public_base_url: Trusted public dashboard base URL for OAuth callback construction

    Returns:
        aiohttp Application
    """
    dashboard = SocialMediaDashboard(
        clip_manager,
        auth_checker=auth_checker,
        auth_session_getter=auth_session_getter,
        auth_level_getter=auth_level_getter,
        oauth_ready_checker=oauth_ready_checker,
        public_base_url=public_base_url,
    )
    return dashboard._build_app()

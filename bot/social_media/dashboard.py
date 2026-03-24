"""
Social Media Clip Dashboard - Web Interface.

Bietet UI für:
- Clip-Übersicht
- Upload-Management (TikTok, YouTube, Instagram)
- Analytics-Dashboard
"""

import html
import asyncio
import ipaddress
import logging
import os
from urllib.parse import urlencode, urlsplit, urlunsplit

from aiohttp import web

from .clip_manager import ClipManager
from .rendering import (
    render_social_media_dashboard,
    render_social_media_privacy,
    render_social_media_terms,
)
from ..storage import readonly_connection, transaction

log = logging.getLogger("TwitchStreams.SocialMediaDashboard")


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
            or "https://admin.earlysalty.de"
        )
        if self._is_localhost_request(request):
            try:
                request_origin = self._normalize_public_origin(str(request.url.origin()))
            except Exception:
                request_origin = None
            if request_origin:
                return request_origin

        if env_origin:
            return env_origin
        return "https://admin.earlysalty.de"

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
        app.router.add_post("/social-media/api/upload", self.queue_upload)
        app.router.add_get("/social-media/api/analytics", self.analytics)

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
            redirect_uri = f"{self._oauth_public_origin(request)}/social-media/oauth/callback"
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

        oauth_mgr = SocialMediaOAuthManager()

        try:
            result = await oauth_mgr.handle_callback(code, state)

            # Redirect back to dashboard with success message
            platform = result.get("platform", "unknown")
            if platform not in {"tiktok", "youtube", "instagram"}:
                platform = "unknown"
            return web.HTTPFound(_dashboard_url(oauth_success=platform))

        except ValueError:
            log.warning("OAuth callback validation failed")
            return web.HTTPFound(_dashboard_url(oauth_error="invalid_callback"))
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

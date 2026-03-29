"""Raid mixin for DashboardV2Server — raid OAuth, history and analytics routes."""

from __future__ import annotations

import html
import os
from typing import Any
from urllib.parse import urlsplit, urlunsplit

import discord
from aiohttp import web

from ... import storage
from ...core.constants import log
from ...raid.scope_profiles import BASE_SCOPE_PROFILE, normalize_scope_profile
from ...raid.views import RaidAuthGenerateView, build_raid_requirements_embed
from .oauth_callback import build_raid_oauth_callback_payload
from .pages import (
    build_raid_analytics_page,
    build_raid_auth_start_html,
    build_raid_history_page,
    build_raid_history_rows,
)

DEFAULT_RAID_OAUTH_SUCCESS_REDIRECT_URL = "https://twitch.earlysalty.com/twitch/dashboard"
PUBLIC_STREAMER_ONBOARDING_URL = "https://twitch.earlysalty.com/twitch/onboarding"
PUBLIC_STREAMER_ONBOARDING_LOGIN = "public:website_onboarding"


class _DashboardRaidMixin:
    """Raid authorization, history, analytics and OAuth callback routes."""

    # ------------------------------------------------------------------ #
    # HTML builders                                                        #
    # ------------------------------------------------------------------ #

    @staticmethod
    def _build_raid_auth_start_html(login: str, auth_url: str) -> str:
        return build_raid_auth_start_html(login, auth_url)

    @staticmethod
    def _build_raid_history_rows(history: list[dict]) -> str:
        return build_raid_history_rows(history)

    @staticmethod
    def _build_raid_history_page(rows_html: str) -> str:
        return build_raid_history_page(rows_html)

    @staticmethod
    def _build_raid_analytics_page(
        *,
        partner_stats: list,
        leechers: list,
        manual_list: list,
        date_min: str,
        date_max: str,
        total: int,
    ) -> str:
        return build_raid_analytics_page(
            partner_stats=partner_stats,
            leechers=leechers,
            manual_list=manual_list,
            date_min=date_min,
            date_max=date_max,
            total=total,
        )

    @staticmethod
    def _raid_oauth_success_redirect_url(candidate: str | None = None) -> str:
        configured = (candidate or "").strip()
        if not configured:
            configured = (os.getenv("TWITCH_RAID_SUCCESS_REDIRECT_URL") or "").strip()
        candidate = configured or DEFAULT_RAID_OAUTH_SUCCESS_REDIRECT_URL
        fallback = DEFAULT_RAID_OAUTH_SUCCESS_REDIRECT_URL

        try:
            parts = urlsplit(candidate)
        except Exception:
            return fallback

        if parts.username or parts.password:
            return fallback

        scheme = (parts.scheme or "").strip().lower()
        host = (parts.hostname or "").strip().lower()
        if scheme not in {"https", "http"}:
            return fallback
        if not host:
            return fallback
        if scheme == "http" and host not in {"127.0.0.1", "localhost", "::1"}:
            return fallback

        path = parts.path or "/"
        return urlunsplit((scheme, parts.netloc, path, parts.query, ""))

    # ------------------------------------------------------------------ #
    # Raid routes                                                          #
    # ------------------------------------------------------------------ #

    def _raid_dashboard_auth_context(self, request: web.Request) -> tuple[str, bool, str]:
        auth_level = ""
        auth_level_getter = getattr(self, "_get_auth_level", None)
        if callable(auth_level_getter):
            try:
                auth_level = str(auth_level_getter(request) or "").strip().lower()
            except Exception:
                auth_level = ""
        is_admin = auth_level in {"admin", "localhost"}

        session_login = ""
        session_getter = getattr(self, "_get_dashboard_auth_session", None)
        if callable(session_getter):
            try:
                dashboard_session = session_getter(request)
            except Exception:
                dashboard_session = None
            if isinstance(dashboard_session, dict):
                session_login = str(dashboard_session.get("twitch_login") or "").strip().lower()
        return auth_level, is_admin, session_login

    @staticmethod
    def _raid_active_partner_login(row: Any, fallback: str = "") -> str:
        if not row:
            return ""
        if hasattr(row, "keys"):
            return str(row.get("twitch_login") or fallback).strip().lower()
        return str((row[2] if len(row) > 2 else fallback) or fallback).strip().lower()

    async def raid_auth_start(self, request: web.Request) -> web.StreamResponse:
        """Create OAuth URL for raid bot authorization.

        Access policy:
        - Streamer dashboard session may only authorize its own Twitch login.
        - Explicit `?login=` overrides require admin token/session gate.
        - Public website onboarding may start the reduced base-scope OAuth without a session.
        """
        requested_login = (request.query.get("login") or "").strip().lower()
        requested_scope_profile_raw = (request.query.get("scope_profile") or "").strip()
        requested_scope_profile = (
            normalize_scope_profile(requested_scope_profile_raw)
            if requested_scope_profile_raw
            else ""
        )
        request_source = (request.query.get("source") or "").strip().lower()
        login = ""
        session_getter = getattr(self, "_get_dashboard_auth_session", None)
        if callable(session_getter):
            try:
                dashboard_session = session_getter(request)
            except Exception:
                log.debug("Could not resolve dashboard auth session for raid auth", exc_info=True)
                dashboard_session = None
            if isinstance(dashboard_session, dict):
                login = str(dashboard_session.get("twitch_login") or "").strip().lower()

        if requested_login:
            if not login or requested_login != login:
                self._require_token(request)
            login = requested_login
        elif not login:
            public_scope_profile = requested_scope_profile or BASE_SCOPE_PROFILE
            allow_public_onboarding = (
                public_scope_profile == BASE_SCOPE_PROFILE
                and request_source in {"", "website_onboarding"}
            )
            if allow_public_onboarding:
                login = PUBLIC_STREAMER_ONBOARDING_LOGIN
                requested_scope_profile = public_scope_profile
            else:
                raise web.HTTPFound(location=PUBLIC_STREAMER_ONBOARDING_URL)

        if not login:
            return web.Response(text="Missing login parameter", status=400)

        auth_manager = self._dashboard_auth_manager()
        if auth_manager:
            client_id = str(getattr(auth_manager, "client_id", "") or "").strip()
            redirect_uri = str(getattr(auth_manager, "redirect_uri", "") or "").strip()
            if not client_id or not redirect_uri:
                return web.Response(text="Raid bot OAuth is not configured", status=503)
            auth_kwargs: dict[str, str] = {}
            if requested_scope_profile:
                auth_kwargs["scope_profile"] = requested_scope_profile
            auth_url = str(auth_manager.generate_auth_url(login, **auth_kwargs))
        else:
            raid_auth_url_cb = getattr(self, "_raid_auth_url_cb", None)
            if not callable(raid_auth_url_cb):
                return web.Response(text="Raid bot not initialized", status=503)
            try:
                auth_kwargs: dict[str, str] = {}
                if requested_scope_profile:
                    auth_kwargs["scope_profile"] = requested_scope_profile
                auth_url = str(await raid_auth_url_cb(login, **auth_kwargs)).strip()
            except Exception as exc:
                status = int(getattr(exc, "status", 503) or 503)
                return web.Response(
                    text=str(getattr(exc, "message", str(exc)) or "Raid bot not initialized"),
                    status=max(400, min(status, 599)),
                )
            if not auth_url:
                return web.Response(text="Raid bot not initialized", status=503)

        raise web.HTTPFound(location=auth_url)

    async def raid_auth_go(self, request: web.Request) -> web.StreamResponse:
        """Kurz-Redirect für Discord-Buttons → leitet zum vollen Twitch-OAuth-URL weiter.

        Kein Token erforderlich – der State ist das Geheimnis (10 Min TTL).
        Discord-Button-URLs sind auf 512 Zeichen limitiert; der volle OAuth-URL
        überschreitet dieses Limit.  Der Button verweist stattdessen auf diesen
        Endpoint, der den gespeicherten URL nachschlägt und weiterleitet.
        """
        state = (request.query.get("state") or "").strip()
        if not state:
            return web.Response(text="Missing state parameter", status=400)

        auth_manager = self._dashboard_auth_manager()
        if auth_manager:
            full_url = auth_manager.get_pending_auth_url(state)
        else:
            raid_go_url_cb = getattr(self, "_raid_go_url_cb", None)
            if not callable(raid_go_url_cb):
                return web.Response(text="Raid bot not initialized", status=503)
            try:
                full_url = await raid_go_url_cb(state)
            except Exception as exc:
                status = int(getattr(exc, "status", 503) or 503)
                return web.Response(
                    text=str(getattr(exc, "message", str(exc)) or "Raid bot not initialized"),
                    status=max(400, min(status, 599)),
                )
        if not full_url:
            return web.Response(
                text="<html><body>Link abgelaufen oder ungültig. "
                "Bitte erneut auf den Button in Discord klicken.</body></html>",
                content_type="text/html",
                status=410,
            )

        raise web.HTTPFound(location=full_url)

    async def raid_requirements(self, request: web.Request) -> web.StreamResponse:
        """Send raid OAuth requirement DM with one-click fresh link generation."""
        self._require_token(request)
        _auth_level, is_admin, session_login = self._raid_dashboard_auth_context(request)

        login = (request.query.get("login") or "").strip().lower()
        if not login:
            return web.Response(text="Missing login parameter", status=400)

        try:
            with storage.readonly_connection() as conn:
                row = storage.load_active_partner(conn, twitch_login=login)
                session_partner = (
                    storage.load_active_partner(conn, twitch_login=session_login)
                    if session_login
                    else None
                )
        except Exception:
            safe_login = str(login or "").replace("\r", "\\r").replace("\n", "\\n")
            log.exception(
                "Failed to load partner authorization for raid requirements (%s)",
                safe_login,
            )
            return web.Response(text="Failed to load Discord link", status=500)

        if not row:
            return web.Response(text="Streamer not found", status=404)

        login = self._raid_active_partner_login(row, login)
        session_partner_login = self._raid_active_partner_login(session_partner, session_login)
        if not is_admin:
            if not session_partner_login:
                return web.Response(text="Dashboard streamer session required", status=403)
            if login != session_partner_login:
                return web.Response(text="Forbidden streamer scope", status=403)

        auth_manager = self._dashboard_auth_manager()
        if not auth_manager:
            raid_requirements_cb = getattr(self, "_raid_requirements_cb", None)
            if not callable(raid_requirements_cb):
                return web.Response(text="Raid bot not initialized", status=503)
            try:
                ok_message = str(await raid_requirements_cb(login))
            except Exception as exc:
                status = int(getattr(exc, "status", 503) or 503)
                return web.Response(
                    text=str(getattr(exc, "message", str(exc)) or "Raid bot not initialized"),
                    status=max(400, min(status, 599)),
                )
            location = self._redirect_location(request, ok=ok_message, default_path="/twitch/admin")
            safe_location = self._safe_internal_redirect(location, fallback="/twitch/admin")
            raise web.HTTPFound(location=safe_location)

        if hasattr(row, "keys"):
            discord_user_id = str(row.get("discord_user_id") or "").strip()
        else:
            discord_user_id = str((row[21] if len(row) > 21 else "") or "").strip()
        if not discord_user_id:
            return web.Response(text="No Discord user linked for this streamer", status=404)

        try:
            user_id_int = int(discord_user_id)
        except (TypeError, ValueError):
            return web.Response(text="Invalid Discord user id", status=400)

        discord_bot = self._dashboard_discord_bot()
        if not discord_bot:
            return web.Response(text="Discord bot not available", status=503)

        user = discord_bot.get_user(user_id_int)
        if user is None:
            try:
                user = await discord_bot.fetch_user(user_id_int)
            except discord.NotFound:
                user = None
            except discord.HTTPException:
                safe_login = str(login or "").replace("\r", "\\r").replace("\n", "\\n")
                log.exception(
                    "Failed to fetch Discord user %s for %s",
                    user_id_int,
                    safe_login,
                )
                user = None

        if user is None:
            return web.Response(text="Discord user not found", status=404)

        embed = build_raid_requirements_embed(login)
        view = RaidAuthGenerateView(auth_manager=auth_manager, twitch_login=login)

        try:
            await user.send(embed=embed, view=view)
        except discord.Forbidden:
            safe_login = str(login or "").replace("\r", "\\r").replace("\n", "\\n")
            log.warning(
                "Discord DM blocked for %s (%s)",
                safe_login,
                user_id_int,
            )
            return web.Response(text="Discord DM blocked", status=403)
        except discord.HTTPException:
            safe_login = str(login or "").replace("\r", "\\r").replace("\n", "\\n")
            log.exception(
                "Failed to send raid requirements DM to %s (%s)",
                safe_login,
                user_id_int,
            )
            return web.Response(text="Failed to send Discord DM", status=502)

        ok_message = f"Anforderungen per Discord an @{login} gesendet"
        location = self._redirect_location(request, ok=ok_message, default_path="/twitch/admin")
        safe_location = self._safe_internal_redirect(location, fallback="/twitch/admin")
        raise web.HTTPFound(location=safe_location)

    async def raid_history(self, request: web.Request) -> web.StreamResponse:
        """Render raid history table for dashboard operators."""
        self._require_token(request)

        try:
            limit = int((request.query.get("limit") or "50").strip())
        except ValueError:
            limit = 50
        limit = max(1, min(limit, 500))
        from_broadcaster = (request.query.get("from") or "").strip().lower()

        history = await self._raid_history_cb(limit=limit, from_broadcaster=from_broadcaster)
        rows_html = self._build_raid_history_rows(history)
        page_html = self._build_raid_history_page(rows_html)
        return web.Response(text=page_html, content_type="text/html")

    async def raid_analytics(self, request: web.Request) -> web.StreamResponse:
        """Raid analytics: sent/received balance, leechers, manual raids."""
        self._require_token(request)

        with storage.readonly_connection() as conn:
            # Active partners set
            partner_rows = conn.execute(
                "SELECT twitch_login FROM twitch_streamers_partner_state WHERE is_partner_active = 1"
            ).fetchall()
            partners: set = {r[0].lower() for r in partner_rows}

            # Sent stats
            sent_rows = conn.execute(
                """
                SELECT from_broadcaster_login, COUNT(*) as cnt, SUM(viewer_count) as viewers
                FROM twitch_raid_history WHERE COALESCE(success, FALSE) IS TRUE
                GROUP BY from_broadcaster_login ORDER BY cnt DESC
                """
            ).fetchall()

            # Received stats
            recv_rows = conn.execute(
                """
                SELECT to_broadcaster_login, COUNT(*) as cnt, SUM(viewer_count) as viewers
                FROM twitch_raid_history WHERE COALESCE(success, FALSE) IS TRUE
                GROUP BY to_broadcaster_login ORDER BY cnt DESC
                """
            ).fetchall()

            # Manual raids
            manual_rows = conn.execute(
                """
                SELECT from_broadcaster_login, to_broadcaster_login, viewer_count, executed_at
                FROM twitch_raid_history
                WHERE reason = 'manual_chat_command'
                ORDER BY executed_at DESC
                """
            ).fetchall()

            # Date range
            date_row = conn.execute(
                "SELECT MIN(executed_at), MAX(executed_at), COUNT(*) FROM twitch_raid_history WHERE COALESCE(success, FALSE) IS TRUE"
            ).fetchone()

        sent_map: dict = {r[0].lower(): {"cnt": r[1], "viewers": r[2] or 0} for r in sent_rows}
        recv_map: dict = {r[0].lower(): {"cnt": r[1], "viewers": r[2] or 0} for r in recv_rows}

        # Per-partner balance (only active partners for main table)
        partner_stats = []
        for login in sorted(partners):
            s = sent_map.get(login, {}).get("cnt", 0)
            r = recv_map.get(login, {}).get("cnt", 0)
            sv = sent_map.get(login, {}).get("viewers", 0)
            rv = recv_map.get(login, {}).get("viewers", 0)
            partner_stats.append(
                {
                    "login": login,
                    "sent": s,
                    "received": r,
                    "balance": s - r,
                    "viewers_sent": sv,
                    "viewers_recv": rv,
                }
            )
        partner_stats.sort(key=lambda x: x["balance"], reverse=True)

        leechers = [p for p in partner_stats if p["sent"] == 0 and p["received"] > 0]

        # External receivers of manual raids (non-partner targets)
        manual_list = []
        for row in manual_rows:
            raider = (row[0] or "").lower()
            target = (row[1] or "").lower()
            manual_list.append(
                {
                    "from": raider,
                    "to": target,
                    "viewers": row[2] or 0,
                    "at": str(row[3] or "")[:16],
                    "is_partner": target in partners,
                }
            )

        date_min = str(date_row[0] or "")[:10]
        date_max = str(date_row[1] or "")[:10]
        total = date_row[2] or 0

        page_html = self._build_raid_analytics_page(
            partner_stats=partner_stats,
            leechers=leechers,
            manual_list=manual_list,
            date_min=date_min,
            date_max=date_max,
            total=total,
        )
        return web.Response(text=page_html, content_type="text/html")

    async def raid_oauth_callback(self, request: web.Request) -> web.StreamResponse:
        """Handle Twitch OAuth callback for raid authorization."""
        bot_service = self._dashboard_bot_runtime()
        raid_bot = bot_service.raid_bot
        auth_manager = self._dashboard_auth_manager()

        code = (request.query.get("code") or "").strip()
        state = (request.query.get("state") or "").strip()
        error = (request.query.get("error") or "").strip()

        if not raid_bot or not auth_manager:
            raid_oauth_callback_cb = getattr(self, "_raid_oauth_callback_cb", None)
            if callable(raid_oauth_callback_cb):
                try:
                    payload = await raid_oauth_callback_cb(code=code, state=state, error=error)
                except Exception as exc:
                    status = int(getattr(exc, "status", 503) or 503)
                    payload = {
                        "title": "Raid-Bot nicht verfügbar",
                        "body_html": (
                            "<p>"
                            + html.escape(
                                str(getattr(exc, "message", str(exc)) or "Raid bot not initialized"),
                                quote=True,
                            )
                            + "</p>"
                        ),
                        "status": max(400, min(status, 599)),
                    }
                title = str(payload.get("title") or "Autorisierung")
                body_html = str(payload.get("body_html") or "<p>Unbekannte Antwort.</p>")
                try:
                    status_code = int(payload.get("status", 200))
                except (TypeError, ValueError):
                    status_code = 200
                status_code = max(200, min(status_code, 599))
                redirect_candidate = str(payload.get("redirect_url") or "").strip()
                if redirect_candidate and status_code < 400:
                    raise web.HTTPFound(
                        location=self._raid_oauth_success_redirect_url(redirect_candidate)
                    )
                return web.Response(
                    text=self._render_oauth_page(title, body_html),
                    status=status_code,
                    content_type="text/html",
                )

        payload = await build_raid_oauth_callback_payload(
            code=code,
            state=state,
            error=error,
            raid_bot=raid_bot,
            auth_manager=auth_manager,
            success_redirect_url=self._raid_oauth_success_redirect_url(),
            failure_title="Fehler bei der Autorisierung",
            failure_body_html=(
                "<p>Beim Speichern der Twitch-Autorisierung ist ein interner Fehler aufgetreten.</p>"
                "<p>Bitte den Vorgang erneut starten.</p>"
            ),
            schedule_background=getattr(self, "_dashboard_schedule_background", None),
        )
        title = str(payload.get("title") or "Autorisierung")
        body_html = str(payload.get("body_html") or "<p>Unbekannte Antwort.</p>")
        try:
            status_code = int(payload.get("status", 200))
        except (TypeError, ValueError):
            status_code = 200
        status_code = max(200, min(status_code, 599))
        redirect_candidate = str(payload.get("redirect_url") or "").strip()
        if redirect_candidate and status_code < 400:
            raise web.HTTPFound(
                location=self._raid_oauth_success_redirect_url(redirect_candidate)
            )
        return web.Response(
            text=self._render_oauth_page(title, body_html),
            status=status_code,
            content_type="text/html",
        )

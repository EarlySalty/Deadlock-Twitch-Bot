"""Route group for dashboard entry, admin, and utility handlers."""

from __future__ import annotations

from typing import Any

from aiohttp import web

from .live.live import DashboardLiveMixin
from .pages import build_roadmap_body, build_scope_panel, build_stats_entry_page
from .route_deps import EntryRouteDeps
from .upstream_errors import is_upstream_service_error


ROADMAP_BODY = build_roadmap_body()


def build_route_defs(server: Any) -> list[web.RouteDef]:
    """Return route definitions for entry, admin, and utility routes."""
    return [
        web.get("/", server.public_home),
        web.get("/dashboads", server.legacy_dashboard_redirect),
        web.get("/dashboards", server.legacy_dashboard_redirect),
        web.get("/twitch", server.index),
        web.get("/twitch/", server.index),
        web.get("/twitch/admin", server.admin),
        web.get("/twitch/admin/announcements", server.admin_announcements_page),
        web.post("/twitch/admin/announcements", server.admin_announcements_save),
        web.get("/twitch/admin/roadmap", server.admin_roadmap_page),
        web.get("/twitch/live", server.admin),
        web.get("/twitch/live-announcement", server.live_announcement_page),
        web.post("/twitch/add_any", server.add_any),
        web.post("/twitch/add_url", server.add_url),
        web.post("/twitch/add_login/{login}", server.add_login),
        web.post("/twitch/add_streamer", server.add_streamer),
        web.post("/twitch/admin/chat_action", server.admin_partner_chat_action),
        web.post("/twitch/admin/manual-plan", server.admin_manual_plan_save),
        web.post("/twitch/admin/manual-plan/clear", server.admin_manual_plan_clear),
        web.post("/twitch/remove", server.remove),
        web.post("/twitch/verify", server.verify),
        web.post("/twitch/archive", server.archive),
        web.post("/twitch/discord_flag", server.discord_flag),
        web.get("/twitch/stats", server.stats),
        web.get("/twitch/partners", server.partner_stats),
        web.get("/twitch/dashboads", server.legacy_dashboard_redirect),
        web.get("/twitch/dashboards", server.legacy_dashboard_redirect),
        web.get("/twitch/auth/logout", server.auth_logout),
        web.post("/twitch/discord_link", server.discord_link),
        web.post("/twitch/reload", server.reload_cog),
    ]


async def index(server: Any, request: web.Request) -> web.StreamResponse:
    """Entrypoint with local-first admin behavior."""
    if server._is_local_request(request) or server._is_discord_admin_request(request):
        destination = "/twitch/admin"
        fallback = "/twitch/admin"
    else:
        destination = "/twitch/dashboard"
        fallback = "/twitch/dashboard"
    if request.query_string:
        destination = f"{destination}?{request.query_string}"
    safe_destination = server._safe_internal_redirect(destination, fallback=fallback)
    raise web.HTTPFound(safe_destination)


async def public_home(server: Any, request: web.Request) -> web.StreamResponse:
    """Root entrypoint redirects to admin or canonical dashboard landing."""
    if server._is_local_request(request) or server._is_discord_admin_request(request):
        destination = "/twitch/admin"
        fallback = "/twitch/admin"
    else:
        destination = "/twitch/dashboard"
        fallback = "/twitch/dashboard"
    if request.query_string:
        destination = f"{destination}?{request.query_string}"
    safe_destination = server._safe_internal_redirect(destination, fallback=fallback)
    raise web.HTTPFound(safe_destination)


async def legacy_dashboard_redirect(server: Any, request: web.Request) -> web.StreamResponse:
    """Redirect legacy dashboard paths to the canonical dashboard landing."""
    destination = "/twitch/dashboard"
    if request.query_string:
        destination = f"{destination}?{request.query_string}"
    safe_destination = server._safe_internal_redirect(destination, fallback="/twitch/dashboard")
    raise web.HTTPFound(safe_destination)


async def admin(server: Any, request: web.Request) -> web.StreamResponse:
    """Legacy partner admin surface."""
    return await DashboardLiveMixin.index(server, request)


async def stats_entry(
    server: Any,
    request: web.Request,
    *,
    deps: EntryRouteDeps,
) -> web.StreamResponse:
    """Canonical public entrypoint that links old and beta analytics dashboards."""
    log = deps.log
    storage_module = deps.storage
    critical_scopes = deps.critical_scopes
    required_scopes = deps.required_scopes
    scope_column_labels = deps.scope_column_labels
    dashboards_discord_login_url = deps.dashboards_discord_login_url
    dashboards_login_url = deps.dashboards_login_url

    if not server._check_v2_auth(request):
        login_url = (
            dashboards_discord_login_url
            if server._should_use_discord_admin_login(request)
            else dashboards_login_url
        )
        response = server._dashboard_auth_redirect_or_unavailable(
            request,
            next_path="/twitch/dashboard",
            fallback_login_url=login_url,
        )
        if isinstance(response, web.HTTPException):
            raise response
        return response

    legacy_url = server._resolve_legacy_stats_url()
    beta_url = "/twitch/dashboard-v2"
    logout_url = (
        "/twitch/auth/discord/logout"
        if server._is_discord_admin_request(request)
        else "/twitch/auth/logout"
    )

    session = server._get_dashboard_auth_session(request)
    twitch_login = (session or {}).get("twitch_login", "")
    missing_scopes: list[str] = []
    missing_critical: list[str] = []
    if twitch_login:
        try:
            with storage_module.readonly_connection() as conn:
                row = conn.execute(
                    "SELECT scopes FROM twitch_raid_auth WHERE LOWER(twitch_login) = LOWER(%s)",
                    [twitch_login],
                ).fetchone()
            if row:
                token_scopes = set((row[0] or "").split())
                missing_scopes = [scope for scope in required_scopes if scope not in token_scopes]
                missing_critical = [scope for scope in missing_scopes if scope in critical_scopes]
            else:
                missing_scopes = list(required_scopes)
                missing_critical = [scope for scope in required_scopes if scope in critical_scopes]
        except Exception:
            log.exception("stats_entry: failed to load scopes for %s", twitch_login)

    scope_panel = build_scope_panel(
        twitch_login=twitch_login,
        missing_scopes=missing_scopes,
        missing_critical=missing_critical,
        required_scopes=required_scopes,
        critical_scopes=critical_scopes,
        scope_column_labels=scope_column_labels,
    )

    page_html = build_stats_entry_page(
        twitch_login=twitch_login,
        logout_url=logout_url,
        legacy_url=legacy_url,
        beta_url=beta_url,
        scope_panel=scope_panel,
    )
    return web.Response(text=page_html, content_type="text/html")


async def auth_logout(
    server: Any,
    request: web.Request,
    *,
    deps: EntryRouteDeps,
) -> web.StreamResponse:
    """Logout and clear dashboard session cookie."""
    log = deps.log
    dashboard_v2_login_url = deps.dashboard_v2_login_url

    session_id = (request.cookies.get(server._session_cookie_name) or "").strip()
    if session_id:
        session = server._delete_dashboard_auth_session(session_id)
        twitch_login = (session or {}).get("twitch_login", "unknown") if session else "unknown"
        log.info(
            "AUDIT dashboard logout: twitch=%s peer=%s",
            server._sanitize_log_value(twitch_login),
            server._sanitize_log_value(server._peer_host(request)),
        )

    response = server._dashboard_auth_redirect_or_unavailable(
        request,
        next_path="/twitch/dashboard-v2",
        fallback_login_url=dashboard_v2_login_url,
    )
    server._clear_session_cookie(response, request)
    partner_session_id = (request.cookies.get(server._partner_access_cookie_name()) or "").strip()
    if partner_session_id:
        server._delete_partner_access_session(partner_session_id)
    server._clear_partner_access_cookie(response, request)
    if isinstance(response, web.HTTPException):
        raise response
    return response


async def discord_link(
    server: Any,
    request: web.Request,
    *,
    deps: EntryRouteDeps,
) -> web.StreamResponse:
    """Persist Discord profile metadata from the stats dashboard."""
    log = deps.log

    server._require_token(request)
    if not callable(server._discord_profile):
        location = server._redirect_location(request, err="Discord-Link ist aktuell nicht verfügbar")
        safe_location = server._safe_internal_redirect(location, fallback="/twitch/stats")
        raise web.HTTPFound(location=safe_location)

    data = await request.post()
    csrf_token = str(data.get("csrf_token") or "").strip()
    if not server._csrf_verify_token(request, csrf_token):
        location = server._redirect_location(request, err="Ungültiges CSRF-Token")
        safe_location = server._safe_internal_redirect(location, fallback="/twitch/stats")
        raise web.HTTPFound(location=safe_location)

    login = (data.get("login") or "").strip()
    discord_user_id = (data.get("discord_user_id") or "").strip()
    discord_display_name = (data.get("discord_display_name") or "").strip()
    member_raw = (data.get("member_flag") or "").strip().lower()
    mark_member = member_raw in {"1", "true", "on", "yes"}

    try:
        message = await server._discord_profile(
            login,
            discord_user_id=discord_user_id or None,
            discord_display_name=discord_display_name or None,
            mark_member=mark_member,
        )
        location = server._redirect_location(request, ok=message)
    except ValueError as exc:
        location = server._redirect_location(request, err=str(exc))
    except Exception as exc:
        if is_upstream_service_error(exc):
            location = server._redirect_location(
                request, err="Discord-Daten konnten nicht gespeichert werden"
            )
            safe_location = server._safe_internal_redirect(location, fallback="/twitch/stats")
            raise web.HTTPFound(location=safe_location)
        log.exception("dashboard discord_link failed")
        location = server._redirect_location(
            request, err="Discord-Daten konnten nicht gespeichert werden"
        )

    safe_location = server._safe_internal_redirect(location, fallback="/twitch/stats")
    raise web.HTTPFound(location=safe_location)


async def reload_cog(
    server: Any,
    request: web.Request,
    *,
    deps: EntryRouteDeps,
) -> web.Response:
    """Optional reload endpoint for admin tooling compatibility."""
    log = deps.log

    await request.post()
    header_token = request.headers.get("X-Admin-Token")
    is_authorized = (
        server._is_local_request(request)
        or server._is_discord_admin_request(request)
        or server._check_admin_token(header_token)
    )
    if not is_authorized:
        log.warning(
            "AUDIT dashboard reload_cog: unauthorized attempt from peer=%s",
            server._sanitize_log_value(server._peer_host(request)),
        )
        return web.Response(text="Unauthorized", status=401)

    log.info(
        "AUDIT dashboard reload_cog: triggered by peer=%s",
        server._sanitize_log_value(server._peer_host(request)),
    )
    if server._reload_cb:
        msg = await server._reload_cb()
        return web.Response(text=msg)
    return web.Response(text="Kein Reload-Handler definiert", status=501)


async def admin_roadmap_page(server: Any, request: web.Request) -> web.StreamResponse:
    """Kanban board for managing roadmap items."""
    if not (server._is_local_request(request) or server._is_discord_admin_request(request)):
        raise web.HTTPFound("/twitch/admin")

    return web.Response(
        content_type="text/html",
        text=server._html(ROADMAP_BODY, "roadmap"),
    )

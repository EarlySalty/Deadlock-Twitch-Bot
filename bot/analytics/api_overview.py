"""
Analytics API v2 - Overview Mixin.

Route setup, overview data, sessions, health scores, network stats, correlations.
"""

from __future__ import annotations

import json
import logging
from datetime import UTC, datetime, timedelta
from pathlib import Path
from typing import Any

from aiohttp import web

from ..core.chat_bots import build_known_chat_bot_not_in_clause
from ..dashboard.live import DashboardLiveMixin
from .raid_metrics import raid_identity_key, recalculate_raid_chat_metrics
from ..storage import pg as storage
from .error_utils import analytics_internal_error_response

log = logging.getLogger("TwitchStreams.AnalyticsV2")
DASHBOARD_LOGIN_URL = "/twitch/auth/login?next=%2Ftwitch%2Fdashboard"
DASHBOARD_DISCORD_LOGIN_URL = "/twitch/auth/discord/login?next=%2Ftwitch%2Fdashboard"
DASHBOARD_V2_LOGIN_URL = "/twitch/auth/login?next=%2Ftwitch%2Fdashboard-v2"
DASHBOARD_V2_DISCORD_LOGIN_URL = "/twitch/auth/discord/login?next=%2Ftwitch%2Fdashboard-v2"
DASHBOARD_VERWALTUNG_LOGIN_URL = "/twitch/auth/login?next=%2Ftwitch%2Fverwaltung"
DASHBOARD_VERWALTUNG_DISCORD_LOGIN_URL = "/twitch/auth/discord/login?next=%2Ftwitch%2Fverwaltung"
DASHBOARD_PRICING_LOGIN_URL = "/twitch/auth/login?next=%2Ftwitch%2Fpricing"
DASHBOARD_PRICING_DISCORD_LOGIN_URL = "/twitch/auth/discord/login?next=%2Ftwitch%2Fpricing"
DASHBOARD_AFFILIATE_PORTAL_LOGIN_URL = "/twitch/auth/login?next=%2Ftwitch%2Faffiliate%2Fportal"
DASHBOARD_AFFILIATE_PORTAL_DISCORD_LOGIN_URL = (
    "/twitch/auth/discord/login?next=%2Ftwitch%2Faffiliate%2Fportal"
)
ADMIN_DASHBOARD_DISCORD_LOGIN_URL = "/twitch/auth/discord/login?next=%2Ftwitch%2Fadmin"
WEBSITE_DIST_ROOT_PATH = Path(__file__).resolve().parents[2] / "website" / "dist"
AFFILIATE_PORTAL_INDEX_PATH = (
    WEBSITE_DIST_ROOT_PATH / "affiliate-portal" / "index.html"
)


class _AnalyticsOverviewMixin:
    """Mixin providing route registration and overview data endpoints."""

    def _register_v2_routes(self, router: web.UrlDispatcher) -> None:
        """Register all v2 API routes."""
        router.add_get("/twitch/api/v2/overview", self._api_v2_overview)
        router.add_get("/twitch/api/v2/monthly-stats", self._api_v2_monthly_stats)
        router.add_get("/twitch/api/v2/weekly-stats", self._api_v2_weekly_stats)
        router.add_get("/twitch/api/v2/hourly-heatmap", self._api_v2_hourly_heatmap)
        router.add_get("/twitch/api/v2/calendar-heatmap", self._api_v2_calendar_heatmap)
        router.add_get("/twitch/api/v2/chat-analytics", self._api_v2_chat_analytics)
        router.add_get("/twitch/api/v2/viewer-overlap", self._api_v2_viewer_overlap)
        router.add_get("/twitch/api/v2/tag-analysis", self._api_v2_tag_analysis)
        router.add_get("/twitch/api/v2/rankings", self._api_v2_rankings)
        router.add_get("/twitch/api/v2/category-comparison", self._api_v2_category_comparison)
        router.add_get("/twitch/api/v2/streamers", self._api_v2_streamers)
        router.add_get("/twitch/api/v2/session/{id}", self._api_v2_session_detail)
        router.add_get("/twitch/api/v2/auth-status", self._api_v2_auth_status)
        router.add_get("/twitch/api/v2/internal-home", self._api_v2_internal_home)
        router.add_post(
            "/twitch/api/v2/internal-home/changelog",
            self._api_v2_internal_home_changelog_create,
        )
        # New Audience Analytics Endpoints
        router.add_get(
            "/twitch/api/v2/watch-time-distribution",
            self._api_v2_watch_time_distribution,
        )
        router.add_get("/twitch/api/v2/follower-funnel", self._api_v2_follower_funnel)
        router.add_get("/twitch/api/v2/tag-analysis-extended", self._api_v2_tag_analysis_extended)
        router.add_get("/twitch/api/v2/title-performance", self._api_v2_title_performance)
        router.add_get("/twitch/api/v2/audience-insights", self._api_v2_audience_insights)
        router.add_get("/twitch/api/v2/audience-demographics", self._api_v2_audience_demographics)
        # Lurker / Raid Retention / Viewer Profiles / Audience Sharing
        router.add_get("/twitch/api/v2/lurker-analysis", self._api_v2_lurker_analysis)
        router.add_get("/twitch/api/v2/raid-retention", self._api_v2_raid_retention)
        router.add_get("/twitch/api/v2/viewer-profiles", self._api_v2_viewer_profiles)
        router.add_get("/twitch/api/v2/audience-sharing", self._api_v2_audience_sharing)
        # Stats-Data Endpoints (from twitch_stats_tracked / twitch_stats_category)
        router.add_get("/twitch/api/v2/viewer-timeline", self._api_v2_viewer_timeline)
        router.add_get("/twitch/api/v2/category-leaderboard", self._api_v2_category_leaderboard)
        router.add_get("/twitch/api/v2/coaching", self._api_v2_coaching)
        router.add_get("/twitch/api/v2/monetization", self._api_v2_monetization)
        router.add_get("/twitch/api/v2/category-timings", self._api_v2_category_timings)
        router.add_get(
            "/twitch/api/v2/category-activity-series",
            self._api_v2_category_activity_series,
        )
        router.add_get("/twitch/api/v2/raid-analytics", self._api_v2_raid_analytics)
        router.add_get("/twitch/api/v2/retention-curve", self._api_v2_retention_curve)
        router.add_get("/twitch/api/v2/loyalty-curve", self._api_v2_loyalty_curve)
        # Viewer Analytics (individual viewer profiles)
        router.add_get("/twitch/api/v2/viewer-directory", self._api_v2_viewer_directory)
        router.add_get("/twitch/api/v2/viewer-detail", self._api_v2_viewer_detail)
        router.add_get("/twitch/api/v2/viewer-segments", self._api_v2_viewer_segments)
        # Chat Deep Analysis
        router.add_get("/twitch/api/v2/chat-hype-timeline", self._api_v2_chat_hype_timeline)
        router.add_get("/twitch/api/v2/chat-content-analysis", self._api_v2_chat_content_analysis)
        router.add_get("/twitch/api/v2/chat-social-graph", self._api_v2_chat_social_graph)
        # Experimental (Labor) – all-game session analytics
        router.add_get("/twitch/api/v2/exp/overview", self._api_v2_exp_overview)
        router.add_get("/twitch/api/v2/exp/game-breakdown", self._api_v2_exp_game_breakdown)
        router.add_get("/twitch/api/v2/exp/game-transitions", self._api_v2_exp_game_transitions)
        router.add_get("/twitch/api/v2/exp/growth-curves", self._api_v2_exp_growth_curves)
        # KI Analyse – Claude Opus deep analysis (admin only)
        router.add_get("/twitch/api/v2/ai/analysis", self._api_v2_ai_analysis)
        router.add_get("/twitch/api/v2/ai/history", self._api_v2_ai_history)
        # Roadmap (public GET, admin POST/PATCH/DELETE)
        router.add_get("/twitch/api/v2/roadmap", self._api_v2_roadmap_get)
        router.add_post("/twitch/api/v2/roadmap", self._api_v2_roadmap_create)
        router.add_patch("/twitch/api/v2/roadmap/{id}", self._api_v2_roadmap_update)
        router.add_delete("/twitch/api/v2/roadmap/{id}", self._api_v2_roadmap_delete)
        # Billing catalog
        router.add_get("/twitch/api/v2/billing/catalog", self._api_v2_billing_catalog)
        router.add_get("/twitch/api/v2/affiliate/portal", self._api_v2_affiliate_portal)
        # Serve the dashboard
        router.add_get("/twitch/dashboard", self._serve_dashboard)
        router.add_get("/twitch/dashboard-v2", self._serve_dashboard_v2)
        router.add_get("/twitch/dashboard-v2/{path:.*}", self._serve_dashboard_v2_assets)
        router.add_get("/twitch/verwaltung", self._serve_verwaltung)
        router.add_get("/twitch/pricing", self._serve_pricing)
        router.add_get("/twitch/affiliate/portal", self._serve_affiliate_portal)
        router.add_get("/website/{path:.*}", self._serve_website_dist_asset)
        router.add_get("/twitch/admin/", self._serve_admin_dashboard)
        router.add_get("/twitch/admin/assets/{path:.*}", self._serve_admin_dashboard_assets)
        router.add_get("/twitch/admin/{path:.*}", self._serve_admin_dashboard_path)
        # Public demo (no auth required)
        self._register_demo_routes(router)

    def _register_demo_routes(self, router: web.UrlDispatcher) -> None:
        """Register public demo endpoints – no authentication required."""
        from .demo_data import (
            build_demo_ai_analysis,
            build_demo_ai_history,
            build_demo_auth_status,
            build_demo_payload,
            build_demo_streamers,
        )

        def _streamer(request: web.Request) -> str | None:
            return str(request.query.get("streamer", "") or "").strip() or None

        def _query_int(request: web.Request, name: str, default: int) -> int:
            try:
                return int(request.query.get(name, str(default)))
            except (TypeError, ValueError):
                return default

        def _query_bool(request: web.Request, name: str, default: bool = False) -> bool:
            raw = str(request.query.get(name, "1" if default else "0") or "").strip().lower()
            if raw in {"1", "true", "yes", "on"}:
                return True
            if raw in {"0", "false", "no", "off"}:
                return False
            return default

        def _demo_payload_j(
            kind: str,
            *,
            with_days: bool = False,
            with_months: bool = False,
            with_limit: bool = False,
            with_metric: bool = False,
            with_source: bool = False,
            with_sort: bool = False,
            with_exclude_external: bool = False,
        ):
            async def _handler(request: web.Request) -> web.Response:
                payload_kwargs: dict[str, Any] = {"streamer": _streamer(request)}
                if with_days:
                    payload_kwargs["days"] = _query_int(request, "days", 30)
                if with_months:
                    payload_kwargs["months"] = _query_int(request, "months", 12)
                if with_limit:
                    payload_kwargs["limit"] = _query_int(request, "limit", 20)
                if with_metric:
                    payload_kwargs["metric"] = request.query.get("metric", "viewers")
                if with_source:
                    payload_kwargs["source"] = request.query.get("source", "category")
                if with_sort:
                    payload_kwargs["sort"] = request.query.get("sort", "avg")
                if with_exclude_external:
                    payload_kwargs["exclude_external"] = _query_bool(request, "exclude_external")
                return web.json_response(build_demo_payload(kind, **payload_kwargs))

            return _handler

        def _viewer_directory_j():
            async def _handler(request: web.Request) -> web.Response:
                page = _query_int(request, "page", 1)
                per_page = _query_int(request, "per_page", 50)
                payload = build_demo_payload(
                    "viewer-directory",
                    streamer=_streamer(request),
                    days=_query_int(request, "days", 30),
                    sort=request.query.get("sort", "sessions"),
                    order=request.query.get("order", "desc"),
                    filter_type=request.query.get("filter", "all"),
                    search=request.query.get("search", ""),
                    page=page,
                    per_page=per_page,
                )
                return web.json_response(payload)

            return _handler

        def _viewer_detail_j():
            async def _handler(request: web.Request) -> web.Response:
                login = request.query.get("login", "").strip()
                if not login:
                    return web.json_response({"error": "login required"}, status=400)
                return web.json_response(
                    build_demo_payload(
                        "viewer-detail",
                        streamer=_streamer(request),
                        days=_query_int(request, "days", 30),
                        login=login,
                    )
                )

            return _handler

        async def _demo_ai_analysis_handler(request: web.Request) -> web.Response:
            try:
                days = int(request.query.get("days", "30"))
            except ValueError:
                days = 30
            game_filter = str(request.query.get("game_filter", "all") or "all").strip().lower()
            if game_filter not in {"all", "deadlock"}:
                game_filter = "all"
            return web.json_response(
                build_demo_ai_analysis(
                    streamer=_streamer(request),
                    days=days,
                    game_filter=game_filter,
                )
            )

        async def _demo_ai_history_handler(request: web.Request) -> web.Response:
            try:
                limit = int(request.query.get("limit", "20"))
            except ValueError:
                limit = 20
            return web.json_response(
                build_demo_ai_history(streamer=_streamer(request), limit=limit)
            )

        async def _demo_auth_status_handler(request: web.Request) -> web.Response:
            return web.json_response(build_demo_auth_status(streamer=_streamer(request)))

        async def _demo_streamers_handler(_request: web.Request) -> web.Response:
            return web.json_response(build_demo_streamers())

        base = "/twitch/demo/api/v2"
        router.add_get(f"{base}/auth-status", _demo_auth_status_handler)
        router.add_get(f"{base}/streamers", _demo_streamers_handler)
        router.add_get(f"{base}/overview", _demo_payload_j("overview", with_days=True))
        router.add_get(f"{base}/monthly-stats", _demo_payload_j("monthly-stats", with_months=True))
        router.add_get(f"{base}/weekly-stats", _demo_payload_j("weekly-stats", with_days=True))
        router.add_get(f"{base}/hourly-heatmap", _demo_payload_j("hourly-heatmap", with_days=True))
        router.add_get(f"{base}/calendar-heatmap", _demo_payload_j("calendar-heatmap", with_days=True))
        router.add_get(f"{base}/chat-analytics", _demo_payload_j("chat-analytics", with_days=True))
        router.add_get(f"{base}/viewer-overlap", _demo_payload_j("viewer-overlap", with_limit=True))
        router.add_get(f"{base}/tag-analysis", _demo_payload_j("tag-analysis", with_days=True, with_limit=True))
        router.add_get(f"{base}/tag-analysis-extended", _demo_payload_j("tag-analysis-extended", with_days=True, with_limit=True))
        router.add_get(f"{base}/title-performance", _demo_payload_j("title-performance", with_days=True, with_limit=True))
        router.add_get(f"{base}/rankings", _demo_payload_j("rankings", with_days=True, with_limit=True, with_metric=True, with_exclude_external=True))
        router.add_get(f"{base}/category-comparison", _demo_payload_j("category-comparison", with_days=True, with_exclude_external=True))
        router.add_get(f"{base}/watch-time-distribution", _demo_payload_j("watch-time-distribution", with_days=True))
        router.add_get(f"{base}/follower-funnel", _demo_payload_j("follower-funnel", with_days=True))
        router.add_get(f"{base}/audience-insights", _demo_payload_j("audience-insights", with_days=True))
        router.add_get(f"{base}/audience-demographics", _demo_payload_j("audience-demographics", with_days=True))
        router.add_get(f"{base}/viewer-timeline", _demo_payload_j("viewer-timeline", with_days=True))
        router.add_get(f"{base}/category-leaderboard", _demo_payload_j("category-leaderboard", with_days=True, with_limit=True, with_sort=True, with_exclude_external=True))
        router.add_get(f"{base}/coaching", _demo_payload_j("coaching", with_days=True))
        router.add_get(f"{base}/monetization", _demo_payload_j("monetization", with_days=True))
        router.add_get(f"{base}/category-timings", _demo_payload_j("category-timings", with_days=True, with_source=True))
        router.add_get(f"{base}/category-activity-series", _demo_payload_j("category-activity-series", with_days=True))
        router.add_get(f"{base}/lurker-analysis", _demo_payload_j("lurker-analysis", with_days=True))
        router.add_get(f"{base}/raid-retention", _demo_payload_j("raid-retention", with_days=True))
        router.add_get(f"{base}/viewer-directory", _viewer_directory_j())
        router.add_get(f"{base}/viewer-detail", _viewer_detail_j())
        router.add_get(f"{base}/viewer-segments", _demo_payload_j("viewer-segments", with_days=True))
        router.add_get(f"{base}/viewer-profiles", _demo_payload_j("viewer-profiles", with_days=True))
        router.add_get(f"{base}/audience-sharing", _demo_payload_j("audience-sharing", with_days=True))
        router.add_get(f"{base}/exp/overview", _demo_payload_j("exp-overview", with_days=True))
        router.add_get(f"{base}/exp/game-breakdown", _demo_payload_j("exp-game-breakdown", with_days=True))
        router.add_get(f"{base}/exp/game-transitions", _demo_payload_j("exp-game-transitions", with_days=True))
        router.add_get(f"{base}/exp/growth-curves", _demo_payload_j("exp-growth-curves", with_days=True))
        router.add_get(f"{base}/ai/analysis", _demo_ai_analysis_handler)
        router.add_get(f"{base}/ai/history", _demo_ai_history_handler)
        # Demo dashboard HTML
        router.add_get("/twitch/demo/", self._serve_demo_dashboard)
        router.add_get("/twitch/demo", self._serve_demo_dashboard)
        router.add_get("/twitch/demo/dashboard-v2/{path:.*}", self._serve_demo_dashboard_assets)

    def _dashboard_auth_redirect_or_unavailable(
        self,
        request: web.Request,
        *,
        next_path: str,
        fallback_login_url: str,
    ) -> web.StreamResponse:
        challenge_builder = getattr(self, "_dashboard_auth_challenge", None)
        if callable(challenge_builder):
            try:
                response = challenge_builder(
                    request,
                    next_path=next_path,
                    allow_discord_admin_login=True,
                )
                if isinstance(response, web.StreamResponse):
                    return response
            except Exception:
                log.debug("Could not build dashboard auth challenge; fallback to login redirect", exc_info=True)
        return web.HTTPFound(fallback_login_url)

    def _admin_dashboard_host_page_gate(
        self, request: web.Request
    ) -> web.Response | None:
        host_checker = getattr(self, "_is_admin_dashboard_host_request", None)
        if not callable(host_checker):
            return None
        try:
            if not bool(host_checker(request)):
                return None
        except Exception:
            log.debug("Could not evaluate admin dashboard host gate", exc_info=True)
            return None
        # Keep user-facing dashboard pages strictly off the admin host.
        return web.Response(text="Not found.", status=404)

    def _admin_dashboard_public_origin(self) -> str:
        origin_getter = getattr(self, "_admin_announcement_public_origin", None)
        if callable(origin_getter):
            try:
                origin = str(origin_getter() or "").strip()
            except Exception:
                origin = ""
            if origin:
                return origin.rstrip("/")
        return "https://admin.earlysalty.de"

    def _admin_dashboard_login_url(self, request: web.Request) -> str:
        login_builder = getattr(self, "_build_discord_admin_login_url", None)
        if callable(login_builder):
            try:
                login_url = str(
                    login_builder(
                        request,
                        next_path=request.rel_url.path_qs if request.rel_url else request.path,
                    )
                    or ""
                ).strip()
            except Exception:
                login_url = ""
            if login_url:
                return login_url
        return ADMIN_DASHBOARD_DISCORD_LOGIN_URL

    def _admin_dashboard_spa_gate(self, request: web.Request) -> web.Response | None:
        local_checker = getattr(self, "_is_local_request", None)
        is_local_request = False
        if callable(local_checker):
            try:
                is_local_request = bool(local_checker(request))
            except Exception:
                is_local_request = False

        host_checker = getattr(self, "_is_admin_dashboard_host_request", None)
        is_admin_host = True
        if callable(host_checker):
            try:
                is_admin_host = bool(host_checker(request))
            except Exception:
                is_admin_host = False

        if not (is_local_request or is_admin_host):
            if request.method in {"GET", "HEAD"}:
                path_qs = request.rel_url.path_qs if request.rel_url else request.path
                return web.HTTPFound(f"{self._admin_dashboard_public_origin()}{path_qs}")
            return web.HTTPForbidden(
                text="This admin dashboard is only available on the admin dashboard host."
            )

        admin_checker = getattr(self, "_check_v2_admin_auth", None)
        if callable(admin_checker):
            try:
                if bool(admin_checker(request)):
                    return None
            except Exception:
                log.debug("Could not evaluate admin auth for admin dashboard SPA", exc_info=True)

        if not bool(getattr(self, "_discord_admin_required", True)):
            return web.HTTPServiceUnavailable(
                text=(
                    "Discord Admin OAuth ist nicht konfiguriert. "
                    "Admin-Zugriff ist bis zur vollständigen Konfiguration deaktiviert."
                )
            )

        login_url = self._admin_dashboard_login_url(request)
        return self._dashboard_auth_redirect_or_unavailable(
            request,
            next_path=request.rel_url.path_qs if request.rel_url else request.path,
            fallback_login_url=login_url,
        )

    @staticmethod
    def _admin_dashboard_path_should_serve_index(raw_path: str) -> bool:
        normalized_path = str(raw_path or "").strip("/")
        if not normalized_path:
            return True
        if normalized_path == "assets" or normalized_path.startswith("assets/"):
            return False
        return "." not in normalized_path.rsplit("/", 1)[-1]

    @staticmethod
    def _admin_dashboard_dist_root():
        import pathlib

        return (pathlib.Path(__file__).resolve().parents[1] / "admin_dashboard" / "dist").resolve()

    @staticmethod
    def _dashboard_runtime_script(runtime_config: dict[str, Any]) -> str:
        runtime_json = json.dumps(runtime_config, separators=(",", ":"), ensure_ascii=False)
        return (
            "<script>"
            f"window.__TWITCH_DASHBOARD_RUNTIME__=Object.freeze({runtime_json});"
            "</script>"
        )

    @classmethod
    def _inject_dashboard_runtime_config(
        cls,
        html: str,
        *,
        runtime_config: dict[str, Any],
        asset_prefix: str | None = None,
    ) -> str:
        rendered_html = html
        if asset_prefix and asset_prefix != "/twitch/dashboard-v2/":
            rendered_html = rendered_html.replace("/twitch/dashboard-v2/", asset_prefix)
        script = cls._dashboard_runtime_script(runtime_config)
        return rendered_html.replace("</head>", f"{script}\n  </head>", 1)

    async def _serve_demo_dashboard(self, request: web.Request) -> web.Response:
        """Serve the demo dashboard HTML without authentication."""
        import pathlib
        from .demo_data import ALLOWED_DEMO_PROFILES, DEFAULT_DEMO_PROFILE

        dist_path = pathlib.Path(__file__).parent / "dashboard_v2" / "dist" / "index.html"
        if not dist_path.exists():
            return web.Response(
                text="Dashboard not built. Run npm run build in dashboard_v2/",
                status=404,
            )
        html = dist_path.read_text(encoding="utf-8")
        runtime_config = {
            "apiBase": "/twitch/demo/api/v2",
            "demoMode": True,
            "allowedDemoProfiles": list(ALLOWED_DEMO_PROFILES),
            "defaultDemoProfile": DEFAULT_DEMO_PROFILE,
        }
        html = self._inject_dashboard_runtime_config(
            html,
            runtime_config=runtime_config,
            asset_prefix="/twitch/demo/dashboard-v2/",
        )
        return web.Response(text=html, content_type="text/html", charset="utf-8")

    async def _serve_dashboard_v2(self, request: web.Request) -> web.Response:
        """Serve the main dashboard HTML."""
        gate_response = self._admin_dashboard_host_page_gate(request)
        if gate_response is not None:
            return gate_response
        if not self._check_v2_auth(request):
            should_use_discord = getattr(self, "_should_use_discord_admin_login", None)
            if callable(should_use_discord) and bool(should_use_discord(request)):
                login_url = DASHBOARD_V2_DISCORD_LOGIN_URL
            else:
                login_url = DASHBOARD_V2_LOGIN_URL
            response = self._dashboard_auth_redirect_or_unavailable(
                request,
                next_path="/twitch/dashboard-v2",
                fallback_login_url=login_url,
            )
            if isinstance(response, web.HTTPException):
                raise response
            return response
        import pathlib

        dist_path = pathlib.Path(__file__).parent / "dashboard_v2" / "dist" / "index.html"
        if dist_path.exists():
            return web.FileResponse(dist_path)
        return web.Response(
            text="Dashboard not built. Run npm run build in dashboard_v2/", status=404
        )

    async def _serve_admin_dashboard(self, request: web.Request) -> web.Response:
        """Serve the dedicated admin dashboard SPA."""
        gate_response = self._admin_dashboard_spa_gate(request)
        if gate_response is not None:
            return gate_response

        dist_path = self._admin_dashboard_dist_root() / "index.html"
        if dist_path.exists():
            return web.FileResponse(dist_path)
        return web.Response(
            text="Admin dashboard not built. Run npm run build in bot/admin_dashboard/",
            status=404,
        )

    async def _serve_admin_dashboard_path(self, request: web.Request) -> web.Response:
        """Serve SPA routes or static files for the admin dashboard."""
        gate_response = self._admin_dashboard_spa_gate(request)
        if gate_response is not None:
            return gate_response

        raw_path = request.match_info.get("path", "")
        if str(raw_path or "").strip("/") == "legacy":
            return await DashboardLiveMixin.index(self, request)
        if self._admin_dashboard_path_should_serve_index(raw_path):
            return await self._serve_admin_dashboard(request)
        return self._resolve_admin_dashboard_asset_response(raw_path)

    async def _serve_dashboard(self, request: web.Request) -> web.Response:
        """Serve the new internal dashboard landing page."""
        gate_response = self._admin_dashboard_host_page_gate(request)
        if gate_response is not None:
            return gate_response
        if not self._check_v2_auth(request):
            should_use_discord = getattr(self, "_should_use_discord_admin_login", None)
            if callable(should_use_discord) and bool(should_use_discord(request)):
                login_url = DASHBOARD_DISCORD_LOGIN_URL
            else:
                login_url = DASHBOARD_LOGIN_URL
            response = self._dashboard_auth_redirect_or_unavailable(
                request,
                next_path="/twitch/dashboard",
                fallback_login_url=login_url,
            )
            if isinstance(response, web.HTTPException):
                raise response
            return response
        import pathlib

        dist_path = pathlib.Path(__file__).parent / "dashboard_v2" / "dist" / "index.html"
        if dist_path.exists():
            return web.FileResponse(dist_path)
        return web.Response(
            text="Dashboard not built. Run npm run build in dashboard_v2/", status=404
        )

    async def _serve_verwaltung(self, request: web.Request) -> web.Response:
        """Serve the account management page."""
        gate_response = self._admin_dashboard_host_page_gate(request)
        if gate_response is not None:
            return gate_response
        if not self._check_v2_auth(request):
            should_use_discord = getattr(self, "_should_use_discord_admin_login", None)
            if callable(should_use_discord) and bool(should_use_discord(request)):
                login_url = DASHBOARD_VERWALTUNG_DISCORD_LOGIN_URL
            else:
                login_url = DASHBOARD_VERWALTUNG_LOGIN_URL
            response = self._dashboard_auth_redirect_or_unavailable(
                request,
                next_path="/twitch/verwaltung",
                fallback_login_url=login_url,
            )
            if isinstance(response, web.HTTPException):
                raise response
            return response
        import pathlib

        dist_path = pathlib.Path(__file__).parent / "dashboard_v2" / "dist" / "index.html"
        if dist_path.exists():
            return web.FileResponse(dist_path)
        return web.Response(
            text="Dashboard not built. Run npm run build in dashboard_v2/", status=404
        )

    async def _serve_pricing(self, request: web.Request) -> web.Response:
        """Serve the pricing page."""
        gate_response = self._admin_dashboard_host_page_gate(request)
        if gate_response is not None:
            return gate_response
        if not self._check_v2_auth(request):
            should_use_discord = getattr(self, "_should_use_discord_admin_login", None)
            if callable(should_use_discord) and bool(should_use_discord(request)):
                login_url = DASHBOARD_PRICING_DISCORD_LOGIN_URL
            else:
                login_url = DASHBOARD_PRICING_LOGIN_URL
            response = self._dashboard_auth_redirect_or_unavailable(
                request,
                next_path="/twitch/pricing",
                fallback_login_url=login_url,
            )
            if isinstance(response, web.HTTPException):
                raise response
            return response
        import pathlib

        dist_path = pathlib.Path(__file__).parent / "dashboard_v2" / "dist" / "index.html"
        if dist_path.exists():
            html = dist_path.read_text(encoding="utf-8")
            html = self._inject_dashboard_runtime_config(
                html,
                runtime_config={
                    "apiBase": "/twitch/api/v2",
                    "demoMode": False,
                    "allowedDemoProfiles": [],
                },
            )
            return web.Response(text=html, content_type="text/html", charset="utf-8")
        return web.Response(
            text="Dashboard not built. Run npm run build in dashboard_v2/", status=404
        )

    async def _serve_affiliate_portal(self, request: web.Request) -> web.Response:
        """Serve the affiliate portal page."""
        gate_response = self._admin_dashboard_host_page_gate(request)
        if gate_response is not None:
            return gate_response
        dist_path = AFFILIATE_PORTAL_INDEX_PATH
        if dist_path.exists():
            return web.FileResponse(dist_path)
        return web.Response(
            text="Affiliate portal not built. Run npm run build in website/", status=404
        )

    async def _serve_website_dist_asset(self, request: web.Request) -> web.Response:
        """Serve static files from website/dist for embedded portal assets."""
        gate_response = self._admin_dashboard_host_page_gate(request)
        if gate_response is not None:
            return gate_response
        return self._resolve_website_dist_asset_response(request.match_info.get("path", ""))

    async def _serve_dashboard_v2_assets(self, request: web.Request) -> web.Response:
        """Serve static assets for the dashboard."""
        gate_response = self._admin_dashboard_host_page_gate(request)
        if gate_response is not None:
            return gate_response
        if not self._check_v2_auth(request):
            should_use_discord = getattr(self, "_should_use_discord_admin_login", None)
            if callable(should_use_discord) and bool(should_use_discord(request)):
                login_url = DASHBOARD_V2_DISCORD_LOGIN_URL
            else:
                login_url = DASHBOARD_V2_LOGIN_URL
            response = self._dashboard_auth_redirect_or_unavailable(
                request,
                next_path="/twitch/dashboard-v2",
                fallback_login_url=login_url,
            )
            if isinstance(response, web.HTTPException):
                raise response
            return response
        return self._resolve_dashboard_v2_asset_response(request.match_info.get("path", ""))

    async def _serve_admin_dashboard_assets(self, request: web.Request) -> web.Response:
        """Serve static assets for the admin dashboard."""
        gate_response = self._admin_dashboard_spa_gate(request)
        if gate_response is not None:
            return gate_response
        raw_path = str(request.match_info.get("path", "") or "").strip("/")
        if not raw_path:
            return web.Response(text="Not found", status=404)
        return self._resolve_admin_dashboard_asset_response(f"assets/{raw_path}")

    async def _serve_demo_dashboard_assets(self, request: web.Request) -> web.Response:
        """Serve static assets for the public demo dashboard without auth."""
        return self._resolve_dashboard_v2_asset_response(request.match_info.get("path", ""))

    def _resolve_dashboard_v2_asset_response(self, raw_path: str) -> web.StreamResponse:
        """Resolve dashboard-v2 dist files with strict path validation."""
        import pathlib

        if not raw_path:
            return web.Response(text="Not found", status=404)

        dist_root = (pathlib.Path(__file__).resolve().parent / "dashboard_v2" / "dist").resolve()
        candidate: pathlib.Path = dist_root

        # Resolve each path segment against actual directory entries to avoid
        # using untrusted input directly in filesystem path expressions.
        for segment in raw_path.split("/"):
            if not segment or segment in {".", ".."} or "\\" in segment:
                return web.Response(text="Not found", status=404)
            if not candidate.is_dir():
                return web.Response(text="Not found", status=404)

            next_candidate = None
            for entry in candidate.iterdir():
                if entry.name == segment:
                    next_candidate = entry
                    break
            if next_candidate is None:
                return web.Response(text="Not found", status=404)
            candidate = next_candidate

        try:
            candidate.resolve().relative_to(dist_root)
        except ValueError:
            return web.Response(text="Not found", status=404)

        if candidate.is_file():
            return web.FileResponse(candidate)
        return web.Response(text="Not found", status=404)

    def _resolve_website_dist_asset_response(self, raw_path: str) -> web.StreamResponse:
        """Resolve website/dist files with strict path validation."""
        if not raw_path:
            return web.Response(text="Not found", status=404)

        dist_root = WEBSITE_DIST_ROOT_PATH.resolve()
        candidate = dist_root

        for segment in raw_path.split("/"):
            if not segment or segment in {".", ".."} or "\\" in segment:
                return web.Response(text="Not found", status=404)
            if not candidate.is_dir():
                return web.Response(text="Not found", status=404)

            next_candidate = None
            for entry in candidate.iterdir():
                if entry.name == segment:
                    next_candidate = entry
                    break
            if next_candidate is None:
                return web.Response(text="Not found", status=404)
            candidate = next_candidate

        try:
            candidate.resolve().relative_to(dist_root)
        except ValueError:
            return web.Response(text="Not found", status=404)

        if candidate.is_file():
            return web.FileResponse(candidate)
        return web.Response(text="Not found", status=404)

    def _resolve_admin_dashboard_asset_response(self, raw_path: str) -> web.StreamResponse:
        """Resolve admin dashboard dist files with strict path validation."""
        if not raw_path:
            return web.Response(text="Not found", status=404)

        dist_root = self._admin_dashboard_dist_root()
        candidate = dist_root

        for segment in raw_path.split("/"):
            if not segment or segment in {".", ".."} or "\\" in segment:
                return web.Response(text="Not found", status=404)
            if not candidate.is_dir():
                return web.Response(text="Not found", status=404)

            next_candidate = None
            for entry in candidate.iterdir():
                if entry.name == segment:
                    next_candidate = entry
                    break
            if next_candidate is None:
                return web.Response(text="Not found", status=404)
            candidate = next_candidate

        try:
            candidate.resolve().relative_to(dist_root)
        except ValueError:
            return web.Response(text="Not found", status=404)

        if candidate.is_file():
            return web.FileResponse(candidate)
        return web.Response(text="Not found", status=404)

    async def _api_v2_overview(self, request: web.Request) -> web.Response:
        """Main overview endpoint with all dashboard data."""
        self._require_v2_auth(request)

        streamer = request.query.get("streamer", "").strip() or None
        days = min(max(int(request.query.get("days", "30")), 7), 365)

        try:
            data = await self._get_overview_data(streamer, days)
            return web.json_response(data)
        except Exception as exc:
            log.exception("Error in overview API")
            return analytics_internal_error_response()

    async def _get_overview_data(self, streamer: str | None, days: int) -> dict[str, Any]:
        """Get comprehensive overview data for the dashboard."""
        with storage.readonly_connection() as conn:
            since_date = (datetime.now(UTC) - timedelta(days=days)).isoformat()
            prev_since_date = (datetime.now(UTC) - timedelta(days=days * 2)).isoformat()

            streamer_login = streamer.lower() if streamer else None

            # Check data exists
            count = conn.execute(
                """
                SELECT COUNT(*)
                FROM twitch_stream_sessions s
                WHERE s.started_at >= %s
                  AND s.ended_at IS NOT NULL
                  AND (COALESCE(%s, '') = '' OR LOWER(s.streamer_login) = %s)
                """,
                [since_date, streamer_login, streamer_login],
            ).fetchone()[0]

            if count == 0:
                return {"empty": True, "error": "Keine Daten für den Zeitraum"}

            # Get sessions
            sessions = self._get_sessions(conn, since_date, streamer, 50)

            # Calculate metrics
            metrics = self._calculate_overview_metrics(conn, since_date, streamer)

            # Calculate previous period metrics for trends
            prev_metrics = conn.execute(
                """
                SELECT
                    AVG(s.avg_viewers) as avg_viewers,
                    SUM(CASE WHEN s.follower_delta IS NOT NULL
                         AND NOT (s.followers_end = 0 AND s.followers_start > 0)
                         THEN s.follower_delta ELSE 0 END) as followers,
                    AVG(CASE
                        WHEN s.avg_viewers >= 3 AND s.peak_viewers > 0
                        THEN LEAST(1.0, s.retention_10m)
                        ELSE NULL
                    END) as retention
                FROM twitch_stream_sessions s
                WHERE s.started_at >= %s AND s.started_at < %s
                  AND s.ended_at IS NOT NULL
                  AND (COALESCE(%s, '') = '' OR LOWER(s.streamer_login) = %s)
                """,
                [prev_since_date, since_date, streamer_login, streamer_login],
            ).fetchone()

            # Calculate trends
            def calc_trend(curr, prev):
                if not prev or prev == 0:
                    return None
                return round(((curr - prev) / abs(prev)) * 100, 1)

            prev_avg = float(prev_metrics[0]) if prev_metrics and prev_metrics[0] else 0
            prev_fol = int(prev_metrics[1]) if prev_metrics and prev_metrics[1] else 0
            prev_ret = float(prev_metrics[2]) * 100 if prev_metrics and prev_metrics[2] else 0

            # Count previous period retention samples for reliability check
            prev_ret_sample = conn.execute(
                """
                SELECT COUNT(*) FROM twitch_stream_sessions s
                WHERE s.started_at >= %s AND s.started_at < %s
                  AND s.ended_at IS NOT NULL
                  AND s.avg_viewers >= 3 AND s.peak_viewers > 0
                  AND s.retention_10m IS NOT NULL
                  AND (COALESCE(%s, '') = '' OR LOWER(s.streamer_login) = %s)
                """,
                [prev_since_date, since_date, streamer_login, streamer_login],
            ).fetchone()
            prev_retention_sample_count = prev_ret_sample[0] if prev_ret_sample else 0

            avg_viewers_trend = calc_trend(metrics.get("avg_avg_viewers", 0), prev_avg)

            # Follower trend: cap at ±999% and suppress when absolute values too small
            curr_fol = metrics.get("total_followers", 0)
            raw_fol_trend = calc_trend(curr_fol, prev_fol)
            if raw_fol_trend is not None and (abs(curr_fol) < 5 and abs(prev_fol) < 5):
                followers_trend = None  # Too few absolute followers for meaningful trend
            elif raw_fol_trend is not None:
                followers_trend = max(-999.0, min(999.0, raw_fol_trend))
            else:
                followers_trend = None

            # Retention trend: only show when BOTH periods have >= 3 sessions
            retention_reliable = metrics.get("retention_sample_count", 0) >= 3
            prev_retention_reliable = prev_retention_sample_count >= 3
            if retention_reliable and prev_retention_reliable and prev_ret > 0:
                retention_trend = calc_trend(metrics.get("avg_retention_10m", 0), prev_ret)
            else:
                retention_trend = None

            # Calculate category percentile for health score
            category_percentile = None
            category_rank = None
            category_total = None
            if streamer:
                cat_data = self._get_category_percentiles(conn, since_date)
                if cat_data["sorted_avgs"]:
                    streamer_avg = cat_data["streamer_map"].get(streamer.lower())
                    if streamer_avg is not None:
                        category_percentile = self._percentile_of(
                            cat_data["sorted_avgs"], streamer_avg
                        )
                        category_total = cat_data["total"]
                        # Rank = total - position (1 = best)
                        category_rank = category_total - int(category_percentile * category_total)

            # Get network stats
            network = self._get_network_stats(conn, since_date, streamer)

            # Get monetization event counts for health score
            monetization_events = self._get_monetization_event_counts(conn, since_date, streamer)

            # Calculate scores
            scores = self._calculate_health_scores(metrics, category_percentile, network, monetization_events)

            # Generate insights
            findings = self._generate_insights(metrics)
            actions = self._generate_actions(metrics)

            # Correlations
            correlations = self._calculate_correlations(sessions)

            result: dict[str, Any] = {
                "streamer": streamer,
                "days": days,
                "scores": scores,
                "summary": {
                    "avgViewers": metrics.get("avg_avg_viewers", 0),
                    "peakViewers": metrics.get("max_peak_viewers", 0),
                    "totalHoursWatched": metrics.get("total_hours_watched", 0),
                    "totalAirtime": metrics.get("total_airtime_hours", 0),
                    "followersDelta": metrics.get("total_followers", 0),
                    "followersGained": metrics.get("gained_followers", 0),
                    "followersPerHour": metrics.get("followers_per_hour", 0),
                    "followersGainedPerHour": metrics.get("gained_followers_per_hour", 0),
                    "retention10m": metrics.get("avg_retention_10m", 0),
                    "retentionReliable": metrics.get("retention_sample_count", 0) >= 3,
                    "uniqueChatters": metrics.get("total_unique_chatters", 0),
                    "streamCount": count,
                    # Trend indicators
                    "avgViewersTrend": avg_viewers_trend,
                    "followersTrend": followers_trend,
                    "retentionTrend": retention_trend,
                },
                "sessions": sessions,
                "findings": findings,
                "actions": actions,
                "correlations": correlations,
                "network": network,
                "dataQuality": {
                    "botFilterApplied": True,
                },
            }
            if category_rank is not None:
                result["categoryRank"] = category_rank
                result["categoryTotal"] = category_total
            return result

    def _get_sessions(
        self, conn, since_date: str, streamer: str | None, limit: int = 50
    ) -> list[dict]:
        """Get list of sessions with metrics."""
        streamer_login = streamer.lower() if streamer else None
        session_bot_clause, session_bot_params = build_known_chat_bot_not_in_clause(
            column_expr="sc.chatter_login"
        )
        rows = conn.execute(
            f"""
            WITH base_sessions AS (
                SELECT
                    s.id,
                    s.started_at,
                    s.duration_seconds,
                    s.start_viewers,
                    s.peak_viewers,
                    s.end_viewers,
                    s.avg_viewers,
                    s.retention_5m,
                    s.retention_10m,
                    s.retention_20m,
                    s.dropoff_pct,
                    s.unique_chatters,
                    s.first_time_chatters,
                    s.returning_chatters,
                    s.followers_start,
                    s.followers_end,
                    s.stream_title
                FROM twitch_stream_sessions s
                WHERE s.started_at >= %s
                  AND s.ended_at IS NOT NULL
                  AND (COALESCE(%s, '') = '' OR LOWER(s.streamer_login) = %s)
                ORDER BY s.started_at DESC
                LIMIT %s
            ),
            filtered_chatters AS (
                SELECT
                    sc.session_id,
                    COUNT(
                        DISTINCT CASE
                            WHEN sc.messages > 0
                            THEN COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id)
                            ELSE NULL
                        END
                    ) AS unique_chatters,
                    COUNT(
                        DISTINCT CASE
                            WHEN sc.messages > 0
                                 AND LOWER(COALESCE(CAST(sc.is_first_time_streamer AS TEXT), '0')) IN ('1', 't', 'true')
                            THEN COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id)
                            ELSE NULL
                        END
                    ) AS first_time_chatters,
                    COUNT(
                        DISTINCT CASE
                            WHEN sc.messages > 0
                                 AND LOWER(COALESCE(CAST(sc.is_first_time_streamer AS TEXT), '0')) NOT IN ('1', 't', 'true')
                            THEN COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id)
                            ELSE NULL
                        END
                    ) AS returning_chatters
                FROM twitch_session_chatters sc
                JOIN base_sessions bs ON bs.id = sc.session_id
                WHERE {session_bot_clause}
                GROUP BY sc.session_id
            ),
            session_chatter_presence AS (
                SELECT sc.session_id, 1 AS has_any_chatters
                FROM twitch_session_chatters sc
                JOIN base_sessions bs ON bs.id = sc.session_id
                GROUP BY sc.session_id
            )
            SELECT
                bs.id,
                CAST(bs.started_at AS DATE) AS start_date,
                CAST(bs.started_at AS TIME) AS start_time,
                bs.duration_seconds,
                bs.start_viewers,
                bs.peak_viewers,
                bs.end_viewers,
                bs.avg_viewers,
                COALESCE(bs.retention_5m, 0),
                COALESCE(bs.retention_10m, 0),
                COALESCE(bs.retention_20m, 0),
                COALESCE(bs.dropoff_pct, 0),
                CASE
                    WHEN scp.has_any_chatters = 1 THEN COALESCE(fc.unique_chatters, 0)
                    ELSE COALESCE(bs.unique_chatters, 0)
                END,
                CASE
                    WHEN scp.has_any_chatters = 1 THEN COALESCE(fc.first_time_chatters, 0)
                    ELSE COALESCE(bs.first_time_chatters, 0)
                END,
                CASE
                    WHEN scp.has_any_chatters = 1 THEN COALESCE(fc.returning_chatters, 0)
                    ELSE COALESCE(bs.returning_chatters, 0)
                END,
                COALESCE(bs.followers_start, 0),
                COALESCE(bs.followers_end, 0),
                COALESCE(bs.stream_title, '')
            FROM base_sessions bs
            LEFT JOIN filtered_chatters fc ON fc.session_id = bs.id
            LEFT JOIN session_chatter_presence scp ON scp.session_id = bs.id
            ORDER BY bs.started_at DESC
        """,
            [
                since_date,
                streamer_login,
                streamer_login,
                limit,
                *session_bot_params,
            ],
        ).fetchall()

        sessions: list[dict[str, Any]] = []
        for r in rows:
            date_val = r[1]
            time_val = r[2]
            date_str = date_val.isoformat() if hasattr(date_val, "isoformat") else (date_val or "")
            time_str = time_val.isoformat() if hasattr(time_val, "isoformat") else (time_val or "")
            peak_viewers = int(r[5]) if r[5] else 0
            avg_viewers = float(r[7]) if r[7] else 0.0
            # Begrenze Retention nur hart auf 100%, nicht auf avg/peak (verzerrt wachsende Streams)
            retention_cap = 1.0

            raw_ret_5m = float(r[8]) if r[8] else 0.0
            raw_ret_10m = float(r[9]) if r[9] else 0.0
            raw_ret_20m = float(r[10]) if r[10] else 0.0
            ret_5m = max(0.0, min(raw_ret_5m, retention_cap))
            ret_10m = max(0.0, min(raw_ret_10m, retention_cap))
            ret_20m = max(0.0, min(raw_ret_20m, retention_cap))

            sessions.append(
                {
                    "id": r[0],
                    "date": date_str,
                    "startTime": time_str,
                    "duration": r[3] or 0,
                    "startViewers": r[4] or 0,
                    "peakViewers": peak_viewers,
                    "endViewers": r[6] or 0,
                    "avgViewers": avg_viewers,
                    "retention5m": ret_5m * 100,
                    "retention10m": ret_10m * 100,
                    "retention20m": ret_20m * 100,
                    "dropoffPct": float(r[11]) * 100 if r[11] else 0,
                    "uniqueChatters": r[12] or 0,
                    "firstTimeChatters": r[13] or 0,
                    "returningChatters": r[14] or 0,
                    "followersStart": r[15] or 0,
                    "followersEnd": r[16] or 0,
                    "title": r[17] or "",
                }
            )

        return sessions

    def _calculate_overview_metrics(
        self, conn, since_date: str, streamer: str | None
    ) -> dict[str, Any]:
        """Calculate all overview metrics."""
        streamer_login = streamer.lower() if streamer else None
        session_bot_clause, session_bot_params = build_known_chat_bot_not_in_clause(
            column_expr="sc.chatter_login"
        )
        rollup_bot_clause, rollup_bot_params = build_known_chat_bot_not_in_clause(
            column_expr="chatter_login"
        )

        row = conn.execute(
            f"""
            WITH filtered_session_chatters AS (
                SELECT
                    sc.session_id,
                    COUNT(
                        DISTINCT CASE
                            WHEN sc.messages > 0
                            THEN COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id)
                            ELSE NULL
                        END
                    ) AS unique_chatters
                FROM twitch_session_chatters sc
                JOIN twitch_stream_sessions s ON s.id = sc.session_id
                WHERE s.started_at >= %s
                  AND s.ended_at IS NOT NULL
                  AND (COALESCE(%s, '') = '' OR LOWER(s.streamer_login) = %s)
                  AND {session_bot_clause}
                GROUP BY sc.session_id
            ),
            session_chatter_presence AS (
                SELECT
                    sc.session_id,
                    1 AS has_any_chatters
                FROM twitch_session_chatters sc
                JOIN twitch_stream_sessions s ON s.id = sc.session_id
                WHERE s.started_at >= %s
                  AND s.ended_at IS NOT NULL
                  AND (COALESCE(%s, '') = '' OR LOWER(s.streamer_login) = %s)
                GROUP BY sc.session_id
            )
            SELECT
                AVG(s.avg_viewers) as avg_avg_viewers,
                MAX(s.peak_viewers) as max_peak_viewers,
                SUM(s.avg_viewers * s.duration_seconds / 3600.0) as total_hours_watched,
                SUM(s.duration_seconds / 3600.0) as total_airtime_hours,
                SUM(CASE WHEN s.follower_delta IS NOT NULL
                     AND NOT (s.followers_end = 0 AND s.followers_start > 0)
                     THEN s.follower_delta ELSE 0 END) as total_followers,
                AVG(CASE
                    WHEN s.avg_viewers >= 3 AND s.peak_viewers > 0
                    THEN LEAST(1.0, s.retention_5m)
                    ELSE NULL
                END) as avg_retention_5m,
                AVG(CASE
                    WHEN s.avg_viewers >= 3 AND s.peak_viewers > 0
                    THEN LEAST(1.0, s.retention_10m)
                    ELSE NULL
                END) as avg_retention_10m,
                AVG(CASE
                    WHEN s.avg_viewers >= 3 AND s.peak_viewers > 0
                    THEN LEAST(1.0, s.retention_20m)
                    ELSE NULL
                END) as avg_retention_20m,
                AVG(s.dropoff_pct) as avg_dropoff,
                COALESCE(SUM(
                    CASE
                        WHEN scp.has_any_chatters = 1 THEN COALESCE(fsc.unique_chatters, 0)
                        ELSE COALESCE(s.unique_chatters, 0)
                    END
                ), 0) as total_unique_chatters,
                AVG(CASE
                    WHEN s.avg_viewers >= 3 AND s.peak_viewers > 0
                    THEN LEAST(
                        100.0,
                        (
                            CASE
                                WHEN scp.has_any_chatters = 1 THEN COALESCE(fsc.unique_chatters, 0)
                                ELSE COALESCE(s.unique_chatters, 0)
                            END
                        ) * 100.0 / NULLIF(s.peak_viewers, 0)
                    )
                    ELSE NULL
                END) as chat_per_100
            FROM twitch_stream_sessions s
            LEFT JOIN filtered_session_chatters fsc ON fsc.session_id = s.id
            LEFT JOIN session_chatter_presence scp ON scp.session_id = s.id
            WHERE s.started_at >= %s
              AND s.ended_at IS NOT NULL
              AND (COALESCE(%s, '') = '' OR LOWER(s.streamer_login) = %s)
        """,
            [
                since_date,
                streamer_login,
                streamer_login,
                *session_bot_params,
                since_date,
                streamer_login,
                streamer_login,
                since_date,
                streamer_login,
                streamer_login,
            ],
        ).fetchone()

        total_airtime = float(row[3]) if row[3] else 0
        total_followers = int(row[4]) if row[4] else 0  # NET (can be negative)

        # Gained followers = only positive session deltas (ignores unfollows)
        gained_row = conn.execute(
            """
            SELECT COALESCE(SUM(CASE WHEN s.follower_delta > 0
                 AND NOT (s.followers_end = 0 AND s.followers_start > 0)
                 THEN s.follower_delta ELSE 0 END), 0)
            FROM twitch_stream_sessions s
            WHERE s.started_at >= %s
              AND s.ended_at IS NOT NULL
              AND (COALESCE(%s, '') = '' OR LOWER(s.streamer_login) = %s)
        """,
            [since_date, streamer_login, streamer_login],
        ).fetchone()
        gained_followers = int(gained_row[0]) if gained_row and gained_row[0] else 0

        # True unique chatters from rollup table (not SUM of per-session counts)
        unique_chatters_sum = int(row[9]) if row[9] else 0
        if streamer:
            true_unique = conn.execute(
                f"""
                SELECT COUNT(DISTINCT chatter_login)
                FROM twitch_chatter_rollup
                WHERE LOWER(streamer_login) = %s
                  AND {rollup_bot_clause}
            """,
                [streamer.lower(), *rollup_bot_params],
            ).fetchone()
            unique_chatters = (
                int(true_unique[0]) if true_unique and true_unique[0] else unique_chatters_sum
            )
        else:
            unique_chatters = unique_chatters_sum

        # Sample counts for data quality gating
        sample_row = conn.execute(
            """
            SELECT
                COUNT(CASE
                    WHEN s.avg_viewers >= 3 AND s.peak_viewers > 0 AND s.retention_10m IS NOT NULL THEN 1
                END),
                COUNT(CASE
                    WHEN s.avg_viewers >= 3 AND s.peak_viewers > 0 AND s.unique_chatters IS NOT NULL THEN 1
                END),
                COUNT(CASE WHEN s.follower_delta IS NOT NULL
                     AND NOT (s.followers_end = 0 AND s.followers_start > 0) THEN 1 END)
            FROM twitch_stream_sessions s
            WHERE s.started_at >= %s
              AND s.ended_at IS NOT NULL
              AND (COALESCE(%s, '') = '' OR LOWER(s.streamer_login) = %s)
        """,
            [since_date, streamer_login, streamer_login],
        ).fetchone()
        retention_sample_count = int(sample_row[0]) if sample_row else 0
        chat_sample_count = int(sample_row[1]) if sample_row else 0
        follower_valid_count = int(sample_row[2]) if sample_row else 0

        # Active chatters (at least 1 message) for engagement rate calculation
        active_chatters_row = conn.execute(
            f"""
            SELECT COUNT(DISTINCT COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id))
            FROM twitch_session_chatters sc
            JOIN twitch_stream_sessions s ON s.id = sc.session_id
            WHERE s.started_at >= %s
              AND s.ended_at IS NOT NULL
              AND (COALESCE(%s, '') = '' OR LOWER(s.streamer_login) = %s)
              AND sc.messages > 0
              AND {session_bot_clause}
            """,
            [since_date, streamer_login, streamer_login, *session_bot_params],
        ).fetchone()
        active_chatters = (
            int(active_chatters_row[0]) if active_chatters_row and active_chatters_row[0] else 0
        )

        # Distinct Zuschauer (Chatters + Chatters-API ohne Nachrichten)
        distinct_viewers_row = conn.execute(
            f"""
            SELECT COUNT(DISTINCT COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id))
            FROM twitch_session_chatters sc
            JOIN twitch_stream_sessions s ON s.id = sc.session_id
            WHERE s.started_at >= %s
              AND s.ended_at IS NOT NULL
              AND (COALESCE(%s, '') = '' OR LOWER(s.streamer_login) = %s)
              AND (sc.messages > 0 OR COALESCE(sc.seen_via_chatters_api, FALSE) IS TRUE)
              AND {session_bot_clause}
            """,
            [since_date, streamer_login, streamer_login, *session_bot_params],
        ).fetchone()
        distinct_viewers = (
            int(distinct_viewers_row[0]) if distinct_viewers_row and distinct_viewers_row[0] else 0
        )

        avg_viewers = float(row[0]) if row[0] else 0
        engagement_rate = (active_chatters / distinct_viewers * 100) if distinct_viewers > 0 else 0

        return {
            "avg_avg_viewers": avg_viewers,
            "max_peak_viewers": int(row[1]) if row[1] else 0,
            "total_hours_watched": float(row[2]) if row[2] else 0,
            "total_airtime_hours": total_airtime,
            "total_followers": total_followers,
            "gained_followers": gained_followers,
            "followers_per_hour": total_followers / total_airtime if total_airtime > 0 else 0,
            "gained_followers_per_hour": gained_followers / total_airtime
            if total_airtime > 0
            else 0,
            "avg_retention_5m": float(row[5]) * 100 if row[5] else 0,
            "avg_retention_10m": float(row[6]) * 100 if row[6] else 0,
            "avg_retention_20m": float(row[7]) * 100 if row[7] else 0,
            "avg_dropoff": float(row[8]) * 100 if row[8] else 0,
            "total_unique_chatters": unique_chatters,
            "active_chatters": active_chatters,
            "unique_viewers": distinct_viewers,
            "engagement_rate": round(engagement_rate, 2),
            "chat_per_100": float(row[10]) if row[10] else 0,
            "retention_sample_count": retention_sample_count,
            "chat_sample_count": chat_sample_count,
            "follower_valid_count": follower_valid_count,
        }

    def _calculate_health_scores(
        self,
        metrics: dict[str, Any],
        category_percentile: float | None = None,
        network_stats: dict[str, int] | None = None,
        monetization_events: dict[str, int] | None = None,
    ) -> dict[str, int]:
        """Calculate health scores from metrics."""
        avg_viewers = metrics.get("avg_avg_viewers", 0)

        # Reach: Based on percentile ranking in category if available, else fallback
        if category_percentile is not None:
            reach = min(100, int(20 + category_percentile * 80))
        else:
            reach = min(100, int(avg_viewers / 5))  # fallback

        # Retention: Based on 10m retention (neutral if insufficient data)
        ret_10m = metrics.get("avg_retention_10m", 0)
        if metrics.get("retention_sample_count", 0) < 3:
            retention = 50
        else:
            retention = min(100, int(ret_10m * 1.5))  # 66% = 100

        # Engagement: % of avg viewers who actively chatted (0-10% bad, 10-20% ok, 20%+ gut)
        engagement_rate = metrics.get("engagement_rate", 0)
        if metrics.get("chat_sample_count", 0) < 3:
            engagement = 50
        else:
            engagement = min(100, int(engagement_rate * 5))  # 20% engagement_rate = 100

        # Growth: Based on followers per hour (floor at 0, negative fph = 0 growth)
        fph = max(0, metrics.get("followers_per_hour", 0))
        growth = min(100, int(fph * 20))  # 5 fph = 100

        # Monetization: Based on real event data (subs, bits, hype trains)
        if monetization_events:
            session_count = max(1, metrics.get("session_count", 1))
            # Weighted score: subs are most valuable, hype trains are rare but high-signal
            weighted = (
                monetization_events.get("sub_events", 0) * 3
                + monetization_events.get("bits_events", 0)
                + monetization_events.get("hype_trains", 0) * 5
            )
            # Normalize: ~10 weighted events per session = score 100
            monetization = min(100, max(0, int((weighted / session_count) * 10)))
        else:
            monetization = 0

        # Network: Based on actual raid activity
        if network_stats:
            sent = network_stats.get("sent", 0)
            received = network_stats.get("received", 0)
            total_raids = sent + received
            # Reciprocity bonus: bidirectional raiding is more valuable
            reciprocity_bonus = min(sent, received) * 10
            # Score: each raid = ~8 points, reciprocity adds up to 20 bonus
            network = min(100, max(0, total_raids * 8 + reciprocity_bonus))
        else:
            network = 0

        # Total: Weighted average
        total = int(
            reach * 0.2
            + retention * 0.25
            + engagement * 0.2
            + growth * 0.15
            + monetization * 0.1
            + network * 0.1
        )

        return {
            "total": total,
            "reach": reach,
            "retention": retention,
            "engagement": engagement,
            "growth": growth,
            "monetization": monetization,
            "network": network,
        }

    def _get_network_stats(self, conn, since_date: str, streamer: str | None) -> dict[str, int]:
        """Get raid network statistics."""
        if not streamer:
            return {"sent": 0, "received": 0, "sentViewers": 0}

        sent = conn.execute(
            """
            SELECT COUNT(*), COALESCE(SUM(viewer_count), 0)
            FROM twitch_raid_history
            WHERE LOWER(from_broadcaster_login) = %s AND executed_at >= %s AND COALESCE(success, FALSE) IS TRUE
        """,
            [streamer.lower(), since_date],
        ).fetchone()

        received = conn.execute(
            """
            SELECT COUNT(*)
            FROM twitch_raid_history
            WHERE LOWER(to_broadcaster_login) = %s AND executed_at >= %s AND COALESCE(success, FALSE) IS TRUE
        """,
            [streamer.lower(), since_date],
        ).fetchone()

        return {
            "sent": sent[0] if sent else 0,
            "sentViewers": int(sent[1]) if sent else 0,
            "received": received[0] if received else 0,
        }

    def _get_monetization_event_counts(
        self, conn, since_date: str, streamer: str | None
    ) -> dict[str, int]:
        """Get monetization event counts for health score calculation."""
        if not streamer:
            return {"sub_events": 0, "bits_events": 0, "hype_trains": 0}

        sl = streamer.lower()
        try:
            subs_row = conn.execute(
                """
                SELECT COUNT(*)
                  FROM twitch_subscription_events e
                  LEFT JOIN twitch_stream_sessions s ON s.id = e.session_id
                  LEFT JOIN twitch_live_state l ON l.twitch_user_id = e.twitch_user_id
                 WHERE e.received_at >= %s
                   AND (%s = '' OR LOWER(COALESCE(s.streamer_login, l.streamer_login, '')) = %s)
                """,
                [since_date, sl, sl],
            ).fetchone()
            sub_events = int(subs_row[0]) if subs_row else 0
        except Exception:
            log.debug("Subscription event count query failed", exc_info=True)
            sub_events = 0

        try:
            bits_row = conn.execute(
                """
                SELECT COUNT(*)
                  FROM twitch_bits_events e
                  LEFT JOIN twitch_stream_sessions s ON s.id = e.session_id
                  LEFT JOIN twitch_live_state l ON l.twitch_user_id = e.twitch_user_id
                 WHERE e.received_at >= %s
                   AND (%s = '' OR LOWER(COALESCE(s.streamer_login, l.streamer_login, '')) = %s)
                """,
                [since_date, sl, sl],
            ).fetchone()
            bits_events = int(bits_row[0]) if bits_row else 0
        except Exception:
            log.debug("Bits event count query failed", exc_info=True)
            bits_events = 0

        try:
            ht_row = conn.execute(
                """
                SELECT COUNT(*) FROM twitch_hype_train_events h
                LEFT JOIN twitch_stream_sessions s ON s.id = h.session_id
                WHERE h.started_at >= %s AND h.ended_at IS NOT NULL
                  AND (%s = '' OR LOWER(s.streamer_login) = %s)
                """,
                [since_date, sl, sl],
            ).fetchone()
            hype_trains = int(ht_row[0]) if ht_row else 0
        except Exception:
            hype_trains = 0

        return {
            "sub_events": sub_events,
            "bits_events": bits_events,
            "hype_trains": hype_trains,
        }

    def _calculate_correlations(self, sessions: list[dict]) -> dict[str, float]:
        """Calculate metric correlations."""
        if len(sessions) < 3:
            return {"durationVsViewers": 0, "chatVsRetention": 0}

        # Simple correlation approximation
        durations = [s["duration"] for s in sessions]
        viewers = [s["avgViewers"] for s in sessions]
        chatters = [s["uniqueChatters"] for s in sessions]
        retention = [s["retention10m"] for s in sessions]

        def corr(a, b):
            if len(a) < 2:
                return 0
            mean_a = sum(a) / len(a)
            mean_b = sum(b) / len(b)
            num = sum((x - mean_a) * (y - mean_b) for x, y in zip(a, b, strict=False))
            den_a = sum((x - mean_a) ** 2 for x in a) ** 0.5
            den_b = sum((y - mean_b) ** 2 for y in b) ** 0.5
            if den_a == 0 or den_b == 0:
                return 0
            return round(num / (den_a * den_b), 2)

        return {
            "durationVsViewers": corr(durations, viewers),
            "chatVsRetention": corr(chatters, retention),
        }

    async def _api_v2_lurker_analysis(self, request: web.Request) -> web.Response:
        """Return basic lurker metrics for a streamer or fall back to demo data."""
        self._require_v2_auth(request)
        self._require_extended_plan(request)

        streamer = request.query.get("streamer", "").strip() or None
        days = min(max(int(request.query.get("days", "30")), 7), 365)

        if not streamer:
            return web.json_response(
                {"dataAvailable": False, "message": "Streamer required"}, status=400
            )

        since_date = (datetime.now(UTC) - timedelta(days=days)).isoformat()
        try:
            with storage.readonly_connection() as conn:
                session_bot_clause, session_bot_params = build_known_chat_bot_not_in_clause(
                    column_expr="sc.chatter_login"
                )
                agg_row = conn.execute(
                    f"""
                    WITH sessions AS (
                        SELECT id
                        FROM twitch_stream_sessions
                        WHERE started_at >= %s
                          AND ended_at IS NOT NULL
                          AND LOWER(streamer_login) = %s
                    ),
                    chatter AS (
                        SELECT
                            COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id) AS viewer_id,
                            COUNT(DISTINCT sc.session_id) AS session_count,
                            SUM(COALESCE(sc.messages, 0)) AS msg_sum,
                            SUM(
                                CASE
                                    WHEN sc.messages = 0 AND COALESCE(sc.seen_via_chatters_api, FALSE) IS TRUE
                                    THEN 1 ELSE 0
                                END
                            ) AS lurk_samples,
                            SUM(CASE WHEN sc.messages > 0 THEN 1 ELSE 0 END) AS active_samples,
                            MAX(
                                CASE
                                    WHEN COALESCE(sc.seen_via_chatters_api, FALSE) IS TRUE
                                    THEN 1 ELSE 0
                                END
                            ) AS seen_via_api,
                            MIN(
                                CASE
                                    WHEN sc.messages = 0 AND COALESCE(sc.seen_via_chatters_api, FALSE) IS TRUE
                                    THEN COALESCE(sc.first_message_at, sc.last_seen_at)
                                    ELSE NULL
                                END
                            ) AS first_lurk_seen,
                            MIN(
                                CASE
                                    WHEN sc.messages > 0
                                    THEN COALESCE(sc.first_message_at, sc.last_seen_at)
                                    ELSE NULL
                                END
                            ) AS first_active_seen,
                            MIN(sc.first_message_at) AS first_seen,
                            MAX(sc.last_seen_at) AS last_seen
                        FROM twitch_session_chatters sc
                        JOIN sessions s ON s.id = sc.session_id
                        WHERE {session_bot_clause}
                        GROUP BY 1
                    )
                    SELECT
                        COUNT(*) AS total_viewers,
                        COUNT(*) FILTER (WHERE seen_via_api = 1) AS seen_sample_viewers,
                        COUNT(*) FILTER (WHERE seen_via_api = 1 AND msg_sum = 0) AS lurker_count,
                        AVG(session_count) FILTER (WHERE seen_via_api = 1 AND msg_sum = 0) AS avg_sessions_lurkers,
                        COUNT(*) FILTER (
                            WHERE seen_via_api = 1
                              AND lurk_samples > 0
                        ) AS eligible_lurkers,
                        COUNT(*) FILTER (
                            WHERE seen_via_api = 1
                              AND lurk_samples > 0
                              AND active_samples > 0
                              AND first_active_seen IS NOT NULL
                              AND first_lurk_seen IS NOT NULL
                              AND first_active_seen > first_lurk_seen
                        ) AS converted_lurkers
                    FROM chatter
                    """,
                    [since_date, streamer.lower(), *session_bot_params],
                ).fetchone()

                total_viewers = int(agg_row[0]) if agg_row and agg_row[0] else 0
                seen_sample_viewers = int(agg_row[1]) if agg_row and agg_row[1] else 0
                lurker_count = int(agg_row[2]) if agg_row and agg_row[2] else 0
                avg_sessions_lurkers = float(agg_row[3]) if agg_row and agg_row[3] else 0.0
                eligible_lurkers = int(agg_row[4]) if agg_row and agg_row[4] else 0
                converted_lurkers = int(agg_row[5]) if agg_row and agg_row[5] else 0

                if total_viewers == 0:
                    return web.json_response(
                        {"dataAvailable": False, "message": "Keine Daten für den Zeitraum"},
                        status=200,
                    )
                if seen_sample_viewers == 0:
                    return web.json_response(
                        {
                            "dataAvailable": False,
                            "message": "Zu wenig Chatter-API/Lurker-Daten im Zeitraum",
                        },
                        status=200,
                    )

                top_rows = conn.execute(
                    f"""
                    WITH sessions AS (
                        SELECT id
                        FROM twitch_stream_sessions
                        WHERE started_at >= %s
                          AND ended_at IS NOT NULL
                          AND LOWER(streamer_login) = %s
                    ),
                    chatter AS (
                        SELECT
                            COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id) AS viewer_id,
                            COUNT(DISTINCT sc.session_id) AS session_count,
                            SUM(COALESCE(sc.messages, 0)) AS msg_sum,
                            MAX(
                                CASE
                                    WHEN COALESCE(sc.seen_via_chatters_api, FALSE) IS TRUE
                                    THEN 1 ELSE 0
                                END
                            ) AS seen_via_api,
                            MIN(sc.first_message_at) AS first_seen,
                            MAX(sc.last_seen_at) AS last_seen
                        FROM twitch_session_chatters sc
                        JOIN sessions s ON s.id = sc.session_id
                        WHERE {session_bot_clause}
                        GROUP BY 1
                    )
                    SELECT viewer_id, session_count, first_seen, last_seen
                    FROM chatter
                    WHERE msg_sum = 0
                      AND seen_via_api = 1
                    ORDER BY session_count DESC
                    LIMIT 25
                    """,
                    [since_date, streamer.lower(), *session_bot_params],
                ).fetchall()

                def _iso(val):
                    if not val:
                        return None
                    return val.isoformat() if hasattr(val, "isoformat") else str(val)

                regular_lurkers = [
                    {
                        "login": r[0] or "",
                        "lurkSessions": int(r[1]) if r[1] else 0,
                        "firstSeen": _iso(r[2]),
                        "lastSeen": _iso(r[3]),
                    }
                    for r in top_rows
                ]

                return web.json_response(
                    {
                        "dataAvailable": True,
                        "regularLurkers": regular_lurkers,
                        "lurkerStats": {
                            "ratio": lurker_count / seen_sample_viewers if seen_sample_viewers else 0.0,
                            "avgSessions": avg_sessions_lurkers,
                            "totalLurkers": lurker_count,
                            "totalViewers": seen_sample_viewers,
                        },
                        "conversionStats": {
                            "rate": converted_lurkers / eligible_lurkers
                            if eligible_lurkers
                            else 0.0,
                            "eligible": eligible_lurkers,
                            "converted": converted_lurkers,
                        },
                    }
                )
        except Exception:
            log.exception("Error in lurker analysis API")
            from .demo_data import get_lurker_analysis

            demo = get_lurker_analysis()
            demo["message"] = "Fallback: Demo-Daten wegen Fehler"
            return web.json_response(demo, status=200)

    async def _api_v2_raid_retention(self, request: web.Request) -> web.Response:
        """Return retention stats for outgoing raids for a streamer."""
        self._require_v2_auth(request)
        self._require_extended_plan(request)

        streamer = request.query.get("streamer", "").strip().lower() or None
        days = min(max(int(request.query.get("days", "90")), 7), 365)

        if not streamer:
            return web.json_response(
                {"dataAvailable": False, "message": "Streamer required"}, status=400
            )

        since_date = (datetime.now(UTC) - timedelta(days=days)).isoformat()
        try:
            with storage.readonly_connection() as conn:
                base_rows = conn.execute(
                    """
                    SELECT
                        raid_id,
                        from_broadcaster_login,
                        to_broadcaster_login,
                        viewer_count_sent,
                        executed_at,
                        target_session_id,
                        chatters_at_plus5m,
                        chatters_at_plus15m,
                        chatters_at_plus30m,
                        new_chatters,
                        known_from_raider
                    FROM twitch_raid_retention
                    WHERE executed_at >= %s
                      AND LOWER(from_broadcaster_login) = %s
                    ORDER BY executed_at DESC
                    LIMIT 100
                    """,
                    [since_date, streamer],
                ).fetchall()

                def _as_int(value: Any) -> int:
                    try:
                        return int(value or 0)
                    except Exception:
                        return 0

                def _row_value(row_obj: Any, idx: int) -> Any:
                    try:
                        return row_obj[idx]
                    except Exception:
                        return None

                base_raids: list[dict[str, Any]] = []
                raid_inputs: list[dict[str, Any]] = []
                for row in base_rows:
                    try:
                        raid_id = int(_row_value(row, 0))
                    except Exception:
                        continue
                    base_raid = {
                        "raid_id": raid_id,
                        "from_login": str(_row_value(row, 1) or "").lower(),
                        "to_login": str(_row_value(row, 2) or "").lower(),
                        "to_broadcaster": _row_value(row, 2),
                        "viewers_sent": _as_int(_row_value(row, 3)),
                        "executed_at": _row_value(row, 4),
                        "stored_plus5m": _as_int(_row_value(row, 6)),
                        "stored_plus15m": _as_int(_row_value(row, 7)),
                        "stored_plus30m": _as_int(_row_value(row, 8)),
                        "stored_new_chatters": _as_int(_row_value(row, 9)),
                        "stored_known_from_raider": _as_int(_row_value(row, 10)),
                    }
                    base_raids.append(base_raid)
                    try:
                        target_session_id = int(_row_value(row, 5))
                    except Exception:
                        continue
                    raid_inputs.append({
                        "raid_id": raid_id,
                        "from_login": base_raid["from_login"],
                        "to_login": base_raid["to_login"],
                        "to_broadcaster": base_raid["to_broadcaster"],
                        "viewers_sent": base_raid["viewers_sent"],
                        "executed_at": base_raid["executed_at"],
                        "target_session_id": target_session_id,
                    })
                raid_metrics = recalculate_raid_chat_metrics(conn, raid_inputs)
                raids = []
                retention_values: list[float] = []
                conversion_values: list[float] = []
                total_new_chatters = 0
                recalculated_raid_count = 0
                stored_fallback_raid_count = 0

                for raid in base_raids:
                    viewers_sent = int(raid["viewers_sent"] or 0)

                    raid_id = int(raid["raid_id"])
                    executed_at = raid["executed_at"]
                    raid_key = raid_identity_key(raid_id, executed_at)
                    metric = raid_metrics.get(raid_key) if raid_key is not None else None
                    if metric is not None:
                        chatters_5m = int(metric.get("plus5m", 0) or 0)
                        chatters_15m = int(metric.get("plus15m", 0) or 0)
                        chatters_30m = int(metric.get("plus30m", 0) or 0)
                        known_from_raider = int(metric.get("known_from_raider", 0) or 0)
                        new_chatters = int(metric.get("new_chatters", 0) or 0)
                        recalculated_raid_count += 1
                    else:
                        chatters_5m = int(raid["stored_plus5m"] or 0)
                        chatters_15m = int(raid["stored_plus15m"] or 0)
                        chatters_30m = int(raid["stored_plus30m"] or 0)
                        known_from_raider = int(raid["stored_known_from_raider"] or 0)
                        new_chatters = int(raid["stored_new_chatters"] or 0)
                        stored_fallback_raid_count += 1

                    ret_pct = (float(chatters_30m) / viewers_sent * 100) if viewers_sent > 0 else 0.0
                    conv_pct = (float(new_chatters) / viewers_sent * 100) if viewers_sent > 0 else 0.0

                    retention_values.append(ret_pct)
                    conversion_values.append(conv_pct)
                    total_new_chatters += new_chatters

                    raids.append(
                        {
                            "raidId": raid_id,
                            "toBroadcaster": raid["to_broadcaster"],
                            "viewersSent": viewers_sent,
                            "executedAt": executed_at.isoformat() if hasattr(executed_at, "isoformat") else str(executed_at),
                            "chattersAt5m": chatters_5m,
                            "chattersAt15m": chatters_15m,
                            "chattersAt30m": chatters_30m,
                            "retention30mPct": round(ret_pct, 1),
                            "newChatters": new_chatters,
                            "chatterConversionPct": round(conv_pct, 1),
                            "knownFromRaider": known_from_raider,
                        }
                    )

            if not raids:
                return web.json_response(
                    {"dataAvailable": False, "message": "Keine Raids im Zeitraum"}, status=200
                )

            def _avg(values: list[float]) -> float:
                return round(sum(values) / len(values), 1) if values else 0.0

            summary = {
                "avgRetentionPct": _avg(retention_values),
                "avgConversionPct": _avg(conversion_values),
                "totalNewChatters": total_new_chatters,
                "raidCount": len(raids),
            }
            if stored_fallback_raid_count == 0:
                raid_metric_source = "recalculated"
            elif recalculated_raid_count == 0:
                raid_metric_source = "stored"
            else:
                raid_metric_source = "mixed"

            return web.json_response(
                {
                    "dataAvailable": True,
                    "summary": summary,
                    "raids": raids,
                    "dataQuality": {
                        "botFilterApplied": raid_metric_source == "recalculated",
                        "raidMetricSource": raid_metric_source,
                        "recalculatedRaidCount": recalculated_raid_count,
                        "storedFallbackRaidCount": stored_fallback_raid_count,
                    },
                }
            )
        except Exception:
            log.exception("Error in raid retention API")
            from .demo_data import get_raid_retention

            demo = get_raid_retention()
            demo["message"] = "Fallback: Demo-Daten wegen Fehler"
            return web.json_response(demo, status=200)

    async def _api_v2_viewer_profiles(self, request: web.Request) -> web.Response:
        """Viewer behavioral profiles based on cross-streamer exclusivity."""
        self._require_v2_auth(request)
        self._require_extended_plan(request)

        streamer = request.query.get("streamer", "").strip().lower() or None
        if not streamer:
            return web.json_response({"error": "streamer required"}, status=400)

        _empty = {
            "dataAvailable": False,
            "message": "Keine Daten vorhanden",
            "profiles": {"exclusive": 0, "loyalMulti": 0, "casual": 0, "explorer": 0, "passive": 0, "total": 0},
            "exclusivityDistribution": [],
        }

        try:
            with storage.readonly_connection() as conn:
                rollup_bot_clause, rollup_bot_params = build_known_chat_bot_not_in_clause(
                    column_expr="chatter_login",
                    placeholder="%s",
                )
                rollup_bot_clause_cr, rollup_bot_params_cr = build_known_chat_bot_not_in_clause(
                    column_expr="cr.chatter_login",
                    placeholder="%s",
                )
                dist_rows = conn.execute(
                    f"""
                    WITH per_viewer AS (
                        SELECT cr.chatter_login,
                               COUNT(DISTINCT cr.streamer_login) AS streamer_count,
                               SUM(cr.total_messages) AS total_messages
                        FROM twitch_chatter_rollup cr
                        WHERE cr.chatter_login IN (
                            SELECT DISTINCT chatter_login
                            FROM twitch_chatter_rollup
                            WHERE LOWER(streamer_login) = %s
                              AND {rollup_bot_clause}
                        )
                          AND {rollup_bot_clause_cr}
                        GROUP BY cr.chatter_login
                    )
                    SELECT streamer_count, COUNT(*) AS viewer_count
                    FROM per_viewer
                    GROUP BY streamer_count
                    ORDER BY streamer_count
                    """,
                    (streamer, *rollup_bot_params, *rollup_bot_params_cr),
                ).fetchall()

                passive_row = conn.execute(
                    f"""
                    SELECT COUNT(*) AS passive
                    FROM twitch_chatter_rollup
                    WHERE LOWER(streamer_login) = %s
                      AND total_sessions >= 3
                      AND total_messages = 0
                      AND {rollup_bot_clause}
                    """,
                    (streamer, *rollup_bot_params),
                ).fetchone()

            if not dist_rows:
                return web.json_response(_empty)

            dist = {r[0]: r[1] for r in dist_rows}
            total_viewers = sum(r[1] for r in dist_rows)

            # Viewer classification: exclusive=1 streamer, loyalMulti=2-3, explorer=8+, passive=silent ≥3 sessions
            exclusive_count = dist.get(1, 0)
            loyal_multi_count = sum(dist.get(i, 0) for i in range(2, 4))
            explorer_count = sum(v for k, v in dist.items() if k >= 8)
            passive_count = int(passive_row[0]) if passive_row and passive_row[0] else 0
            casual_count = max(0, total_viewers - exclusive_count - loyal_multi_count - explorer_count - passive_count)

            return web.json_response({
                "dataAvailable": True,
                "profiles": {
                    "exclusive": exclusive_count,
                    "loyalMulti": loyal_multi_count,
                    "casual": casual_count,
                    "explorer": explorer_count,
                    "passive": passive_count,
                    "total": total_viewers,
                },
                "exclusivityDistribution": [
                    {"streamerCount": int(r[0]), "viewerCount": int(r[1])}
                    for r in dist_rows
                ],
            })
        except Exception:
            log.exception("Error in viewer profiles API")
            from .demo_data import get_viewer_profiles
            demo = get_viewer_profiles()
            demo["message"] = "Fallback: Demo-Daten wegen Fehler"
            return web.json_response(demo, status=200)

    async def _api_v2_audience_sharing(self, request: web.Request) -> web.Response:
        """Cross-streamer audience overlap with inflow/outflow and Jaccard similarity."""
        self._require_v2_auth(request)
        self._require_extended_plan(request)

        streamer = request.query.get("streamer", "").strip().lower() or None
        days = min(max(int(request.query.get("days", "30")), 7), 365)
        if not streamer:
            return web.json_response({"error": "streamer required"}, status=400)

        since_date = (datetime.now(UTC) - timedelta(days=days)).isoformat()
        try:
            with storage.readonly_connection() as conn:
                rollup_bot_clause, rollup_bot_params = build_known_chat_bot_not_in_clause(
                    column_expr="chatter_login",
                    placeholder="%s",
                )
                rollup_bot_clause_cr1, rollup_bot_params_cr1 = build_known_chat_bot_not_in_clause(
                    column_expr="cr1.chatter_login",
                    placeholder="%s",
                )
                rollup_bot_clause_cr2, rollup_bot_params_cr2 = build_known_chat_bot_not_in_clause(
                    column_expr="cr2.chatter_login",
                    placeholder="%s",
                )
                my_total_row = conn.execute(
                    f"""
                    SELECT COUNT(DISTINCT chatter_login) AS total
                    FROM twitch_chatter_rollup
                    WHERE LOWER(streamer_login) = %s
                      AND {rollup_bot_clause}
                    """,
                    (streamer, *rollup_bot_params),
                ).fetchone()
                my_total = int(my_total_row[0]) if my_total_row and my_total_row[0] else 0

                shared_rows = conn.execute(
                    f"""
                    SELECT
                        cr2.streamer_login                                                    AS other_streamer,
                        COUNT(DISTINCT cr1.chatter_login)                                    AS shared_viewers,
                        COUNT(DISTINCT CASE WHEN cr2.first_seen_at >= %s THEN cr1.chatter_login END) AS inflow,
                        COUNT(DISTINCT CASE WHEN cr2.last_seen_at < %s THEN cr1.chatter_login END)   AS outflow,
                        COUNT(DISTINCT cr2.chatter_login)                                    AS other_total
                    FROM twitch_chatter_rollup cr1
                    JOIN twitch_chatter_rollup cr2
                        ON cr1.chatter_login = cr2.chatter_login
                       AND LOWER(cr2.streamer_login) != LOWER(cr1.streamer_login)
                    WHERE LOWER(cr1.streamer_login) = %s
                      AND {rollup_bot_clause_cr1}
                      AND {rollup_bot_clause_cr2}
                    GROUP BY cr2.streamer_login
                    HAVING COUNT(DISTINCT cr1.chatter_login) >= 3
                    ORDER BY shared_viewers DESC
                    LIMIT 20
                    """,
                    (
                        since_date,
                        since_date,
                        streamer,
                        *rollup_bot_params_cr1,
                        *rollup_bot_params_cr2,
                    ),
                ).fetchall()

                if not shared_rows:
                    return web.json_response({
                        "dataAvailable": False,
                        "message": "Keine Daten vorhanden",
                        "current": [],
                        "timeline": [],
                        "totalUniqueViewers": my_total,
                        "dataQuality": {"months": 0, "minSharedFilter": 3},
                    })

                top_streamers = [str(r[0]).lower() for r in shared_rows[:5] if r[0]]
                if top_streamers:
                    top_placeholders = ",".join(["%s"] * len(top_streamers))
                    timeline_rows = conn.execute(
                        f"""
                        SELECT
                            TO_CHAR(
                                date_trunc(
                                    'month',
                                    CASE
                                        WHEN cr1.first_seen_at > cr2.first_seen_at THEN cr1.first_seen_at
                                        ELSE cr2.first_seen_at
                                    END
                                ),
                                'YYYY-MM'
                            ) AS month,
                            cr2.streamer_login AS other_streamer,
                            COUNT(DISTINCT cr1.chatter_login) AS shared_viewers_that_month
                        FROM twitch_chatter_rollup cr1
                        JOIN twitch_chatter_rollup cr2
                            ON cr1.chatter_login = cr2.chatter_login
                           AND LOWER(cr2.streamer_login) IN ({top_placeholders})
                           AND LOWER(cr2.streamer_login) != LOWER(cr1.streamer_login)
                        WHERE LOWER(cr1.streamer_login) = %s
                          AND {rollup_bot_clause_cr1}
                          AND {rollup_bot_clause_cr2}
                        GROUP BY month, cr2.streamer_login
                        ORDER BY month
                        """,
                        (
                            *top_streamers,
                            streamer,
                            *rollup_bot_params_cr1,
                            *rollup_bot_params_cr2,
                        ),
                    ).fetchall()
                else:
                    timeline_rows = []

            current_data = []
            months_set: set[str] = set()
            for r in shared_rows:
                shared_count = int(r[1])
                other_total = int(r[4]) if r[4] else 0
                union_total = my_total + other_total - shared_count
                jaccard = round(shared_count / union_total, 3) if union_total > 0 else 0
                current_data.append({
                    "streamer": r[0],
                    "sharedViewers": shared_count,
                    "inflow": int(r[2]) if r[2] else 0,
                    "outflow": int(r[3]) if r[3] else 0,
                    "jaccardSimilarity": jaccard,
                })

            timeline_data = []
            for r in timeline_rows:
                if r[0]:
                    months_set.add(r[0])
                timeline_data.append({
                    "month": r[0] or "",
                    "streamer": r[1],
                    "sharedViewers": int(r[2]) if r[2] else 0,
                })

            return web.json_response({
                "dataAvailable": True,
                "current": current_data,
                "timeline": timeline_data,
                "totalUniqueViewers": my_total,
                "dataQuality": {"months": len(months_set), "minSharedFilter": 3},
            })
        except Exception:
            log.exception("Error in audience sharing API")
            from .demo_data import get_audience_sharing
            demo = get_audience_sharing()
            demo["message"] = "Fallback: Demo-Daten wegen Fehler"
            return web.json_response(demo, status=200)

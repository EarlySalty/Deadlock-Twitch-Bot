"""Routes mixin for DashboardV2Server — core routes and route registration."""

from __future__ import annotations

import html
import json
import os
import asyncio
from datetime import UTC, datetime
from typing import Any
from urllib import error as _urlerror
from urllib import parse as _urlparse
from urllib import request as _urlrequest
from uuid import uuid4
import hashlib
import secrets

from aiohttp import web

from .. import storage
from ..core.constants import log
from . import abbo_routes as _abbo_routes
from . import abbo_billing_routes as _abbo_billing_routes
from .billing.billing_plans import (
    BILLING_CYCLE_DISCOUNTS as _BILLING_CYCLE_DISCOUNTS,
    BILLING_STRIPE_QUICKSTART_URL as _BILLING_STRIPE_QUICKSTART_URL,
    build_billing_catalog as _build_billing_catalog,
    billing_cycle_label as _billing_cycle_label,
    billing_is_paid_plan as _billing_is_paid_plan,
    billing_is_paid_plan_id as _billing_is_paid_plan_id,
    billing_plan_has_entitlement as _billing_plan_has_entitlement,
    format_eur_cents as _format_eur_cents,
    normalize_billing_cycle as _normalize_billing_cycle,
)
from .live.live import DashboardLiveMixin, _CRITICAL_SCOPES, _REQUIRED_SCOPES, _SCOPE_COLUMN_LABELS

TWITCH_DASHBOARDS_LOGIN_URL = "/twitch/auth/login?next=%2Ftwitch%2Fdashboard"
TWITCH_DASHBOARD_V2_LOGIN_URL = "/twitch/auth/login?next=%2Ftwitch%2Fdashboard-v2"
TWITCH_DASHBOARDS_DISCORD_LOGIN_URL = "/twitch/auth/discord/login?next=%2Ftwitch%2Fdashboard"
TWITCH_ABBO_LOGIN_URL = "/twitch/auth/login?next=%2Ftwitch%2Fabbo"
TWITCH_ABBO_DISCORD_LOGIN_URL = "/twitch/auth/discord/login?next=%2Ftwitch%2Fabbo"


def _billing_public_error_message(default_code: str, *, http_status: int | None = None) -> str:
    if http_status is not None and int(http_status) > 0:
        return f"stripe_http_error_{int(http_status)}"
    return str(default_code or "stripe_checkout_create_failed")


class _DashboardRoutesMixin:
    """Core dashboard routes and route table registration."""

    @staticmethod
    def _billing_form_pairs(payload: Any, *, prefix: str = "") -> list[tuple[str, str]]:
        pairs: list[tuple[str, str]] = []
        if payload is None:
            return pairs
        if isinstance(payload, dict):
            for key, value in payload.items():
                key_str = str(key or "").strip()
                if not key_str:
                    continue
                child_prefix = f"{prefix}[{key_str}]" if prefix else key_str
                pairs.extend(_DashboardRoutesMixin._billing_form_pairs(value, prefix=child_prefix))
            return pairs
        if isinstance(payload, (list, tuple)):
            for index, value in enumerate(payload):
                child_prefix = f"{prefix}[{index}]"
                pairs.extend(_DashboardRoutesMixin._billing_form_pairs(value, prefix=child_prefix))
            return pairs
        if not prefix:
            return pairs
        if isinstance(payload, bool):
            pairs.append((prefix, "true" if payload else "false"))
        else:
            pairs.append((prefix, str(payload)))
        return pairs

    @staticmethod
    def _form_checkbox_enabled(payload: Any, field_name: str) -> bool:
        truthy_values = {"1", "true", "on", "yes"}
        values: list[Any] = []
        getall = getattr(payload, "getall", None)
        if callable(getall):
            try:
                values = list(getall(field_name))
            except KeyError:
                values = []
        if not values:
            getter = getattr(payload, "get", None)
            raw_value = getter(field_name) if callable(getter) else None
            if isinstance(raw_value, (list, tuple, set)):
                values = list(raw_value)
            elif raw_value is not None:
                values = [raw_value]
        return any(str(value or "").strip().lower() in truthy_values for value in values)

    def _abbo_scope_state(
        self,
        *,
        twitch_login: str,
        twitch_user_id: str = "",
    ) -> dict[str, Any]:
        return _abbo_routes._abbo_scope_state(
            self,
            twitch_login=twitch_login,
            twitch_user_id=twitch_user_id,
        )

    def _abbo_upsert_lurker_tax_setting(
        self,
        *,
        twitch_login: str,
        twitch_user_id: str = "",
        plan_id: str = "",
        enabled: bool,
    ) -> bool:
        return _abbo_routes._abbo_upsert_lurker_tax_setting(
            self,
            twitch_login=twitch_login,
            twitch_user_id=twitch_user_id,
            plan_id=plan_id,
            enabled=enabled,
        )

    @classmethod
    def _billing_create_checkout_session_rest(
        cls,
        *,
        stripe_secret_key: str,
        session_payload: dict[str, Any],
        idempotency_key: str = "",
    ) -> tuple[dict[str, Any] | None, str | None]:
        secret_key = str(stripe_secret_key or "").strip()
        if not secret_key:
            return None, "stripe_secret_key_missing"

        body = _urlparse.urlencode(cls._billing_form_pairs(session_payload)).encode("utf-8")
        request_obj = _urlrequest.Request(
            url="https://api.stripe.com/v1/checkout/sessions",
            data=body,
            method="POST",
            headers={
                "Authorization": f"Bearer {secret_key}",
                "Content-Type": "application/x-www-form-urlencoded",
            },
        )
        if idempotency_key:
            request_obj.add_header("Idempotency-Key", str(idempotency_key))

        try:
            with _urlrequest.urlopen(request_obj, timeout=25) as response:
                raw_text = response.read().decode("utf-8", errors="replace")
        except _urlerror.HTTPError as exc:
            raw_text = exc.read().decode("utf-8", errors="replace")
            status_code = int(exc.code or 0)
            try:
                parsed = json.loads(raw_text)
            except Exception:
                parsed = {}
            if isinstance(parsed, dict):
                error_obj = parsed.get("error")
                if isinstance(error_obj, dict):
                    error_type = str(error_obj.get("type") or "").strip() or "unknown"
                    log.warning(
                        "stripe rest checkout create failed (HTTP %s, type=%s)",
                        status_code,
                        error_type,
                    )
                else:
                    log.warning("stripe rest checkout create failed (HTTP %s)", status_code)
            else:
                log.warning("stripe rest checkout create failed (HTTP %s)", status_code)
            return None, _billing_public_error_message(
                "stripe_checkout_create_failed",
                http_status=status_code,
            )
        except Exception as exc:
            log.warning("stripe rest checkout create failed (%s)", type(exc).__name__)
            return None, _billing_public_error_message("stripe_checkout_create_failed")

        try:
            payload = json.loads(raw_text)
        except Exception:
            return None, "stripe_invalid_json_response"
        if not isinstance(payload, dict):
            return None, "stripe_invalid_response_type"
        if str(payload.get("id") or "").strip():
            return payload, None
        error_obj = payload.get("error")
        if isinstance(error_obj, dict):
            error_type = str(error_obj.get("type") or "").strip() or "unknown"
            log.warning("stripe rest checkout create returned error payload (type=%s)", error_type)
            return None, _billing_public_error_message("stripe_checkout_create_failed")
        return None, "stripe_checkout_create_failed"

    def _billing_create_checkout_session_best_effort(
        self,
        *,
        session_payload: dict[str, Any],
        idempotency_key: str = "",
    ) -> tuple[Any | None, str | None]:
        """Create Stripe Checkout Session via SDK, fallback to direct REST."""
        stripe_secret_key = str(getattr(self, "_billing_stripe_secret_key", "") or "").strip()
        if not stripe_secret_key:
            return None, "stripe_secret_key_missing"

        stripe, _import_error = self._billing_import_stripe()
        if stripe is not None:
            try:
                stripe.api_key = stripe_secret_key
                if idempotency_key:
                    session = stripe.checkout.Session.create(
                        **session_payload,
                        idempotency_key=idempotency_key,
                    )
                else:
                    session = stripe.checkout.Session.create(**session_payload)
                return session, None
            except Exception as exc:
                log.warning(
                    "stripe sdk checkout create failed; fallback to REST (%s)",
                    type(exc).__name__,
                )

        return self._billing_create_checkout_session_rest(
            stripe_secret_key=stripe_secret_key,
            session_payload=session_payload,
            idempotency_key=idempotency_key,
        )

    @staticmethod
    def _billing_origin_from_url(raw_url: str | None) -> str | None:
        value = str(raw_url or "").strip()
        if not value:
            return None
        try:
            parsed = _urlparse.urlsplit(value)
        except Exception:
            return None

        scheme = str(parsed.scheme or "").strip().lower()
        host = str(parsed.hostname or "").strip().lower()
        if scheme not in {"http", "https"}:
            return None
        if not parsed.netloc or not host:
            return None
        if parsed.username or parsed.password:
            return None
        if scheme == "http" and host not in {"127.0.0.1", "localhost", "::1"}:
            return None
        return _urlparse.urlunsplit((scheme, parsed.netloc, "", "", "")).rstrip("/")

    def _billing_configured_public_origin(self) -> str:
        candidates = (
            getattr(self, "_billing_checkout_success_url", ""),
            getattr(self, "_billing_checkout_cancel_url", ""),
            getattr(self, "_oauth_redirect_uri", ""),
            getattr(self, "_discord_admin_redirect_uri", ""),
            os.getenv("TWITCH_ADMIN_PUBLIC_URL", ""),
            os.getenv("MASTER_DASHBOARD_PUBLIC_URL", ""),
            "https://admin.earlysalty.de",
        )
        for candidate in candidates:
            origin = self._billing_origin_from_url(candidate)
            if origin:
                return origin
        return "https://admin.earlysalty.de"

    def _billing_base_url_for_request(self, request: web.Request) -> str:
        checker = getattr(self, "_is_local_request", None)
        is_local_request = False
        if callable(checker):
            try:
                is_local_request = bool(checker(request))
            except Exception:
                is_local_request = False
        if is_local_request:
            secure_checker = getattr(self, "_is_secure_request", None)
            is_secure = bool(secure_checker(request)) if callable(secure_checker) else False
            scheme = "https" if is_secure else "http"
            host = str(getattr(request, "host", "") or "").strip()
            if host:
                return f"{scheme}://{host}".rstrip("/")
        return self._billing_configured_public_origin()

    async def _billing_create_checkout_session_best_effort_async(
        self,
        *,
        session_payload: dict[str, Any],
        idempotency_key: str = "",
    ) -> tuple[Any | None, str | None]:
        # Stripe SDK and urllib are blocking; run them outside the event loop.
        return await asyncio.to_thread(
            self._billing_create_checkout_session_best_effort,
            session_payload=session_payload,
            idempotency_key=idempotency_key,
        )

    # ------------------------------------------------------------------ #
    # CSRF Token Protection                                                #
    # ------------------------------------------------------------------ #

    def _csrf_session(self, request: web.Request) -> dict[str, Any]:
        """Resolve the active authenticated session used for CSRF state."""
        dashboard_getter = getattr(self, "_get_dashboard_auth_session", None)
        if callable(dashboard_getter):
            try:
                dashboard_session = dashboard_getter(request)
            except Exception:
                dashboard_session = None
            if isinstance(dashboard_session, dict):
                return dashboard_session

        admin_getter = getattr(self, "_get_discord_admin_session", None)
        if callable(admin_getter):
            try:
                admin_session = admin_getter(request)
            except Exception:
                admin_session = None
            if isinstance(admin_session, dict):
                return admin_session
        return {}

    def _csrf_generate_token(self, request: web.Request) -> str:
        """Generate and store CSRF token in session."""
        token = secrets.token_urlsafe(32)
        session = self._csrf_session(request)
        session["csrf_token"] = token
        return token

    def _csrf_get_token(self, request: web.Request) -> str | None:
        """Get stored CSRF token from session."""
        session = self._csrf_session(request)
        return session.get("csrf_token", "")

    def _csrf_verify_token(self, request: web.Request, provided_token: str) -> bool:
        """Verify provided CSRF token against stored token."""
        stored_token = self._csrf_get_token(request)
        if not stored_token or not provided_token:
            return False
        try:
            return secrets.compare_digest(stored_token, provided_token)
        except Exception:
            return False

    # ------------------------------------------------------------------ #
    # Core routes                                                          #
    # ------------------------------------------------------------------ #

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
                log.debug(
                    "Could not build dashboard auth challenge; fallback to login redirect",
                    exc_info=True,
                )
        return web.HTTPFound(fallback_login_url)

    async def index(self, request: web.Request) -> web.StreamResponse:
        """Entrypoint with local-first admin behavior.

        Local requests should land directly in the legacy stats/admin UI.
        Public/proxied requests land on the canonical dashboard entry page.
        """
        if self._is_local_request(request) or self._is_discord_admin_request(request):
            destination = "/twitch/admin"
            fallback = "/twitch/admin"
        else:
            destination = "/twitch/dashboard"
            fallback = "/twitch/dashboard"
        if request.query_string:
            destination = f"{destination}?{request.query_string}"
        safe_destination = self._safe_internal_redirect(destination, fallback=fallback)
        raise web.HTTPFound(safe_destination)

    async def public_home(self, request: web.Request) -> web.StreamResponse:
        """Root entrypoint redirects to admin (local) or canonical dashboard landing."""
        if self._is_local_request(request) or self._is_discord_admin_request(request):
            destination = "/twitch/admin"
            fallback = "/twitch/admin"
        else:
            destination = "/twitch/dashboard"
            fallback = "/twitch/dashboard"
        if request.query_string:
            destination = f"{destination}?{request.query_string}"
        safe_destination = self._safe_internal_redirect(destination, fallback=fallback)
        raise web.HTTPFound(safe_destination)

    async def legacy_dashboard_redirect(self, request: web.Request) -> web.StreamResponse:
        """Redirect legacy dashboard paths to the canonical dashboard landing."""
        destination = "/twitch/dashboard"
        if request.query_string:
            destination = f"{destination}?{request.query_string}"
        safe_destination = self._safe_internal_redirect(destination, fallback="/twitch/dashboard")
        raise web.HTTPFound(safe_destination)

    async def admin(self, request: web.Request) -> web.StreamResponse:
        """Legacy partner admin surface (streamer management)."""
        return await DashboardLiveMixin.index(self, request)

    async def stats_entry(self, request: web.Request) -> web.StreamResponse:
        """Canonical public entrypoint that links old + beta analytics dashboards."""
        if not self._check_v2_auth(request):
            login_url = (
                TWITCH_DASHBOARDS_DISCORD_LOGIN_URL
                if self._should_use_discord_admin_login(request)
                else TWITCH_DASHBOARDS_LOGIN_URL
            )
            response = self._dashboard_auth_redirect_or_unavailable(
                request,
                next_path="/twitch/dashboard",
                fallback_login_url=login_url,
            )
            if isinstance(response, web.HTTPException):
                raise response
            return response

        legacy_url = self._resolve_legacy_stats_url()
        beta_url = "/twitch/dashboard-v2"
        logout_url = (
            "/twitch/auth/discord/logout"
            if self._is_discord_admin_request(request)
            else "/twitch/auth/logout"
        )

        # Look up scopes for the logged-in user
        session = self._get_dashboard_auth_session(request)
        twitch_login = (session or {}).get("twitch_login", "")
        missing_scopes: list[str] = []
        missing_critical: list[str] = []
        if twitch_login:
            try:
                with storage.get_conn() as conn:
                    row = conn.execute(
                        "SELECT scopes FROM twitch_raid_auth WHERE LOWER(twitch_login) = LOWER(?)",
                        [twitch_login],
                    ).fetchone()
                if row:
                    token_scopes = set((row[0] or "").split())
                    missing_scopes = [s for s in _REQUIRED_SCOPES if s not in token_scopes]
                    missing_critical = [s for s in missing_scopes if s in _CRITICAL_SCOPES]
                else:
                    missing_scopes = list(_REQUIRED_SCOPES)
                    missing_critical = [s for s in _REQUIRED_SCOPES if s in _CRITICAL_SCOPES]
            except Exception:
                log.exception("stats_entry: failed to load scopes for %s", twitch_login)

        # Build scope status HTML block
        if twitch_login and missing_scopes:
            scope_items = "".join(
                f"<li style='margin-bottom:4px;'>"
                f"<span style='color:{'#f87171' if s in _CRITICAL_SCOPES else '#fbbf24'};margin-right:6px;'>"
                f"{'⚠' if s in _CRITICAL_SCOPES else '○'}</span>"
                f"<code style='font-size:12px;background:#1f2937;padding:1px 5px;border-radius:4px;'>{html.escape(s)}</code>"
                f"<span style='color:#94a3b8;font-size:12px;margin-left:6px;'>{html.escape(_SCOPE_COLUMN_LABELS.get(s, ''))}</span>"
                f"</li>"
                for s in missing_scopes
            )
            critical_note = (
                f"<p style='color:#f87171;font-size:13px;margin-top:8px;'>"
                f"⚠ {len(missing_critical)} kritische Scope(s) fehlen — einige Features sind deaktiviert.</p>"
                if missing_critical else ""
            )
            scope_panel = (
                "<div style='background:#111827;border:1px solid #7f1d1d;border-radius:12px;"
                "padding:18px;margin-bottom:20px;'>"
                "<h3 style='margin:0 0 10px;color:#fca5a5;font-size:15px;'>Fehlende OAuth-Scopes</h3>"
                f"<p style='color:#94a3b8;font-size:13px;margin:0 0 10px;'>"
                f"Für <strong style='color:#e2e8f0;'>{html.escape(twitch_login)}</strong> fehlen "
                f"{len(missing_scopes)} von {len(_REQUIRED_SCOPES)} Scopes. "
                f"Bitte neu authentifizieren.</p>"
                f"<ul style='list-style:none;margin:0;padding:0;'>{scope_items}</ul>"
                f"{critical_note}"
                "</div>"
            )
        elif twitch_login:
            scope_panel = (
                "<div style='background:#111827;border:1px solid #14532d;border-radius:12px;"
                "padding:14px 18px;margin-bottom:20px;display:flex;align-items:center;gap:10px;'>"
                "<span style='color:#4ade80;font-size:18px;'>✓</span>"
                "<span style='color:#86efac;font-size:14px;'>Alle OAuth-Scopes vorhanden</span>"
                "</div>"
            )
        else:
            scope_panel = ""

        page_html = (
            "<!doctype html><html lang='de'><head><meta charset='utf-8'>"
            "<meta name='viewport' content='width=device-width,initial-scale=1'>"
            "<title>Twitch Analytics</title>"
            "<style>"
            "* { box-sizing: border-box; }"
            "body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif; "
            "background: #0f172a; color: #e2e8f0; margin: 0; line-height: 1.5; }"
            ".wrap { max-width: 1200px; margin: 0 auto; padding: 24px 18px; }"
            ".header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 32px; "
            "padding-bottom: 20px; border-bottom: 1px solid #1f2937; }"
            ".header-left { display: flex; align-items: center; gap: 12px; }"
            ".logo { width: 32px; height: 32px; background: #9333ea; border-radius: 50%; display: flex; "
            "align-items: center; justify-content: center; font-weight: bold; color: #fff; }"
            ".header-title { font-size: 22px; font-weight: 600; color: #e2e8f0; margin: 0; }"
            ".header-right { display: flex; align-items: center; gap: 16px; }"
            ".user-chip { background: #1f2937; border: 1px solid #374151; padding: 8px 14px; border-radius: 20px; "
            "font-size: 14px; color: #e2e8f0; }"
            ".logout-btn { color: #60a5fa; text-decoration: none; font-size: 14px; cursor: pointer; "
            "padding: 8px 12px; border: none; background: none; transition: color 0.2s; }"
            ".logout-btn:hover { color: #93c5fd; }"
            ".welcome { font-size: 20px; font-weight: 600; margin: 0 0 24px; color: #e2e8f0; }"
            ".kpi-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); "
            "gap: 16px; margin-bottom: 32px; }"
            ".kpi-tile { background: #111827; border: 1px solid #1f2937; border-radius: 12px; padding: 20px; }"
            ".kpi-label { font-size: 13px; color: #94a3b8; text-transform: uppercase; letter-spacing: 0.5px; "
            "margin-bottom: 12px; font-weight: 500; }"
            ".kpi-value { font-size: 32px; font-weight: 700; color: #e2e8f0; margin: 0; }"
            ".kpi-trend { font-size: 12px; color: #4ade80; margin-top: 8px; }"
            ".skeleton { background: linear-gradient(90deg, #1f2937 25%, #2d3748 50%, #1f2937 75%); "
            "background-size: 200% 100%; animation: shimmer 1.5s infinite; border-radius: 6px; height: 40px; "
            "margin-top: 8px; }"
            "@keyframes shimmer { 0% { background-position: 200% 0; } 100% { background-position: -200% 0; } }"
            ".nav-cards { display: grid; grid-template-columns: repeat(2, 1fr); "
            "gap: 20px; margin-bottom: 32px; }"
            "@media (max-width: 768px) { .nav-cards { grid-template-columns: 1fr; } }"
            ".card { background: #111827; border: 1px solid #1f2937; border-radius: 12px; padding: 28px; "
            "transition: transform 0.2s, box-shadow 0.2s, border-color 0.2s; min-height: 220px; "
            "display: flex; flex-direction: column; }"
            ".card:hover { transform: translateY(-2px); box-shadow: 0 12px 32px rgba(0, 0, 0, 0.4); "
            "border-color: #374151; }"
            ".card .btn { margin-top: auto; }"
            ".card-accent-purple { border-left: 4px solid #9333ea; }"
            ".card-accent-blue { border-left: 4px solid #2563eb; }"
            ".card-accent-teal { border-left: 4px solid #14b8a6; }"
            ".card-title { font-size: 18px; font-weight: 600; margin: 0 0 8px; color: #e2e8f0; }"
            ".card-badge { display: inline-block; background: #1f2937; color: #fbbf24; font-size: 11px; "
            "padding: 4px 8px; border-radius: 4px; margin-bottom: 12px; font-weight: 600; }"
            ".card-desc { color: #94a3b8; font-size: 14px; margin: 12px 0; }"
            ".card-bullets { list-style: none; padding: 0; margin: 12px 0; }"
            ".card-bullets li { color: #cbd5e1; font-size: 13px; margin-bottom: 8px; padding-left: 20px; "
            "position: relative; }"
            ".card-bullets li:before { content: '•'; position: absolute; left: 8px; color: #64748b; }"
            ".btn { display: inline-block; margin-top: 16px; padding: 12px 18px; border-radius: 8px; "
            "text-decoration: none; background: #2563eb; color: #fff; font-weight: 600; font-size: 14px; "
            "cursor: pointer; border: none; transition: all 0.2s; }"
            ".btn:hover { background: #1d4ed8; transform: translateX(2px); }"
            ".btn:active { transform: translateX(0); }"
            ".insights-panel { background: linear-gradient(135deg, #1f2937 0%, #111827 100%); "
            "border: 1px solid #1f2937; border-radius: 12px; padding: 24px; border-left: 4px solid #8b5cf6; "
            "margin-top: 32px; }"
            ".insights-title { font-size: 16px; font-weight: 600; color: #e2e8f0; margin: 0 0 16px; display: flex; "
            "align-items: center; gap: 8px; }"
            ".insights-list { list-style: none; padding: 0; margin: 0; }"
            ".insights-list li { color: #cbd5e1; font-size: 14px; margin-bottom: 8px; display: flex; "
            "align-items: flex-start; gap: 10px; }"
            ".insights-list li:before { content: '💡'; font-size: 16px; flex-shrink: 0; }"
            ".hidden { display: none; }"
            "</style></head><body><div class='wrap'>"
            "<div class='header'>"
            "<div class='header-left'><div class='logo'>◉</div>"
            "<h1 class='header-title'>Twitch Analytics</h1></div>"
            "<div class='header-right'>"
            f"<span class='user-chip'>{html.escape(twitch_login)}</span>"
            f"<a class='logout-btn' href='{logout_url}'>Logout</a>"
            "</div></div>"
            f"<h2 class='welcome'>Willkommen, {html.escape(twitch_login)}!</h2>"
            f"{scope_panel}"
            "<div class='kpi-grid'>"
            "<div class='kpi-tile'><div class='kpi-label'>Ø Viewer</div>"
            "<p class='kpi-value' id='kpi-viewers'>—</p><div class='skeleton' id='skeleton-viewers'></div>"
            "<div id='trend-viewers' class='kpi-trend'></div></div>"
            "<div class='kpi-tile'><div class='kpi-label'>Streams (30 Tage)</div>"
            "<p class='kpi-value' id='kpi-streams'>—</p><div class='skeleton' id='skeleton-streams'></div>"
            "<div id='trend-streams' class='kpi-trend'></div></div>"
            "<div class='kpi-tile'><div class='kpi-label'>Neue Follower</div>"
            "<p class='kpi-value' id='kpi-followers'>—</p><div class='skeleton' id='skeleton-followers'></div>"
            "<div id='trend-followers' class='kpi-trend'></div></div>"
            "<div class='kpi-tile'><div class='kpi-label'>Retention</div>"
            "<p class='kpi-value' id='kpi-retention'>—</p><div class='skeleton' id='skeleton-retention'></div>"
            "<div id='trend-retention' class='kpi-trend'></div></div>"
            "</div>"
            "<div class='nav-cards'>"
            "<div class='card card-accent-purple'>"
            "<span class='card-badge'>BETA</span>"
            "<h3 class='card-title'>📊 Analyse Dashboard</h3>"
            "<p class='card-desc'>Umfangreiche Analyse deiner Stream-Performance mit erweiterten Insights.</p>"
            "<ul class='card-bullets'>"
            "<li>Retention & Raid-Tracking</li>"
            "<li>Zuschauer-Rankings</li>"
            "<li>Trendanalysen</li>"
            "</ul>"
            f"<a class='btn' href='{beta_url}'>Öffnen →</a>"
            "</div>"
            "<div class='card card-accent-blue'>"
            "<h3 class='card-title'>📈 Stats (Alt)</h3>"
            "<p class='card-desc'>Klassisches Dashboard mit detaillierten Statistiken und Logs.</p>"
            "<ul class='card-bullets'>"
            "<li>Viewer-Verlauf</li>"
            "<li>Stream-Logs</li>"
            "</ul>"
            f"<a class='btn' href='{legacy_url}'>Öffnen →</a>"
            "</div>"
            "<div class='card card-accent-teal'>"
            "<h3 class='card-title'>🎨 Live Message Builder</h3>"
            "<p class='card-desc'>Baue deine Go-Live Nachricht mit Text, Embed, Feldern und Button inklusive Live-Vorschau.</p>"
            "<ul class='card-bullets'>"
            "<li>Placeholder-System ({channel}, {title}, {viewer_count})</li>"
            "<li>Rollen-Ping & Allowed Mentions</li>"
            "<li>Testversand per Discord-DM</li>"
            "</ul>"
            "<a class='btn' href='/twitch/live-announcement'>Öffnen →</a>"
            "</div>"
            + (f"<div class='card card-accent-teal' style='grid-column: span 2; opacity: 0.7; border-color: #334155;'>"
               "<span class='card-badge' style='background: #1e293b; color: #64748b;'>GEPLANT</span>"
               "<h3 class='card-title' style='color: #94a3b8;'>📱 Social Media Publisher</h3>"
               "<p class='card-desc'>Verwalte deine Twitch-Clips und veröffentliche auf TikTok, YouTube & Instagram.</p>"
               "<p style='color: #64748b; font-size: 13px; margin-top: 12px;'>✨ Kommendes Feature — wird in Kürze verfügbar sein</p>"
               "</div>"
               if twitch_login.lower() == "earlysalty"
               else ""
            )
            + "</div>"
            "<div class='insights-panel hidden' id='insights-panel'>"
            "<h3 class='insights-title'>💡 Insights</h3>"
            "<ul class='insights-list' id='insights-list'></ul>"
            "</div>"
            "</div>"
            "<script>"
            "async function loadStats() {"
            f"  const login = {json.dumps(twitch_login)};"
            "  try {"
            "    const res = await fetch(`/twitch/api/v2/overview?streamer=${{login}}&days=30`);"
            "    if (!res.ok) throw new Error(`HTTP ${{res.status}}`);"
            "    const data = await res.json();"
            "    if (data.error || !data.overview) return;"
            "    const o = data.overview;"
            "    const hideSkeletons = () => {"
            "      ['viewers', 'streams', 'followers', 'retention'].forEach(k => {"
            "        const skel = document.getElementById(`skeleton-${{k}}`);"
            "        if (skel) skel.style.display = 'none';"
            "      });"
            "    };"
            "    if (o.avg_viewers != null) {"
            "      const rounded = Math.round(o.avg_viewers);"
            "      document.getElementById('kpi-viewers').textContent = rounded.toLocaleString('de-DE');"
            "      if (o.avg_viewers_trend != null && o.avg_viewers_trend !== 0) {"
            "        const trend = o.avg_viewers_trend > 0 ? '▲' : '▼';"
            "        const color = o.avg_viewers_trend > 0 ? '#4ade80' : '#f87171';"
            "        const sign = o.avg_viewers_trend > 0 ? '+' : '';"
            "        const elem = document.getElementById('trend-viewers');"
            "        elem.textContent = `${{trend}} ${{sign}}${o.avg_viewers_trend.toFixed(1)}%`;"
            "        elem.style.color = color;"
            "      }"
            "    }"
            "    if (o.streams_count != null) {"
            "      document.getElementById('kpi-streams').textContent = o.streams_count.toString();"
            "    }"
            "    if (o.new_followers != null) {"
            "      document.getElementById('kpi-followers').textContent = o.new_followers.toLocaleString('de-DE');"
            "    }"
            "    if (o.retention != null) {"
            "      const pct = Math.round(o.retention * 100);"
            "      document.getElementById('kpi-retention').textContent = pct + '%';"
            "    }"
            "    hideSkeletons();"
            "    if (data.findings && Array.isArray(data.findings) && data.findings.length > 0) {"
            "      const list = document.getElementById('insights-list');"
            "      data.findings.slice(0, 2).forEach(f => {"
            "        if (f) {"
            "          const li = document.createElement('li');"
            "          li.textContent = f;"
            "          list.appendChild(li);"
            "        }"
            "      });"
            "      document.getElementById('insights-panel').classList.remove('hidden');"
            "    }"
            "  } catch (err) {"
            "    console.error('Failed to load stats:', err);"
            "  }"
            "}"
            "document.addEventListener('DOMContentLoaded', loadStats);"
            "</script>"
            "</body></html>"
        )
        return web.Response(text=page_html, content_type="text/html")

    async def abbo_entry(self, request: web.Request) -> web.StreamResponse:
        return await _abbo_routes.abbo_entry(self, request)

    async def abbo_pay(self, request: web.Request) -> web.StreamResponse:
        return await _abbo_billing_routes.abbo_pay(self, request)

    async def abbo_profile_save(self, request: web.Request) -> web.StreamResponse:
        return await _abbo_billing_routes.abbo_profile_save(self, request)

    async def abbo_cancel(self, request: web.Request) -> web.StreamResponse:
        return await _abbo_billing_routes.abbo_cancel(self, request)

    async def abbo_invoices(self, request: web.Request) -> web.StreamResponse:
        return await _abbo_billing_routes.abbo_invoices(self, request)

    async def abbo_stripe_settings(self, request: web.Request) -> web.StreamResponse:
        return await _abbo_billing_routes.abbo_stripe_settings(self, request)

    async def api_billing_catalog(self, request: web.Request) -> web.Response:
        """Expose prepared subscription plans and cycle pricing for dashboard UI."""
        if not self._check_v2_auth(request):
            return web.json_response({"error": "auth_required"}, status=401)

        cycle_raw = (request.query.get("cycle") or "1").strip()
        payload = _build_billing_catalog(cycle_raw)
        readiness = self._billing_stripe_readiness_payload()
        cycle = int(payload.get("cycle_months") or 1)
        price_map = self._billing_price_id_map()
        current_plan = self._billing_current_plan_for_request(request)
        current_plan_id = str(current_plan.get("plan_id") or "raid_free").strip() or "raid_free"
        for plan in list(payload.get("plans") or []):
            plan_id = str(plan.get("id") or "").strip()
            plan["is_current"] = bool(plan_id and plan_id == current_plan_id)
            if not _billing_is_paid_plan(plan):
                plan["checkout_available"] = False
                plan["stripe_price_id"] = None
                continue
            price_id = self._billing_price_id_for_plan(plan_id, cycle, price_map=price_map)
            plan["stripe_price_id"] = price_id or None
            plan["checkout_available"] = bool(price_id and readiness.get("checkout_ready"))

        payment = dict(payload.get("payment") or {})
        payment["integration_state"] = str(readiness.get("integration_state") or "planned")
        payment["checkout_enabled"] = bool(
            readiness.get("checkout_ready") and readiness.get("price_map_ready")
        )
        payment["invoice_preview_path"] = "/twitch/api/billing/invoice-preview"
        payment["invoice_page_path"] = "/twitch/abbo/rechnung"
        payment["stripe_sync_path"] = "/twitch/api/billing/stripe/sync-products"
        payload["payment"] = payment
        payload["current_subscription"] = current_plan
        return web.json_response(payload, dumps=lambda data: json.dumps(data, ensure_ascii=True))

    async def api_billing_readiness(self, request: web.Request) -> web.Response:
        """Expose Stripe setup readiness without leaking any secrets."""
        if not self._check_v2_auth(request):
            return web.json_response({"error": "auth_required"}, status=401)

        payload = self._billing_stripe_readiness_payload()
        return web.json_response(payload, dumps=lambda data: json.dumps(data, ensure_ascii=True))

    async def api_billing_stripe_webhook(self, request: web.Request) -> web.Response:
        """Receive and verify Stripe webhook events for subscription lifecycle updates."""
        self._billing_refresh_runtime_secrets()
        webhook_secret = str(getattr(self, "_billing_stripe_webhook_secret", "") or "").strip()
        if not webhook_secret:
            return web.json_response(
                {"error": "stripe_webhook_secret_missing"},
                status=503,
            )

        stripe, import_error = self._billing_import_stripe()
        if stripe is None:
            return web.json_response(
                {"error": "stripe_sdk_missing"},
                status=503,
            )

        payload_bytes = await request.read()
        signature = str(request.headers.get("Stripe-Signature") or "").strip()
        if not signature:
            return web.json_response({"error": "stripe_signature_missing"}, status=400)

        try:
            event = stripe.Webhook.construct_event(
                payload=payload_bytes,
                sig_header=signature,
                secret=webhook_secret,
            )
        except Exception:
            return web.json_response({"error": "invalid_stripe_signature"}, status=400)

        event_id = str(self._billing_stripe_obj_get(event, "id", "") or "").strip()
        event_type = str(self._billing_stripe_obj_get(event, "type", "") or "").strip()
        event_data = self._billing_stripe_obj_get(event, "data", {}) or {}
        event_object = self._billing_stripe_obj_get(event_data, "object", {}) or {}
        object_id = str(self._billing_stripe_obj_get(event_object, "id", "") or "").strip()
        livemode = bool(self._billing_stripe_obj_get(event, "livemode", False))
        payload_text = payload_bytes.decode("utf-8", errors="replace")
        received_at = datetime.now(UTC).isoformat()

        duplicate = False
        action = "ignored"
        try:
            with storage.get_conn() as conn:
                self._billing_ensure_storage_tables(conn)
                if event_id:
                    try:
                        conn.execute(
                            """
                            INSERT INTO twitch_billing_events (
                                stripe_event_id,
                                event_type,
                                object_id,
                                received_at,
                                livemode,
                                payload
                            ) VALUES (?, ?, ?, ?, ?, ?)
                            """,
                            (
                                event_id,
                                event_type,
                                object_id,
                                received_at,
                                1 if livemode else 0,
                                payload_text,
                            ),
                        )
                    except Exception as exc:
                        err_text = str(exc).lower()
                        if "unique" in err_text or "primary key" in err_text:
                            duplicate = True
                        else:
                            raise

                if not duplicate:
                    action = self._billing_apply_webhook_event(
                        conn,
                        stripe=stripe,
                        event_id=event_id,
                        event_type=event_type,
                        event_object=event_object,
                    )
        except Exception:
            log.exception("stripe webhook processing failed")
            return web.json_response({"error": "stripe_webhook_processing_failed"}, status=500)

        return web.json_response(
            {
                "ok": True,
                "status": "duplicate" if duplicate else "processed",
                "event_id": event_id,
                "event_type": event_type,
                "action": action,
            },
            dumps=lambda data: json.dumps(data, ensure_ascii=True),
        )

    async def api_billing_checkout_preview(self, request: web.Request) -> web.Response:
        """Validate plan selection and return Stripe-ready checkout metadata (without live checkout)."""
        if not self._check_v2_auth(request):
            return web.json_response({"error": "auth_required"}, status=401)

        body = await self._billing_read_request_body(request)
        selected_plan_id = str(body.get("plan_id") or "").strip()
        catalog = _build_billing_catalog(body.get("cycle_months"))
        selected_plan = next(
            (plan for plan in catalog["plans"] if str(plan.get("id")) == selected_plan_id),
            None,
        )
        if not selected_plan:
            return web.json_response(
                {
                    "error": "unknown_plan_id",
                    "available_plan_ids": [str(plan.get("id")) for plan in catalog["plans"]],
                },
                status=404,
            )

        total_cents = int((selected_plan.get("price") or {}).get("total_net_cents") or 0)
        readiness = self._billing_stripe_readiness_payload()
        cycle = int(catalog.get("cycle_months") or 1)
        price_id = self._billing_price_id_for_plan(selected_plan_id, cycle)
        checkout_possible = bool(
            total_cents > 0
            and readiness.get("checkout_ready")
            and readiness.get("price_map_ready")
            and price_id
        )
        if total_cents <= 0:
            message = "Dieser Plan bleibt kostenlos und benoetigt keinen Stripe-Checkout."
        elif checkout_possible:
            message = "Stripe Checkout ist bereit und kann direkt gestartet werden."
        elif readiness.get("checkout_ready") and not price_id:
            message = "Checkout Keys sind gesetzt, aber für diesen Plan fehlt noch eine Stripe Price ID."
        else:
            message = "Stripe Checkout ist noch nicht vollstaendig konfiguriert."
        payload = {
            "ready": bool(total_cents <= 0 or checkout_possible),
            "provider": "stripe",
            "integration_state": str(readiness.get("integration_state") or "planned"),
            "currency": catalog["currency"],
            "tax_mode": catalog["tax_mode"],
            "gross_available": catalog["gross_available"],
            "cycle_months": catalog["cycle_months"],
            "cycle_label": catalog["cycle_label"],
            "plan": selected_plan,
            "stripe_price_id": price_id or None,
            "checkout_session_path": "/twitch/api/billing/checkout-session",
            "invoice_preview_path": "/twitch/api/billing/invoice-preview",
            "invoice_page_path": "/twitch/abbo/rechnung",
            "message": message,
            "stripe_docs_url": _BILLING_STRIPE_QUICKSTART_URL,
            "next_steps": [
                "stripe_product_price_ids_hinterlegen",
                "checkout_session_endpoint_live_testen",
                "webhook_verarbeitung_fuer_abos_aktivieren",
            ],
        }
        return web.json_response(payload, dumps=lambda data: json.dumps(data, ensure_ascii=True))

    async def api_billing_checkout_session(self, request: web.Request) -> web.Response:
        """Create a live Stripe Checkout Session for a paid billing plan."""
        if not self._check_v2_auth(request):
            return web.json_response({"error": "auth_required"}, status=401)

        body = await self._billing_read_request_body(request)
        selected_plan_id = str(body.get("plan_id") or "").strip()
        if not selected_plan_id:
            return web.json_response(
                {
                    "error": "plan_id_required",
                    "contract_version": "2026-02-27",
                    "required_fields": ["plan_id"],
                },
                status=400,
            )

        catalog = _build_billing_catalog(body.get("cycle_months"))
        selected_plan = next(
            (plan for plan in catalog["plans"] if str(plan.get("id")) == selected_plan_id),
            None,
        )
        if not selected_plan:
            return web.json_response(
                {
                    "error": "unknown_plan_id",
                    "contract_version": "2026-02-27",
                    "available_plan_ids": [str(plan.get("id")) for plan in catalog["plans"]],
                },
                status=404,
            )

        quantity_raw = body.get("quantity", 1)
        try:
            quantity = int(quantity_raw or 1)
        except (TypeError, ValueError):
            quantity = -1
        if quantity < 1 or quantity > 24:
            return web.json_response(
                {
                    "error": "invalid_quantity",
                    "contract_version": "2026-02-27",
                    "allowed_range": [1, 24],
                },
                status=400,
            )

        unit_net_cents = int((selected_plan.get("price") or {}).get("total_net_cents") or 0)
        if unit_net_cents <= 0:
            return web.json_response(
                {
                    "error": "free_plan_no_checkout_required",
                    "contract_version": "2026-02-27",
                    "plan_id": selected_plan_id,
                },
                status=400,
            )

        default_success_url = str(getattr(self, "_billing_checkout_success_url", "") or "").strip()
        default_cancel_url = str(getattr(self, "_billing_checkout_cancel_url", "") or "").strip()
        success_url = str(body.get("success_url") or default_success_url).strip()
        cancel_url = str(body.get("cancel_url") or default_cancel_url).strip()

        allowed_redirect_hosts = list(self._billing_checkout_allowed_redirect_hosts())

        if success_url and not self._billing_is_http_url(success_url):
            return web.json_response(
                {
                    "error": "invalid_success_url",
                    "contract_version": "2026-02-27",
                    "field": "success_url",
                    "allowed_hosts": allowed_redirect_hosts,
                },
                status=400,
            )
        if cancel_url and not self._billing_is_http_url(cancel_url):
            return web.json_response(
                {
                    "error": "invalid_cancel_url",
                    "contract_version": "2026-02-27",
                    "field": "cancel_url",
                    "allowed_hosts": allowed_redirect_hosts,
                },
                status=400,
            )

        readiness = self._billing_stripe_readiness_payload()
        if not bool(readiness.get("checkout_ready")):
            return web.json_response(
                {
                    "error": "checkout_not_ready",
                    "contract_version": "2026-02-27",
                    "missing": list(readiness.get("missing") or []),
                    "readiness": readiness,
                },
                status=409,
            )
        if not bool(readiness.get("price_map_ready")):
            return web.json_response(
                {
                    "error": "stripe_price_id_map_missing",
                    "contract_version": "2026-02-27",
                    "required_price_ids": int(readiness.get("required_price_ids") or 0),
                    "mapped_price_ids": int(readiness.get("mapped_price_ids") or 0),
                    "missing_price_slots": list(readiness.get("missing_price_slots") or []),
                },
                status=409,
            )
        if not success_url or not cancel_url:
            return web.json_response(
                {
                    "error": "missing_checkout_urls",
                    "contract_version": "2026-02-27",
                    "required_fields": ["success_url", "cancel_url"],
                },
                status=409,
            )

        cycle_months = int(catalog.get("cycle_months") or 1)
        stripe_price_id = self._billing_price_id_for_plan(selected_plan_id, cycle_months)
        if not stripe_price_id:
            return web.json_response(
                {
                    "error": "missing_stripe_price_id",
                    "contract_version": "2026-02-27",
                    "plan_id": selected_plan_id,
                    "cycle_months": cycle_months,
                },
                status=409,
            )

        customer_reference = self._billing_primary_ref_for_request(request)
        billing_profile = self._billing_profile_for_request(request)

        customer_email = str(
            body.get("customer_email") or billing_profile.get("recipient_email") or ""
        ).strip()
        idempotency_key = str(
            request.headers.get("Idempotency-Key") or body.get("idempotency_key") or ""
        ).strip()

        raw_metadata = body.get("metadata")
        metadata: dict[str, str] = {}
        if isinstance(raw_metadata, dict):
            for raw_key, raw_value in raw_metadata.items():
                key = str(raw_key or "").strip()
                if not key:
                    continue
                value = str(raw_value or "").strip()
                if value:
                    metadata[key[:40]] = value[:500]

        total_net_cents = unit_net_cents * quantity
        metadata["plan_id"] = selected_plan_id
        metadata["cycle_months"] = str(cycle_months)
        metadata["quantity"] = str(quantity)
        if customer_reference:
            metadata["customer_reference"] = customer_reference

        stripe_secret_key = str(getattr(self, "_billing_stripe_secret_key", "") or "").strip()
        if not stripe_secret_key:
            return web.json_response(
                {
                    "error": "stripe_secret_key_missing",
                    "contract_version": "2026-02-27",
                },
                status=409,
            )

        session_payload: dict[str, Any] = {
            "mode": "subscription",
            "success_url": success_url,
            "cancel_url": cancel_url,
            "line_items": [{"price": stripe_price_id, "quantity": quantity}],
            "billing_address_collection": "required",
            "tax_id_collection": {"enabled": True},
            "metadata": metadata,
        }
        if customer_reference:
            session_payload["client_reference_id"] = customer_reference
        if customer_email:
            session_payload["customer_email"] = customer_email

        stripe_session, checkout_error = await self._billing_create_checkout_session_best_effort_async(
            session_payload=session_payload,
            idempotency_key=idempotency_key,
        )
        if stripe_session is None:
            return web.json_response(
                {
                    "error": "stripe_checkout_create_failed",
                    "contract_version": "2026-02-27",
                    "message": str(checkout_error or "Stripe checkout create failed"),
                },
                status=502,
            )

        session_id = str(self._billing_stripe_obj_get(stripe_session, "id", "") or "")
        session_url = str(self._billing_stripe_obj_get(stripe_session, "url", "") or "")
        expires_at_epoch = int(self._billing_stripe_obj_get(stripe_session, "expires_at", 0) or 0)
        expires_at_iso = (
            datetime.fromtimestamp(expires_at_epoch, tz=UTC).isoformat()
            if expires_at_epoch > 0
            else None
        )

        payload = {
            "ok": True,
            "provider": "stripe",
            "integration_state": "live",
            "contract_version": "2026-02-27",
            "currency": catalog["currency"],
            "tax_mode": catalog["tax_mode"],
            "request": {
                "plan_id": selected_plan_id,
                "stripe_price_id": stripe_price_id,
                "cycle_months": cycle_months,
                "quantity": quantity,
                "success_url": success_url,
                "cancel_url": cancel_url,
                "customer_reference": customer_reference,
                "customer_email": customer_email,
                "idempotency_key": idempotency_key,
                "metadata": metadata,
            },
            "plan": selected_plan,
            "amount": {
                "unit_net_cents": unit_net_cents,
                "total_net_cents": total_net_cents,
                "unit_net_label": _format_eur_cents(unit_net_cents),
                "total_net_label": _format_eur_cents(total_net_cents),
            },
            "checkout": {
                "status": "created",
                "mode": "subscription",
                "session_id": session_id,
                "session_url": session_url or None,
                "expires_at": expires_at_iso,
            },
            "invoice_preview_path": "/twitch/api/billing/invoice-preview",
            "invoice_page_path": "/twitch/abbo/rechnung",
            "message": "Stripe Checkout Session wurde erfolgreich erstellt.",
        }
        return web.json_response(payload, status=201, dumps=lambda data: json.dumps(data, ensure_ascii=True))

    async def abbo_invoice(self, request: web.Request) -> web.StreamResponse:
        return await _abbo_billing_routes.abbo_invoice(self, request)

    async def api_billing_invoice_preview(self, request: web.Request) -> web.Response:
        """Return structured + HTML invoice preview for the selected billing plan."""
        if not self._check_v2_auth(request):
            return web.json_response({"error": "auth_required"}, status=401)

        body = await self._billing_read_request_body(request)
        selected_plan_id = str(body.get("plan_id") or "").strip()
        if not selected_plan_id:
            return web.json_response(
                {
                    "error": "plan_id_required",
                    "required_fields": ["plan_id"],
                },
                status=400,
            )

        cycle_months = _normalize_billing_cycle(body.get("cycle_months"))
        catalog = _build_billing_catalog(cycle_months)
        selected_plan = next(
            (plan for plan in catalog.get("plans") or [] if str(plan.get("id") or "") == selected_plan_id),
            None,
        )
        if not selected_plan:
            return web.json_response(
                {
                    "error": "unknown_plan_id",
                    "available_plan_ids": [str(plan.get("id") or "") for plan in catalog.get("plans") or []],
                },
                status=404,
            )

        try:
            quantity = int(body.get("quantity") or 1)
        except (TypeError, ValueError):
            quantity = 1
        quantity = min(max(quantity, 1), 24)

        session = self._csrf_session(request)
        billing_profile = self._billing_profile_for_request(request)
        customer_reference = self._billing_primary_ref_for_request(request)
        customer_name = str(
            body.get("customer_name")
            or billing_profile.get("recipient_name")
            or session.get("display_name")
            or session.get("twitch_login")
            or "Streamer Partner"
        ).strip()
        customer_email = str(
            body.get("customer_email")
            or billing_profile.get("recipient_email")
            or ""
        ).strip()

        invoice = self._billing_build_invoice_preview(
            plan=selected_plan,
            cycle_months=cycle_months,
            quantity=quantity,
            customer_reference=customer_reference,
            customer_name=customer_name,
            customer_email=customer_email,
            customer_profile=billing_profile,
        )
        payload = {
            "ok": True,
            "provider": "stripe",
            "invoice": invoice,
            "html": self._billing_render_invoice_html(invoice),
        }
        return web.json_response(payload, dumps=lambda data: json.dumps(data, ensure_ascii=True))

    async def api_billing_stripe_sync_products(self, request: web.Request) -> web.Response:
        """Create/reuse Stripe products and prices and persist IDs into the Windows vault."""
        admin_error = self._require_v2_admin_api(request)
        if admin_error is not None:
            return admin_error

        body = await self._billing_read_request_body(request)
        dry_run_raw = str(body.get("dry_run") or "").strip().lower()
        dry_run = dry_run_raw in {"1", "true", "yes", "on"}

        stripe, import_error = self._billing_import_stripe()
        if stripe is None:
            return web.json_response(
                {
                    "error": "stripe_sdk_missing",
                    "message": _billing_public_error_message("stripe_sdk_missing"),
                },
                status=503,
            )

        stripe_secret_key = str(getattr(self, "_billing_stripe_secret_key", "") or "").strip()
        if not stripe_secret_key:
            return web.json_response(
                {
                    "error": "stripe_secret_key_missing",
                    "missing": ["stripe_secret_key"],
                },
                status=409,
            )
        stripe.api_key = stripe_secret_key

        product_map = self._billing_product_id_map()
        price_map = self._billing_price_id_map()
        cycle_catalogs = {
            cycle: _build_billing_catalog(cycle) for cycle in sorted(_BILLING_CYCLE_DISCOUNTS.keys())
        }
        base_catalog = cycle_catalogs.get(1) or _build_billing_catalog(1)
        paid_plans = [plan for plan in list(base_catalog.get("plans") or []) if _billing_is_paid_plan(plan)]

        operations: list[dict[str, Any]] = []
        created_products = 0
        reused_products = 0
        created_prices = 0
        reused_prices = 0

        for plan in paid_plans:
            plan_id = str(plan.get("id") or "").strip()
            plan_name = str(plan.get("name") or "").strip() or plan_id
            plan_description = str(plan.get("description") or "").strip()
            product_id = str(product_map.get(plan_id) or "").strip()
            operation: dict[str, Any] = {
                "plan_id": plan_id,
                "name": plan_name,
                "product": {"id": product_id or None, "status": "missing"},
                "prices": [],
            }

            if product_id and not dry_run:
                try:
                    product_obj = await asyncio.to_thread(stripe.Product.retrieve, product_id)
                    if bool(self._billing_stripe_obj_get(product_obj, "deleted", False)):
                        product_id = ""
                    else:
                        operation["product"] = {"id": product_id, "status": "reused"}
                        reused_products += 1
                except Exception:
                    product_id = ""

            if not product_id:
                if dry_run:
                    operation["product"] = {"id": None, "status": "would_create"}
                else:
                    try:
                        product_obj = await asyncio.to_thread(
                            stripe.Product.create,
                            name=plan_name,
                            description=plan_description or None,
                            metadata={
                                "plan_id": plan_id,
                                "source": "twitch.earlysalty.com",
                                "billing": "subscriptions",
                            },
                        )
                    except Exception as exc:
                        log.warning(
                            "stripe product create failed for %s (%s)",
                            plan_id,
                            type(exc).__name__,
                        )
                        return web.json_response(
                            {
                                "error": "stripe_product_create_failed",
                                "plan_id": plan_id,
                                "message": "stripe_product_create_failed",
                            },
                            status=502,
                        )
                    product_id = str(self._billing_stripe_obj_get(product_obj, "id", "") or "").strip()
                    if not product_id:
                        return web.json_response(
                            {
                                "error": "stripe_product_id_missing",
                                "plan_id": plan_id,
                            },
                            status=502,
                        )
                    product_map[plan_id] = product_id
                    operation["product"] = {"id": product_id, "status": "created"}
                    created_products += 1

            for cycle in sorted(_BILLING_CYCLE_DISCOUNTS.keys()):
                cycle_catalog = cycle_catalogs.get(cycle) or _build_billing_catalog(cycle)
                cycle_plan = next(
                    (
                        entry
                        for entry in list(cycle_catalog.get("plans") or [])
                        if str(entry.get("id") or "") == plan_id
                    ),
                    None,
                )
                if not cycle_plan:
                    continue
                amount_cents = int((cycle_plan.get("price") or {}).get("total_net_cents") or 0)
                if amount_cents <= 0:
                    continue

                cycle_map = price_map.setdefault(plan_id, {})
                price_id = str(cycle_map.get(cycle) or "").strip()
                lookup_key = f"deadlock_{plan_id}_{cycle}m_net_v1"
                price_status = "missing"

                if price_id and not dry_run:
                    try:
                        price_obj = await asyncio.to_thread(stripe.Price.retrieve, price_id)
                        price_status = "reused"
                        reused_prices += 1
                    except Exception:
                        price_id = ""

                if not price_id and not dry_run:
                    try:
                        price_list = await asyncio.to_thread(
                            stripe.Price.list,
                            active=True,
                            lookup_keys=[lookup_key],
                            limit=1,
                        )
                        existing_prices = list(
                            self._billing_stripe_obj_get(price_list, "data", []) or []
                        )
                    except Exception:
                        existing_prices = []

                    if existing_prices:
                        existing_price = existing_prices[0]
                        price_id = str(self._billing_stripe_obj_get(existing_price, "id", "") or "").strip()
                        if price_id:
                            cycle_map[cycle] = price_id
                            price_status = "reused_lookup"
                            reused_prices += 1

                if not price_id:
                    if dry_run:
                        price_status = "would_create"
                    else:
                        try:
                            price_obj = await asyncio.to_thread(
                                stripe.Price.create,
                                currency="eur",
                                product=product_id,
                                unit_amount=amount_cents,
                                recurring={"interval": "month", "interval_count": cycle},
                                lookup_key=lookup_key,
                                metadata={
                                    "plan_id": plan_id,
                                    "cycle_months": str(cycle),
                                    "source": "twitch.earlysalty.com",
                                },
                            )
                        except Exception as exc:
                            log.warning(
                                "stripe price create failed for %s/%sm (%s)",
                                plan_id,
                                cycle,
                                type(exc).__name__,
                            )
                            return web.json_response(
                                {
                                    "error": "stripe_price_create_failed",
                                    "plan_id": plan_id,
                                    "cycle_months": cycle,
                                    "message": "stripe_price_create_failed",
                                },
                                status=502,
                            )
                        price_id = str(self._billing_stripe_obj_get(price_obj, "id", "") or "").strip()
                        if not price_id:
                            return web.json_response(
                                {
                                    "error": "stripe_price_id_missing",
                                    "plan_id": plan_id,
                                    "cycle_months": cycle,
                                },
                                status=502,
                            )
                        cycle_map[cycle] = price_id
                        price_status = "created"
                        created_prices += 1

                operation["prices"].append(
                    {
                        "cycle_months": cycle,
                        "amount_net_cents": amount_cents,
                        "price_id": price_id or None,
                        "lookup_key": lookup_key,
                        "status": price_status,
                    }
                )
            operations.append(operation)

        persisted = False
        if not dry_run:
            product_persisted = self._billing_set_product_id_map(product_map)
            price_persisted = self._billing_set_price_id_map(price_map)
            persisted = bool(product_persisted and price_persisted)

        readiness = self._billing_stripe_readiness_payload()
        payload = {
            "ok": True,
            "provider": "stripe",
            "dry_run": dry_run,
            "persisted_to_windows_vault": persisted,
            "created_products": created_products,
            "reused_products": reused_products,
            "created_prices": created_prices,
            "reused_prices": reused_prices,
            "operations": operations,
            "product_id_map": product_map,
            "price_id_map": price_map,
            "readiness": readiness,
        }
        return web.json_response(payload, dumps=lambda data: json.dumps(data, ensure_ascii=True))

    async def auth_logout(self, request: web.Request) -> web.StreamResponse:
        """Logout and clear dashboard session cookie."""
        session_id = (request.cookies.get(self._session_cookie_name) or "").strip()
        if session_id:
            session = self._auth_sessions.pop(session_id, None)
            twitch_login = (session or {}).get("twitch_login", "unknown") if session else "unknown"
            log.info(
                "AUDIT dashboard logout: twitch=%s peer=%s",
                self._sanitize_log_value(twitch_login),
                self._sanitize_log_value(self._peer_host(request)),
            )
            try:
                from ..storage import sessions_db
                sessions_db.delete_session(session_id)
            except Exception as _exc:
                log.debug("Could not delete dashboard session from DB: %s", _exc)

        response = self._dashboard_auth_redirect_or_unavailable(
            request,
            next_path="/twitch/dashboard-v2",
            fallback_login_url=TWITCH_DASHBOARD_V2_LOGIN_URL,
        )
        self._clear_session_cookie(response, request)
        if isinstance(response, web.HTTPException):
            raise response
        return response

    async def discord_link(self, request: web.Request) -> web.StreamResponse:
        """Persist Discord profile metadata from the stats dashboard."""
        self._require_token(request)
        if not callable(self._discord_profile):
            location = self._redirect_location(
                request, err="Discord-Link ist aktuell nicht verfügbar"
            )
            safe_location = self._safe_internal_redirect(location, fallback="/twitch/stats")
            raise web.HTTPFound(location=safe_location)

        data = await request.post()
        csrf_token = str(data.get("csrf_token") or "").strip()
        if not self._csrf_verify_token(request, csrf_token):
            location = self._redirect_location(request, err="Ungültiges CSRF-Token")
            safe_location = self._safe_internal_redirect(location, fallback="/twitch/stats")
            raise web.HTTPFound(location=safe_location)
        login = (data.get("login") or "").strip()
        discord_user_id = (data.get("discord_user_id") or "").strip()
        discord_display_name = (data.get("discord_display_name") or "").strip()
        member_raw = (data.get("member_flag") or "").strip().lower()
        mark_member = member_raw in {"1", "true", "on", "yes"}

        try:
            message = await self._discord_profile(
                login,
                discord_user_id=discord_user_id or None,
                discord_display_name=discord_display_name or None,
                mark_member=mark_member,
            )
            location = self._redirect_location(request, ok=message)
        except ValueError as exc:
            location = self._redirect_location(request, err=str(exc))
        except Exception:
            log.exception("dashboard discord_link failed")
            location = self._redirect_location(
                request, err="Discord-Daten konnten nicht gespeichert werden"
            )
        safe_location = self._safe_internal_redirect(location, fallback="/twitch/stats")
        raise web.HTTPFound(location=safe_location)

    async def market_research(self, request: web.Request) -> web.StreamResponse:
        """Serve the internal Market Research dashboard."""
        self._require_token(request)

        page_html = """
        <!DOCTYPE html>
        <html lang="de">
        <head>
            <meta charset="UTF-8">
            <meta name="viewport" content="width=device-width, initial-scale=1.0">
            <title>Deadlock Market Research (Internal)</title>
            <script src="https://cdn.jsdelivr.net/npm/chart.js"></script>
            <style>
                body { font-family: 'Segoe UI', sans-serif; background: #0f172a; color: #e2e8f0; margin: 0; padding: 20px; }
                .container { max-width: 1400px; margin: 0 auto; }
                h1 { color: #f8fafc; border-bottom: 1px solid #334155; padding-bottom: 10px; }
                .card { background: #1e293b; border-radius: 8px; padding: 20px; margin-bottom: 20px; border: 1px solid #334155; }
                .grid-2 { display: grid; grid-template-columns: 1fr 1fr; gap: 20px; }
                table { width: 100%; border-collapse: collapse; margin-top: 10px; }
                th, td { text-align: left; padding: 12px; border-bottom: 1px solid #334155; }
                th { color: #94a3b8; font-weight: 600; text-transform: uppercase; font-size: 0.85rem; }
                tr:hover { background: #334155; }
                .badge { padding: 4px 8px; border-radius: 4px; font-size: 0.8rem; font-weight: 600; }
                .badge-live { background: #ef4444; color: white; }
                .stat-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 20px; margin-bottom: 20px; }
                .stat-box { background: #0f172a; padding: 15px; border-radius: 6px; text-align: center; border: 1px solid #334155; }
                .stat-val { font-size: 2rem; font-weight: bold; color: #38bdf8; }
                .stat-label { color: #94a3b8; font-size: 0.9rem; }
                .progress-bar { background: #334155; height: 8px; border-radius: 4px; overflow: hidden; margin-top: 5px; }
                .progress-fill { height: 100%; background: #38bdf8; }
                .sentiment-pos { color: #4ade80; }
                .sentiment-neg { color: #f87171; }
                .question-item { border-left: 4px solid #38bdf8; padding: 10px; margin-bottom: 10px; background: #0f172a; border-radius: 0 4px 4px 0; }
                .question-meta { font-size: 0.8rem; color: #94a3b8; margin-top: 5px; }
            </style>
        </head>
        <body>
            <div class="container">
                <h1>Deadlock DACH Market Research 🕵️‍♂️</h1>

                <div class="stat-grid" id="kpi">
                    <!-- Loaded via JS -->
                </div>

                <div class="card">
                    <h2>📈 Market Volume (24h)</h2>
                    <div style="height: 300px; position: relative;">
                        <canvas id="marketChart"></canvas>
                    </div>
                </div>

                <div class="grid-2">
                    <div class="card">
                        <h2>🔥 Meta Snapshot (Top Mentions 1h)</h2>
                        <table id="meta-table">
                            <thead><tr><th>Term</th><th>Mentions</th><th>Trend</th></tr></thead>
                            <tbody></tbody>
                        </table>
                    </div>
                    <div class="card">
                        <h2>🌡️ Sentiment Analysis</h2>
                        <div id="sentiment-chart" style="padding: 20px; text-align: center;"></div>
                    </div>
                </div>

                <div class="grid-2">
                    <div class="card">
                        <h2>🕸️ Viewer Overlap (Shared Chatters)</h2>
                        <table id="overlap-table">
                            <thead><tr><th>Streamer A</th><th>Streamer B</th><th>Shared Users</th></tr></thead>
                            <tbody></tbody>
                        </table>
                    </div>
                    <div class="card">
                        <h2>❓ Question Radar (Latest)</h2>
                        <div id="questions" style="max-height: 400px; overflow-y: auto; padding-right: 10px;">
                            <!-- Questions go here -->
                        </div>
                    </div>
                </div>

                <div class="card">
                    <h2>Live Monitored Channels</h2>
                    <table id="channels">
                        <thead>
                            <tr>
                                <th>Streamer</th>
                                <th>Viewers</th>
                                <th>Chat Activity</th>
                                <th>Lurker %</th>
                                <th>Msg/Min</th>
                                <th>Top Topic</th>
                            </tr>
                        </thead>
                        <tbody></tbody>
                    </table>
                </div>
            </div>

            <script>
                let marketChart = null;
                const escapeHtml = (value) => String(value ?? '')
                    .replace(/&/g, '&amp;')
                    .replace(/</g, '&lt;')
                    .replace(/>/g, '&gt;')
                    .replace(/"/g, '&quot;')
                    .replace(/'/g, '&#39;');

                const showError = (msg) => {
                    const kpi = document.getElementById('kpi');
                    if (kpi) {
                        kpi.innerHTML = `
                            <div class="stat-box">
                                <div class="stat-val" style="color:#f87171;">Fehler</div>
                                <div class="stat-label">${escapeHtml(msg)}</div>
                            </div>
                        `;
                    }
                };

                async function loadData() {
                    const res = await fetch('/twitch/api/market_data');
                    let data = null;
                    try {
                        data = await res.json();
                    } catch (err) {
                        console.error('market_data: invalid JSON', err);
                        showError('Daten konnten nicht geladen werden.');
                        return;
                    }

                    if (!res.ok || !data || data.error) {
                        const msg = (data && data.error) ? data.error : `${res.status} ${res.statusText}`;
                        console.error('market_data: request failed', msg);
                        showError(msg);
                        return;
                    }

                    const {
                        total_monitored = 0,
                        total_viewers = 0,
                        avg_chat_health = 0,
                        total_messages = 0,
                        avg_lurker_ratio = 0,
                        market_history = [],
                        questions = [],
                        meta_snapshot = [],
                        sentiment = { positive: 0, negative: 0, neutral: 0, pos_pct: 0, neg_pct: 0, neu_pct: 0 },
                        overlap = [],
                        channels = [],
                    } = data || {};

                    const safeNumber = (val) => {
                        const num = Number(val);
                        return Number.isFinite(num) ? num : 0;
                    };

                    // KPIs
                    document.getElementById('kpi').innerHTML = `
                        <div class="stat-box">
                            <div class="stat-val">${total_monitored}</div>
                            <div class="stat-label">Active Monitored Channels</div>
                        </div>
                        <div class="stat-box">
                            <div class="stat-val">${safeNumber(total_viewers).toLocaleString()}</div>
                            <div class="stat-label">Total Deadlock Viewers (DACH)</div>
                        </div>
                        <div class="stat-box">
                            <div class="stat-val">${safeNumber(avg_chat_health).toFixed(1)}%</div>
                            <div class="stat-label">Avg Chat Engagement</div>
                        </div>
                        <div class="stat-box">
                            <div class="stat-val">${safeNumber(total_messages).toLocaleString()}</div>
                            <div class="stat-label">Messages Analyzed (1h)</div>
                        </div>
                        <div class="stat-box">
                            <div class="stat-val">${safeNumber(avg_lurker_ratio).toFixed(1)}%</div>
                            <div class="stat-label">Avg Lurker Ratio</div>
                        </div>
                    `;

                    // Market Chart
                    const ctx = document.getElementById('marketChart').getContext('2d');
                    const chartLabels = market_history.map(h => {
                        const d = new Date(h.ts + 'Z');
                        return d.getHours().toString().padStart(2, '0') + ':' + d.getMinutes().toString().padStart(2, '0');
                    });

                    const chartData = {
                        labels: chartLabels,
                        datasets: [
                            {
                                label: 'Total Viewers',
                                data: market_history.map(h => safeNumber(h.total_viewers)),
                                borderColor: '#38bdf8',
                                backgroundColor: 'rgba(56, 189, 248, 0.1)',
                                fill: true,
                                tension: 0.4
                            },
                            {
                                label: 'Streamer Count',
                                data: market_history.map(h => safeNumber(h.streamer_count) * 10), // Scale for visibility
                                borderColor: '#f472b6',
                                borderDash: [5, 5],
                                tension: 0.1,
                                yAxisID: 'y1'
                            }
                        ]
                    };

                    if (marketChart) {
                        marketChart.data = chartData;
                        marketChart.update();
                    } else {
                        marketChart = new Chart(ctx, {
                            type: 'line',
                            data: chartData,
                            options: {
                                responsive: true,
                                maintainAspectRatio: false,
                                scales: {
                                    y: { beginAtZero: true, grid: { color: '#334155' } },
                                    y1: { position: 'right', beginAtZero: true, grid: { display: false } },
                                    x: { grid: { display: false } }
                                },
                                plugins: { legend: { labels: { color: '#e2e8f0' } } }
                            }
                        });
                    }

                    // Questions
                    document.getElementById('questions').innerHTML = questions.map(q => `
                        <div class="question-item">
                            <div>${escapeHtml(q.content)}</div>
                            <div class="question-meta">in @${escapeHtml(q.streamer)} • ${(q.ts || '').split('T')[1]?.substring(0, 5) || '--:--'} Uhr</div>
                        </div>
                    `).join('');

                    // Meta Snapshot
                    document.getElementById('meta-table').querySelector('tbody').innerHTML = meta_snapshot.map(m => `
                        <tr>
                            <td><strong>${escapeHtml(m.term)}</strong></td>
                            <td>${safeNumber(m.count)}</td>
                            <td><div class="progress-bar"><div class="progress-fill" style="width: ${Math.min(100, safeNumber(m.count) * 2)}%"></div></div></td>
                        </tr>
                    `).join('');

                    // Sentiment
                    const sent = sentiment;
                    document.getElementById('sentiment-chart').innerHTML = `
                        <div style="display: flex; justify-content: space-around; font-size: 1.2rem;">
                            <div class="sentiment-pos">Positiv: ${sent.positive} (${sent.pos_pct}%)</div>
                            <div style="color: #94a3b8;">Neutral: ${sent.neutral} (${sent.neu_pct}%)</div>
                            <div class="sentiment-neg">Negativ: ${sent.negative} (${sent.neg_pct}%)</div>
                        </div>
                        <div style="display: flex; height: 20px; margin-top: 15px; border-radius: 10px; overflow: hidden;">
                            <div style="width: ${sent.pos_pct}%; background: #4ade80;"></div>
                            <div style="width: ${sent.neu_pct}%; background: #94a3b8;"></div>
                            <div style="width: ${sent.neg_pct}%; background: #f87171;"></div>
                        </div>
                    `;

                    // Overlap
                    document.getElementById('overlap-table').querySelector('tbody').innerHTML = overlap.map(o => `
                        <tr>
                            <td>${escapeHtml(o.a)}</td>
                            <td>${escapeHtml(o.b)}</td>
                            <td>${safeNumber(o.shared)}</td>
                        </tr>
                    `).join('');

                    // Channels Table
                    const tbody = document.querySelector('#channels tbody');
                    tbody.innerHTML = channels.map(c => `
                        <tr>
                            <td>
                                <strong>${escapeHtml(c.login)}</strong>
                                ${c.is_live ? '<span class="badge badge-live">LIVE</span>' : ''}
                            </td>
                            <td>${safeNumber(c.viewers)}</td>
                            <td>${safeNumber(c.chat_health).toFixed(1)}%</td>
                            <td>${safeNumber(c.lurker_ratio).toFixed(1)}%</td>
                            <td>${safeNumber(c.msg_per_min).toFixed(1)}</td>
                            <td>${escapeHtml(c.top_topic || '-')}</td>
                        </tr>
                    `).join('');
                }
                loadData();
                setInterval(loadData, 30000);
            </script>
        </body>
        </html>
        """
        return web.Response(text=page_html, content_type="text/html")

    async def api_market_data(self, request: web.Request) -> web.Response:
        """API providing aggregated data for market research including Meta & Sentiment."""
        admin_token = request.headers.get("X-Admin-Token")
        if not (
            self._is_local_request(request)
            or self._is_discord_admin_request(request)
            or self._check_admin_token(admin_token)
        ):
            return web.json_response({"error": "Unauthorized"}, status=401)

        try:
            with storage.get_conn() as conn:
                def _to_iso(val: Any) -> Any:
                    """Convert datetime-like objects to ISO strings for JSON serialization."""
                    return val.isoformat() if hasattr(val, "isoformat") else val

                def _json_default(obj: Any) -> str:
                    """Fallback serializer for json.dumps to handle datetime objects safely."""
                    return obj.isoformat() if hasattr(obj, "isoformat") else str(obj)

                # 1. Active Monitored Channels
                rows = conn.execute("""
                    SELECT s.twitch_login, l.last_viewer_count
                    FROM twitch_streamers s
                    LEFT JOIN twitch_live_state l ON s.twitch_user_id = l.twitch_user_id
                    WHERE s.is_monitored_only = 1
                """).fetchall()

                channels = []
                total_viewers = 0

                for r in rows:
                    login = r[0]
                    viewers = r[1] or 0
                    total_viewers += viewers

                    # Recent chat stats
                    chat_stats = conn.execute(
                        """
                        SELECT COUNT(*), COUNT(DISTINCT chatter_login)
                        FROM twitch_chat_messages
                        WHERE streamer_login = ?
                          AND message_ts >= datetime('now', '-1 hour')
                    """,
                        [login],
                    ).fetchone()

                    msgs = chat_stats[0] or 0
                    active_chatters = chat_stats[1] or 0

                    # Lurker stats
                    session_id_row = conn.execute(
                        "SELECT active_session_id FROM twitch_live_state WHERE streamer_login = ?",
                        (login,),
                    ).fetchone()

                    lurkers = 0
                    total_connected = active_chatters
                    if session_id_row and session_id_row[0]:
                        lurker_stats = conn.execute(
                            """
                            SELECT COUNT(*), SUM(CASE WHEN messages = 0 THEN 1 ELSE 0 END)
                            FROM twitch_session_chatters WHERE session_id = ?
                        """,
                            (session_id_row[0],),
                        ).fetchone()
                        if lurker_stats:
                            total_connected = lurker_stats[0] or active_chatters
                            lurkers = lurker_stats[1] or 0

                    channels.append(
                        {
                            "login": login,
                            "viewers": viewers,
                            "is_live": viewers > 0,
                            "chat_health": min(100, (active_chatters / max(1, viewers)) * 100)
                            if viewers > 0
                            else 0,
                            "lurker_ratio": (lurkers / max(1, total_connected)) * 100,
                            "msg_per_min": msgs / 60.0,
                            "top_topic": "n/a",
                        }
                    )

                channels.sort(key=lambda x: x["viewers"], reverse=True)
                avg_health = sum(c["chat_health"] for c in channels) / max(1, len(channels))
                avg_lurker = sum(c["lurker_ratio"] for c in channels) / max(1, len(channels))

                # --- 2. Market History (24h) ---
                history_rows = conn.execute("""
                    SELECT ts_utc, SUM(viewer_count) as total_viewers, COUNT(DISTINCT streamer) as streamer_count
                    FROM twitch_stats_category
                    WHERE ts_utc >= datetime('now', '-24 hours')
                    GROUP BY ts_utc
                    ORDER BY ts_utc ASC
                """).fetchall()
                market_history = [
                    {"ts": _to_iso(r[0]), "total_viewers": r[1], "streamer_count": r[2]}
                    for r in history_rows
                ]

                # --- 3. Question Radar ---
                question_rows = conn.execute(
                    """
                    SELECT content, streamer_login, message_ts
                    FROM twitch_chat_messages
                    WHERE message_ts >= datetime('now', '-6 hours')
                      AND content LIKE ?
                      AND length(content) > 10
                    ORDER BY message_ts DESC
                    LIMIT 20
                """,
                    ("%?%",),
                ).fetchall()
                questions = [
                    {"content": r[0], "streamer": r[1], "ts": _to_iso(r[2])}
                    for r in question_rows
                ]

                # --- 4. Meta Snapshot & Sentiment (1h) ---
                deadlock_terms = [
                    "abrams",
                    "bebop",
                    "dynamo",
                    "grey talon",
                    "haze",
                    "infernus",
                    "ivy",
                    "kelvin",
                    "lady geist",
                    "mcginnis",
                    "mo & krill",
                    "paradox",
                    "pocket",
                    "seven",
                    "vindicta",
                    "viscous",
                    "warden",
                    "wraith",
                    "yamato",
                    "lash",
                    "shiv",
                    "urn",
                    "midboss",
                    "soul",
                    "flex slot",
                    "build",
                    "op",
                    "nerf",
                    "buff",
                    "patch",
                ]
                recent_msgs = conn.execute(
                    "SELECT content FROM twitch_chat_messages WHERE message_ts >= datetime('now', '-1 hour')"
                ).fetchall()

                term_counts = {t: 0 for t in deadlock_terms}
                sentiment = {"positive": 0, "negative": 0, "neutral": 0}
                pos_words = {
                    "pog",
                    "gg",
                    "nice",
                    "cool",
                    "krass",
                    "lol",
                    "win",
                    "stark",
                }
                neg_words = {
                    "rip",
                    "bad",
                    "lose",
                    "troll",
                    "cringe",
                    "throw",
                    "sucks",
                    "lag",
                }

                for row in recent_msgs:
                    content = (row[0] or "").lower()
                    for t in deadlock_terms:
                        if t in content:
                            term_counts[t] += 1
                    is_pos = any(w in content for w in pos_words)
                    is_neg = any(w in content for w in neg_words)
                    if is_pos and not is_neg:
                        sentiment["positive"] += 1
                    elif is_neg and not is_pos:
                        sentiment["negative"] += 1
                    else:
                        sentiment["neutral"] += 1

                meta_snapshot = sorted(
                    [{"term": k, "count": v} for k, v in term_counts.items() if v > 0],
                    key=lambda x: x["count"],
                    reverse=True,
                )[:10]
                total_sent = sum(sentiment.values()) or 1
                sent_data = {
                    "positive": sentiment["positive"],
                    "negative": sentiment["negative"],
                    "neutral": sentiment["neutral"],
                    "pos_pct": round(sentiment["positive"] / total_sent * 100, 1),
                    "neg_pct": round(sentiment["negative"] / total_sent * 100, 1),
                    "neu_pct": round(sentiment["neutral"] / total_sent * 100, 1),
                }

                # --- 5. Overlap (Top 5 Pairs) ---
                top_logins = [c["login"] for c in channels[:5]]
                overlap = []
                if len(top_logins) >= 2:
                    login_slots = (top_logins + ["!unused!"] * 5)[:5]
                    rows_overlap = conn.execute(
                        """
                        SELECT c1.streamer_login, c2.streamer_login, COUNT(DISTINCT c1.chatter_login)
                        FROM twitch_chat_messages c1
                        JOIN twitch_chat_messages c2 ON c1.chatter_login = c2.chatter_login AND c1.streamer_login < c2.streamer_login
                        WHERE c1.message_ts >= datetime('now', '-6 hours') AND c2.message_ts >= datetime('now', '-6 hours')
                          AND c1.streamer_login IN (?, ?, ?, ?, ?)
                          AND c2.streamer_login IN (?, ?, ?, ?, ?)
                        GROUP BY 1, 2 ORDER BY 3 DESC LIMIT 5
                    """,
                        login_slots + login_slots,
                    ).fetchall()
                    overlap = [{"a": ro[0], "b": ro[1], "shared": ro[2]} for ro in rows_overlap]

                payload = {
                    "total_monitored": len(channels),
                    "total_viewers": total_viewers,
                    "avg_chat_health": avg_health,
                    "avg_lurker_ratio": avg_lurker,
                    "total_messages": len(recent_msgs),
                    "market_history": market_history,
                    "questions": questions,
                    "channels": channels,
                    "meta_snapshot": meta_snapshot,
                    "sentiment": sent_data,
                    "overlap": overlap,
                }

                return web.json_response(
                    payload, dumps=lambda data: json.dumps(data, default=_json_default)
                )
        except Exception:
            error_id = uuid4().hex[:12]
            log.exception("Market API Error id=%s", error_id)
            return web.json_response(
                {
                    "error": "market_data_failed",
                    "error_id": error_id,
                },
                status=500,
            )

    async def reload_cog(self, request: web.Request) -> web.Response:
        """Optional reload endpoint for admin tooling compatibility."""
        post_data = await request.post()
        body_token = post_data.get("token", "")
        header_token = request.headers.get("X-Admin-Token")
        is_authorized = (
            self._is_local_request(request)
            or self._is_discord_admin_request(request)
            or self._check_admin_token(header_token)
        )
        if not is_authorized:
            log.warning(
                "AUDIT dashboard reload_cog: unauthorized attempt from peer=%s",
                self._sanitize_log_value(self._peer_host(request)),
            )
            return web.Response(text="Unauthorized", status=401)

        log.info(
            "AUDIT dashboard reload_cog: triggered by peer=%s",
            self._sanitize_log_value(self._peer_host(request)),
        )
        if self._reload_cb:
            msg = await self._reload_cb()
            return web.Response(text=msg)
        return web.Response(text="Kein Reload-Handler definiert", status=501)

    # ------------------------------------------------------------------ #
    # Route registration                                                   #
    # ------------------------------------------------------------------ #

    def _register_social_media_routes(self, app: web.Application) -> None:
        """Register Social Media Clip Publisher routes."""
        try:
            from ..social_media import ClipManager, create_social_media_app

            # Reuse the primary Twitch API instance so manual clip fetch works.
            twitch_api = None
            raid_bot = getattr(self, "_raid_bot", None)
            cog = getattr(raid_bot, "_cog", None) if raid_bot is not None else None
            if cog is not None:
                twitch_api = getattr(cog, "api", None)
            clip_manager = ClipManager(twitch_api=twitch_api)
            if twitch_api is None:
                log.warning(
                    "Social Media Dashboard registered without Twitch API instance. "
                    "Manual clip fetching will return 503 until API is available."
                )

            # Create social media dashboard with auth checker
            social_app = create_social_media_app(
                clip_manager=clip_manager,
                auth_checker=self._check_v2_auth,
                auth_session_getter=self._get_dashboard_auth_session,
                auth_level_getter=self._get_auth_level,
                oauth_ready_checker=getattr(self, "_is_twitch_oauth_ready", None),
                public_base_url=self._billing_configured_public_origin(),
            )

            # Mount social media routes
            for route in social_app.router.routes():
                app.router.add_route(
                    route.method,
                    route.resource.canonical,
                    route.handler,
                )

            log.info("Social Media Dashboard routes registered successfully")
        except Exception:
            log.exception("Failed to register Social Media Dashboard routes")

    async def abbo_promo_settings(self, request: web.Request) -> web.StreamResponse:
        return await _abbo_routes.abbo_promo_settings(self, request)

    async def abbo_lurker_tax_settings(self, request: web.Request) -> web.StreamResponse:
        return await _abbo_routes.abbo_lurker_tax_settings(self, request)

    async def abbo_promo_message(self, request: web.Request) -> web.StreamResponse:
        return await _abbo_routes.abbo_promo_message(self, request)

    async def admin_roadmap_page(self, request: web.Request) -> web.StreamResponse:
        """Kanban board for managing roadmap items (admin only)."""
        if not (self._is_local_request(request) or self._is_discord_admin_request(request)):
            raise web.HTTPFound("/twitch/admin")

        body = """
<div class="hero">
  <div>
    <p class="eyebrow">Admin</p>
    <h1>Roadmap</h1>
    <p class="lead">Feature-Planung verwalten – Drag &amp; Drop zwischen den Spalten um den Status zu ändern.</p>
  </div>
</div>

<div id="kanban-root">
  <div class="kanban-board">
    <div class="kanban-col col-planned" id="col-planned" data-status="planned"
         ondragover="event.preventDefault();this.classList.add('drag-over')"
         ondragleave="this.classList.remove('drag-over')"
         ondrop="handleDrop(event,'planned')">
      <div class="kanban-col-header">
        <span class="kanban-col-title">Geplant</span>
        <span class="kanban-count" id="cnt-planned">0</span>
      </div>
      <div id="cards-planned"></div>
    </div>
    <div class="kanban-col col-in_progress" id="col-in_progress" data-status="in_progress"
         ondragover="event.preventDefault();this.classList.add('drag-over')"
         ondragleave="this.classList.remove('drag-over')"
         ondrop="handleDrop(event,'in_progress')">
      <div class="kanban-col-header">
        <span class="kanban-col-title">In Arbeit</span>
        <span class="kanban-count" id="cnt-in_progress">0</span>
      </div>
      <div id="cards-in_progress"></div>
    </div>
    <div class="kanban-col col-done" id="col-done" data-status="done"
         ondragover="event.preventDefault();this.classList.add('drag-over')"
         ondragleave="this.classList.remove('drag-over')"
         ondrop="handleDrop(event,'done')">
      <div class="kanban-col-header">
        <span class="kanban-col-title">Fertig</span>
        <span class="kanban-count" id="cnt-done">0</span>
      </div>
      <div id="cards-done"></div>
    </div>
  </div>
</div>

<div class="add-item-form" id="add-form">
  <h3>Neues Feature hinzufügen</h3>
  <div class="form-row">
    <label>Titel<input type="text" id="new-title" placeholder="Feature-Titel" /></label>
    <label>Status
      <select id="new-status">
        <option value="planned">Geplant</option>
        <option value="in_progress">In Arbeit</option>
        <option value="done">Fertig</option>
      </select>
    </label>
    <label>Priorität (höher = oben)<input type="number" id="new-priority" value="0" style="width:100%" /></label>
  </div>
  <label>Beschreibung (optional)<textarea id="new-desc" rows="2" placeholder="Kurze Beschreibung..."></textarea></label>
  <div class="form-actions">
    <button class="btn" onclick="addItem()">Hinzufügen</button>
    <span id="add-status" style="font-size:.85rem;color:var(--muted)"></span>
  </div>
</div>

<script>
let dragId = null;
const STATUSES = ['planned','in_progress','done'];

async function loadRoadmap() {
  try {
    const res = await fetch('/twitch/api/v2/roadmap');
    const data = await res.json();
    STATUSES.forEach(s => {
      const container = document.getElementById('cards-' + s);
      const count = document.getElementById('cnt-' + s);
      const items = data[s] || [];
      count.textContent = items.length;
      container.innerHTML = '';
      if (items.length === 0) {
        container.innerHTML = '<div class="kanban-empty">Keine Einträge</div>';
        return;
      }
      items.forEach(item => container.appendChild(makeCard(item)));
    });
  } catch(e) {
    console.error('Roadmap laden fehlgeschlagen', e);
  }
}

function makeCard(item) {
  const card = document.createElement('div');
  card.className = 'kanban-card';
  card.draggable = true;
  card.dataset.id = item.id;
  card.innerHTML = `
    <button class="kanban-card-delete" title="Löschen" onclick="deleteItem(${item.id})">✕</button>
    <p class="kanban-card-title">${escHtml(item.title)}</p>
    ${item.description ? `<p class="kanban-card-desc">${escHtml(item.description)}</p>` : ''}
  `;
  card.addEventListener('dragstart', () => {
    dragId = item.id;
    card.classList.add('dragging');
  });
  card.addEventListener('dragend', () => card.classList.remove('dragging'));
  return card;
}

function escHtml(str) {
  return String(str || '').replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');
}

async function handleDrop(event, newStatus) {
  event.preventDefault();
  event.currentTarget.classList.remove('drag-over');
  if (!dragId) return;
  const id = dragId;
  dragId = null;
  try {
    const res = await fetch('/twitch/api/v2/roadmap/' + id, {
      method: 'PATCH',
      headers: {'Content-Type':'application/json'},
      body: JSON.stringify({status: newStatus})
    });
    if (!res.ok) throw new Error(await res.text());
    loadRoadmap();
  } catch(e) {
    console.error('Drag-Drop Fehler', e);
    loadRoadmap();
  }
}

async function deleteItem(id) {
  if (!confirm('Eintrag wirklich löschen?')) return;
  try {
    await fetch('/twitch/api/v2/roadmap/' + id, { method: 'DELETE' });
    loadRoadmap();
  } catch(e) { console.error(e); }
}

async function addItem() {
  const title = document.getElementById('new-title').value.trim();
  if (!title) { alert('Titel fehlt'); return; }
  const status = document.getElementById('new-status').value;
  const priority = parseInt(document.getElementById('new-priority').value) || 0;
  const desc = document.getElementById('new-desc').value.trim();
  const statusEl = document.getElementById('add-status');
  try {
    statusEl.textContent = 'Speichern...';
    const res = await fetch('/twitch/api/v2/roadmap', {
      method: 'POST',
      headers: {'Content-Type':'application/json'},
      body: JSON.stringify({title, status, priority, description: desc || null})
    });
    if (!res.ok) throw new Error(await res.text());
    document.getElementById('new-title').value = '';
    document.getElementById('new-desc').value = '';
    document.getElementById('new-priority').value = '0';
    statusEl.textContent = 'Gespeichert!';
    setTimeout(() => statusEl.textContent = '', 2000);
    loadRoadmap();
  } catch(e) {
    statusEl.textContent = 'Fehler: ' + e.message;
    console.error(e);
  }
}

loadRoadmap();
</script>
"""
        return web.Response(
            content_type="text/html",
            text=self._html(body, "roadmap"),
        )

    def attach(self, app: web.Application) -> None:
        app.add_routes(
            [
                web.get("/", self.public_home),
                web.get("/dashboads", self.legacy_dashboard_redirect),
                web.get("/dashboards", self.legacy_dashboard_redirect),
                web.get("/twitch", self.index),
                web.get("/twitch/", self.index),
                web.get("/twitch/admin", self.admin),
                web.get("/twitch/admin/announcements", self.admin_announcements_page),
                web.post("/twitch/admin/announcements", self.admin_announcements_save),
                web.get("/twitch/admin/roadmap", self.admin_roadmap_page),
                web.get("/twitch/live", self.admin),
                web.get("/twitch/live-announcement", self.live_announcement_page),
                web.post("/twitch/add_any", self.add_any),
                web.post("/twitch/add_url", self.add_url),
                web.post("/twitch/add_login/{login}", self.add_login),
                web.post("/twitch/add_streamer", self.add_streamer),
                web.post("/twitch/admin/chat_action", self.admin_partner_chat_action),
                web.post("/twitch/admin/manual-plan", self.admin_manual_plan_save),
                web.post("/twitch/admin/manual-plan/clear", self.admin_manual_plan_clear),
                web.post("/twitch/remove", self.remove),
                web.post("/twitch/verify", self.verify),
                web.post("/twitch/archive", self.archive),
                web.post("/twitch/discord_flag", self.discord_flag),
                web.get("/twitch/stats", self.stats),
                web.get("/twitch/partners", self.partner_stats),
                web.get("/twitch/dashboads", self.legacy_dashboard_redirect),
                web.get("/twitch/dashboards", self.legacy_dashboard_redirect),
                web.get("/twitch/abo", self.abbo_entry),
                web.get("/twitch/abbo", self.abbo_entry),
                web.get("/twitch/abos", self.abbo_entry),
                web.get("/twitch/abbo/bezahlen", self.abbo_pay),
                web.post("/twitch/abbo/rechnungsdaten", self.abbo_profile_save),
                web.get("/twitch/abbo/kündigen", self.abbo_cancel),
                web.post("/twitch/abbo/kündigen", self.abbo_cancel),
                web.get("/twitch/abbo/rechnungen", self.abbo_invoices),
                web.get("/twitch/abbo/stripe-settings", self.abbo_stripe_settings),
                web.post("/twitch/abbo/promo-settings", self.abbo_promo_settings),
                web.post("/twitch/abbo/lurker-tax-settings", self.abbo_lurker_tax_settings),
                web.post("/twitch/abbo/promo-message", self.abbo_promo_message),
                web.get("/twitch/abbo/rechnung", self.abbo_invoice),
                web.get("/twitch/impressum", self.abbo_impressum),
                web.get("/twitch/datenschutz", self.abbo_datenschutz),
                web.get("/twitch/agb", self.abbo_agb),
                web.get("/twitch/raid/auth", self.raid_auth_start),
                web.get("/twitch/raid/go", self.raid_auth_go),
                web.get("/twitch/raid/requirements", self.raid_requirements),
                web.get("/twitch/raid/history", self.raid_history),
                web.get("/twitch/raid/analytics", self.raid_analytics),
                web.get("/twitch/auth/login", self.auth_login),
                web.get("/twitch/auth/callback", self.auth_callback),
                web.get("/twitch/auth/logout", self.auth_logout),
                web.get("/twitch/auth/discord/login", self.discord_auth_login),
                web.get("/twitch/auth/discord/callback", self.discord_auth_callback),
                web.get("/twitch/auth/discord/logout", self.discord_auth_logout),
                web.get("/twitch/raid/callback", self.raid_oauth_callback),
                web.post("/twitch/discord_link", self.discord_link),
                web.post("/twitch/reload", self.reload_cog),
                web.get("/twitch/market", self.market_research),
                web.get("/twitch/api/market_data", self.api_market_data),
                web.get("/twitch/api/live-announcement/config", self.api_live_announcement_config),
                web.post("/twitch/api/live-announcement/config", self.api_live_announcement_save_config),
                web.post("/twitch/api/live-announcement/test", self.api_live_announcement_test_send),
                web.get("/twitch/api/live-announcement/preview", self.api_live_announcement_preview),
                web.get("/twitch/api/billing/catalog", self.api_billing_catalog),
                web.get("/twitch/api/billing/readiness", self.api_billing_readiness),
                web.post("/twitch/api/billing/stripe/webhook", self.api_billing_stripe_webhook),
                web.post("/twitch/api/billing/checkout-preview", self.api_billing_checkout_preview),
                web.post("/twitch/api/billing/checkout-session", self.api_billing_checkout_session),
                web.post("/twitch/api/billing/invoice-preview", self.api_billing_invoice_preview),
                web.post("/twitch/api/billing/stripe/sync-products", self.api_billing_stripe_sync_products),
            ]
        )
        self._register_v2_routes(app.router)
        self._affiliate_register_routes(app)
        self._register_social_media_routes(app)

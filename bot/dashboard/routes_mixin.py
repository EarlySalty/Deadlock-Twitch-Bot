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
from .route_deps import BillingRouteDeps, EntryRouteDeps, MarketRouteDeps
from . import routes_billing as _routes_billing
from . import routes_entry as _routes_entry
from . import routes_market as _routes_market
from . import routes_settings as _routes_settings
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
from .live.live import _CRITICAL_SCOPES, _REQUIRED_SCOPES, _SCOPE_COLUMN_LABELS

TWITCH_DASHBOARDS_LOGIN_URL = "/twitch/auth/login?next=%2Ftwitch%2Fdashboard"
TWITCH_DASHBOARD_V2_LOGIN_URL = "/twitch/auth/login?next=%2Ftwitch%2Fdashboard-v2"
TWITCH_ABBO_LOGIN_URL = "/twitch/auth/login?next=%2Ftwitch%2Fabbo"


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
            "https://admin.deutsche-deadlock-community.de",
        )
        for candidate in candidates:
            origin = self._billing_origin_from_url(candidate)
            if origin:
                return origin
        return "https://admin.deutsche-deadlock-community.de"

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
        safe_login_url = (
            self._safe_discord_admin_login_redirect(fallback_login_url)
            if "/twitch/auth/discord/login" in str(fallback_login_url or "")
            else (
                self._safe_internal_redirect(fallback_login_url, fallback="/twitch/auth/login")
                if hasattr(self, "_safe_internal_redirect")
                else "/twitch/auth/login"
            )
        )
        return web.HTTPFound(safe_login_url)

    def _entry_route_deps(self) -> EntryRouteDeps:
        return EntryRouteDeps(
            critical_scopes=_CRITICAL_SCOPES,
            dashboard_v2_login_url=TWITCH_DASHBOARD_V2_LOGIN_URL,
            dashboards_discord_login_url=self._build_discord_admin_login_url(
                None,
                next_path="/twitch/dashboard",
            ),
            dashboards_login_url=TWITCH_DASHBOARDS_LOGIN_URL,
            html=html,
            json=json,
            log=log,
            required_scopes=_REQUIRED_SCOPES,
            scope_column_labels=_SCOPE_COLUMN_LABELS,
            storage=storage,
        )

    def _billing_route_deps(self) -> BillingRouteDeps:
        return BillingRouteDeps(
            asyncio=asyncio,
            billing_cycle_discounts=_BILLING_CYCLE_DISCOUNTS,
            billing_is_paid_plan=_billing_is_paid_plan,
            billing_public_error_message=_billing_public_error_message,
            billing_stripe_quickstart_url=_BILLING_STRIPE_QUICKSTART_URL,
            build_billing_catalog=_build_billing_catalog,
            format_eur_cents=_format_eur_cents,
            json=json,
            log=log,
            normalize_billing_cycle=_normalize_billing_cycle,
            storage=storage,
        )

    def _market_route_deps(self) -> MarketRouteDeps:
        return MarketRouteDeps(
            json=json,
            log=log,
            storage=storage,
            uuid4=uuid4,
        )

    async def index(self, request: web.Request) -> web.StreamResponse:
        return await _routes_entry.index(self, request)

    async def public_home(self, request: web.Request) -> web.StreamResponse:
        return await _routes_entry.public_home(self, request)

    async def legacy_dashboard_redirect(self, request: web.Request) -> web.StreamResponse:
        return await _routes_entry.legacy_dashboard_redirect(self, request)

    async def legacy_admin_redirect(self, request: web.Request) -> web.StreamResponse:
        return await _routes_entry.legacy_admin_redirect(self, request)

    async def legacy_admin(self, request: web.Request) -> web.StreamResponse:
        return await _routes_entry.legacy_admin(self, request)

    async def admin_dashboard_redirect(self, request: web.Request) -> web.StreamResponse:
        return await _routes_entry.admin_dashboard_redirect(self, request)

    async def admin_legacy_redirect(self, request: web.Request) -> web.StreamResponse:
        return await _routes_entry.admin_legacy_redirect(self, request)

    async def admin(self, request: web.Request) -> web.StreamResponse:
        return await _routes_entry.admin(self, request)

    async def stats_entry(self, request: web.Request) -> web.StreamResponse:
        return await _routes_entry.stats_entry(self, request, deps=self._entry_route_deps())

    async def abbo_entry(self, request: web.Request) -> web.StreamResponse:
        return await _routes_billing.abbo_entry(self, request)

    async def abbo_pay(self, request: web.Request) -> web.StreamResponse:
        return await _routes_billing.abbo_pay(self, request)

    async def abbo_profile_save(self, request: web.Request) -> web.StreamResponse:
        return await _routes_billing.abbo_profile_save(self, request)

    async def abbo_cancel(self, request: web.Request) -> web.StreamResponse:
        return await _routes_billing.abbo_cancel(self, request)

    async def abbo_invoices(self, request: web.Request) -> web.StreamResponse:
        return await _routes_billing.abbo_invoices(self, request)

    async def abbo_stripe_settings(self, request: web.Request) -> web.StreamResponse:
        return await _routes_billing.abbo_stripe_settings(self, request)

    async def api_billing_catalog(self, request: web.Request) -> web.Response:
        return await _routes_billing.api_billing_catalog(
            self,
            request,
            deps=self._billing_route_deps(),
        )

    async def api_billing_readiness(self, request: web.Request) -> web.Response:
        return await _routes_billing.api_billing_readiness(
            self,
            request,
            deps=self._billing_route_deps(),
        )

    async def api_billing_stripe_webhook(self, request: web.Request) -> web.Response:
        return await _routes_billing.api_billing_stripe_webhook(
            self,
            request,
            deps=self._billing_route_deps(),
        )

    async def api_billing_checkout_preview(self, request: web.Request) -> web.Response:
        return await _routes_billing.api_billing_checkout_preview(
            self,
            request,
            deps=self._billing_route_deps(),
        )

    async def api_billing_checkout_session(self, request: web.Request) -> web.Response:
        return await _routes_billing.api_billing_checkout_session(
            self,
            request,
            deps=self._billing_route_deps(),
        )

    async def abbo_invoice(self, request: web.Request) -> web.StreamResponse:
        return await _routes_billing.abbo_invoice(self, request)

    async def api_billing_invoice_preview(self, request: web.Request) -> web.Response:
        return await _routes_billing.api_billing_invoice_preview(
            self,
            request,
            deps=self._billing_route_deps(),
        )

    async def api_billing_stripe_sync_products(self, request: web.Request) -> web.Response:
        return await _routes_billing.api_billing_stripe_sync_products(
            self,
            request,
            deps=self._billing_route_deps(),
        )

    async def auth_logout(self, request: web.Request) -> web.StreamResponse:
        return await _routes_entry.auth_logout(self, request, deps=self._entry_route_deps())

    async def discord_link(self, request: web.Request) -> web.StreamResponse:
        return await _routes_entry.discord_link(self, request, deps=self._entry_route_deps())

    async def market_research(self, request: web.Request) -> web.StreamResponse:
        return await _routes_market.market_research(self, request)

    async def api_market_data(self, request: web.Request) -> web.Response:
        return await _routes_market.api_market_data(
            self,
            request,
            deps=self._market_route_deps(),
        )

    async def reload_cog(self, request: web.Request) -> web.Response:
        return await _routes_entry.reload_cog(self, request, deps=self._entry_route_deps())

    # ------------------------------------------------------------------ #
    # Route registration                                                   #
    # ------------------------------------------------------------------ #

    def _register_social_media_routes(self, app: web.Application) -> None:
        """Register Social Media Clip Publisher routes."""
        try:
            from ..social_media import ClipManager, create_social_media_app

            clip_manager = getattr(self, "_social_media_clip_manager", None)
            if clip_manager is None:
                resolver = getattr(self, "_dashboard_clip_manager", None)
                if callable(resolver):
                    clip_manager = resolver()
            twitch_api = getattr(clip_manager, "twitch_api", None)
            if twitch_api is None:
                resolver = getattr(self, "_dashboard_twitch_api", None)
                if callable(resolver):
                    twitch_api = resolver()
            if twitch_api is None and clip_manager is not None:
                twitch_api = getattr(clip_manager, "api", None)
            if twitch_api is None:
                twitch_api = getattr(self, "_social_media_twitch_api", None)

            if clip_manager is None:
                clip_manager = ClipManager(twitch_api=twitch_api)
            self._social_media_clip_manager = clip_manager
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
        return await _routes_settings.abbo_promo_settings(self, request)

    async def abbo_lurker_tax_settings(self, request: web.Request) -> web.StreamResponse:
        return await _routes_settings.abbo_lurker_tax_settings(self, request)

    async def abbo_promo_message(self, request: web.Request) -> web.StreamResponse:
        return await _routes_settings.abbo_promo_message(self, request)

    async def admin_roadmap_page(self, request: web.Request) -> web.StreamResponse:
        return await _routes_entry.admin_roadmap_page(self, request)

    def attach(self, app: web.Application) -> None:
        app.add_routes(_routes_entry.build_route_defs(self))
        app.add_routes(_routes_billing.build_route_defs(self))
        app.add_routes(_routes_settings.build_route_defs(self))
        app.add_routes(_routes_market.build_route_defs(self))
        app.add_routes(
            [
                web.get("/twitch/raid/auth", self.raid_auth_start),
                web.get("/twitch/raid/go", self.raid_auth_go),
                web.get("/twitch/raid/requirements", self.raid_requirements),
                web.get("/twitch/raid/history", self.raid_history),
                web.get("/twitch/raid/analytics", self.raid_analytics),
                web.get("/twitch/auth/login", self.auth_login),
                web.get("/twitch/auth/callback", self.auth_callback),
                web.get("/callback/twitch", self.auth_callback),
                web.post("/twitch/auth/partner/link", self.auth_partner_link),
                web.post("/twitch/auth/partner/login", self.auth_partner_login),
                web.get("/twitch/auth/discord/login", self.discord_auth_login),
                web.get("/twitch/auth/discord/callback", self.discord_auth_callback),
                web.get("/twitch/auth/discord/logout", self.discord_auth_logout),
                web.get("/twitch/raid/callback", self.raid_oauth_callback),
                web.get("/twitch/api/live-announcement/config", self.api_live_announcement_config),
                web.post("/twitch/api/live-announcement/config", self.api_live_announcement_save_config),
                web.post("/twitch/api/live-announcement/test", self.api_live_announcement_test_send),
                web.get("/twitch/api/live-announcement/preview", self.api_live_announcement_preview),
            ]
        )
        self._register_v2_routes(app.router)
        self._affiliate_register_routes(app)
        self._register_social_media_routes(app)

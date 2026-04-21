"""Partner dashboard auth mixin for one-time login exchange and cookie sessions."""

from __future__ import annotations

from typing import Any
from urllib.parse import urlsplit

from aiohttp import web

from ...core.constants import log
from .services import PartnerAccessBinding, PartnerAccessService, PartnerLoginTokenService


class _DashboardPartnerAuthMixin:
    """Partner-specific auth helpers kept separate from the main OAuth mixin."""

    @staticmethod
    def _partner_link_request_origin_host(raw_url: str | None) -> str:
        candidate = str(raw_url or "").strip()
        if not candidate:
            return ""
        try:
            parsed = urlsplit(candidate)
        except Exception:
            return ""
        if parsed.scheme not in {"http", "https"} or not parsed.netloc:
            return ""
        return str(parsed.netloc or "").strip().lower()

    def _partner_link_has_same_origin(self, request: web.Request) -> bool:
        headers = getattr(request, "headers", {}) or {}
        request_host = str(
            headers.get("Host") or getattr(request, "host", "") or ""
        ).strip().lower()
        if not request_host:
            return False

        origin_host = self._partner_link_request_origin_host(headers.get("Origin"))
        if origin_host:
            return origin_host == request_host

        referer_host = self._partner_link_request_origin_host(headers.get("Referer"))
        if referer_host:
            return referer_host == request_host
        return False

    def _partner_access_service(self) -> PartnerAccessService:
        service = getattr(self, "_partner_access_service_cache", None)
        if isinstance(service, PartnerAccessService):
            return service
        service = PartnerAccessService(self)
        self._partner_access_service_cache = service
        return service

    def _partner_login_token_service(self) -> PartnerLoginTokenService:
        service = getattr(self, "_partner_login_token_service_cache", None)
        if isinstance(service, PartnerLoginTokenService):
            return service
        service = PartnerLoginTokenService(self)
        self._partner_login_token_service_cache = service
        return service

    def _partner_access_cookie_name(self) -> str:
        return self._cookie_service().partner_access_cookie_name()

    def _partner_access_session_ttl(self) -> int:
        configured_ttl = max(
            300,
            int(getattr(self, "_session_ttl_seconds", 6 * 3600) or 6 * 3600),
        )
        return min(configured_ttl, 1800)

    def _partner_access_request_context(self, request: web.Request) -> dict[str, str]:
        return PartnerAccessBinding.capture(request)

    @staticmethod
    def _partner_access_binding_matches(
        session: dict[str, Any],
        request_context: dict[str, str],
    ) -> bool:
        return PartnerAccessBinding.matches(session, request_context)

    def _get_partner_access_session(self, request: web.Request) -> dict[str, Any] | None:
        return self._partner_access_service().load(request)

    def _create_partner_access_session(self, request: web.Request) -> str:
        return self._partner_access_service().create(request)

    def _delete_partner_access_session(self, session_id: str) -> None:
        self._partner_access_service().delete(session_id)

    def _set_partner_access_cookie(
        self, response: web.StreamResponse, request: web.Request, session_id: str
    ) -> None:
        self._cookie_service().set_partner_access_cookie(response, request, session_id)

    def _clear_partner_access_cookie(
        self, response: web.StreamResponse, request: web.Request
    ) -> None:
        self._cookie_service().clear_partner_access_cookie(response, request)

    async def auth_partner_link(self, request: web.Request) -> web.StreamResponse:
        """Issue a short-lived one-time partner login token for browser bootstrap."""
        auth_mode = self._partner_login_token_service().issue_request_authorization_mode(request)
        if not auth_mode:
            raise web.HTTPUnauthorized(text="missing or invalid partner link credentials")
        if auth_mode == "admin_session" and not self._partner_link_has_same_origin(request):
            raise web.HTTPForbidden(text="same-origin partner link request required")
        if not self._check_rate_limit(request, max_requests=10, window_seconds=60.0):
            return web.Response(text="Zu viele Anfragen. Bitte warte kurz.", status=429)

        next_path = None
        query = getattr(request, "query", {}) or {}
        if hasattr(query, "get"):
            next_path = query.get("next")

        if not next_path:
            json_reader = getattr(request, "json", None)
            if callable(json_reader):
                try:
                    payload = await json_reader()
                except Exception:
                    payload = None
                if isinstance(payload, dict):
                    next_path = payload.get("next")

        try:
            issued_token = self._partner_login_token_service().issue(next_path=next_path)
        except RuntimeError as exc:
            log.warning("Could not issue partner login link: %s", exc)
            return web.Response(
                text="Partner-Login-Link konnte nicht sicher erstellt werden.",
                status=503,
            )

        return web.json_response(
            {
                "login_path": "/twitch/auth/partner/login",
                "login_method": "POST",
                "login_token": issued_token["token"],
                "next_path": issued_token["next_path"],
                "expires_in": issued_token["expires_in"],
            },
            headers={
                "Cache-Control": "no-store, max-age=0",
                "Pragma": "no-cache",
            },
        )

    async def auth_partner_login(self, request: web.Request) -> web.StreamResponse:
        """Consume a one-time partner login token and create the partner access cookie."""
        if not self._check_rate_limit(request, max_requests=20, window_seconds=60.0):
            return web.Response(text="Zu viele Anfragen. Bitte warte kurz.", status=429)

        token = ""
        post_reader = getattr(request, "post", None)
        if callable(post_reader):
            try:
                form_data = await post_reader()
            except Exception:
                form_data = None
            if hasattr(form_data, "get"):
                token = str(form_data.get("token") or "").strip()

        if not token:
            json_reader = getattr(request, "json", None)
            if callable(json_reader):
                try:
                    payload = await json_reader()
                except Exception:
                    payload = None
                if isinstance(payload, dict):
                    token = str(payload.get("token") or "").strip()

        if not token:
            return web.Response(text="Fehlendes Partner-Login-Token.", status=400)

        login_state = self._partner_login_token_service().consume(token)
        if not isinstance(login_state, dict):
            return web.Response(
                text="Partner-Login-Token ungültig oder abgelaufen.",
                status=401,
            )

        destination = self._safe_internal_redirect(
            login_state.get("next_path"),
            fallback="/twitch/analyse",
        )
        if (
            self._get_dashboard_auth_session(request)
            or self._get_discord_admin_session(request)
            or self._get_partner_access_session(request)
        ):
            response = web.HTTPSeeOther(destination)
            response.headers["Cache-Control"] = "no-store, max-age=0"
            response.headers["Pragma"] = "no-cache"
            raise response

        session_id = self._create_partner_access_session(request)
        response = web.HTTPSeeOther(destination)
        self._set_partner_access_cookie(response, request, session_id)
        response.headers["Cache-Control"] = "no-store, max-age=0"
        response.headers["Pragma"] = "no-cache"
        raise response

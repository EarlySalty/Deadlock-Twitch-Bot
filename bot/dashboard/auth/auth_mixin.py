"""Auth mixin for DashboardV2Server — Twitch OAuth and Discord admin session management."""

from __future__ import annotations

import asyncio
import hashlib
import os
import secrets
import time
from typing import Any
from urllib.parse import parse_qsl, urlencode, urlparse, urlsplit, urlunsplit

import aiohttp
from aiohttp import web

from ... import storage
from ...core.constants import log
from .services import (
    DashboardAuthCookieService,
    DashboardSessionService,
)
from .state_store import (
    DashboardAuthRateLimitStore,
    DashboardAuthRateLimitStoreUnavailable,
    DashboardAuthStateCache,
    DashboardAuthStateRepository,
)

TWITCH_OAUTH_AUTHORIZE_URL = "https://id.twitch.tv/oauth2/authorize"
TWITCH_OAUTH_TOKEN_URL = "https://id.twitch.tv/oauth2/token"  # noqa: S105
TWITCH_HELIX_USERS_URL = "https://api.twitch.tv/helix/users"
DISCORD_API_BASE_URL = "https://discord.com/api/v10"
LEGACY_TWITCH_OAUTH_CALLBACK_PATH = "/twitch/auth/callback"
SHARED_TWITCH_OAUTH_CALLBACK_PATH = "/callback/twitch"
SHARED_DISCORD_OAUTH_CALLBACK_PATH = "/callback/discord"
DISCORD_OAUTH_INTERNAL_API_BASE_URL = "http://127.0.0.1:8766"
DISCORD_OAUTH_INTERNAL_TOKEN_HEADER = "X-Internal-Token"
DISCORD_OAUTH_INITIATE_PATH = "/internal/v1/discord/initiate"
DISCORD_OAUTH_CONSUME_RESULT_PATH = "/internal/v1/discord/consume-result"
TWITCH_DISCORD_LINK_FALLBACK_PATH = "/twitch/verwaltung"
TWITCH_PUBLIC_DASHBOARD_BASE_URL = (
    os.getenv("TWITCH_PUBLIC_DASHBOARD_BASE_URL")
    or "https://deutsche-deadlock-community.de"
).strip().rstrip("/")
_DDC_PENTEST_DISABLE_RATE_LIMITS = str(
    os.getenv("DDC_PENTEST_DISABLE_RATE_LIMITS", "0")
).strip().lower() not in {"", "0", "false", "no", "off"}

# Pfade die für nicht-aktive Partner (departnered/opt-out/token_error/bot_banned) erlaubt bleiben.
# "passive" = User darf sich einloggen und Verwaltung/Plan/Affiliate nutzen, aber nicht Analyse/Social/Title.
# Hard-Kill ist nur technical_pause_reason='blocked' — wer da landet, kommt schon vom Login-Gate nicht durch.
_PASSIVE_ALLOWED_EXACT_PATHS = frozenset({
    "/twitch/verwaltung",
    "/twitch/pricing",
    "/twitch/abbo",
    "/twitch/abbo/bezahlen",
    "/twitch/abbo/kündigen",
    "/twitch/abbo/lurker-tax-settings",
    "/twitch/abbo/promo-message",
    "/twitch/abbo/promo-settings",
    "/twitch/abbo/rechnung",
    "/twitch/abbo/rechnungen",
    "/twitch/abbo/rechnungsdaten",
    "/twitch/abbo/stripe-settings",
    "/twitch/dashboard",
    "/twitch/dashboards",
    "/twitch/dashboads",
    "/twitch/affiliate/portal",
    "/twitch/affiliate/signup",
    "/twitch/affiliate/signup/complete",
    "/twitch/affiliate/claim",
    "/twitch/affiliate/connect/stripe",
    "/twitch/affiliate/connect/stripe/callback",
    "/twitch/raid/auth",
})
_PASSIVE_ALLOWED_PREFIXES: tuple[str, ...] = (
    "/twitch/auth/",
    "/callback/twitch",
    "/callback/discord",
    "/twitch/api/v2/internal-home",
    "/twitch/api/v2/auth-status",
    "/twitch/api/billing/",
    "/twitch/api/v2/billing/",
    "/twitch/api/affiliate/",
    "/twitch/api/v2/affiliate/",
    "/twitch/auth/discord/",
    "/twitch/auth/partner/",
    "/twitch/agb",
    "/twitch/datenschutz",
    "/twitch/impressum",
    "/twitch/legal/",
    "/health",
    "/healthz",
    "/readyz",
    "/twitch/raid/auth",
)
_PUBLIC_PATH_PREFIXES: tuple[str, ...] = (
    "/health",
    "/healthz",
    "/readyz",
    "/twitch/auth/",
    "/twitch/eventsub/",
    "/twitch/api/billing/stripe/webhook",
    "/twitch/agb",
    "/twitch/datenschutz",
    "/twitch/impressum",
    "/twitch/legal/",
    "/twitch/demo",
    "/twitch/raid/callback",
    "/callback/twitch",
    "/callback/discord",
    "/twitch/api/v2/public/",
)


def _path_matches_passive_allowed(path: str) -> bool:
    if not path:
        return False
    if path in _PASSIVE_ALLOWED_EXACT_PATHS:
        return True
    return any(path.startswith(prefix) for prefix in _PASSIVE_ALLOWED_PREFIXES)


def _path_is_public(path: str) -> bool:
    if not path:
        return False
    return any(path.startswith(prefix) for prefix in _PUBLIC_PATH_PREFIXES)


def _build_passive_fp(request: web.Request) -> str:
    """Build a coarse browser fingerprint from stable request headers."""
    ua = str(request.headers.get("User-Agent") or "").strip()
    lang = str(request.headers.get("Accept-Language") or "").split(",")[0].strip()
    platform = str(request.headers.get("Sec-CH-UA-Platform") or "").strip().strip('"')
    raw = f"{ua}|{lang}|{platform}"
    return hashlib.sha256(raw.encode("utf-8")).hexdigest()[:32]


class _DashboardAuthMixin:
    """Twitch OAuth login/callback and Discord admin session management."""

    # ------------------------------------------------------------------ #
    # OAuth configuration helpers                                          #
    # ------------------------------------------------------------------ #

    def _is_oauth_configured(self) -> bool:
        return bool(self._oauth_client_id and self._oauth_client_secret)

    def _build_oauth_redirect_uri(self) -> str | None:
        configured = (self._oauth_redirect_uri or "").strip()
        if not configured:
            return None

        candidate = configured if "://" in configured else f"https://{configured}"
        try:
            parsed = urlparse(candidate)
        except Exception:
            log.warning("TWITCH_DASHBOARD_AUTH_REDIRECT_URI is invalid and cannot be parsed")
            return None

        scheme = (parsed.scheme or "").strip().lower()
        host = (parsed.hostname or "").strip().lower()
        path = (parsed.path or "").rstrip("/")

        if parsed.username or parsed.password:
            log.warning("TWITCH_DASHBOARD_AUTH_REDIRECT_URI must not contain user info")
            return None
        if scheme not in {"https", "http"}:
            log.warning("TWITCH_DASHBOARD_AUTH_REDIRECT_URI must use http(s)")
            return None
        if scheme == "http" and host not in {"127.0.0.1", "localhost", "::1"}:
            log.warning(
                "TWITCH_DASHBOARD_AUTH_REDIRECT_URI must use https unless host is localhost"
            )
            return None
        if not parsed.netloc:
            log.warning("TWITCH_DASHBOARD_AUTH_REDIRECT_URI is missing host")
            return None
        if path == "/twitch/raid/callback":
            log.warning(
                "TWITCH_DASHBOARD_AUTH_REDIRECT_URI points to raid callback and is not allowed"
            )
            return None
        if path not in {
            LEGACY_TWITCH_OAUTH_CALLBACK_PATH,
            SHARED_TWITCH_OAUTH_CALLBACK_PATH,
        }:
            log.warning(
                "TWITCH_DASHBOARD_AUTH_REDIRECT_URI must point to /twitch/auth/callback "
                "or /callback/twitch"
            )
            return None

        return urlunsplit((scheme, parsed.netloc, path, "", ""))

    def _oauth_callback_cookie_path(self) -> str:
        redirect_uri = self._build_oauth_redirect_uri()
        if not redirect_uri:
            return LEGACY_TWITCH_OAUTH_CALLBACK_PATH
        try:
            path = (urlsplit(redirect_uri).path or "").rstrip("/")
        except Exception:
            return LEGACY_TWITCH_OAUTH_CALLBACK_PATH
        if path in {
            LEGACY_TWITCH_OAUTH_CALLBACK_PATH,
            SHARED_TWITCH_OAUTH_CALLBACK_PATH,
        }:
            return path
        return LEGACY_TWITCH_OAUTH_CALLBACK_PATH

    @staticmethod
    def _request_path(request: web.Request) -> str:
        rel_url = getattr(request, "rel_url", None)
        path = str(getattr(rel_url, "path", "") or "").strip()
        if path:
            return path
        path_qs = str(getattr(rel_url, "path_qs", "") or "").strip()
        if not path_qs:
            return ""
        return urlsplit(path_qs).path or path_qs.split("?", 1)[0]

    def _is_shared_twitch_oauth_callback_request(self, request: web.Request) -> bool:
        return self._request_path(request) == SHARED_TWITCH_OAUTH_CALLBACK_PATH

    def _is_shared_discord_oauth_callback_request(self, request: web.Request) -> bool:
        return self._request_path(request) == SHARED_DISCORD_OAUTH_CALLBACK_PATH

    async def _delegate_shared_twitch_oauth_callback(
        self, request: web.Request
    ) -> web.StreamResponse | None:
        if not self._is_shared_twitch_oauth_callback_request(request):
            return None
        state = str(request.query.get("state") or "").strip()
        if not state:
            return None
        auth_manager = None
        resolver = getattr(self, "_resolve_dashboard_auth_manager", None)
        if callable(resolver):
            try:
                auth_manager = resolver()
            except Exception:
                auth_manager = None
        if auth_manager is None:
            raid_bot = getattr(self, "_raid_bot", None)
            auth_manager = getattr(raid_bot, "auth_manager", None)
        has_state_details = getattr(auth_manager, "has_state_details", None)
        raid_callback = getattr(self, "raid_oauth_callback", None)
        if not callable(raid_callback):
            return None
        if callable(has_state_details):
            try:
                if not bool(has_state_details(state)):
                    return None
            except Exception:
                log.debug("Could not inspect shared Twitch OAuth state for raid callback", exc_info=True)
                return None
        return await raid_callback(request)

    async def shared_discord_auth_callback(
        self, request: web.Request
    ) -> web.StreamResponse:
        """Resolve the single public Discord callback into the admin completion flow."""
        if not self._is_shared_discord_oauth_callback_request(request):
            return web.Response(text="Not Found", status=404)

        state_id = str(request.query.get("state") or request.query.get("state_id") or "").strip()
        if not state_id:
            response = web.Response(text="Fehlender OAuth-State.", status=400)
            self._set_no_store_headers(response)
            return response

        error = str(request.query.get("error") or "").strip()
        target = self._build_discord_admin_route_url(
            "/twitch/auth/discord/complete",
            query={"state_id": state_id},
        )
        if error:
            target = self._build_discord_admin_route_url(
                "/twitch/auth/discord/complete",
                query={"state_id": state_id, "error": error},
            )
        response = web.HTTPFound(target)
        self._set_no_store_headers(response)
        raise response

    def _load_dashboard_oauth_state(self, state: str) -> dict[str, Any] | None:
        state_key = str(state or "").strip()
        if not state_key:
            return None
        cached_state = self._dashboard_auth_state_cache("_oauth_states").get(state_key)
        if cached_state:
            return cached_state
        try:
            return self._dashboard_auth_state_repo().load_twitch_oauth_state(
                state_key,
                now=time.time(),
            )
        except Exception as exc:
            log.warning(
                "Could not load persisted Twitch OAuth state %s: %s",
                self._sanitize_log_value(state_key),
                self._sanitize_log_value(exc),
            )
            return None

    @staticmethod
    def _render_oauth_page(title: str, body_html: str) -> str:
        import html
        return (
            "<!doctype html><html lang='de'><head><meta charset='utf-8'>"
            "<meta name='viewport' content='width=device-width,initial-scale=1'>"
            f"<title>{html.escape(title, quote=True)}</title>"
            "<style>"
            "body{font-family:Segoe UI,Arial,sans-serif;background:#0f172a;color:#e2e8f0;margin:0;}"
            ".wrap{max-width:760px;margin:0 auto;padding:36px 18px;}"
            ".card{background:#111827;border:1px solid #1f2937;border-radius:12px;padding:20px;}"
            "h1{margin:0 0 12px 0;font-size:24px;}"
            "p{line-height:1.5;margin:10px 0;}"
            "code{background:#0b1220;border:1px solid #23304a;padding:2px 6px;border-radius:6px;}"
            "a{color:#93c5fd;}"
            "</style></head><body><div class='wrap'><div class='card'>"
            f"<h1>{html.escape(title)}</h1>{body_html}</div></div></body></html>"
        )

    def _normalize_next_path(self, raw_path: str | None) -> str:
        fallback = "/twitch/dashboard"
        candidate = (raw_path or "").strip()
        if not candidate:
            return fallback
        parsed = urlparse(candidate)
        if parsed.scheme or parsed.netloc:
            return fallback
        if not candidate.startswith("/"):
            return fallback
        if candidate.startswith("/analyse"):
            return candidate
        if candidate.startswith("/social-media-admin"):
            return candidate
        if not candidate.startswith("/twitch"):
            return fallback
        return candidate

    def _normalize_discord_link_next_path(self, raw_path: str | None) -> str:
        canonical_getter = getattr(self, "_canonical_post_login_destination", None)
        if callable(canonical_getter):
            try:
                normalized = str(canonical_getter(raw_path) or "").strip()
            except Exception:
                normalized = ""
            if normalized:
                return normalized
        return TWITCH_DISCORD_LINK_FALLBACK_PATH

    def _build_public_dashboard_route_url(
        self,
        request: web.Request | None,
        path: str,
        *,
        query: dict[str, str] | None = None,
        raw_query: str | None = None,
    ) -> str:
        normalized_path = str(path or "").strip() or "/"
        if not normalized_path.startswith("/"):
            normalized_path = f"/{normalized_path}"

        base_url = TWITCH_PUBLIC_DASHBOARD_BASE_URL
        checker = getattr(self, "_is_local_request", None)
        is_local_request = False
        if callable(checker) and request is not None:
            try:
                is_local_request = bool(checker(request))
            except Exception:
                is_local_request = False
        if is_local_request and request is not None:
            secure_checker = getattr(self, "_is_secure_request", None)
            is_secure = bool(secure_checker(request)) if callable(secure_checker) else False
            host = str(getattr(request, "host", "") or "").strip()
            if host:
                base_url = f"{'https' if is_secure else 'http'}://{host}".rstrip("/")

        if raw_query is not None:
            query_string = raw_query.lstrip("?")
        elif query:
            query_string = urlencode(query)
        else:
            query_string = ""
        if query_string:
            return f"{base_url}{normalized_path}?{query_string}"
        return f"{base_url}{normalized_path}"

    @staticmethod
    def _append_redirect_status(
        location: str | None,
        *,
        ok: str | None = None,
        err: str | None = None,
        fallback: str = TWITCH_DISCORD_LINK_FALLBACK_PATH,
    ) -> str:
        try:
            parts = urlsplit(str(location or "").strip() or fallback)
        except Exception:
            parts = urlsplit(fallback)
        if parts.scheme or parts.netloc or not (parts.path or "").startswith("/"):
            parts = urlsplit(fallback)
        params = dict(parse_qsl(parts.query, keep_blank_values=True))
        params.pop("ok", None)
        params.pop("err", None)
        if ok:
            params["ok"] = ok
        if err:
            params["err"] = err
        query = urlencode(params)
        return urlunsplit(("", "", parts.path or fallback, query, ""))

    @staticmethod
    def _safe_internal_redirect(
        location: str | None, *, fallback: str = "/analyse"
    ) -> str:
        candidate = (location or "").strip()
        if not candidate:
            return fallback
        try:
            parts = urlsplit(candidate)
        except Exception:
            return fallback
        if parts.scheme or parts.netloc:
            return fallback
        if not candidate.startswith("/"):
            return fallback
        return candidate

    @staticmethod
    def _safe_oauth_authorize_redirect(location: str | None) -> str:
        candidate = (location or "").strip()
        if not candidate:
            return TWITCH_OAUTH_AUTHORIZE_URL
        try:
            parts = urlsplit(candidate)
        except Exception:
            return TWITCH_OAUTH_AUTHORIZE_URL
        host = (parts.netloc or "").split("@")[-1].split(":", 1)[0].strip().lower()
        if parts.scheme != "https" or host != "id.twitch.tv" or parts.path != "/oauth2/authorize":
            return TWITCH_OAUTH_AUTHORIZE_URL
        return candidate

    @staticmethod
    def _canonical_post_login_destination(next_path: str | None) -> str:
        fallback = "/twitch/dashboard"
        candidate = (next_path or "").strip()
        if not candidate:
            return fallback
        try:
            parts = urlsplit(candidate)
        except Exception:
            return fallback
        if parts.scheme or parts.netloc:
            return fallback

        normalized_path = (parts.path or "").rstrip("/") or "/"
        mapped_path = normalized_path
        if normalized_path == "/twitch/abo":
            mapped_path = "/twitch/abbo"
        elif normalized_path == "/twitch/abos":
            mapped_path = "/twitch/abbo"
        elif normalized_path == "/twitch/dashboads":
            mapped_path = "/twitch/dashboard"
        elif normalized_path == "/twitch/dashboards":
            mapped_path = "/twitch/dashboard"

        if mapped_path in {
            "/twitch/abbo",
            "/twitch/abbo/stripe-settings",
            "/twitch/abbo/rechnungen",
            "/twitch/abbo/rechnung",
            "/twitch/abbo/kündigen",
            "/twitch/stats",
            "/twitch/dashboard",
            "/analyse",
            "/twitch/verwaltung",
            "/twitch/pricing",
            "/twitch/raid/auth",
            "/twitch/live-announcement",
        }:
            query_suffix = f"?{parts.query}" if parts.query else ""
            return f"{mapped_path}{query_suffix}"
        return fallback

    def _build_dashboard_login_url(self, request: web.Request) -> str:
        next_path = self._normalize_next_path(
            request.rel_url.path_qs if request.rel_url else "/twitch/dashboard"
        )
        if self._should_use_discord_admin_login(request):
            return self._build_discord_admin_login_url(request, next_path=next_path)
        return f"/twitch/auth/login?{urlencode({'next': next_path})}"

    def _is_twitch_oauth_ready(self) -> bool:
        """Return True when Twitch OAuth login can be started safely."""
        if not self._is_oauth_configured():
            return False
        return bool(self._build_oauth_redirect_uri())

    def _is_public_host_discord_admin_route(self, request: web.Request) -> bool:
        admin_host_checker = getattr(self, "_is_admin_dashboard_host_request", None)
        is_local_request = False
        local_checker = getattr(self, "_is_local_request", None)
        if callable(local_checker):
            try:
                is_local_request = bool(local_checker(request))
            except Exception:
                is_local_request = False
        if is_local_request:
            return False
        if not callable(admin_host_checker):
            return False
        try:
            return not bool(admin_host_checker(request))
        except Exception:
            return False

    @staticmethod
    def _oauth_unavailable_response() -> web.Response:
        return web.Response(
            text=(
                "Twitch OAuth ist aktuell nicht konfiguriert oder die Redirect-URI ist ungültig. "
                "Bitte OAuth-Einstellungen prüfen."
            ),
            status=503,
        )

    @staticmethod
    def _set_no_store_headers(response: web.StreamResponse) -> web.StreamResponse:
        response.headers["Cache-Control"] = "no-store, max-age=0"
        response.headers["Pragma"] = "no-cache"
        return response

    def _dashboard_auth_challenge(
        self,
        request: web.Request,
        *,
        next_path: str | None = None,
        allow_discord_admin_login: bool = True,
    ) -> web.StreamResponse:
        """Return redirect to login or 503 when OAuth is unavailable."""
        normalized_next = self._normalize_next_path(
            next_path or (request.rel_url.path_qs if request.rel_url else "/twitch/dashboard")
        )

        if allow_discord_admin_login and self._should_use_discord_admin_login(request):
            if self._discord_admin_required:
                discord_login_url = self._build_discord_admin_login_url(
                    request,
                    next_path=normalized_next,
                )
                safe_discord_login_url = self._safe_discord_admin_login_redirect(discord_login_url)
                return web.HTTPFound(safe_discord_login_url)
            return web.Response(
                text=(
                    "Discord Admin OAuth ist nicht konfiguriert. "
                    "Bitte Client ID, Client Secret und Redirect URI setzen."
                ),
                status=503,
            )

        if self._is_twitch_oauth_ready():
            return web.HTTPFound(f"/twitch/auth/login?{urlencode({'next': normalized_next})}")
        return self._oauth_unavailable_response()

    def _dashboard_auth_state_repo(self) -> DashboardAuthStateRepository:
        repo = getattr(self, "_dashboard_auth_state_repo_cache", None)
        if isinstance(repo, DashboardAuthStateRepository):
            return repo
        repo = DashboardAuthStateRepository()
        self._dashboard_auth_state_repo_cache = repo
        return repo

    def _dashboard_auth_state_cache(self, attr_name: str) -> DashboardAuthStateCache:
        return DashboardAuthStateCache(self, attr_name)

    def _cookie_service(self) -> DashboardAuthCookieService:
        service = getattr(self, "_dashboard_auth_cookie_service_cache", None)
        if isinstance(service, DashboardAuthCookieService):
            return service
        service = DashboardAuthCookieService(self)
        self._dashboard_auth_cookie_service_cache = service
        return service

    def _dashboard_session_service(self) -> DashboardSessionService:
        service = getattr(self, "_dashboard_session_service_cache", None)
        if isinstance(service, DashboardSessionService):
            return service
        service = DashboardSessionService(self)
        self._dashboard_session_service_cache = service
        return service

    def _dashboard_auth_rate_limit_store(self) -> DashboardAuthRateLimitStore:
        store = getattr(self, "_dashboard_auth_rate_limit_store_cache", None)
        if isinstance(store, DashboardAuthRateLimitStore):
            return store
        store = DashboardAuthRateLimitStore()
        self._dashboard_auth_rate_limit_store_cache = store
        return store

    def _mark_dashboard_sessions_db_loaded(self) -> None:
        if hasattr(self, "_sessions_db_loaded"):
            self._sessions_db_loaded = True

    def _mark_discord_sessions_db_loaded(self) -> None:
        if hasattr(self, "_discord_sessions_db_loaded"):
            self._discord_sessions_db_loaded = True

    def _check_rate_limit(
        self,
        request: web.Request,
        *,
        max_requests: int = 10,
        window_seconds: float = 60.0,
    ) -> bool:
        if _DDC_PENTEST_DISABLE_RATE_LIMITS:
            return True
        key_builder = getattr(self, "_rate_limit_key", None)
        if callable(key_builder):
            try:
                key = str(key_builder(request) or "").strip() or "unknown"
            except Exception:
                key = "unknown"
        else:
            peer_host = getattr(self, "_peer_host", None)
            if callable(peer_host):
                try:
                    key = str(peer_host(request) or "").strip() or "unknown"
                except Exception:
                    key = "unknown"
            else:
                key = "unknown"
        try:
            return self._dashboard_auth_rate_limit_store().allow_request(
                key=key,
                max_requests=max_requests,
                window_seconds=window_seconds,
            )
        except DashboardAuthRateLimitStoreUnavailable as exc:
            # Fail-open: wenn der Rate-Limit Store nicht verfügbar ist, erlauben wir
            # den Request (nicht blockieren) und loggen nur ein Warning.
            # Der Rate-Limiter ist ein Komfort-Feature, kein Sicherheits-Feature -
            # absolute Rate-Limiting erfolgt über den Discord OAuth Flow selbst.
            log.warning(
                "Dashboard auth rate limit store unavailable (%s); allowing request (fail-open).",
                exc,
            )
            return True
        except Exception:
            log.exception(
                "Unexpected dashboard auth rate limit failure; allowing request (fail-open)"
            )
            return True

    # ------------------------------------------------------------------ #
    # Twitch OAuth session management                                      #
    # ------------------------------------------------------------------ #

    def _cleanup_auth_state(self) -> None:
        self._dashboard_session_service().cleanup()

    def _get_dashboard_auth_session(self, request: web.Request) -> dict[str, Any] | None:
        # Load the main dashboard session (Twitch OAuth / partner login)
        session = self._dashboard_session_service().load(request)

        # Also check for Discord admin session - if present, it grants admin access
        # even if the main session is a regular partner session
        discord_admin = self._get_discord_admin_session(request)
        if discord_admin:
            if session is None:
                # Use the persistent discord_admin session object
                # for state persistence (like CSRF tokens)
                session = discord_admin
                session["is_admin"] = True
            else:
                # Regular session exists - upgrade to admin
                session["is_admin"] = True
                session["admin_info"] = {
                    "user_id": discord_admin.get("user_id"),
                    "username": discord_admin.get("username"),
                    "display_name": discord_admin.get("display_name"),
                    "auth_type": "discord_admin",
                }
        return session

    def _set_session_cookie(
        self, response: web.StreamResponse, request: web.Request, session_id: str
    ) -> None:
        self._cookie_service().set_session_cookie(response, request, session_id)

    @staticmethod
    def _is_valid_oauth_context_token(token: str) -> bool:
        candidate = (token or "").strip()
        if len(candidate) < 16 or len(candidate) > 128:
            return False
        return all(ch.isalnum() or ch in {"-", "_"} for ch in candidate)

    def _oauth_context_cookie_name(self) -> str:
        return self._cookie_service().oauth_context_cookie_name()

    def _set_oauth_context_cookie(
        self, response: web.StreamResponse, request: web.Request, token: str
    ) -> None:
        self._cookie_service().set_oauth_context_cookie(response, request, token)

    def _clear_oauth_context_cookie(
        self, response: web.StreamResponse, request: web.Request
    ) -> None:
        self._cookie_service().clear_oauth_context_cookie(response, request)

    def _discord_oauth_context_cookie_name(self) -> str:
        return self._cookie_service().discord_oauth_context_cookie_name()

    def _set_discord_oauth_context_cookie(
        self, response: web.StreamResponse, request: web.Request, token: str
    ) -> None:
        self._cookie_service().set_discord_oauth_context_cookie(response, request, token)

    def _clear_discord_oauth_context_cookie(
        self, response: web.StreamResponse, request: web.Request
    ) -> None:
        self._cookie_service().clear_discord_oauth_context_cookie(response, request)

    def _clear_session_cookie(self, response: web.StreamResponse, request: web.Request) -> None:
        self._cookie_service().clear_session_cookie(response, request)

    def _delete_dashboard_auth_session(self, session_id: str) -> dict[str, Any] | None:
        return self._dashboard_session_service().delete(session_id)

    def _create_dashboard_session(
        self, *, twitch_login: str, twitch_user_id: str, display_name: str
    ) -> str:
        return self._dashboard_session_service().create(
            twitch_login=twitch_login,
            twitch_user_id=twitch_user_id,
            display_name=display_name,
        )

    def _is_partner_allowed(
        self, *, twitch_login: str, twitch_user_id: str
    ) -> dict[str, Any] | None:
        login = (twitch_login or "").strip().lower()
        user_id = (twitch_user_id or "").strip()
        if not login and not user_id:
            return None

        with storage.readonly_connection() as conn:
            sql = """
                SELECT p.twitch_login, p.twitch_user_id
                FROM twitch_partners p
                WHERE LOWER(COALESCE(p.technical_pause_reason, '')) <> 'blocked'
                  AND (
                      LOWER(p.twitch_login) = LOWER(%s)
                      OR (%s <> '' AND p.twitch_user_id = %s)
                  )
                ORDER BY CASE
                    WHEN COALESCE(p.status, '') = 'active' THEN 0
                    WHEN COALESCE(p.status, '') = 'archived' THEN 1
                    WHEN COALESCE(p.status, '') = 'departnered' THEN 2
                    ELSE 3
                END,
                COALESCE(p.departnered_at, p.admin_archived_at, p.partnered_at) DESC
                LIMIT 1
            """
            row = conn.execute(sql, (login, user_id, user_id)).fetchone()

        if not row:
            return None

        if hasattr(row, "keys"):
            return {
                "twitch_login": str(row["twitch_login"] or ""),
                "twitch_user_id": str(row["twitch_user_id"] or ""),
            }
        return {
            "twitch_login": str(row[0] or ""),
            "twitch_user_id": str(row[1] or ""),
        }

    def _resolve_partner_active_status(
        self, *, twitch_login: str, twitch_user_id: str
    ) -> str:
        """
        Liefert "active" wenn Partner is_partner_active=1, sonst "passive".
        Aufrufer prüfen separat ob die Session überhaupt einen Partner hat.
        """
        login = (twitch_login or "").strip().lower()
        user_id = (twitch_user_id or "").strip()
        if not login and not user_id:
            return "passive"
        try:
            with storage.readonly_connection() as conn:
                row = conn.execute(
                    """
                    SELECT
                        CASE
                            WHEN COALESCE(p.status, '') = 'active'
                                 AND COALESCE(p.manual_partner_opt_out, 0) = 0
                                 AND LOWER(COALESCE(p.technical_pause_reason, '')) NOT IN ('blocked', 'bot_banned', 'token_error', 'token_error_expired')
                            THEN 1 ELSE 0
                        END AS is_active
                    FROM twitch_partners p
                    WHERE LOWER(COALESCE(p.technical_pause_reason, '')) <> 'blocked'
                      AND (
                          LOWER(p.twitch_login) = LOWER(%s)
                          OR (%s <> '' AND p.twitch_user_id = %s)
                      )
                    ORDER BY CASE
                        WHEN COALESCE(p.status, '') = 'active' THEN 0
                        WHEN COALESCE(p.status, '') = 'archived' THEN 1
                        WHEN COALESCE(p.status, '') = 'departnered' THEN 2
                        ELSE 3
                    END,
                    COALESCE(p.departnered_at, p.admin_archived_at, p.partnered_at) DESC
                    LIMIT 1
                    """,
                    (login, user_id, user_id),
                ).fetchone()
        except Exception:
            log.debug(
                "Could not resolve partner active status for %s",
                self._sanitize_log_value(login or user_id),
                exc_info=True,
            )
            return "passive"
        if not row:
            return "passive"
        is_active = row[0] if not hasattr(row, "keys") else row["is_active"]
        return "active" if int(is_active or 0) == 1 else "passive"

    def _reactivate_partner_after_valid_auth(
        self, *, twitch_login: str, twitch_user_id: str
    ) -> dict[str, Any] | None:
        login = (twitch_login or "").strip()
        user_id = (twitch_user_id or "").strip()
        if not login and not user_id:
            return None
        with storage.transaction() as conn:
            return storage.reactivate_partner_after_valid_auth(
                conn,
                twitch_login=login,
                twitch_user_id=user_id,
            )

    async def _is_partner_allowed_async(
        self, *, twitch_login: str, twitch_user_id: str
    ) -> dict[str, Any] | None:
        return await asyncio.to_thread(
            self._is_partner_allowed,
            twitch_login=twitch_login,
            twitch_user_id=twitch_user_id,
        )

    async def _exchange_code_for_user(self, code: str, redirect_uri: str) -> dict[str, str] | None:
        if not self._is_oauth_configured():
            return None

        timeout = aiohttp.ClientTimeout(total=20)
        async with aiohttp.ClientSession(timeout=timeout) as session:
            async with session.post(
                TWITCH_OAUTH_TOKEN_URL,
                data={
                    "client_id": self._oauth_client_id,
                    "client_secret": self._oauth_client_secret,
                    "code": code,
                    "grant_type": "authorization_code",
                    "redirect_uri": redirect_uri,
                },
            ) as token_resp:
                if token_resp.status != 200:
                    log.warning(
                        "Dashboard OAuth exchange failed with status %s",
                        token_resp.status,
                    )
                    return None
                token_data = await token_resp.json()

            access_token = str(token_data.get("access_token") or "").strip()
            if not access_token:
                return None

            async with session.get(
                TWITCH_HELIX_USERS_URL,
                headers={
                    "Authorization": f"Bearer {access_token}",
                    "Client-Id": str(self._oauth_client_id),
                },
            ) as user_resp:
                if user_resp.status != 200:
                    log.warning(
                        "Dashboard OAuth user lookup failed with status %s",
                        user_resp.status,
                    )
                    return None
                user_data = await user_resp.json()

        users = user_data.get("data") if isinstance(user_data, dict) else None
        if not isinstance(users, list) or not users:
            return None
        user = users[0] or {}
        return {
            "twitch_login": str(user.get("login") or "").strip().lower(),
            "twitch_user_id": str(user.get("id") or "").strip(),
            "display_name": str(user.get("display_name") or user.get("login") or "").strip(),
        }

    # ------------------------------------------------------------------ #
    # Discord admin OAuth helpers                                          #
    # ------------------------------------------------------------------ #

    def _cleanup_discord_admin_state(self) -> None:
        now = time.time()
        oauth_states = self._dashboard_auth_state_cache("_discord_admin_oauth_states")
        admin_sessions = self._dashboard_auth_state_cache("_discord_admin_sessions")
        oauth_states.prune_by_created_at(
            ttl_seconds=self._discord_admin_state_ttl,
            now=now,
            max_items=1000,
        )
        admin_sessions.prune_by_expires_at(now=now, max_items=5000)

        try:
            self._dashboard_auth_state_repo().delete_expired(now)
        except Exception as _exc:
            log.debug("Could not purge expired discord auth state from DB: %s", _exc)

    def _set_discord_admin_cookie(
        self,
        response: web.StreamResponse,
        request: web.Request,
        session_id: str,
    ) -> None:
        response.set_cookie(
            self._discord_admin_cookie_name,
            session_id,
            max_age=self._discord_admin_session_ttl,
            httponly=True,
            secure=self._is_secure_request(request),
            samesite="Lax",
            path="/",
        )

    def _clear_discord_admin_cookie(
        self, response: web.StreamResponse, request: web.Request
    ) -> None:
        response.del_cookie(
            self._discord_admin_cookie_name,
            path="/",
            httponly=True,
            samesite="Lax",
            secure=self._is_secure_request(request),
        )

    def _get_discord_admin_session(self, request: web.Request) -> dict[str, Any] | None:
        if not self._discord_admin_required:
            return None

        self._cleanup_discord_admin_state()
        self._mark_discord_sessions_db_loaded()
        admin_sessions = self._dashboard_auth_state_cache("_discord_admin_sessions")
        cookies = getattr(request, "cookies", {}) or {}
        session_id = (cookies.get(self._discord_admin_cookie_name) or "").strip()
        if not session_id:
            return None
        session = admin_sessions.get(session_id)
        if not session:
            try:
                session = self._dashboard_auth_state_repo().load_discord_admin_session(
                    session_id,
                    now=time.time(),
                )
            except Exception as _exc:
                log.debug("Could not load discord admin session from DB: %s", _exc)
                session = None
            if not session:
                return None
            admin_sessions.put(session_id, session)
        now = time.time()
        if float(session.get("expires_at", 0.0)) <= now:
            admin_sessions.pop(session_id, None)
            try:
                self._dashboard_auth_state_repo().delete_session(session_id)
            except Exception as _exc:
                log.debug("Could not delete expired discord session from DB: %s", _exc)
            return None

        old_expires = float(session.get("expires_at", 0.0))
        session["expires_at"] = now + self._discord_admin_session_ttl
        session["last_seen_at"] = now
        session.setdefault("auth_type", "discord_admin")
        if session["expires_at"] - old_expires > 1800:
            try:
                self._dashboard_auth_state_repo().save_discord_admin_session(
                    session_id=session_id,
                    payload=session,
                    created_at=float(session.get("created_at", now)),
                    expires_at=session["expires_at"],
                )
            except Exception as _exc:
                log.debug("Could not refresh discord admin session in DB: %s", _exc)
        return session

    def _is_discord_admin_request(self, request: web.Request) -> bool:
        return bool(self._get_discord_admin_session(request))

    @staticmethod
    def _discord_oauth_internal_api_base_url() -> str:
        return DISCORD_OAUTH_INTERNAL_API_BASE_URL

    def _discord_oauth_internal_api_token(self) -> str:
        loader = getattr(self, "_load_secret_value", None)
        if callable(loader):
            token = str(
                loader(
                    "TWITCH_INTERNAL_API_TOKEN",
                    "MASTER_BROKER_TOKEN",
                    "MAIN_BOT_INTERNAL_TOKEN",
                )
                or ""
            ).strip()
            if token:
                return token
        for env_name in ("TWITCH_INTERNAL_API_TOKEN", "MASTER_BROKER_TOKEN", "MAIN_BOT_INTERNAL_TOKEN"):
            token = str(os.getenv(env_name) or "").strip()
            if token:
                return token
        return ""

    async def _post_discord_oauth_internal_api(
        self,
        path: str,
        payload: dict[str, Any],
    ) -> dict[str, Any] | None:
        token = self._discord_oauth_internal_api_token()
        if not token:
            log.warning("Discord OAuth internal API token is missing.")
            return None
        timeout = aiohttp.ClientTimeout(total=20)
        headers = {
            DISCORD_OAUTH_INTERNAL_TOKEN_HEADER: token,
            "Content-Type": "application/json",
        }
        url = f"{self._discord_oauth_internal_api_base_url().rstrip('/')}{path}"
        async with aiohttp.ClientSession(timeout=timeout) as session:
            async with session.post(url, json=payload, headers=headers) as response:
                if response.status != 200:
                    body = await response.text()
                    log.warning(
                        "Discord OAuth internal API failed (path=%s status=%s body=%s)",
                        path,
                        response.status,
                        self._sanitize_log_value(body[:200]),
                    )
                    return None
                data = await response.json()
        return data if isinstance(data, dict) else None

    async def _fetch_delegated_discord_authorize_url(
        self,
        *,
        redirect_after: str,
        scope: str,
        requesting_service: str,
        metadata: dict[str, Any] | None = None,
    ) -> tuple[str | None, str | None]:
        data = await self._post_discord_oauth_internal_api(
            DISCORD_OAUTH_INITIATE_PATH,
            {
                "scope": scope,
                "redirect_after": redirect_after,
                "requesting_service": requesting_service,
                "metadata": metadata or {},
            },
        )
        authorize_url = str((data or {}).get("authorize_url") or "").strip()
        state_id = str((data or {}).get("state_id") or "").strip()
        if not authorize_url or not state_id:
            return None, None
        try:
            parsed = urlsplit(authorize_url)
        except Exception:
            return None, None
        query_pairs = list(parse_qsl(parsed.query, keep_blank_values=True))
        return (
            urlunsplit((parsed.scheme, parsed.netloc, parsed.path, urlencode(query_pairs), parsed.fragment)),
            state_id,
        )

    async def _fetch_delegated_discord_session(
        self,
        *,
        state_id: str,
    ) -> dict[str, Any] | None:
        return await self._post_discord_oauth_internal_api(
            DISCORD_OAUTH_CONSUME_RESULT_PATH,
            {
                "state_id": state_id,
            },
        )

    async def _exchange_discord_admin_code(
        self,
        code: str,
        redirect_uri: str,
    ) -> dict[str, Any] | None:
        payload = {
            "client_id": self._discord_admin_client_id,
            "client_secret": self._discord_admin_client_secret,
            "grant_type": "authorization_code",
            "code": code,
            "redirect_uri": redirect_uri,
        }
        headers = {"Content-Type": "application/x-www-form-urlencoded"}
        timeout = aiohttp.ClientTimeout(total=20)
        async with aiohttp.ClientSession(timeout=timeout) as session:
            async with session.post(
                f"{DISCORD_API_BASE_URL}/oauth2/token",
                data=payload,
                headers=headers,
            ) as response:
                if response.status != 200:
                    body = await response.text()
                    log.warning(
                        "Discord admin OAuth exchange failed (status=%s body=%s)",
                        response.status,
                        self._sanitize_log_value(body[:200]),
                    )
                    return None
                data = await response.json()
        return data if isinstance(data, dict) else None

    async def _fetch_discord_admin_user(self, access_token: str) -> dict[str, Any] | None:
        if not access_token:
            return None
        timeout = aiohttp.ClientTimeout(total=20)
        headers = {"Authorization": f"Bearer {access_token}"}
        async with aiohttp.ClientSession(timeout=timeout) as session:
            async with session.get(
                f"{DISCORD_API_BASE_URL}/users/@me", headers=headers
            ) as response:
                if response.status != 200:
                    body = await response.text()
                    log.warning(
                        "Discord admin user lookup failed (status=%s body=%s)",
                        response.status,
                        self._sanitize_log_value(body[:200]),
                    )
                    return None
                data = await response.json()
        return data if isinstance(data, dict) else None

    async def _check_discord_admin_membership(self, user_id: int) -> tuple[bool, str]:
        owner_override_user_id = getattr(self, "_discord_admin_owner_user_id", None)
        if isinstance(owner_override_user_id, int) and owner_override_user_id > 0:
            if user_id == owner_override_user_id:
                return True, "owner_override"

        if not user_id:
            return False, "invalid_user_id"

        resolver = getattr(self, "_dashboard_discord_bot", None)
        discord_bot = resolver() if callable(resolver) else None

        guilds: list[Any] = []
        seen: set[int] = set()
        for guild_id in self._discord_admin_guild_ids:
            guild = discord_bot.get_guild(guild_id) if discord_bot else None
            if guild and guild.id not in seen:
                guilds.append(guild)
                seen.add(guild.id)
        if not self._discord_admin_guild_ids:
            log.error(
                "Discord admin login denied because no admin guild allowlist is configured."
            )
            return False, "admin_guild_not_configured"

        for guild in guilds:
            member = guild.get_member(user_id)
            if member is None:
                try:
                    member = await guild.fetch_member(user_id)
                except Exception:
                    member = None
            if member is None:
                continue
            perms = getattr(member, "guild_permissions", None)
            if perms and bool(getattr(perms, "administrator", False)):
                return True, f"guild_admin:{guild.id}"
            role_ids = {
                int(role.id) for role in getattr(member, "roles", []) if getattr(role, "id", None)
            }
            if self._discord_admin_moderator_role_id in role_ids:
                return True, f"moderator_role:{guild.id}"
        return False, "missing_admin_or_moderator_role"

    # ------------------------------------------------------------------ #
    # Twitch OAuth routes                                                  #
    # ------------------------------------------------------------------ #

    async def auth_login(self, request: web.Request) -> web.StreamResponse:
        """Kick off Twitch OAuth login for dashboard access."""
        next_path = self._normalize_next_path(request.query.get("next"))

        if self._check_v2_auth(request):
            destination = self._canonical_post_login_destination(next_path)
            raise web.HTTPFound(destination)

        if not self._check_rate_limit(request, max_requests=30, window_seconds=60.0):
            return web.Response(text="Zu viele Anfragen. Bitte warte kurz.", status=429)

        if not self._is_oauth_configured():
            return web.Response(
                text="Twitch OAuth ist aktuell nicht konfiguriert.",
                status=503,
            )

        self._cleanup_auth_state()
        redirect_uri = self._build_oauth_redirect_uri()
        if not redirect_uri:
            return web.Response(
                text=(
                    "Twitch OAuth Redirect-URI ist nicht konfiguriert oder ungültig. "
                    "Bitte eine gültige /twitch/auth/callback oder /callback/twitch URL konfigurieren."
                ),
                status=503,
            )
        request_cookies = getattr(request, "cookies", {}) or {}
        existing_context_token = (
            request_cookies.get(self._oauth_context_cookie_name()) or ""
        ).strip()
        context_token = (
            existing_context_token
            if self._is_valid_oauth_context_token(existing_context_token)
            else secrets.token_urlsafe(24)
        )
        state = secrets.token_urlsafe(24)
        state_payload = {
            "created_at": time.time(),
            "next_path": next_path,
            "redirect_uri": redirect_uri,
            "context_token": context_token,
        }
        self._dashboard_auth_state_cache("_oauth_states").put(state, state_payload)
        try:
            self._dashboard_auth_state_repo().save_twitch_oauth_state(
                state=state,
                payload=state_payload,
                ttl_seconds=self._oauth_state_ttl_seconds,
            )
        except Exception as exc:
            log.warning(
                "Could not persist Twitch OAuth state %s: %s",
                self._sanitize_log_value(state),
                self._sanitize_log_value(exc),
            )
            self._dashboard_auth_state_cache("_oauth_states").pop(state, None)
            return web.Response(
                text="OAuth-Status konnte nicht sicher gespeichert werden. Bitte erneut versuchen.",
                status=503,
            )
        auth_url = f"{TWITCH_OAUTH_AUTHORIZE_URL}?{urlencode({'client_id': self._oauth_client_id, 'redirect_uri': redirect_uri, 'response_type': 'code', 'state': state})}"
        safe_auth_url = self._safe_oauth_authorize_redirect(auth_url)
        response = web.HTTPFound(safe_auth_url)
        self._set_oauth_context_cookie(response, request, context_token)
        self._set_no_store_headers(response)
        raise response

    async def auth_callback(self, request: web.Request) -> web.StreamResponse:
        """Handle Twitch OAuth callback, verify partner status, and create session."""
        if not self._check_rate_limit(request, max_requests=30, window_seconds=60.0):
            return web.Response(text="Zu viele Anfragen. Bitte warte kurz.", status=429)

        if not self._is_oauth_configured():
            return web.Response(text="OAuth ist nicht konfiguriert.", status=503)

        self._cleanup_auth_state()

        state = (request.query.get("state") or "").strip()
        code = (request.query.get("code") or "").strip()
        error = (request.query.get("error") or "").strip()[:64]
        if error:
            if not self._load_dashboard_oauth_state(state):
                delegated = await self._delegate_shared_twitch_oauth_callback(request)
                if delegated is not None:
                    return delegated
            safe_error = "".join(c for c in error if c.isalnum() or c in "_-")
            response = web.Response(
                text=f"OAuth-Fehler: {safe_error}. Bitte Login erneut starten.",
                status=401,
            )
            self._set_no_store_headers(response)
            return response

        if not state or not code:
            response = web.Response(text="Fehlender OAuth state/code.", status=400)
            self._set_no_store_headers(response)
            return response

        cached_state_data = self._dashboard_auth_state_cache("_oauth_states").pop(state, None)
        try:
            state_data = self._dashboard_auth_state_repo().consume_twitch_oauth_state(
                state,
                now=time.time(),
            )
        except Exception as exc:
            log.warning(
                "Could not load persisted Twitch OAuth state %s: %s",
                self._sanitize_log_value(state),
                self._sanitize_log_value(exc),
            )
            state_data = None
        if state_data is None:
            state_data = cached_state_data
        if not state_data:
            delegated = await self._delegate_shared_twitch_oauth_callback(request)
            if delegated is not None:
                return delegated
            response = web.Response(text="OAuth state ungültig oder abgelaufen.", status=400)
            self._set_no_store_headers(response)
            return response
        created_at = float(state_data.get("created_at", 0.0) or 0.0)
        if created_at <= 0.0 or time.time() - created_at > self._oauth_state_ttl_seconds:
            response = web.Response(text="OAuth state ungültig oder abgelaufen.", status=400)
            self._clear_oauth_context_cookie(response, request)
            return response

        expected_context_token = str(state_data.get("context_token") or "").strip()
        request_cookies = getattr(request, "cookies", {}) or {}
        presented_context_token = (
            request_cookies.get(self._oauth_context_cookie_name()) or ""
        ).strip()
        if (
            not expected_context_token
            or not self._is_valid_oauth_context_token(expected_context_token)
            or not presented_context_token
            or not secrets.compare_digest(expected_context_token, presented_context_token)
        ):
            response = web.Response(text="OAuth state ungültig oder abgelaufen.", status=400)
            self._clear_oauth_context_cookie(response, request)
            self._set_no_store_headers(response)
            return response

        user = await self._exchange_code_for_user(code, str(state_data.get("redirect_uri") or ""))
        if not user:
            response = web.Response(
                text="OAuth-Austausch fehlgeschlagen. Bitte erneut versuchen.",
                status=401,
            )
            self._set_no_store_headers(response)
            return response

        try:
            partner = await self._is_partner_allowed_async(
                twitch_login=user.get("twitch_login") or "",
                twitch_user_id=user.get("twitch_user_id") or "",
            )
            if not partner:
                log.warning(
                    "AUDIT dashboard login denied: twitch=%s peer=%s",
                    self._sanitize_log_value(user.get("twitch_login")),
                    self._sanitize_log_value(self._peer_host(request)),
                )
                response = web.Response(
                    text=(
                        f"Kein Zugriff: Twitch-Account '{user.get('display_name') or user.get('twitch_login')}' "
                        "ist nicht als Streamer-Partner freigegeben."
                    ),
                    status=403,
                )
                self._set_no_store_headers(response)
                return response

            try:
                await asyncio.to_thread(
                    self._reactivate_partner_after_valid_auth,
                    twitch_login=partner.get("twitch_login")
                    or user.get("twitch_login")
                    or "",
                    twitch_user_id=partner.get("twitch_user_id")
                    or user.get("twitch_user_id")
                    or "",
                )
            except Exception:
                log.debug(
                    "Could not auto-reactivate partner state after dashboard login for twitch=%s",
                    self._sanitize_log_value(user.get("twitch_login")),
                    exc_info=True,
                )

            session_id = self._create_dashboard_session(
                twitch_login=partner.get("twitch_login") or user.get("twitch_login") or "",
                twitch_user_id=partner.get("twitch_user_id") or user.get("twitch_user_id") or "",
                display_name=user.get("display_name") or "",
            )
            log.info(
                "AUDIT dashboard login success: twitch=%s peer=%s",
                self._sanitize_log_value(partner.get("twitch_login")),
                self._sanitize_log_value(self._peer_host(request)),
            )
            destination = self._safe_internal_redirect(
                self._canonical_post_login_destination(
                    self._normalize_next_path(state_data.get("next_path"))
                ),
                fallback="/twitch/dashboard",
            )
            response = web.HTTPFound(destination)
            self._set_session_cookie(response, request, session_id)
            self._clear_oauth_context_cookie(response, request)
            self._set_no_store_headers(response)
            raise response
        except web.HTTPException:
            raise
        except Exception:
            log.exception(
                "Dashboard OAuth callback failed after Twitch user exchange for state=%s login=%s peer=%s",
                self._sanitize_log_value(state),
                self._sanitize_log_value(user.get("twitch_login")),
                self._sanitize_log_value(self._peer_host(request)),
            )
            response = web.Response(
                text="Dashboard-Login konnte gerade nicht abgeschlossen werden. Bitte erneut versuchen.",
                status=503,
            )
            self._set_no_store_headers(response)
            return response

    # ------------------------------------------------------------------ #
    # Discord admin OAuth routes                                           #
    # ------------------------------------------------------------------ #

    async def discord_auth_login(self, request: web.Request) -> web.StreamResponse:
        if self._is_public_host_discord_admin_route(request):
            return web.Response(text="Not Found", status=404)
        if not self._check_rate_limit(request, max_requests=10, window_seconds=60.0):
            raise web.HTTPTooManyRequests(
                text="Too many login attempts. Please wait a minute and try again.",
                headers={"Retry-After": "60"},
            )
        if not self._discord_admin_required:
            return web.Response(
                text=(
                    "Discord Admin OAuth ist nicht konfiguriert. "
                    "Bitte internen API-Token setzen."
                ),
                status=503,
            )
        existing = self._get_discord_admin_session(request)
        next_path = self._normalize_discord_admin_next_path(request.query.get("next"))
        if existing:
            destination = self._safe_internal_redirect(
                self._canonical_discord_admin_post_login_path(next_path),
                fallback="/twitch/admin",
            )
            raise web.HTTPFound(destination)

        complete_url = self._build_discord_admin_route_url("/twitch/auth/discord/complete")
        authorize_url, _state_id = await self._fetch_delegated_discord_authorize_url(
            redirect_after=complete_url,
            scope="identify",
            requesting_service="twitch-admin",
            metadata={"next_path": next_path},
        )
        safe_auth_url = self._safe_discord_admin_login_redirect(authorize_url)
        response = web.HTTPFound(safe_auth_url)
        self._set_no_store_headers(response)
        raise response

    async def discord_auth_complete(self, request: web.Request) -> web.StreamResponse:
        if self._is_public_host_discord_admin_route(request):
            return web.Response(text="Not Found", status=404)
        if not self._check_rate_limit(request, max_requests=20, window_seconds=60.0):
            raise web.HTTPTooManyRequests(
                text="Too many OAuth callback requests. Please wait a minute and try again.",
                headers={"Retry-After": "60"},
            )
        if not self._discord_admin_required:
            return web.Response(
                text=(
                    "Discord Admin OAuth ist nicht konfiguriert. "
                    "Bitte internen API-Token setzen."
                ),
                status=503,
            )

        state_id = (request.query.get("state_id") or "").strip()
        if not state_id:
            response = web.Response(text="Fehlender state_id.", status=400)
            self._set_no_store_headers(response)
            return response
        error = str(request.query.get("error") or "").strip()
        if error:
            response = web.Response(
                text=f"Discord OAuth Fehler: {error}",
                status=401,
            )
            self._set_no_store_headers(response)
            return response

        session_payload = await self._fetch_delegated_discord_session(
            state_id=state_id,
        )
        discord_id = str((session_payload or {}).get("discord_id") or "").strip()
        if not discord_id.isdigit():
            response = web.Response(text="Discord User konnte nicht geladen werden.", status=401)
            self._set_no_store_headers(response)
            return response

        user_id = int(discord_id)
        returned_role_ids = {
            str(role).strip()
            for role in ((session_payload or {}).get("discord_roles") or [])
            if str(role).strip()
        }

        allowed = False
        reason = "missing_admin_or_moderator_role"
        owner_override_user_id = getattr(self, "_discord_admin_owner_user_id", None)
        if isinstance(owner_override_user_id, int) and owner_override_user_id > 0 and user_id == owner_override_user_id:
            allowed = True
            reason = "owner_override"
        elif str(getattr(self, "_discord_admin_moderator_role_id", "")).strip() in returned_role_ids:
            allowed = True
            reason = "moderator_role:delegated"
        else:
            allowed, reason = await self._check_discord_admin_membership(user_id)

        if not allowed:
            log.warning(
                "AUDIT twitch-dashboard discord login denied: user=%s reason=%s peer=%s",
                self._sanitize_log_value(str(user_id)),
                self._sanitize_log_value(reason),
                self._sanitize_log_value(self._peer_host(request)),
            )
            response = web.Response(
                text=(
                    "Kein Zugriff. Es wird Administrator-Recht oder die Moderator-Rolle benötigt."
                ),
                status=403,
            )
            self._set_no_store_headers(response)
            return response

        username = str((session_payload or {}).get("discord_name") or "").strip()
        display_name = username or f"User {user_id}"

        now = time.time()
        session_id = secrets.token_urlsafe(32)
        peer_host = self._peer_host(request)
        client_ip = self._effective_client_host(request, peer_host) or peer_host or ""
        next_path = str(
            ((session_payload or {}).get("service_metadata") or {}).get("next_path") or ""
        ).strip()
        destination = self._safe_internal_redirect(
            self._canonical_discord_admin_post_login_path(next_path),
            fallback="/twitch/admin",
        )
        discord_session_data = {
            "auth_type": "discord_admin",
            "user_id": user_id,
            "username": username,
            "display_name": display_name,
            "reason": reason,
            "created_at": now,
            "last_seen_at": now,
            "expires_at": now + self._discord_admin_session_ttl,
            "client_ip": client_ip,
            "passive_fp": _build_passive_fp(request),
            "fp_pending": True,
            "post_fp_destination": destination,
        }
        self._dashboard_auth_state_cache("_discord_admin_sessions").put(
            session_id,
            discord_session_data,
        )
        try:
            self._dashboard_auth_state_repo().save_discord_admin_session(
                session_id=session_id,
                payload=discord_session_data,
                created_at=now,
                expires_at=now + self._discord_admin_session_ttl,
            )
        except Exception as _exc:
            log.debug("Could not persist discord admin session to DB: %s", _exc)

        asyncio.ensure_future(
            self._register_session_in_discord_dashboard(
                session_id=session_id,
                user_id=user_id,
                username=username,
                display_name=display_name,
                expires_at=now + self._discord_admin_session_ttl,
            )
        )

        log.info(
            "AUDIT twitch-dashboard discord login success: user=%s reason=%s peer=%s",
            self._sanitize_log_value(str(user_id)),
            self._sanitize_log_value(reason),
            self._sanitize_log_value(self._peer_host(request)),
        )
        response = web.HTTPFound("/twitch/auth/fingerprint")
        self._set_discord_admin_cookie(response, request, session_id)
        self._set_no_store_headers(response)
        raise response

    async def _register_session_in_discord_dashboard(
        self,
        *,
        session_id: str,
        user_id: int,
        username: str,
        display_name: str,
        expires_at: float,
    ) -> None:
        """Best-effort: registriert die Session auch im Discord Dashboard (Gegenrichtung)."""
        base_url = self._discord_admin_base_url
        token = self._discord_oauth_internal_api_token()
        if not base_url or not token:
            return
        url = f"{base_url}/internal/twitch/v1/discord/import-session"
        try:
            async with aiohttp.ClientSession() as client:
                await client.post(
                    url,
                    json={
                        "session_id": session_id,
                        "user_id": str(user_id),
                        "username": username,
                        "display_name": display_name,
                        "expires_at": expires_at,
                    },
                    headers={"X-Internal-Token": token},
                    timeout=aiohttp.ClientTimeout(total=3.0),
                )
        except Exception:
            pass

    async def discord_link_auth_login(self, request: web.Request) -> web.StreamResponse:
        if not self._check_rate_limit(request, max_requests=10, window_seconds=60.0):
            raise web.HTTPTooManyRequests(
                text="Too many Discord link attempts. Please wait a minute and try again.",
                headers={"Retry-After": "60"},
            )

        next_path = self._normalize_discord_link_next_path(request.query.get("next"))
        if not self._check_v2_auth(request):
            response = self._dashboard_auth_redirect_or_unavailable(
                request,
                next_path=next_path,
                fallback_login_url=f"/twitch/auth/login?{urlencode({'next': next_path})}",
            )
            if isinstance(response, web.HTTPException):
                raise response
            return response

        session = self._get_dashboard_auth_session(request) or {}
        twitch_login = str(session.get("twitch_login") or "").strip().lower()
        twitch_user_id = str(session.get("twitch_user_id") or "").strip()
        if not twitch_login:
            response = web.Response(
                text="Die Dashboard-Session ist keinem Twitch-Streamer zugeordnet.",
                status=401,
            )
            self._set_no_store_headers(response)
            return response

        complete_url = self._build_public_dashboard_route_url(
            request,
            "/twitch/auth/discord/link/complete",
        )
        authorize_url, _state_id = await self._fetch_delegated_discord_authorize_url(
            redirect_after=complete_url,
            scope="identify",
            requesting_service="twitch-dashboard-link",
            metadata={
                "next_path": next_path,
                "twitch_login": twitch_login,
                "twitch_user_id": twitch_user_id,
            },
        )
        if not authorize_url:
            response = web.Response(
                text="Discord-Link ist aktuell nicht verfügbar.",
                status=503,
            )
            self._set_no_store_headers(response)
            return response

        safe_auth_url = self._safe_discord_admin_login_redirect(authorize_url)
        response = web.HTTPFound(safe_auth_url)
        self._set_no_store_headers(response)
        raise response

    async def discord_link_auth_complete(self, request: web.Request) -> web.StreamResponse:
        if not self._check_rate_limit(request, max_requests=20, window_seconds=60.0):
            raise web.HTTPTooManyRequests(
                text="Too many OAuth callback requests. Please wait a minute and try again.",
                headers={"Retry-After": "60"},
            )

        session = self._get_dashboard_auth_session(request) or {}
        twitch_login = str(session.get("twitch_login") or "").strip().lower()
        twitch_user_id = str(session.get("twitch_user_id") or "").strip()
        fallback_location = self._safe_internal_redirect(
            TWITCH_DISCORD_LINK_FALLBACK_PATH,
            fallback=TWITCH_DISCORD_LINK_FALLBACK_PATH,
        )
        if not twitch_login:
            response = web.HTTPFound(
                self._append_redirect_status(
                    fallback_location,
                    err="Twitch-Session fehlt. Bitte erneut anmelden.",
                )
            )
            self._set_no_store_headers(response)
            raise response

        state_id = (request.query.get("state_id") or "").strip()
        if not state_id:
            response = web.HTTPFound(
                self._append_redirect_status(
                    fallback_location,
                    err="Fehlender Discord-OAuth-State.",
                )
            )
            self._set_no_store_headers(response)
            raise response

        error = str(request.query.get("error") or "").strip()
        if error:
            response = web.HTTPFound(
                self._append_redirect_status(
                    fallback_location,
                    err=f"Discord OAuth Fehler: {error}",
                )
            )
            self._set_no_store_headers(response)
            raise response

        session_payload = await self._fetch_delegated_discord_session(state_id=state_id)
        service_metadata = dict((session_payload or {}).get("service_metadata") or {})
        next_path = self._normalize_discord_link_next_path(service_metadata.get("next_path"))
        safe_next_path = self._safe_internal_redirect(
            next_path,
            fallback=TWITCH_DISCORD_LINK_FALLBACK_PATH,
        )
        expected_login = str(service_metadata.get("twitch_login") or "").strip().lower()
        expected_user_id = str(service_metadata.get("twitch_user_id") or "").strip()
        if (expected_login and expected_login != twitch_login) or (
            expected_user_id and twitch_user_id and expected_user_id != twitch_user_id
        ):
            response = web.HTTPFound(
                self._append_redirect_status(
                    safe_next_path,
                    err="Discord-Link passt nicht zur aktiven Twitch-Session.",
                )
            )
            self._set_no_store_headers(response)
            raise response

        discord_id = str((session_payload or {}).get("discord_id") or "").strip()
        if not discord_id.isdigit():
            response = web.HTTPFound(
                self._append_redirect_status(
                    safe_next_path,
                    err="Discord-User konnte nicht geladen werden.",
                )
            )
            self._set_no_store_headers(response)
            raise response

        discord_name = str((session_payload or {}).get("discord_name") or "").strip()
        discord_roles = {
            str(role).strip()
            for role in ((session_payload or {}).get("discord_roles") or [])
            if str(role).strip()
        }
        profile_saver = getattr(self, "_discord_profile", None)
        if not callable(profile_saver):
            response = web.HTTPFound(
                self._append_redirect_status(
                    safe_next_path,
                    err="Discord-Link ist aktuell nicht verfügbar.",
                )
            )
            self._set_no_store_headers(response)
            raise response

        try:
            message = await profile_saver(
                twitch_login,
                discord_user_id=discord_id,
                discord_display_name=discord_name or None,
                mark_member=bool(discord_roles),
            )
        except ValueError as exc:
            message = ""
            error_message = str(exc)
        except Exception:
            log.exception("dashboard discord link completion failed")
            message = ""
            error_message = "Discord-Daten konnten nicht gespeichert werden."
        else:
            error_message = ""

        response = web.HTTPFound(
            self._append_redirect_status(
                safe_next_path,
                ok=message or None,
                err=error_message or None,
            )
        )
        self._set_no_store_headers(response)
        raise response

    async def discord_auth_logout(self, request: web.Request) -> web.StreamResponse:
        if self._is_public_host_discord_admin_route(request):
            return web.Response(text="Not Found", status=404)
        session_id = (request.cookies.get(self._discord_admin_cookie_name) or "").strip()
        if session_id:
            self._dashboard_auth_state_cache("_discord_admin_sessions").pop(session_id, None)
            try:
                self._dashboard_auth_state_repo().delete_session(session_id)
            except Exception as _exc:
                log.debug("Could not delete discord admin session from DB: %s", _exc)
        response = web.HTTPFound(self._discord_admin_logout_url())
        self._clear_discord_admin_cookie(response, request)
        self._set_no_store_headers(response)
        raise response


def build_partner_status_gate_middleware(server: "_DashboardAuthMixin"):
    """
    Aiohttp-Middleware: lehnt Active-Only-Routen für Partner mit Status 'passive' ab.
    Admin-Sessions und nicht-eingeloggte Requests werden durchgelassen — der individuelle
    Route-Handler entscheidet weiterhin selbst über Auth.
    """

    @web.middleware
    async def _partner_status_gate(request: web.Request, handler: Any) -> web.StreamResponse:
        path = request.path or ""
        if request.method == "OPTIONS" or _path_is_public(path):
            return await handler(request)

        try:
            session = server._get_dashboard_auth_session(request) or {}
        except Exception:
            log.debug("partner_status_gate: could not load session", exc_info=True)
            return await handler(request)

        if not session:
            return await handler(request)

        if session.get("is_admin") or session.get("auth_type") == "discord_admin":
            return await handler(request)

        twitch_login = str(session.get("twitch_login") or "").strip()
        twitch_user_id = str(session.get("twitch_user_id") or "").strip()
        if not twitch_login and not twitch_user_id:
            return await handler(request)

        if _path_matches_passive_allowed(path):
            return await handler(request)

        try:
            status = await asyncio.to_thread(
                server._resolve_partner_active_status,
                twitch_login=twitch_login,
                twitch_user_id=twitch_user_id,
            )
        except Exception:
            log.debug("partner_status_gate: status lookup failed", exc_info=True)
            return await handler(request)

        if status == "active":
            return await handler(request)

        log.info(
            "partner_status_gate: denied passive partner twitch=%s path=%s",
            server._sanitize_log_value(twitch_login),
            server._sanitize_log_value(path),
        )
        wants_json = (
            "application/json" in (request.headers.get("Accept") or "")
            or path.startswith("/twitch/api/")
        )
        if wants_json:
            return web.json_response(
                {
                    "error": "partner_inactive",
                    "message": (
                        "Dein Streamer-Account ist aktuell nicht als aktiver Partner geführt. "
                        "Bitte authentifiziere dich neu, um diesen Bereich zu nutzen."
                    ),
                    "reauth_url": "/twitch/raid/auth",
                },
                status=403,
            )
        return web.HTTPFound("/twitch/verwaltung?inactive=1")

    return _partner_status_gate

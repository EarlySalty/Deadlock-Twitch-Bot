"""Embedded aiohttp app serving only the Twitch analytics dashboard v2."""

from __future__ import annotations

import asyncio
import hashlib
import ipaddress
import os
import secrets
import time
from collections.abc import Awaitable, Callable
from typing import Any
from urllib.parse import parse_qsl, urlencode, urlparse, urlsplit, urlunsplit

from aiohttp import web

from ..analytics.api_v2 import AnalyticsV2Mixin
from ..core.constants import log
from ..core.twitch_login import normalize_twitch_login
from ..runtime.contracts import DashboardBotService
from ..runtime.dashboard_runtime import DashboardRuntimeServices
from ..secret_store import keyring_enabled
from ..storage import pg as storage_pg
from .admin.legal_mixin import _DashboardLegalMixin
from .affiliate.affiliate_mixin import _DashboardAffiliateMixin
from .auth.auth_mixin import _DashboardAuthMixin, build_partner_status_gate_middleware
from .auth.partner_auth_mixin import _DashboardPartnerAuthMixin
from .billing.billing_mixin import _DashboardBillingMixin
from .core.stats import DashboardStatsMixin
from .core.templates import DashboardTemplateMixin
from .admin.announcement_mode_mixin import DashboardAdminAnnouncementMixin
from .live.live import DashboardLiveMixin
from .live.live_announcement_mixin import DashboardLiveAnnouncementMixin
from .routes_mixin import _DashboardRoutesMixin
from .raids.raid_mixin import _DashboardRaidMixin

TWITCH_OAUTH_AUTHORIZE_URL = "https://id.twitch.tv/oauth2/authorize"
TWITCH_OAUTH_TOKEN_URL = "https://id.twitch.tv/oauth2/token"  # noqa: S105
TWITCH_HELIX_USERS_URL = "https://api.twitch.tv/helix/users"
DISCORD_API_BASE_URL = "https://discord.com/api/v10"
TWITCH_DASHBOARDS_LOGIN_URL = "/twitch/auth/login?next=%2Ftwitch%2Fdashboards"
TWITCH_DASHBOARD_V2_LOGIN_URL = "/twitch/auth/login?next=%2Fanalyse"
DEFAULT_DASHBOARD_MODERATOR_ROLE_ID = 1337518124647579661
KEYRING_SERVICE_NAME = "DeadlockBot"
# Public Stripe Connect OAuth client ID (`ca_...`). Safe to commit directly.
STATIC_STRIPE_CONNECT_CLIENT_ID = ""
TWITCH_ADMIN_PUBLIC_URL = (
    os.getenv("TWITCH_ADMIN_PUBLIC_URL")
    or os.getenv("MASTER_DASHBOARD_PUBLIC_URL")
    or "https://admin.deutsche-deadlock-community.de"
).strip()


def _normalize_frame_ancestor_origin(raw_origin: str) -> str | None:
    candidate = (raw_origin or "").strip()
    if not candidate:
        return None
    parsed = urlsplit(candidate)
    if parsed.scheme not in {"https", "http"}:
        return None
    if not parsed.netloc or parsed.path not in {"", "/"}:
        return None
    if parsed.query or parsed.fragment or "@" in parsed.netloc:
        return None
    return f"{parsed.scheme}://{parsed.netloc}".rstrip("/")


_DEMO_EMBED_ALLOWED_ANCESTORS: tuple[str, ...] = tuple(
    origin
    for origin in (
        _normalize_frame_ancestor_origin(item)
        for item in os.getenv(
            "TWITCH_DEMO_EMBED_ORIGINS",
            "https://deutsche-deadlock-community.de",
        ).split(",")
    )
    if origin
)

_DEFAULT_TRUSTED_PROXY_CIDRS: tuple[str, ...] = (
    "127.0.0.1/32",
    "::1/128",
)


class DashboardV2Server(
    _DashboardPartnerAuthMixin,
    _DashboardAuthMixin,
    _DashboardAffiliateMixin,
    _DashboardRaidMixin,
    _DashboardLegalMixin,
    _DashboardBillingMixin,
    _DashboardRoutesMixin,
    DashboardAdminAnnouncementMixin,
    DashboardLiveAnnouncementMixin,
    DashboardLiveMixin,
    DashboardStatsMixin,
    DashboardTemplateMixin,
    AnalyticsV2Mixin,
):
    """Minimal dashboard server exposing only v2 routes and APIs."""

    def __init__(
        self,
        *,
        app_token: str | None,
        noauth: bool,
        partner_token: str | None,
        oauth_client_id: str | None = None,
        oauth_client_secret: str | None = None,
        oauth_redirect_uri: str | None = None,
        session_ttl_seconds: int = 6 * 3600,
        legacy_stats_url: str | None = None,
        dashboard_services: DashboardRuntimeServices | None = None,
        add_cb: Callable[[str, bool], Awaitable[str]] | None = None,
        remove_cb: Callable[[str], Awaitable[str]] | None = None,
        list_cb: Callable[[], Awaitable[list[dict]]] | None = None,
        stats_cb: Callable[..., Awaitable[dict]] | None = None,
        verify_cb: Callable[[str, str], Awaitable[str]] | None = None,
        archive_cb: Callable[[str, str], Awaitable[str]] | None = None,
        discord_flag_cb: Callable[[str, bool], Awaitable[str]] | None = None,
        discord_profile_cb: Callable[[str, str | None, str | None, bool], Awaitable[str]]
        | None = None,
        raid_history_cb: Callable[..., Awaitable[list[dict]]] | None = None,
        raid_auth_url_cb: Callable[[str], Awaitable[str]] | None = None,
        raid_go_url_cb: Callable[[str], Awaitable[str | None]] | None = None,
        raid_requirements_cb: Callable[[str], Awaitable[str]] | None = None,
        raid_oauth_callback_cb: Callable[..., Awaitable[dict]] | None = None,
        reload_cb: Callable[[], Awaitable[str]] | None = None,
        social_media_clip_manager: Any | None = None,
        social_media_twitch_api: Any | None = None,
    ) -> None:
        services = dashboard_services or DashboardRuntimeServices()
        bot_service = services.resolve_bot_service() or DashboardBotService()
        self._dashboard_services = services
        self._dashboard_bot_service_view = bot_service
        self._token = app_token
        self._noauth = noauth
        if noauth:
            log.warning(
                "Dashboard läuft im NOAUTH-Modus – alle Authentifizierungsprüfungen sind deaktiviert!"
            )
        self._partner_token = partner_token
        self._oauth_client_id = oauth_client_id
        self._oauth_client_secret = oauth_client_secret
        self._oauth_redirect_uri = oauth_redirect_uri
        self._billing_stripe_publishable_key = self._load_secret_value(
            "STRIPE_PUBLISHABLE_KEY",
            "TWITCH_BILLING_STRIPE_PUBLISHABLE_KEY",
        )
        self._billing_stripe_secret_key = self._load_secret_value(
            "STRIPE_SECRET_KEY",
            "TWITCH_BILLING_STRIPE_SECRET_KEY",
        )
        self._billing_stripe_webhook_secret = self._load_secret_value(
            "STRIPE_WEBHOOK_SECRET",
            "TWITCH_BILLING_STRIPE_WEBHOOK_SECRET",
        )
        self._billing_checkout_success_url = self._load_secret_value(
            "STRIPE_CHECKOUT_SUCCESS_URL",
            "TWITCH_BILLING_CHECKOUT_SUCCESS_URL",
        )
        self._billing_checkout_cancel_url = self._load_secret_value(
            "STRIPE_CHECKOUT_CANCEL_URL",
            "TWITCH_BILLING_CHECKOUT_CANCEL_URL",
        )
        self._billing_stripe_price_map_raw = self._load_secret_value(
            "STRIPE_PRICE_ID_MAP",
            "TWITCH_BILLING_STRIPE_PRICE_ID_MAP",
        )
        self._billing_stripe_product_map_raw = self._load_secret_value(
            "STRIPE_PRODUCT_ID_MAP",
            "TWITCH_BILLING_STRIPE_PRODUCT_ID_MAP",
        )
        self._stripe_connect_client_id = str(
            STATIC_STRIPE_CONNECT_CLIENT_ID
            or self._load_secret_value("STRIPE_CONNECT_CLIENT_ID")
        ).strip()
        self._affiliate_oauth_states: dict = {}
        self._affiliate_connect_states: dict = {}
        self._affiliate_sessions: dict = {}
        self._session_ttl_seconds = max(6 * 3600, int(session_ttl_seconds or 6 * 3600))
        self._legacy_stats_url = (legacy_stats_url or "").strip() or None
        self._session_cookie_name = "twitch_dash_session"
        self._oauth_states: dict[str, dict[str, Any]] = {}
        self._auth_sessions: dict[str, dict[str, Any]] = {}
        self._sessions_db_loaded: bool = False
        self._oauth_state_ttl_seconds = 600
        self._reload_cb = (
            reload_cb
            if callable(reload_cb)
            else services.reload_cb
            if callable(services.reload_cb)
            else bot_service.reload_cb()
        )
        self._add = (
            add_cb
            if callable(add_cb)
            else services.add_cb
            if callable(services.add_cb)
            else self._empty_add
        )
        self._remove = (
            remove_cb
            if callable(remove_cb)
            else services.remove_cb
            if callable(services.remove_cb)
            else self._empty_remove
        )
        self._list = (
            list_cb
            if callable(list_cb)
            else services.list_cb
            if callable(services.list_cb)
            else self._empty_list
        )
        self._stats = (
            stats_cb
            if callable(stats_cb)
            else services.stats_cb
            if callable(services.stats_cb)
            else self._empty_stats
        )
        self._verify = (
            verify_cb
            if callable(verify_cb)
            else services.verify_cb
            if callable(services.verify_cb)
            else self._empty_verify
        )
        self._archive = (
            archive_cb
            if callable(archive_cb)
            else services.archive_cb
            if callable(services.archive_cb)
            else self._empty_archive
        )
        self._discord_flag = (
            discord_flag_cb
            if callable(discord_flag_cb)
            else services.discord_flag_cb
            if callable(services.discord_flag_cb)
            else self._empty_discord_flag
        )
        self._discord_profile = (
            discord_profile_cb
            if callable(discord_profile_cb)
            else services.discord_profile_cb
            if callable(services.discord_profile_cb)
            else None
        )
        self._raid_history_cb = (
            raid_history_cb
            if callable(raid_history_cb)
            else services.raid_history_cb
            if callable(services.raid_history_cb)
            else self._empty_raid_history
        )
        self._raid_auth_url_cb = (
            raid_auth_url_cb
            if callable(raid_auth_url_cb)
            else services.raid_auth_url_cb
            if callable(services.raid_auth_url_cb)
            else None
        )
        self._raid_go_url_cb = (
            raid_go_url_cb
            if callable(raid_go_url_cb)
            else services.raid_go_url_cb
            if callable(services.raid_go_url_cb)
            else None
        )
        self._raid_requirements_cb = (
            raid_requirements_cb
            if callable(raid_requirements_cb)
            else services.raid_requirements_cb
            if callable(services.raid_requirements_cb)
            else None
        )
        self._raid_oauth_callback_cb = (
            raid_oauth_callback_cb
            if callable(raid_oauth_callback_cb)
            else services.raid_oauth_callback_cb
            if callable(services.raid_oauth_callback_cb)
            else None
        )
        self._social_media_clip_manager = (
            social_media_clip_manager
            or services.social_media_clip_manager
            or bot_service.clip_manager()
        )
        self._social_media_twitch_api = (
            social_media_twitch_api
            or services.social_media_twitch_api
            or bot_service.twitch_api()
        )
        self._redirect_uri = str(getattr(self._dashboard_auth_manager(), "redirect_uri", "") or "").strip()
        self._master_dashboard_href = "/twitch/admin"
        self._discord_admin_base_url = TWITCH_ADMIN_PUBLIC_URL.rstrip("/")
        self._discord_admin_client_id = ""
        self._discord_admin_client_secret = ""
        self._discord_admin_enabled = True
        owner_user_id_raw = self._load_secret_value(
            "TWITCH_ADMIN_OWNER_USER_ID",
            "DISCORD_ADMIN_OWNER_USER_ID",
        )
        self._discord_admin_owner_user_id = self._parse_optional_int(owner_user_id_raw)
        if self._discord_admin_owner_user_id:
            log.warning(
                "Discord admin owner override enabled for user_id=%s. "
                "Use only for explicit recovery scenarios.",
                self._discord_admin_owner_user_id,
            )
        self._discord_admin_moderator_role_id = DEFAULT_DASHBOARD_MODERATOR_ROLE_ID
        self._discord_admin_guild_ids = self._parse_int_csv_env(
            "TWITCH_ADMIN_DISCORD_GUILD_IDS",
            "DISCORD_ADMIN_GUILD_IDS",
        )
        self._trusted_proxy_networks = self._parse_proxy_networks(
            "TWITCH_TRUSTED_PROXY_CIDRS",
            "TRUSTED_PROXY_CIDRS",
        )
        # Discord Admin nutzt einen eigenen Cookie, der mit dem Discord Dashboard geteilt wird
        self._discord_admin_cookie_name = "master_dash_session"
        # Admin Discord sessions sind 24h gültig (statt der generischen 6h)
        self._discord_admin_session_ttl = 24 * 3600
        self._discord_admin_state_ttl = 600
        self._discord_admin_oauth_states: dict[str, dict[str, Any]] = {}
        self._discord_admin_sessions: dict[str, dict[str, Any]] = {}
        self._discord_sessions_db_loaded: bool = False
        self._discord_admin_required = self._discord_admin_enabled and bool(
            self._discord_admin_base_url and self._discord_oauth_internal_api_token()
        )
        if self._discord_admin_enabled and not self._discord_admin_required:
            log.error(
                "Twitch Admin Discord OAuth ist unvollständig (Base URL/Internal API Token fehlen). "
                "Admin-Zugriff bleibt deaktiviert, bis die Konfiguration vollständig ist."
            )

    def _dashboard_bot_runtime(self) -> DashboardBotService:
        return getattr(self, "_dashboard_bot_service_view", DashboardBotService())

    def _dashboard_auth_manager(self) -> Any | None:
        return self._dashboard_bot_runtime().auth_manager()

    def _dashboard_discord_bot(self) -> Any | None:
        return self._dashboard_bot_runtime().discord_bot()

    def _dashboard_chat_bot(self) -> Any | None:
        return self._dashboard_bot_runtime().chat_bot()

    def _dashboard_token_manager(self) -> Any | None:
        return self._dashboard_bot_runtime().token_manager()

    def _dashboard_twitch_api(self) -> Any | None:
        twitch_api = getattr(self, "_social_media_twitch_api", None)
        if twitch_api is not None:
            return twitch_api
        return self._dashboard_bot_runtime().twitch_api()

    def _dashboard_clip_manager(self) -> Any | None:
        clip_manager = getattr(self, "_social_media_clip_manager", None)
        if clip_manager is not None:
            return clip_manager
        return self._dashboard_bot_runtime().clip_manager()

    def _dashboard_eventsub_webhook_handler(self) -> Any | None:
        handler = getattr(self._dashboard_services, "eventsub_webhook_handler", None)
        if handler is not None:
            return handler
        return self._dashboard_bot_runtime().eventsub_webhook_handler()

    def _dashboard_schedule_background(self, coro: Awaitable[Any], name: str) -> Any | None:
        scheduler = self._dashboard_bot_runtime().schedule_background()
        if not callable(scheduler):
            return None
        return scheduler(coro, name)

    async def _empty_add(self, _: str, __: bool) -> str:
        return "Add-Funktion ist aktuell nicht verfügbar"

    async def _empty_remove(self, _: str) -> str:
        return "Remove-Funktion ist aktuell nicht verfügbar"

    async def _empty_list(self) -> list[dict]:
        return []

    async def _empty_stats(self, **_: Any) -> dict:
        return {"tracked": {}, "category": {}}

    async def _empty_verify(self, _: str, __: str) -> str:
        return "Verify-Funktion ist aktuell nicht verfügbar"

    async def _empty_archive(self, _: str, __: str) -> str:
        return "Archive-Funktion ist aktuell nicht verfügbar"

    async def _empty_discord_flag(self, _: str, __: bool) -> str:
        return "Discord-Flag-Funktion ist aktuell nicht verfügbar"

    async def _empty_raid_history(self, **_: Any) -> list[dict]:
        return []

    async def admin(self, request: web.Request) -> web.StreamResponse:
        """Serve the new admin SPA at /twitch/admin while preserving /twitch/live."""
        if request.path.rstrip("/") == "/twitch/admin":
            serve_admin_dashboard = getattr(self, "_serve_admin_dashboard", None)
            if callable(serve_admin_dashboard):
                return await serve_admin_dashboard(request)
        return await DashboardLiveMixin.index(self, request)

    async def validate_admin_session(self, request: web.Request) -> web.Response:
        """Validate shared admin auth for Caddy forward_auth and refresh the cookie."""
        if self._is_local_request(request):
            return web.Response(status=200)

        session = self._get_discord_admin_session(request)
        if not session:
            dashboard_session = self._get_dashboard_auth_session(request) or {}
            if dashboard_session.get("is_admin"):
                session = dashboard_session

        if not session and getattr(self, "_discord_admin_required", False):
            cookie_name = getattr(self, "_discord_admin_cookie_name", "master_dash_session")
            ext_session_id = (request.cookies.get(str(cookie_name)) or "").strip()
            if ext_session_id:
                session = await self._fetch_discord_dashboard_session(ext_session_id)

        if not session:
            return web.Response(status=401)

        stored_ip = str(session.get("client_ip") or "").strip()
        if stored_ip:
            peer_host = self._peer_host(request)
            current_ip = ""
            if self._is_trusted_proxy_host(peer_host):
                # Caddy forward_auth does not reliably preserve the original client IP
                # on the auth subrequest, so only enforce the binding when a forwarded
                # client address is actually present.
                current_ip = self._forwarded_client_host(request)
            else:
                current_ip = self._host_without_port(peer_host)
            if current_ip and current_ip != stored_ip:
                log.warning(
                    "AUDIT admin session IP mismatch: stored=%s current=%s",
                    self._sanitize_log_value(stored_ip),
                    self._sanitize_log_value(current_ip),
                )
                return web.Response(status=401)

        stored_passive_fp = str(session.get("passive_fp") or "").strip()
        if stored_passive_fp:
            ua = str(request.headers.get("User-Agent") or "").strip()
            lang = str(request.headers.get("Accept-Language") or "").split(",")[0].strip()
            platform = str(request.headers.get("Sec-CH-UA-Platform") or "").strip().strip('"')
            current_passive_fp = hashlib.sha256(
                f"{ua}|{lang}|{platform}".encode("utf-8")
            ).hexdigest()[:32]
            if current_passive_fp != stored_passive_fp:
                log.warning("AUDIT admin session passive FP mismatch")
                return web.Response(status=401)

        if session.get("fp_pending") is True:
            log.warning("AUDIT admin session fp_pending - fingerprint step incomplete")
            return web.Response(status=401)

        if session.get("source") != "discord_dashboard" and not str(session.get("js_fp") or "").strip():
            log.warning("AUDIT admin session missing JS fingerprint")
            return web.Response(status=401)

        response = web.Response(status=200)
        cookie_name = (
            getattr(self, "_discord_admin_cookie_name", None)
            or getattr(self, "_session_cookie_name", None)
            or "twitch_dash_session"
        )
        session_id = (request.cookies.get(str(cookie_name)) or "").strip()
        if session_id:
            self._set_discord_admin_cookie(response, request, session_id)
        username = str(session.get("username") or session.get("display_name") or "admin").strip() or "admin"
        response.headers["X-Admin-User"] = username
        return response

    async def fingerprint_page(self, request: web.Request) -> web.StreamResponse:
        from .auth.fingerprint_mixin import fingerprint_page as _fingerprint_page

        return await _fingerprint_page(self, request)

    async def fingerprint_submit(self, request: web.Request) -> web.StreamResponse:
        from .auth.fingerprint_mixin import fingerprint_submit as _fingerprint_submit

        return await _fingerprint_submit(self, request)

    async def _fetch_discord_dashboard_session(self, session_id: str) -> "dict[str, Any] | None":
        """Validates a master_dash_session against Discord Dashboard's internal API.

        Returns a synthetic session dict (source='discord_dashboard') on success, None otherwise.
        The result is cached in _discord_admin_sessions for up to 5 minutes.
        """
        import aiohttp as _aiohttp

        base_url = self._discord_admin_base_url
        token = self._discord_oauth_internal_api_token()
        if not base_url or not token:
            return None
        url = f"{base_url}/internal/twitch/v1/discord/validate-session"
        try:
            async with _aiohttp.ClientSession() as client:
                async with client.post(
                    url,
                    json={"session_id": session_id},
                    headers={"X-Internal-Token": token},
                    timeout=_aiohttp.ClientTimeout(total=3.0),
                ) as resp:
                    if resp.status != 200:
                        return None
                    data = await resp.json()
        except Exception:
            return None
        if not data.get("valid"):
            return None
        now = time.time()
        synth: dict[str, Any] = {
            "auth_type": "discord_admin",
            "user_id": str(data.get("user_id") or ""),
            "username": str(data.get("username") or ""),
            "display_name": str(data.get("display_name") or ""),
            "source": "discord_dashboard",
            "fp_pending": False,
            "js_fp": "discord_validated",
            "passive_fp": "",
            "client_ip": "",
            "created_at": now,
            "last_seen_at": now,
            "expires_at": min(float(data.get("expires_at") or now + 300), now + 300),
        }
        cache = self._dashboard_auth_state_cache("_discord_admin_sessions")
        cache.put(session_id, synth)
        db_expires_at = now + getattr(self, "_discord_admin_session_ttl", 86400)
        try:
            self._dashboard_auth_state_repo().save_discord_admin_session(
                session_id=session_id,
                payload=synth,
                created_at=now,
                expires_at=db_expires_at,
            )
        except Exception:
            pass
        return synth

    @classmethod
    def _load_secret_value(cls, *keys: str) -> str:
        for raw_key in keys:
            key = str(raw_key or "").strip()
            if not key:
                continue
            value = cls._read_keyring_secret(key)
            if value:
                return value
        for raw_key in keys:
            key = str(raw_key or "").strip()
            if not key:
                continue
            value = str(os.getenv(key) or "").strip()
            if value:
                return value
        return ""

    @staticmethod
    def _read_keyring_secret(key: str) -> str:
        secret_key = (key or "").strip()
        if not secret_key:
            return ""
        if not keyring_enabled():
            return ""
        try:
            import keyring
        except Exception:
            return ""
        try:
            value = keyring.get_password(KEYRING_SERVICE_NAME, secret_key)
            if not value:
                value = keyring.get_password(f"{secret_key}@{KEYRING_SERVICE_NAME}", secret_key)
        except Exception:
            return ""
        return str(value or "").strip()

    @staticmethod
    def _write_keyring_secret(key: str, value: str | None) -> bool:
        secret_key = (key or "").strip()
        if not secret_key:
            return False
        if not keyring_enabled():
            return False
        try:
            import keyring
        except Exception:
            return False
        try:
            keyring.set_password(KEYRING_SERVICE_NAME, secret_key, str(value or ""))
        except Exception:
            return False
        return True

    @staticmethod
    def _parse_optional_int(value: Any) -> int | None:
        raw = str(value or "").strip()
        if not raw:
            return None
        try:
            parsed = int(raw)
        except (TypeError, ValueError):
            return None
        return parsed if parsed > 0 else None

    @classmethod
    def _parse_int_csv_env(cls, *keys: str) -> tuple[int, ...]:
        values: list[int] = []
        seen: set[int] = set()
        for raw_key in keys:
            raw_value = str(os.getenv(str(raw_key or "").strip()) or "").strip()
            if not raw_value:
                continue
            for item in raw_value.split(","):
                parsed = cls._parse_optional_int(item)
                if parsed is None or parsed in seen:
                    continue
                seen.add(parsed)
                values.append(parsed)
        return tuple(values)

    @staticmethod
    def _parse_proxy_networks(*keys: str) -> tuple[ipaddress._BaseNetwork, ...]:
        raw_values = [
            str(os.getenv(str(raw_key or "").strip()) or "").strip()
            for raw_key in keys
            if str(raw_key or "").strip()
        ]
        source = next((value for value in raw_values if value), "")
        cidrs = source.split(",") if source else list(_DEFAULT_TRUSTED_PROXY_CIDRS)
        networks: list[ipaddress._BaseNetwork] = []
        seen: set[str] = set()
        for item in cidrs:
            candidate = str(item or "").strip()
            if not candidate or candidate in seen:
                continue
            try:
                network = ipaddress.ip_network(candidate, strict=False)
            except ValueError:
                log.warning(
                    "Ignoring invalid trusted proxy CIDR: %s",
                    self._sanitize_log_value(candidate),
                )
                continue
            seen.add(candidate)
            networks.append(network)
        return tuple(networks)

    def _is_trusted_proxy_host(self, raw: str | None) -> bool:
        host = self._host_without_port(raw)
        if not host:
            return False
        try:
            address = ipaddress.ip_address(host)
        except ValueError:
            return False
        return any(address in network for network in self._trusted_proxy_networks)

    def _forwarded_client_host(self, request: web.Request) -> str:
        forwarded_for = request.headers.get("X-Forwarded-For") or ""
        for candidate in reversed(forwarded_for.split(",")):
            host = self._host_without_port(candidate)
            if host:
                return host
        return self._host_without_port(request.headers.get("X-Real-IP"))

    def _check_admin_token(self, token: str | None) -> bool:
        if self._noauth:
            return True
        if not token or not self._token:
            return False
        try:
            return secrets.compare_digest(str(token), str(self._token))
        except Exception:
            return False

    @staticmethod
    def _host_without_port(raw: str | None) -> str:
        if not raw:
            return ""
        host = raw.split(",")[0].strip()
        if not host:
            return ""
        if host.startswith("["):
            end = host.find("]")
            if end != -1:
                host = host[1:end]
        elif ":" in host:
            host = host.split(":", 1)[0]
        return host.lower()

    @staticmethod
    def _is_loopback_host(raw: str | None) -> bool:
        host = DashboardV2Server._host_without_port(raw)
        if not host:
            return False
        if host == "localhost":
            return True
        try:
            return ipaddress.ip_address(host).is_loopback
        except ValueError:
            return False

    @staticmethod
    def _peer_host(request: web.Request) -> str:
        remote = (request.remote or "").strip() if hasattr(request, "remote") else ""
        if remote:
            return remote
        transport = getattr(request, "transport", None)
        if transport is None:
            return ""
        peer = transport.get_extra_info("peername")
        if isinstance(peer, tuple) and peer:
            return str(peer[0]).strip()
        if isinstance(peer, str):
            return peer.strip()
        return ""

    def _effective_client_host(self, request: web.Request, peer_host: str) -> str:
        if self._is_trusted_proxy_host(peer_host):
            forwarded_host = self._forwarded_client_host(request)
            if forwarded_host:
                return forwarded_host
        return self._host_without_port(peer_host)

    def _rate_limit_key(self, request: web.Request) -> str:
        peer = self._peer_host(request)
        resolved = self._effective_client_host(request, peer)
        return resolved or self._host_without_port(peer) or "unknown"

    def _is_local_request(self, request: web.Request) -> bool:
        host_header = request.headers.get("Host") or request.host or ""
        request_host = self._host_without_port(host_header)
        if not self._is_loopback_host(request_host):
            return False

        peer_host = self._peer_host(request)
        if not peer_host:
            return False
        client_host = self._effective_client_host(request, peer_host)
        return self._is_loopback_host(client_host)

    @staticmethod
    def _normalize_login(value: str) -> str | None:
        return normalize_twitch_login(value)

    @staticmethod
    def _sanitize_log_value(value: Any) -> str:
        text = "" if value is None else str(value)
        return text.replace("\r", "\\r").replace("\n", "\\n")

    @staticmethod
    def _normalize_discord_admin_next_path(raw: str | None) -> str:
        fallback = "/twitch/admin"
        candidate = (raw or "").strip()
        if not candidate:
            return fallback
        try:
            parts = urlsplit(candidate)
        except Exception:
            return fallback
        if parts.scheme or parts.netloc:
            return fallback
        if not candidate.startswith("/") or not candidate.startswith("/twitch"):
            return fallback
        return candidate

    def _build_discord_admin_route_url(
        self,
        path: str,
        *,
        query: dict[str, str] | None = None,
        raw_query: str | None = None,
    ) -> str:
        base_url = str(getattr(self, "_discord_admin_base_url", "") or TWITCH_ADMIN_PUBLIC_URL).rstrip(
            "/"
        )
        normalized_path = str(path or "").strip() or "/"
        if not normalized_path.startswith("/"):
            normalized_path = f"/{normalized_path}"
        if raw_query is not None:
            query_string = raw_query.lstrip("?")
        elif query:
            query_string = urlencode(query)
        else:
            query_string = ""
        if query_string:
            return f"{base_url}{normalized_path}?{query_string}"
        return f"{base_url}{normalized_path}"

    def _build_discord_admin_login_url(
        self, request: web.Request | None, *, next_path: str | None = None
    ) -> str:
        if not self._discord_admin_required:
            return "/twitch/dashboard"
        normalized_next = self._normalize_discord_admin_next_path(
            next_path
            or (
                request.rel_url.path_qs
                if request is not None and request.rel_url
                else "/twitch/admin"
            )
        )
        return self._build_discord_admin_route_url(
            "/twitch/auth/discord/login",
            query={"next": normalized_next},
        )

    def _discord_admin_logout_url(self) -> str:
        if not self._discord_admin_required:
            return "/twitch/dashboard"
        return self._build_discord_admin_route_url("/twitch/auth/discord/logout")

    def _safe_discord_admin_login_redirect(self, raw_url: str | None) -> str:
        fallback = self._build_discord_admin_route_url("/twitch/auth/discord/login")
        candidate = (raw_url or "").strip()
        if not candidate:
            return fallback
        try:
            parsed = urlsplit(candidate)
        except Exception:
            return fallback
        if not parsed.scheme and not parsed.netloc:
            if not (parsed.path or "").startswith("/twitch/auth/discord/login"):
                return fallback
            return candidate

        admin_host = (
            urlsplit(
                str(getattr(self, "_discord_admin_base_url", "") or TWITCH_ADMIN_PUBLIC_URL)
            ).netloc.split("@")[-1].split(":", 1)[0].strip().lower()
        )
        host = (parsed.netloc or "").split("@")[-1].split(":", 1)[0].strip().lower()
        path = (parsed.path or "").strip()
        if parsed.scheme == "https" and host == admin_host and path == "/twitch/auth/discord/login":
            return candidate
        if parsed.scheme == "https" and host in {"discord.com", "www.discord.com"}:
            if path in {"/oauth2/authorize", "/api/oauth2/authorize", "/api/v10/oauth2/authorize"}:
                return candidate
        return fallback

    @staticmethod
    def _canonical_discord_admin_post_login_path(raw: str | None) -> str:
        normalized = DashboardV2Server._normalize_discord_admin_next_path(raw)
        parts = urlsplit(normalized)
        normalized_path = (parts.path or "").rstrip("/") or "/"
        query_suffix = f"?{parts.query}" if parts.query else ""
        if normalized_path == "/twitch/abo":
            return "/twitch/abbo"
        if normalized_path == "/twitch/abbo":
            return "/twitch/abbo"
        if normalized_path == "/twitch/abos":
            return "/twitch/abbo"
        if normalized_path == "/twitch/abbo/stripe-settings":
            return "/twitch/abbo/stripe-settings"
        if normalized_path == "/twitch/abbo/rechnungen":
            return "/twitch/abbo/rechnungen"
        if normalized_path == "/twitch/abbo/rechnung":
            return "/twitch/abbo/rechnung"
        if normalized_path == "/twitch/abbo/kündigen":
            return "/twitch/abbo/kündigen"
        if normalized_path == "/twitch/dashboads":
            return "/twitch/dashboard"
        if normalized_path == "/twitch/dashboards":
            return "/twitch/dashboard"
        if normalized_path == "/twitch/dashboard":
            return "/twitch/dashboard"
        if normalized_path == "/twitch/admin/announcements":
            return f"/twitch/admin/announcements{query_suffix}"
        if normalized_path == "/twitch/admin/legacy":
            return f"/twitch/admin/legacy{query_suffix}"
        return "/twitch/admin"

    @staticmethod
    def _path_matches_prefixes(path: str, prefixes: tuple[str, ...]) -> bool:
        normalized_path = str(path or "").strip()
        if not normalized_path:
            return False
        for prefix in prefixes:
            normalized_prefix = str(prefix or "").rstrip("/")
            if not normalized_prefix:
                continue
            if normalized_path == normalized_prefix or normalized_path.startswith(
                f"{normalized_prefix}/"
            ):
                return True
        return False

    def _normalized_discord_admin_redirect_uri(self) -> str | None:
        return None

    def _require_token(self, request: web.Request) -> None:
        admin_only_prefixes = (
            "/twitch/admin",
            "/twitch/live",
            "/twitch/add_any",
            "/twitch/add_url",
            "/twitch/add_login",
            "/twitch/add_streamer",
            "/twitch/remove",
            "/twitch/verify",
            "/twitch/archive",
            "/twitch/discord_flag",
            "/twitch/discord_link",
            "/twitch/raid/auth",
            "/twitch/raid/requirements",
            "/twitch/raid/history",
            "/twitch/raid/analytics",
            "/twitch/reload",
            "/twitch/market",
            "/twitch/stats",
        )
        if self._path_matches_prefixes(request.path, admin_only_prefixes):
            if not self._discord_admin_required:
                raise web.HTTPServiceUnavailable(
                    text=(
                        "Discord Admin OAuth ist nicht konfiguriert. "
                        "Admin-Zugriff ist bis zur vollständigen Konfiguration deaktiviert."
                    )
                )
            if self._is_discord_admin_request(request):
                return
            login_url = self._build_discord_admin_login_url(
                request,
                next_path=request.rel_url.path_qs if request.rel_url else request.path,
            )
            safe_login_url = self._safe_discord_admin_login_redirect(login_url)
            if request.method in {"GET", "HEAD"}:
                raise web.HTTPFound(safe_login_url)
            raise web.HTTPUnauthorized(
                text="Discord admin authentication required",
                headers={"X-Auth-Login": safe_login_url},
            )

        if self._check_v2_auth(request):
            return
        raise web.HTTPUnauthorized(text="missing or invalid authentication")

    def _require_partner_token(self, request: web.Request) -> None:
        if self._check_v2_auth(request):
            return
        if self._noauth:
            return
        raise web.HTTPUnauthorized(text="missing or invalid partner authentication")

    def _redirect_location(
        self,
        request: web.Request,
        *,
        ok: str | None = None,
        err: str | None = None,
        default_path: str = "/twitch/stats",
    ) -> str:
        if default_path == "/twitch/stats":
            admin_action_prefixes = (
                "/twitch/admin",
                "/twitch/live",
                "/twitch/add_any",
                "/twitch/add_url",
                "/twitch/add_login",
                "/twitch/add_streamer",
                "/twitch/admin/chat_action",
                "/twitch/remove",
                "/twitch/verify",
                "/twitch/archive",
                "/twitch/discord_flag",
                "/twitch/raid/auth",
                "/twitch/raid/requirements",
                "/twitch/raid/history",
                "/twitch/raid/analytics",
            )
            if self._path_matches_prefixes(request.path, admin_action_prefixes):
                default_path = "/twitch/admin"

        referer = request.headers.get("Referer")
        if referer:
            try:
                parts = urlsplit(referer)
                if parts.path and parts.path.startswith("/") and not parts.path.startswith("//"):
                    params = dict(parse_qsl(parts.query, keep_blank_values=True))
                    params.pop("ok", None)
                    params.pop("err", None)
                    if ok:
                        params["ok"] = ok
                    if err:
                        params["err"] = err
                    candidate = urlunsplit(("", "", parts.path, urlencode(params), ""))
                    safe_redirect = getattr(self, "_safe_internal_redirect", None)
                    if callable(safe_redirect):
                        return safe_redirect(candidate, fallback=default_path)
                    return candidate or default_path
            except Exception:
                log.debug("Could not construct redirect from referer", exc_info=True)

        params: dict[str, str] = {}
        if ok:
            params["ok"] = ok
        if err:
            params["err"] = err
        candidate = f"{default_path}?{urlencode(params)}" if params else default_path
        safe_redirect = getattr(self, "_safe_internal_redirect", None)
        if callable(safe_redirect):
            return safe_redirect(candidate, fallback=default_path)
        return candidate

    def _is_secure_request(self, request: web.Request) -> bool:
        peer = self._peer_host(request)
        if self._is_trusted_proxy_host(peer):
            forwarded_proto = (
                (request.headers.get("X-Forwarded-Proto") or "").split(",")[0].strip().lower()
            )
            if forwarded_proto:
                return forwarded_proto == "https"
        return bool(request.secure)

    def _resolve_legacy_stats_url(self) -> str:
        # The public "stats" entry must stay on the user surface and must not jump into admin.
        return "/twitch/dashboard"

    def _should_use_discord_admin_login(self, request: web.Request) -> bool:
        if not self._discord_admin_required:
            return False
        admin_context_prefixes = (
            "/twitch/admin",
            "/twitch/live",
            "/twitch/add_any",
            "/twitch/add_url",
            "/twitch/add_login",
            "/twitch/add_streamer",
            "/twitch/admin/chat_action",
            "/twitch/remove",
            "/twitch/verify",
            "/twitch/archive",
            "/twitch/discord_flag",
            "/twitch/discord_link",
            "/twitch/raid/auth",
            "/twitch/raid/requirements",
            "/twitch/raid/history",
            "/twitch/raid/analytics",
            "/twitch/reload",
            "/twitch/market",
        )
        return self._path_matches_prefixes(request.path, admin_context_prefixes)

    async def _do_add(self, raw: str) -> str:
        login = self._normalize_login(raw)
        if not login:
            raise web.HTTPBadRequest(text="invalid twitch login or url")
        msg = await self._add(login, False)
        return msg or "added"


@web.middleware
async def _security_headers_middleware(request: web.Request, handler: Any) -> web.StreamResponse:
    """Attach minimal security headers to every response."""
    response = await handler(request)
    is_demo_embed_path = request.path == "/twitch/demo" or request.path.startswith("/twitch/demo/")
    if is_demo_embed_path and _DEMO_EMBED_ALLOWED_ANCESTORS:
        frame_ancestors = " ".join(["'self'", *_DEMO_EMBED_ALLOWED_ANCESTORS])
        response.headers["Content-Security-Policy"] = f"frame-ancestors {frame_ancestors}"
        response.headers.pop("X-Frame-Options", None)
    else:
        response.headers.setdefault("X-Frame-Options", "DENY")
    response.headers.setdefault("X-Content-Type-Options", "nosniff")
    response.headers.setdefault("X-XSS-Protection", "1; mode=block")
    response.headers.setdefault("Referrer-Policy", "strict-origin-when-cross-origin")
    response.headers.setdefault("Cross-Origin-Opener-Policy", "same-origin")
    return response


def build_v2_app(
    *,
    noauth: bool,
    token: str | None,
    partner_token: str | None = None,
    oauth_client_id: str | None = None,
    oauth_client_secret: str | None = None,
    oauth_redirect_uri: str | None = None,
    session_ttl_seconds: int = 6 * 3600,
    legacy_stats_url: str | None = None,
    dashboard_services: DashboardRuntimeServices | None = None,
    add_cb: Callable[[str, bool], Awaitable[str]] | None = None,
    remove_cb: Callable[[str], Awaitable[str]] | None = None,
    list_cb: Callable[[], Awaitable[list[dict]]] | None = None,
    stats_cb: Callable[..., Awaitable[dict]] | None = None,
    verify_cb: Callable[[str, str], Awaitable[str]] | None = None,
    archive_cb: Callable[[str, str], Awaitable[str]] | None = None,
    discord_flag_cb: Callable[[str, bool], Awaitable[str]] | None = None,
    discord_profile_cb: Callable[[str, str | None, str | None, bool], Awaitable[str]] | None = None,
    raid_history_cb: Callable[..., Awaitable[list[dict]]] | None = None,
    raid_auth_url_cb: Callable[[str], Awaitable[str]] | None = None,
    raid_go_url_cb: Callable[[str], Awaitable[str | None]] | None = None,
    raid_requirements_cb: Callable[[str], Awaitable[str]] | None = None,
    raid_oauth_callback_cb: Callable[..., Awaitable[dict]] | None = None,
    reload_cb: Callable[[], Awaitable[str]] | None = None,
    eventsub_webhook_handler: Any | None = None,
    social_media_clip_manager: Any | None = None,
    social_media_twitch_api: Any | None = None,
) -> web.Application:
    server = DashboardV2Server(
        app_token=token,
        noauth=noauth,
        partner_token=partner_token,
        oauth_client_id=oauth_client_id,
        oauth_client_secret=oauth_client_secret,
        oauth_redirect_uri=oauth_redirect_uri,
        session_ttl_seconds=session_ttl_seconds,
        legacy_stats_url=legacy_stats_url,
        dashboard_services=dashboard_services,
        add_cb=add_cb,
        remove_cb=remove_cb,
        list_cb=list_cb,
        stats_cb=stats_cb,
        verify_cb=verify_cb,
        archive_cb=archive_cb,
        discord_flag_cb=discord_flag_cb,
        discord_profile_cb=discord_profile_cb,
        raid_history_cb=raid_history_cb,
        raid_auth_url_cb=raid_auth_url_cb,
        raid_go_url_cb=raid_go_url_cb,
        raid_requirements_cb=raid_requirements_cb,
        raid_oauth_callback_cb=raid_oauth_callback_cb,
        reload_cb=reload_cb,
        social_media_clip_manager=social_media_clip_manager,
        social_media_twitch_api=social_media_twitch_api,
    )

    app = web.Application(
        middlewares=[
            _security_headers_middleware,
            build_partner_status_gate_middleware(server),
        ]
    )

    async def _bootstrap_and_register_routes(_: web.Application) -> None:
        await asyncio.to_thread(storage_pg.prepare_runtime_storage)
        server.attach(app)
        resolved_eventsub_handler = eventsub_webhook_handler or server._dashboard_eventsub_webhook_handler()
        if resolved_eventsub_handler is not None:
            app.router.add_post(
                "/twitch/eventsub/callback",
                resolved_eventsub_handler.handle_request,
            )

    app.on_startup.append(_bootstrap_and_register_routes)
    return app


__all__ = ["DashboardV2Server", "build_v2_app"]

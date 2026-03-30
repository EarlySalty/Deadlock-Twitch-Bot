"""Dashboard auth services for session lifecycle, cookies, and partner bootstrap."""

from __future__ import annotations

import re
import secrets
import time
from typing import Any
from urllib.parse import urlencode

from aiohttp import web

from ...core.constants import log


class DashboardAuthCookieService:
    """Encapsulate dashboard auth cookie naming and mutation."""

    def __init__(self, owner: Any) -> None:
        self._owner = owner

    def session_cookie_name(self) -> str:
        return str(getattr(self._owner, "_session_cookie_name", "") or "").strip()

    def oauth_context_cookie_name(self) -> str:
        base_name = self.session_cookie_name() or "twitch_dash_session"
        return f"{base_name}_oauth_ctx"

    def discord_oauth_context_cookie_name(self) -> str:
        base_name = self.session_cookie_name() or "twitch_dash_session"
        return f"{base_name}_discord_oauth_ctx"

    def partner_access_cookie_name(self) -> str:
        base_name = self.session_cookie_name() or "twitch_dash_session"
        return f"{base_name}_partner"

    def set_session_cookie(
        self, response: web.StreamResponse, request: web.Request, session_id: str
    ) -> None:
        response.set_cookie(
            self.session_cookie_name(),
            session_id,
            max_age=int(getattr(self._owner, "_session_ttl_seconds", 6 * 3600) or 6 * 3600),
            httponly=True,
            secure=self._owner._is_secure_request(request),
            samesite="Lax",
            path="/",
        )

    def clear_session_cookie(self, response: web.StreamResponse, request: web.Request) -> None:
        response.del_cookie(
            self.session_cookie_name(),
            path="/",
            httponly=True,
            samesite="Lax",
            secure=self._owner._is_secure_request(request),
        )

    def set_oauth_context_cookie(
        self, response: web.StreamResponse, request: web.Request, token: str
    ) -> None:
        response.set_cookie(
            self.oauth_context_cookie_name(),
            token,
            max_age=int(getattr(self._owner, "_oauth_state_ttl_seconds", 600) or 600),
            httponly=True,
            secure=self._owner._is_secure_request(request),
            samesite="Lax",
            path="/twitch/auth/callback",
        )

    def clear_oauth_context_cookie(
        self, response: web.StreamResponse, request: web.Request
    ) -> None:
        response.del_cookie(
            self.oauth_context_cookie_name(),
            path="/twitch/auth/callback",
            httponly=True,
            samesite="Lax",
            secure=self._owner._is_secure_request(request),
        )

    def set_discord_oauth_context_cookie(
        self, response: web.StreamResponse, request: web.Request, token: str
    ) -> None:
        response.set_cookie(
            self.discord_oauth_context_cookie_name(),
            token,
            max_age=int(getattr(self._owner, "_oauth_state_ttl_seconds", 600) or 600),
            httponly=True,
            secure=self._owner._is_secure_request(request),
            samesite="Lax",
            path="/twitch/auth/discord/callback",
        )

    def clear_discord_oauth_context_cookie(
        self, response: web.StreamResponse, request: web.Request
    ) -> None:
        response.del_cookie(
            self.discord_oauth_context_cookie_name(),
            path="/twitch/auth/discord/callback",
            httponly=True,
            samesite="Lax",
            secure=self._owner._is_secure_request(request),
        )

    def set_partner_access_cookie(
        self, response: web.StreamResponse, request: web.Request, session_id: str
    ) -> None:
        response.set_cookie(
            self.partner_access_cookie_name(),
            session_id,
            max_age=self._owner._partner_access_session_ttl(),
            httponly=True,
            secure=self._owner._is_secure_request(request),
            samesite="Lax",
            path="/",
        )

    def clear_partner_access_cookie(
        self, response: web.StreamResponse, request: web.Request
    ) -> None:
        response.del_cookie(
            self.partner_access_cookie_name(),
            path="/",
            httponly=True,
            samesite="Lax",
            secure=self._owner._is_secure_request(request),
        )


class DashboardSessionService:
    """Handle durable dashboard OAuth sessions without inflating the mixin."""

    def __init__(self, owner: Any) -> None:
        self._owner = owner

    def cleanup(self) -> None:
        now = time.time()
        oauth_states = self._owner._dashboard_auth_state_cache("_oauth_states")
        auth_sessions = self._owner._dashboard_auth_state_cache("_auth_sessions")
        oauth_states.prune_by_created_at(
            ttl_seconds=self._owner._oauth_state_ttl_seconds,
            now=now,
            max_items=500,
        )
        auth_sessions.prune_by_expires_at(now=now, max_items=2000)

        try:
            self._owner._dashboard_auth_state_repo().delete_expired(now)
        except Exception as exc:
            log.debug("Could not purge expired dashboard auth state from DB: %s", exc)

    def load(self, request: web.Request) -> dict[str, Any] | None:
        self.cleanup()
        self._owner._mark_dashboard_sessions_db_loaded()
        auth_sessions = self._owner._dashboard_auth_state_cache("_auth_sessions")
        session_cookie_name = self._owner._cookie_service().session_cookie_name()
        cookies = getattr(request, "cookies", {}) or {}
        session_id = (cookies.get(session_cookie_name) or "").strip()
        if not session_id:
            return None

        session = auth_sessions.get(session_id)
        if not session:
            try:
                session = self._owner._dashboard_auth_state_repo().load_dashboard_session(
                    session_id,
                    now=time.time(),
                )
            except Exception as exc:
                log.debug("Could not load dashboard session from DB: %s", exc)
                session = None
            if not session:
                return None
            auth_sessions.put(session_id, session)

        now = time.time()
        expires_at = float(session.get("expires_at", 0.0) or 0.0)
        if expires_at <= now:
            auth_sessions.pop(session_id, None)
            try:
                self._owner._dashboard_auth_state_repo().delete_session(session_id)
            except Exception as exc:
                log.debug("Could not delete expired session from DB: %s", exc)
            return None

        old_expires = expires_at
        session["expires_at"] = now + self._owner._session_ttl_seconds
        if session["expires_at"] - old_expires > 1800:
            try:
                self._owner._dashboard_auth_state_repo().save_dashboard_session(
                    session_id=session_id,
                    payload=session,
                    created_at=float(session.get("created_at", now) or now),
                    expires_at=session["expires_at"],
                )
            except Exception as exc:
                log.debug("Could not refresh dashboard session in DB: %s", exc)
        return session

    def create(self, *, twitch_login: str, twitch_user_id: str, display_name: str) -> str:
        self.cleanup()
        session_id = secrets.token_urlsafe(32)
        now = time.time()
        session_data = {
            "twitch_login": twitch_login,
            "twitch_user_id": twitch_user_id,
            "display_name": display_name or twitch_login,
            "is_partner": True,
            "created_at": now,
            "expires_at": now + self._owner._session_ttl_seconds,
        }
        self._owner._dashboard_auth_state_cache("_auth_sessions").put(session_id, session_data)
        try:
            self._owner._dashboard_auth_state_repo().save_dashboard_session(
                session_id=session_id,
                payload=session_data,
                created_at=now,
                expires_at=now + self._owner._session_ttl_seconds,
            )
        except Exception as exc:
            log.debug("Could not persist dashboard session to DB: %s", exc)
        return session_id

    def delete(self, session_id: str) -> dict[str, Any] | None:
        normalized_session_id = str(session_id or "").strip()
        if not normalized_session_id:
            return None
        session = self._owner._dashboard_auth_state_cache("_auth_sessions").pop(
            normalized_session_id,
            None,
        )
        try:
            self._owner._dashboard_auth_state_repo().delete_session(normalized_session_id)
        except Exception as exc:
            log.debug("Could not delete dashboard session from DB: %s", exc)
        return session if isinstance(session, dict) else None


class PartnerAccessBinding:
    """Create a coarse, less fragile request fingerprint for partner bootstrap sessions."""

    _USER_AGENT_FAMILY_RE = re.compile(r"([A-Za-z][A-Za-z0-9_-]{1,31})")

    @classmethod
    def capture(cls, request: web.Request) -> dict[str, str]:
        user_agent = str(request.headers.get("User-Agent") or "").strip()[:256]
        return {
            "user_agent_family": cls._user_agent_family(user_agent),
            "user_agent_platform": cls._user_agent_platform(user_agent),
        }

    @classmethod
    def matches(cls, session: dict[str, Any], request_binding: dict[str, str]) -> bool:
        expected_family = str(session.get("user_agent_family") or "").strip()
        actual_family = str(request_binding.get("user_agent_family") or "").strip()
        expected_platform = str(session.get("user_agent_platform") or "").strip()
        actual_platform = str(request_binding.get("user_agent_platform") or "").strip()

        family_matches = (
            bool(expected_family and actual_family)
            and secrets.compare_digest(expected_family, actual_family)
        )
        platform_matches = (
            bool(expected_platform and actual_platform)
            and secrets.compare_digest(expected_platform, actual_platform)
        )

        if expected_family and actual_family and expected_platform and actual_platform:
            return family_matches or platform_matches
        if expected_family and actual_family:
            return family_matches
        if expected_platform and actual_platform:
            return platform_matches
        return True

    @classmethod
    def _user_agent_family(cls, user_agent: str) -> str:
        if not user_agent:
            return ""
        match = cls._USER_AGENT_FAMILY_RE.search(user_agent)
        return (match.group(1).lower() if match else "")[:32]

    @staticmethod
    def _user_agent_platform(user_agent: str) -> str:
        candidate = user_agent.lower()
        if not candidate:
            return ""
        if "iphone" in candidate or "ipad" in candidate or "ios" in candidate:
            return "ios"
        if "android" in candidate:
            return "android"
        if "windows" in candidate:
            return "windows"
        if "mac os" in candidate or "macintosh" in candidate:
            return "macos"
        if "linux" in candidate:
            return "linux"
        return ""


class PartnerAccessService:
    """Handle partner cookie bootstrap and its durable session lifecycle."""

    def __init__(self, owner: Any) -> None:
        self._owner = owner

    def load(self, request: web.Request) -> dict[str, Any] | None:
        cookies = getattr(request, "cookies", {}) or {}
        session_id = (cookies.get(self._owner._partner_access_cookie_name()) or "").strip()
        if not session_id:
            return None

        now = time.time()
        session_cache = self._owner._dashboard_auth_state_cache("_partner_access_sessions")
        session = session_cache.get(session_id)
        if not session:
            try:
                session = self._owner._dashboard_auth_state_repo().load_partner_access_session(
                    session_id,
                    now=now,
                )
            except Exception as exc:
                log.debug("Could not load partner access session from DB: %s", exc)
                session = None
            if not session:
                return None
            session_cache.put(session_id, session)

        expires_at = float(session.get("expires_at", 0.0) or 0.0)
        request_binding = PartnerAccessBinding.capture(request)
        if expires_at <= now or not PartnerAccessBinding.matches(session, request_binding):
            session_cache.pop(session_id, None)
            try:
                self._owner._dashboard_auth_state_repo().delete_session(session_id)
            except Exception as exc:
                log.debug("Could not delete invalid partner access session from DB: %s", exc)
            return None

        ttl_seconds = self._owner._partner_access_session_ttl()
        old_expires = expires_at
        session["expires_at"] = now + ttl_seconds
        session.setdefault("auth_type", "partner_token")
        if session["expires_at"] - old_expires > 300:
            try:
                self._owner._dashboard_auth_state_repo().save_partner_access_session(
                    session_id=session_id,
                    payload=session,
                    created_at=float(session.get("created_at", now) or now),
                    expires_at=session["expires_at"],
                )
            except Exception as exc:
                log.debug("Could not refresh partner access session in DB: %s", exc)
        session_cache.put(session_id, session)
        return session

    def create(self, request: web.Request) -> str:
        session_id = secrets.token_urlsafe(32)
        now = time.time()
        ttl_seconds = self._owner._partner_access_session_ttl()
        session_data = {
            "auth_type": "partner_token",
            **PartnerAccessBinding.capture(request),
            "created_at": now,
            "expires_at": now + ttl_seconds,
        }
        self._owner._dashboard_auth_state_cache("_partner_access_sessions").put(
            session_id,
            session_data,
        )
        try:
            self._owner._dashboard_auth_state_repo().save_partner_access_session(
                session_id=session_id,
                payload=session_data,
                created_at=now,
                expires_at=now + ttl_seconds,
            )
        except Exception as exc:
            log.debug("Could not persist partner access session to DB: %s", exc)
        return session_id

    def delete(self, session_id: str) -> None:
        normalized_session_id = str(session_id or "").strip()
        if not normalized_session_id:
            return
        self._owner._dashboard_auth_state_cache("_partner_access_sessions").pop(
            normalized_session_id,
            None,
        )
        try:
            self._owner._dashboard_auth_state_repo().delete_session(normalized_session_id)
        except Exception as exc:
            log.debug("Could not delete partner access session from DB: %s", exc)

    def bootstrap_redirect_location(self, request: web.Request) -> str:
        path = str(getattr(request, "path", "") or "").strip() or "/twitch/dashboard-v2"
        query = getattr(request, "query", {}) or {}
        query_items = [
            (str(key), str(value))
            for key, value in query.items()
            if str(key) != "partner_token"
        ]
        candidate = path
        if query_items:
            candidate = f"{candidate}?{urlencode(query_items)}"
        return self._owner._safe_internal_redirect(candidate, fallback=path)

    def consume_bootstrap(self, request: web.Request) -> web.StreamResponse | None:
        if self._owner._get_dashboard_auth_session(request) or self._owner._get_discord_admin_session(
            request
        ):
            return None
        if self.load(request):
            return None

        configured_partner_token = str(getattr(self._owner, "_partner_token", "") or "").strip()
        query = getattr(request, "query", {}) or {}
        presented_partner_token = str(query.get("partner_token") or "").strip()
        if not configured_partner_token or not presented_partner_token:
            return None
        try:
            if not secrets.compare_digest(presented_partner_token, configured_partner_token):
                return None
        except Exception:
            return None

        session_id = self.create(request)
        response = web.HTTPFound(self.bootstrap_redirect_location(request))
        self._owner._set_partner_access_cookie(response, request, session_id)
        return response

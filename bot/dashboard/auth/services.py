"""Dashboard auth services for session lifecycle, cookies, and partner login flows."""

from __future__ import annotations

import base64
import hashlib
import hmac
import json
import re
import secrets
import time
from typing import Any

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

    @staticmethod
    def _sanitize_cookie_value(value: str) -> str:
        text = str(value or "").strip()
        if not text:
            return ""
        return re.sub(r"[^A-Za-z0-9._~=-]", "", text)

    @staticmethod
    def _sanitize_log_value(value: object | None) -> str:
        if value is None:
            return "<none>"
        return str(value).replace("\r", "\\r").replace("\n", "\\n")

    def set_session_cookie(
        self, response: web.StreamResponse, request: web.Request, session_id: str
    ) -> None:
        response.set_cookie(
            self.session_cookie_name(),
            self._sanitize_cookie_value(session_id),
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
        path_getter = getattr(self._owner, "_oauth_callback_cookie_path", None)
        cookie_path = "/twitch/auth/callback"
        if callable(path_getter):
            try:
                candidate = str(path_getter() or "").strip()
            except Exception:
                candidate = ""
            if candidate.startswith("/"):
                cookie_path = candidate
        response.set_cookie(
            self.oauth_context_cookie_name(),
            self._sanitize_cookie_value(token),
            max_age=int(getattr(self._owner, "_oauth_state_ttl_seconds", 600) or 600),
            httponly=True,
            secure=self._owner._is_secure_request(request),
            samesite="Lax",
            path=cookie_path,
        )

    def clear_oauth_context_cookie(
        self, response: web.StreamResponse, request: web.Request
    ) -> None:
        path_getter = getattr(self._owner, "_oauth_callback_cookie_path", None)
        cookie_path = "/twitch/auth/callback"
        if callable(path_getter):
            try:
                candidate = str(path_getter() or "").strip()
            except Exception:
                candidate = ""
            if candidate.startswith("/"):
                cookie_path = candidate
        response.del_cookie(
            self.oauth_context_cookie_name(),
            path=cookie_path,
            httponly=True,
            samesite="Lax",
            secure=self._owner._is_secure_request(request),
        )

    def set_discord_oauth_context_cookie(
        self, response: web.StreamResponse, request: web.Request, token: str
    ) -> None:
        response.set_cookie(
            self.discord_oauth_context_cookie_name(),
            self._sanitize_cookie_value(token),
            max_age=int(getattr(self._owner, "_oauth_state_ttl_seconds", 600) or 600),
            httponly=True,
            secure=self._owner._is_secure_request(request),
            samesite="Lax",
            path="/twitch/auth/discord/complete",
        )

    def clear_discord_oauth_context_cookie(
        self, response: web.StreamResponse, request: web.Request
    ) -> None:
        response.del_cookie(
            self.discord_oauth_context_cookie_name(),
            path="/twitch/auth/discord/complete",
            httponly=True,
            samesite="Lax",
            secure=self._owner._is_secure_request(request),
        )

    def set_partner_access_cookie(
        self, response: web.StreamResponse, request: web.Request, session_id: str
    ) -> None:
        response.set_cookie(
            self.partner_access_cookie_name(),
            self._sanitize_cookie_value(session_id),
            max_age=self._owner._partner_access_session_ttl(),
            httponly=True,
            secure=self._owner._is_secure_request(request),
            samesite="Strict",
            path="/",
        )

    def clear_partner_access_cookie(
        self, response: web.StreamResponse, request: web.Request
    ) -> None:
        response.del_cookie(
            self.partner_access_cookie_name(),
            path="/",
            httponly=True,
            samesite="Strict",
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
    """Create a coarse, less fragile request fingerprint for partner dashboard sessions."""

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


class PartnerLoginTokenService:
    """Issue and consume one-time signed login tokens for partner dashboard access."""

    _TOKEN_VERSION = 1

    def __init__(self, owner: Any) -> None:
        self._owner = owner

    def issue(
        self,
        *,
        next_path: str | None = None,
        now: float | None = None,
    ) -> dict[str, Any]:
        secret = self._signing_secret()
        if not secret:
            raise RuntimeError("Partner login signing secret is not configured.")

        current = time.time() if now is None else float(now)
        ttl_seconds = self._token_ttl_seconds()
        expires_at = current + ttl_seconds
        state_id = secrets.token_urlsafe(24)
        normalized_next_path = self._normalize_next_path(next_path)
        state_cache = self._state_cache()
        state_cache.prune_by_expires_at(now=current, max_items=512)
        payload = {
            "v": self._TOKEN_VERSION,
            "sid": state_id,
            "next": normalized_next_path,
            "iat": int(current),
            "exp": int(expires_at),
        }
        state_payload = {
            "created_at": current,
            "expires_at": expires_at,
            "next_path": normalized_next_path,
        }
        state_cache.put(state_id, state_payload)
        try:
            self._owner._dashboard_auth_state_repo().save_partner_login_state(
                state=state_id,
                payload=state_payload,
                ttl_seconds=ttl_seconds,
                now=current,
            )
        except Exception as exc:
            state_cache.pop(state_id, None)
            raise RuntimeError("Could not persist partner login token state.") from exc

        return {
            "token": self._serialize_token(payload, secret=secret),
            "next_path": normalized_next_path,
            "expires_at": expires_at,
            "expires_in": ttl_seconds,
        }

    def consume(
        self,
        token: str,
        *,
        now: float | None = None,
    ) -> dict[str, Any] | None:
        secret = self._signing_secret()
        if not secret:
            return None

        current = time.time() if now is None else float(now)
        state_cache = self._state_cache()
        state_cache.prune_by_expires_at(now=current, max_items=512)
        payload = self._deserialize_token(token, secret=secret)
        if not payload:
            return None

        state_id = str(payload.get("sid") or "").strip()
        next_path = self._normalize_next_path(payload.get("next"))
        issued_at = int(payload.get("iat") or 0)
        expires_at = int(payload.get("exp") or 0)
        if not state_id or issued_at <= 0 or expires_at <= current or expires_at <= issued_at:
            return None

        try:
            persisted_state = self._owner._dashboard_auth_state_repo().consume_partner_login_state(
                state_id,
                now=current,
            )
        except Exception as exc:
            log.warning(
                "Could not load persisted partner login state %s: %s",
                self._sanitize_cookie_value(state_id),
                self._sanitize_log_value(exc),
            )
            return None
        state_cache.pop(state_id, None)
        state_data = persisted_state if isinstance(persisted_state, dict) else None
        if not isinstance(state_data, dict):
            return None

        if self._normalize_next_path(state_data.get("next_path")) != next_path:
            return None
        stored_expires_at = float(state_data.get("expires_at", 0.0) or 0.0)
        if stored_expires_at <= current:
            return None

        return {
            "next_path": next_path,
            "issued_at": issued_at,
            "expires_at": expires_at,
        }

    def issue_request_authorization_mode(self, request: web.Request) -> str | None:
        if bool(getattr(self._owner, "_noauth", False)):
            return "noauth"

        is_local_request = getattr(self._owner, "_is_local_request", None)
        if callable(is_local_request):
            try:
                if bool(is_local_request(request)):
                    return "localhost"
            except Exception:
                pass

        headers = getattr(request, "headers", {}) or {}
        admin_header = str(headers.get("X-Admin-Token") or "").strip()
        configured_admin_token = str(getattr(self._owner, "_token", "") or "").strip()
        if configured_admin_token and admin_header:
            try:
                if secrets.compare_digest(admin_header, configured_admin_token):
                    return "admin_header"
            except Exception:
                pass

        is_discord_admin_request = getattr(self._owner, "_is_discord_admin_request", None)
        if callable(is_discord_admin_request):
            try:
                if bool(is_discord_admin_request(request)):
                    return "admin_session"
            except Exception:
                pass

        auth_level_getter = getattr(self._owner, "_get_auth_level", None)
        if callable(auth_level_getter):
            try:
                auth_level = str(auth_level_getter(request) or "").strip().lower()
            except Exception:
                auth_level = ""
            if auth_level == "localhost":
                return "localhost"
            if auth_level == "admin":
                return "admin_session"
        return None

    def is_issue_request_authorized(self, request: web.Request) -> bool:
        return self.issue_request_authorization_mode(request) is not None

    def _state_cache(self) -> Any:
        return self._owner._dashboard_auth_state_cache("_partner_login_states")

    def _token_ttl_seconds(self) -> int:
        configured = int(getattr(self._owner, "_partner_login_token_ttl_seconds", 180) or 180)
        return min(max(30, configured), 600)

    def _signing_secret(self) -> str:
        return str(getattr(self._owner, "_partner_token", "") or "").strip()

    def _normalize_next_path(self, raw_next_path: Any) -> str:
        normalizer = getattr(self._owner, "_normalize_next_path", None)
        fallback = "/twitch/dashboard-v2"
        if callable(normalizer):
            candidate = normalizer(raw_next_path)
        else:
            candidate = str(raw_next_path or "").strip() or fallback
        return self._owner._safe_internal_redirect(candidate, fallback=fallback)

    @staticmethod
    def _encode_component(raw_bytes: bytes) -> str:
        return base64.urlsafe_b64encode(raw_bytes).rstrip(b"=").decode("ascii")

    @staticmethod
    def _decode_component(component: str) -> bytes:
        normalized = str(component or "").strip()
        if not normalized:
            raise ValueError("Token component is empty.")
        padding = "=" * (-len(normalized) % 4)
        return base64.urlsafe_b64decode(f"{normalized}{padding}".encode("ascii"))

    def _serialize_token(self, payload: dict[str, Any], *, secret: str) -> str:
        payload_bytes = json.dumps(
            payload,
            separators=(",", ":"),
            sort_keys=True,
        ).encode("utf-8")
        payload_segment = self._encode_component(payload_bytes)
        signature = hmac.new(
            secret.encode("utf-8"),
            payload_segment.encode("ascii"),
            hashlib.sha256,
        ).digest()
        signature_segment = self._encode_component(signature)
        return f"{payload_segment}.{signature_segment}"

    def _deserialize_token(self, token: str, *, secret: str) -> dict[str, Any] | None:
        parts = str(token or "").strip().split(".", 1)
        if len(parts) != 2:
            return None
        payload_segment, signature_segment = parts
        try:
            expected_signature = hmac.new(
                secret.encode("utf-8"),
                payload_segment.encode("ascii"),
                hashlib.sha256,
            ).digest()
            presented_signature = self._decode_component(signature_segment)
            if not hmac.compare_digest(expected_signature, presented_signature):
                return None
            payload_bytes = self._decode_component(payload_segment)
            payload = json.loads(payload_bytes.decode("utf-8"))
        except Exception:
            return None
        if not isinstance(payload, dict):
            return None
        if int(payload.get("v") or 0) != self._TOKEN_VERSION:
            return None
        return payload


class PartnerAccessService:
    """Handle durable partner dashboard sessions after login exchange."""

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

"""Neutral HTTP client for the bot internal API."""

from __future__ import annotations

import asyncio
import json
import re
from ipaddress import ip_address
from typing import Any
from urllib.parse import unquote, urlencode, urlsplit, urlunsplit

import aiohttp

from .app import IDEMPOTENCY_KEY_HEADER, INTERNAL_API_BASE_PATH, INTERNAL_TOKEN_HEADER

_LOGIN_SEGMENT_RE = re.compile(r"^[a-z0-9_]{3,25}$")


class InternalApiClientError(RuntimeError):
    """Safe, user-facing upstream error."""

    def __init__(self, *, status: int, code: str, message: str) -> None:
        super().__init__(message)
        self.status = int(status)
        self.code = str(code)
        self.message = str(message)


class InternalApiClient:
    """Typed wrapper around the bot-internal HTTP API."""

    def __init__(
        self,
        *,
        base_url: str,
        token: str,
        allow_non_loopback: bool = False,
        timeout_seconds: float = 10.0,
        session: aiohttp.ClientSession | None = None,
    ) -> None:
        self._base_url = self._normalize_base_url(
            base_url,
            allow_non_loopback=bool(allow_non_loopback),
        )
        self._token = (token or "").strip()
        if not self._token:
            raise ValueError("token is required")
        self._timeout_seconds = max(0.5, float(timeout_seconds or 10.0))

        self._session = session
        self._owns_session = session is None

    @staticmethod
    def _sanitize_message(value: str, *, fallback: str) -> str:
        text = (value or "").replace("\r", " ").replace("\n", " ").strip()
        if not text:
            return fallback
        if len(text) > 220:
            return f"{text[:217]}..."
        return text

    @staticmethod
    def _normalize_path(path: str) -> str:
        cleaned = str(path or "").strip()
        if not cleaned.startswith("/"):
            cleaned = f"/{cleaned}"
        return cleaned

    @staticmethod
    def _is_loopback_host(host: str) -> bool:
        normalized = str(host or "").strip().lower().rstrip(".")
        if not normalized:
            return False
        if normalized == "localhost":
            return True
        try:
            return ip_address(normalized).is_loopback
        except ValueError:
            return False

    @classmethod
    def _normalize_base_url(cls, value: str, *, allow_non_loopback: bool) -> str:
        raw = (value or "").strip()
        if not raw:
            raise ValueError("base_url is required")
        if "://" not in raw:
            raw = f"http://{raw}"

        try:
            parsed = urlsplit(raw)
        except Exception as exc:
            raise ValueError("base_url is invalid") from exc
        if not parsed.netloc:
            raise ValueError("base_url is invalid")
        if parsed.username or parsed.password:
            raise ValueError("base_url must not contain credentials")
        scheme = (parsed.scheme or "http").lower()
        if scheme not in {"http", "https"}:
            raise ValueError("base_url must use http or https")

        host = (parsed.hostname or "").strip()
        if not host:
            raise ValueError("base_url is invalid")
        if not allow_non_loopback and not cls._is_loopback_host(host):
            raise ValueError(
                "base_url host must resolve to loopback unless allow_non_loopback=True"
            )
        try:
            port = parsed.port
        except ValueError as exc:
            raise ValueError("base_url is invalid") from exc
        host_for_netloc = f"[{host}]" if ":" in host else host
        normalized_netloc = f"{host_for_netloc}:{port}" if port is not None else host_for_netloc

        path = (parsed.path or "").rstrip("/")
        internal_base = INTERNAL_API_BASE_PATH.rstrip("/")
        if path == internal_base:
            path = ""
        elif path.endswith(internal_base):
            path = path[: -len(internal_base)]

        normalized_path = path.rstrip("/")
        return urlunsplit(
            (
                scheme,
                normalized_netloc,
                normalized_path,
                "",
                "",
            )
        )

    @staticmethod
    def _parse_json(text: str) -> tuple[Any, bool]:
        raw = (text or "").strip()
        if not raw:
            return {}, True
        try:
            return json.loads(raw), True
        except json.JSONDecodeError:
            return None, False

    @staticmethod
    def _extract_error_text(payload: Any) -> str:
        if isinstance(payload, dict):
            for key in ("message", "error", "detail", "reason"):
                value = payload.get(key)
                if isinstance(value, str) and value.strip():
                    return value.strip()
        return ""

    def _map_http_error(self, status: int, payload: Any) -> InternalApiClientError:
        upstream_message = self._extract_error_text(payload)
        if status in {400, 404}:
            code = "bad_request" if status == 400 else "not_found"
            fallback = (
                "Bot internal API rejected the request."
                if status == 400
                else "Requested resource was not found."
            )
            return InternalApiClientError(
                status=status,
                code=code,
                message=self._sanitize_message(upstream_message, fallback=fallback),
            )
        if status in {401, 403}:
            return InternalApiClientError(
                status=502,
                code="upstream_auth_failed",
                message="Dashboard service failed to authenticate with bot internal API.",
            )
        if status == 429:
            return InternalApiClientError(
                status=503,
                code="upstream_rate_limited",
                message="Bot internal API is currently rate limited.",
            )
        if status >= 500:
            return InternalApiClientError(
                status=502,
                code="upstream_unavailable",
                message="Bot internal API is currently unavailable.",
            )
        return InternalApiClientError(
            status=502,
            code="upstream_error",
            message="Bot internal API request failed.",
        )

    async def _get_session(self) -> aiohttp.ClientSession:
        if self._session is None or self._session.closed:
            timeout = aiohttp.ClientTimeout(total=self._timeout_seconds)
            self._session = aiohttp.ClientSession(timeout=timeout)
            self._owns_session = True
        return self._session

    async def close(self) -> None:
        if self._owns_session and self._session is not None and not self._session.closed:
            await self._session.close()

    async def get_observability_snapshot(self) -> dict[str, Any]:
        payload = await self._request_json("GET", f"{INTERNAL_API_BASE_PATH}/debug/observability")
        if isinstance(payload, dict):
            return payload
        raise InternalApiClientError(
            status=502,
            code="invalid_payload",
            message="Bot internal API returned an invalid observability payload.",
        )

    async def _request_json(
        self,
        method: str,
        path: str,
        *,
        headers: dict[str, str] | None = None,
        query: dict[str, Any] | None = None,
        payload: dict[str, Any] | None = None,
    ) -> Any:
        normalized_path = self._normalize_path(path)
        query_suffix = ""
        if query:
            compact = {k: v for k, v in query.items() if v is not None}
            if compact:
                query_suffix = f"?{urlencode(compact)}"
        url = f"{self._base_url}{normalized_path}{query_suffix}"
        request_headers = {INTERNAL_TOKEN_HEADER: self._token}
        if headers:
            for key, value in headers.items():
                text_value = str(value or "").strip()
                if text_value:
                    request_headers[str(key)] = text_value
        session = await self._get_session()

        try:
            response = await session.request(
                method=method,
                url=url,
                headers=request_headers,
                json=payload,
                allow_redirects=False,
            )
        except asyncio.TimeoutError as exc:
            raise InternalApiClientError(
                status=504,
                code="upstream_timeout",
                message="Bot internal API request timed out.",
            ) from exc
        except aiohttp.ClientError as exc:
            raise InternalApiClientError(
                status=502,
                code="upstream_connection_failed",
                message="Bot internal API is unreachable.",
            ) from exc

        try:
            raw_text = await response.text()
        except Exception:
            raw_text = ""
        finally:
            response.release()

        parsed, is_json = self._parse_json(raw_text)
        if response.status >= 400:
            raise self._map_http_error(response.status, parsed if is_json else None)

        if not is_json:
            raise InternalApiClientError(
                status=502,
                code="upstream_invalid_json",
                message="Bot internal API returned invalid JSON.",
            )
        return parsed

    @staticmethod
    def _message_or_default(payload: Any, *, fallback: str) -> str:
        if isinstance(payload, dict):
            message = payload.get("message")
            if message is not None:
                return str(message)
        return fallback

    @staticmethod
    def _normalize_login_path_segment(login: str) -> str:
        normalized = unquote(str(login or "")).strip().lower()
        if not _LOGIN_SEGMENT_RE.fullmatch(normalized):
            raise InternalApiClientError(
                status=400,
                code="bad_request",
                message="Streamer login is invalid.",
            )
        return normalized

    @staticmethod
    def _normalize_discord_user_id_value(discord_user_id: str | int) -> str:
        normalized = str(discord_user_id or "").strip()
        if not normalized.isdigit():
            raise InternalApiClientError(
                status=400,
                code="bad_request",
                message="Discord user ID is invalid.",
            )
        return normalized

    @staticmethod
    def _normalize_positive_id_value(value: str | int, *, field_name: str) -> str:
        normalized = str(value or "").strip()
        if not normalized.isdigit() or int(normalized) <= 0:
            raise InternalApiClientError(
                status=400,
                code="bad_request",
                message=f"{field_name} is invalid.",
            )
        return normalized

    @staticmethod
    def _normalize_optional_positive_id_value(
        value: str | int | None,
        *,
        field_name: str,
    ) -> str | None:
        if value is None:
            return None
        normalized = str(value).strip()
        if not normalized:
            return None
        if not normalized.isdigit() or int(normalized) <= 0:
            raise InternalApiClientError(
                status=400,
                code="bad_request",
                message=f"{field_name} is invalid.",
            )
        return normalized

    @staticmethod
    def _normalize_required_text(value: str, *, field_name: str, max_length: int) -> str:
        normalized = str(value or "").replace("\r", " ").replace("\n", " ").strip()
        if not normalized or len(normalized) > max_length:
            raise InternalApiClientError(
                status=400,
                code="bad_request",
                message=f"{field_name} is invalid.",
            )
        return normalized

    @staticmethod
    def _normalize_tracking_token_value(value: str) -> str:
        normalized = str(value or "").strip()
        if not normalized or len(normalized) > 128:
            raise InternalApiClientError(
                status=400,
                code="bad_request",
                message="tracking_token is invalid.",
            )
        return normalized

    @staticmethod
    def _validate_raid_state_payload(payload: Any, *, context: str) -> dict[str, Any]:
        if not isinstance(payload, dict):
            raise InternalApiClientError(
                status=502,
                code="upstream_invalid_shape",
                message=f"Bot internal API returned an invalid {context} payload.",
            )
        return payload

    @staticmethod
    def _validate_live_announcements_payload(payload: Any) -> list[dict[str, Any]]:
        if not isinstance(payload, list):
            raise InternalApiClientError(
                status=502,
                code="upstream_invalid_shape",
                message="Bot internal API returned an invalid live announcements payload.",
            )
        normalized: list[dict[str, Any]] = []
        for item in payload:
            if not isinstance(item, dict):
                raise InternalApiClientError(
                    status=502,
                    code="upstream_invalid_shape",
                    message="Bot internal API returned an invalid live announcement entry.",
                )
            required_keys = {
                "streamer_login",
                "message_id",
                "tracking_token",
                "referral_url",
                "button_label",
                "channel_id",
            }
            if not required_keys.issubset(item.keys()):
                raise InternalApiClientError(
                    status=502,
                    code="upstream_invalid_shape",
                    message="Bot internal API returned an incomplete live announcement entry.",
                )
            normalized.append(dict(item))
        return normalized

    async def healthz(self) -> dict[str, Any]:
        payload = await self._request_json("GET", f"{INTERNAL_API_BASE_PATH}/healthz")
        if not isinstance(payload, dict):
            raise InternalApiClientError(
                status=502,
                code="upstream_invalid_shape",
                message="Bot internal API returned an invalid health payload.",
            )
        return payload

    async def get_streamers(self) -> list[dict[str, Any]]:
        payload = await self._request_json("GET", f"{INTERNAL_API_BASE_PATH}/streamers")
        if isinstance(payload, list):
            return [item for item in payload if isinstance(item, dict)]
        if isinstance(payload, dict) and isinstance(payload.get("streamers"), list):
            return [item for item in payload.get("streamers", []) if isinstance(item, dict)]
        raise InternalApiClientError(
            status=502,
            code="upstream_invalid_shape",
            message="Bot internal API returned an invalid streamers payload.",
        )

    async def add_streamer(self, login: str, *, require_link: bool = False) -> str:
        payload = await self._request_json(
            "POST",
            f"{INTERNAL_API_BASE_PATH}/streamers",
            payload={"login": login, "require_link": bool(require_link)},
        )
        return self._message_or_default(payload, fallback="added")

    async def remove_streamer(self, login: str) -> str:
        normalized_login = self._normalize_login_path_segment(login)
        payload = await self._request_json(
            "DELETE",
            f"{INTERNAL_API_BASE_PATH}/streamers/{normalized_login}",
        )
        return self._message_or_default(payload, fallback="removed")

    async def verify_streamer(self, login: str, *, mode: str) -> str:
        normalized_login = self._normalize_login_path_segment(login)
        payload = await self._request_json(
            "POST",
            f"{INTERNAL_API_BASE_PATH}/streamers/{normalized_login}/verify",
            payload={"mode": mode},
        )
        return self._message_or_default(payload, fallback="verified")

    async def archive_streamer(self, login: str, *, mode: str) -> str:
        normalized_login = self._normalize_login_path_segment(login)
        payload = await self._request_json(
            "POST",
            f"{INTERNAL_API_BASE_PATH}/streamers/{normalized_login}/archive",
            payload={"mode": mode},
        )
        return self._message_or_default(payload, fallback="updated")

    async def set_discord_flag(self, login: str, *, is_on_discord: bool) -> str:
        normalized_login = self._normalize_login_path_segment(login)
        payload = await self._request_json(
            "POST",
            f"{INTERNAL_API_BASE_PATH}/streamers/{normalized_login}/discord-flag",
            payload={"is_on_discord": bool(is_on_discord)},
        )
        return self._message_or_default(payload, fallback="updated")

    async def save_discord_profile(
        self,
        login: str,
        *,
        discord_user_id: str | None,
        discord_display_name: str | None,
        mark_member: bool,
    ) -> str:
        normalized_login = self._normalize_login_path_segment(login)
        payload = await self._request_json(
            "POST",
            f"{INTERNAL_API_BASE_PATH}/streamers/{normalized_login}/discord-profile",
            payload={
                "discord_user_id": discord_user_id,
                "discord_display_name": discord_display_name,
                "mark_member": bool(mark_member),
            },
        )
        return self._message_or_default(payload, fallback="updated")

    async def get_stats(
        self,
        *,
        hour_from: int | None = None,
        hour_to: int | None = None,
        streamer: str | None = None,
    ) -> dict[str, Any]:
        payload = await self._request_json(
            "GET",
            f"{INTERNAL_API_BASE_PATH}/stats",
            query={
                "hour_from": hour_from,
                "hour_to": hour_to,
                "streamer": streamer,
            },
        )
        if not isinstance(payload, dict):
            raise InternalApiClientError(
                status=502,
                code="upstream_invalid_shape",
                message="Bot internal API returned an invalid stats payload.",
            )
        return payload

    async def get_streamer_analytics(self, login: str, *, days: int = 30) -> dict[str, Any]:
        normalized_login = self._normalize_login_path_segment(login)
        payload = await self._request_json(
            "GET",
            f"{INTERNAL_API_BASE_PATH}/analytics/streamer/{normalized_login}",
            query={"days": int(days)},
        )
        if not isinstance(payload, dict):
            raise InternalApiClientError(
                status=502,
                code="upstream_invalid_shape",
                message="Bot internal API returned an invalid streamer analytics payload.",
            )
        return payload

    async def get_analytics_comparison(self, *, days: int = 30) -> dict[str, Any]:
        payload = await self._request_json(
            "GET",
            f"{INTERNAL_API_BASE_PATH}/analytics/comparison",
            query={"days": int(days)},
        )
        if not isinstance(payload, dict):
            raise InternalApiClientError(
                status=502,
                code="upstream_invalid_shape",
                message="Bot internal API returned an invalid comparison payload.",
            )
        return payload

    async def get_session(self, session_id: int) -> dict[str, Any]:
        payload = await self._request_json(
            "GET",
            f"{INTERNAL_API_BASE_PATH}/sessions/{int(session_id)}",
        )
        if not isinstance(payload, dict):
            raise InternalApiClientError(
                status=502,
                code="upstream_invalid_shape",
                message="Bot internal API returned an invalid session payload.",
            )
        return payload

    async def get_active_live_announcements(self) -> list[dict[str, Any]]:
        payload = await self._request_json(
            "GET",
            f"{INTERNAL_API_BASE_PATH}/live/active-announcements",
        )
        return self._validate_live_announcements_payload(payload)

    async def record_live_link_click(
        self,
        *,
        streamer_login: str,
        tracking_token: str,
        discord_user_id: str | int,
        discord_username: str,
        guild_id: str | int | None,
        channel_id: str | int,
        message_id: str | int,
        source_hint: str,
        idempotency_key: str | None = None,
    ) -> dict[str, Any]:
        normalized_login = self._normalize_login_path_segment(streamer_login)
        normalized_tracking_token = self._normalize_tracking_token_value(tracking_token)
        normalized_discord_user_id = self._normalize_discord_user_id_value(discord_user_id)
        normalized_discord_username = self._normalize_required_text(
            discord_username,
            field_name="discord_username",
            max_length=200,
        )
        normalized_guild_id = self._normalize_optional_positive_id_value(
            guild_id,
            field_name="guild_id",
        )
        normalized_channel_id = self._normalize_positive_id_value(
            channel_id,
            field_name="channel_id",
        )
        normalized_message_id = self._normalize_positive_id_value(
            message_id,
            field_name="message_id",
        )
        normalized_source_hint = self._normalize_required_text(
            source_hint,
            field_name="source_hint",
            max_length=100,
        )

        extra_headers: dict[str, str] | None = None
        if idempotency_key is not None:
            normalized_idempotency_key = self._normalize_required_text(
                idempotency_key,
                field_name="idempotency_key",
                max_length=128,
            )
            extra_headers = {IDEMPOTENCY_KEY_HEADER: normalized_idempotency_key}

        payload = await self._request_json(
            "POST",
            f"{INTERNAL_API_BASE_PATH}/live/link-click",
            headers=extra_headers,
            payload={
                "streamer_login": normalized_login,
                "tracking_token": normalized_tracking_token,
                "discord_user_id": normalized_discord_user_id,
                "discord_username": normalized_discord_username,
                "guild_id": normalized_guild_id,
                "channel_id": normalized_channel_id,
                "message_id": normalized_message_id,
                "source_hint": normalized_source_hint,
            },
        )
        if not isinstance(payload, dict):
            raise InternalApiClientError(
                status=502,
                code="upstream_invalid_shape",
                message="Bot internal API returned an invalid live link click payload.",
            )
        return payload

    async def get_raid_auth_url(
        self,
        login: str,
        discord_user_id: str | int | None = None,
    ) -> str:
        query: dict[str, Any] = {"login": login}
        if discord_user_id is not None:
            normalized_discord_user_id = str(discord_user_id).strip()
            if normalized_discord_user_id:
                query["discord_user_id"] = normalized_discord_user_id
        payload = await self._request_json(
            "GET",
            f"{INTERNAL_API_BASE_PATH}/raid/auth-url",
            query=query,
        )
        if not isinstance(payload, dict):
            raise InternalApiClientError(
                status=502,
                code="upstream_invalid_shape",
                message="Bot internal API returned an invalid raid auth payload.",
            )
        auth_url = str(payload.get("auth_url") or "").strip()
        if not auth_url:
            raise InternalApiClientError(
                status=502,
                code="upstream_invalid_shape",
                message="Bot internal API returned an empty raid auth URL.",
            )
        return auth_url

    async def get_raid_auth_state(self, *, discord_user_id: str | int) -> dict[str, Any]:
        normalized_discord_id = self._normalize_discord_user_id_value(discord_user_id)
        payload = await self._request_json(
            "GET",
            f"{INTERNAL_API_BASE_PATH}/raid/auth-state",
            query={"discord_user_id": normalized_discord_id},
        )
        return self._validate_raid_state_payload(payload, context="raid auth state")

    async def get_raid_block_state(
        self,
        *,
        discord_user_id: str | int | None = None,
        twitch_login: str | None = None,
    ) -> dict[str, Any]:
        normalized_discord_id = None
        if discord_user_id is not None:
            normalized_discord_id = self._normalize_discord_user_id_value(discord_user_id)
        normalized_login = None
        if twitch_login is not None:
            normalized_login = self._normalize_login_path_segment(twitch_login)
        if normalized_discord_id is None and normalized_login is None:
            raise InternalApiClientError(
                status=400,
                code="bad_request",
                message="discord_user_id or twitch_login is required.",
            )
        payload = await self._request_json(
            "GET",
            f"{INTERNAL_API_BASE_PATH}/raid/block-state",
            query={
                "discord_user_id": normalized_discord_id,
                "twitch_login": normalized_login,
            },
        )
        return self._validate_raid_state_payload(payload, context="raid block state")

    async def get_raid_go_url(self, state: str) -> str | None:
        try:
            payload = await self._request_json(
                "GET",
                f"{INTERNAL_API_BASE_PATH}/raid/go-url",
                query={"state": state},
            )
        except InternalApiClientError as exc:
            if exc.code == "not_found":
                return None
            raise
        if not isinstance(payload, dict):
            raise InternalApiClientError(
                status=502,
                code="upstream_invalid_shape",
                message="Bot internal API returned an invalid raid redirect payload.",
            )
        auth_url = str(payload.get("auth_url") or "").strip()
        return auth_url or None

    async def send_raid_requirements(self, login: str) -> str:
        payload = await self._request_json(
            "POST",
            f"{INTERNAL_API_BASE_PATH}/raid/requirements",
            payload={"login": login},
        )
        return self._message_or_default(payload, fallback="sent")

    async def process_raid_oauth_callback(
        self,
        *,
        code: str,
        state: str,
        error: str,
    ) -> dict[str, Any]:
        payload = await self._request_json(
            "POST",
            f"{INTERNAL_API_BASE_PATH}/raid/oauth-callback",
            payload={"code": code, "state": state, "error": error},
        )
        if not isinstance(payload, dict):
            raise InternalApiClientError(
                status=502,
                code="upstream_invalid_shape",
                message="Bot internal API returned an invalid raid callback payload.",
            )
        return payload


__all__ = ["InternalApiClient", "InternalApiClientError"]

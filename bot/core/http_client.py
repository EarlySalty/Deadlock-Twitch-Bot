"""Shared HTTP client primitives for the bot internal API."""

from __future__ import annotations

import asyncio
import json
import re
from ipaddress import ip_address
from typing import Any
from urllib.parse import unquote, urlencode, urlsplit, urlunsplit

import aiohttp

_LOGIN_SEGMENT_RE = re.compile(r"^[a-z0-9_]{3,25}$")


class HttpClientError(RuntimeError):
    """Safe, user-facing upstream error."""

    def __init__(self, *, status: int, code: str, message: str) -> None:
        super().__init__(message)
        self.status = int(status)
        self.code = str(code)
        self.message = str(message)


class BaseInternalHttpClient:
    """Shared transport, validation and common endpoint helpers."""

    api_base_path = ""
    token_header = ""
    error_type: type[HttpClientError] = HttpClientError

    def __init__(
        self,
        *,
        base_url: str,
        token: str,
        allow_non_loopback: bool = False,
        timeout_seconds: float = 10.0,
        session: aiohttp.ClientSession | None = None,
    ) -> None:
        if not self.api_base_path:
            raise ValueError("api_base_path is required")
        if not self.token_header:
            raise ValueError("token_header is required")
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

    def _make_error(self, *, status: int, code: str, message: str) -> HttpClientError:
        return self.error_type(status=status, code=code, message=message)

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
        is_loopback = cls._is_loopback_host(host)
        if not allow_non_loopback and not is_loopback:
            raise ValueError(
                "base_url host must resolve to loopback unless allow_non_loopback=True"
            )
        if not is_loopback and scheme != "https":
            raise ValueError("base_url must use https for non-loopback hosts")
        try:
            port = parsed.port
        except ValueError as exc:
            raise ValueError("base_url is invalid") from exc
        host_for_netloc = f"[{host}]" if ":" in host else host
        normalized_netloc = f"{host_for_netloc}:{port}" if port is not None else host_for_netloc

        path = (parsed.path or "").rstrip("/")
        internal_base = cls.api_base_path.rstrip("/")
        if path == internal_base:
            path = ""
        elif path.endswith(internal_base):
            path = path[: -len(internal_base)]

        normalized_path = path.rstrip("/")
        return urlunsplit((scheme, normalized_netloc, normalized_path, "", ""))

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

    def _map_http_error_common(
        self,
        status: int,
        payload: Any,
        *,
        preserve_server_error_code: bool = False,
        preserve_server_message: bool = False,
    ) -> HttpClientError:
        upstream_message = self._extract_error_text(payload)
        upstream_code = ""
        if isinstance(payload, dict):
            upstream_code = str(payload.get("error") or "").strip()
        if status in {400, 404}:
            code = "bad_request" if status == 400 else "not_found"
            fallback = (
                "Bot internal API rejected the request."
                if status == 400
                else "Requested resource was not found."
            )
            return self._make_error(
                status=status,
                code=code,
                message=self._sanitize_message(upstream_message, fallback=fallback),
            )
        if status in {401, 403}:
            return self._make_error(
                status=502,
                code="upstream_auth_failed",
                message="Dashboard service failed to authenticate with bot internal API.",
            )
        if status == 429:
            return self._make_error(
                status=503,
                code="upstream_rate_limited",
                message="Bot internal API is currently rate limited.",
            )
        if status >= 500:
            return self._make_error(
                status=502,
                code=upstream_code if preserve_server_error_code and upstream_code else "upstream_unavailable",
                message=(
                    self._sanitize_message(
                        upstream_message,
                        fallback="Bot internal API is currently unavailable.",
                    )
                    if preserve_server_message
                    else "Bot internal API is currently unavailable."
                ),
            )
        return self._make_error(
            status=502,
            code="upstream_error",
            message="Bot internal API request failed.",
        )

    def _map_http_error(self, status: int, payload: Any) -> HttpClientError:
        raise NotImplementedError

    async def _get_session(self) -> aiohttp.ClientSession:
        if self._session is None or self._session.closed:
            timeout = aiohttp.ClientTimeout(total=self._timeout_seconds)
            self._session = aiohttp.ClientSession(timeout=timeout)
            self._owns_session = True
        return self._session

    async def close(self) -> None:
        if self._owns_session and self._session is not None and not self._session.closed:
            await self._session.close()

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
        request_headers = {self.token_header: self._token}
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
            raise self._make_error(
                status=504,
                code="upstream_timeout",
                message="Bot internal API request timed out.",
            ) from exc
        except aiohttp.ClientError as exc:
            raise self._make_error(
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
            raise self._make_error(
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

    def _normalize_login_path_segment(self, login: str) -> str:
        normalized = unquote(str(login or "")).strip().lower()
        if not _LOGIN_SEGMENT_RE.fullmatch(normalized):
            raise self._make_error(
                status=400,
                code="bad_request",
                message="Streamer login is invalid.",
            )
        return normalized

    def _normalize_discord_user_id_value(self, discord_user_id: str | int) -> str:
        normalized = str(discord_user_id or "").strip()
        if not normalized.isdigit():
            raise self._make_error(
                status=400,
                code="bad_request",
                message="Discord user ID is invalid.",
            )
        return normalized

    def _normalize_positive_id_value(self, value: str | int, *, field_name: str) -> str:
        normalized = str(value or "").strip()
        if not normalized.isdigit() or int(normalized) <= 0:
            raise self._make_error(
                status=400,
                code="bad_request",
                message=f"{field_name} is invalid.",
            )
        return normalized

    def _normalize_optional_positive_id_value(
        self,
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
            raise self._make_error(
                status=400,
                code="bad_request",
                message=f"{field_name} is invalid.",
            )
        return normalized

    def _normalize_required_text(self, value: str, *, field_name: str, max_length: int) -> str:
        normalized = str(value or "").replace("\r", " ").replace("\n", " ").strip()
        if not normalized or len(normalized) > max_length:
            raise self._make_error(
                status=400,
                code="bad_request",
                message=f"{field_name} is invalid.",
            )
        return normalized

    def _normalize_tracking_token_value(self, value: str) -> str:
        normalized = str(value or "").strip()
        if not normalized or len(normalized) > 128:
            raise self._make_error(
                status=400,
                code="bad_request",
                message="tracking_token is invalid.",
            )
        return normalized

    def _validate_dict_payload(self, payload: Any, *, message: str, code: str = "upstream_invalid_shape") -> dict[str, Any]:
        if isinstance(payload, dict):
            return payload
        raise self._make_error(status=502, code=code, message=message)

    def _validate_raid_state_payload(self, payload: Any, *, context: str) -> dict[str, Any]:
        return self._validate_dict_payload(
            payload,
            message=f"Bot internal API returned an invalid {context} payload.",
        )

    def _validate_live_announcements_payload(self, payload: Any) -> list[dict[str, Any]]:
        if not isinstance(payload, list):
            raise self._make_error(
                status=502,
                code="upstream_invalid_shape",
                message="Bot internal API returned an invalid live announcements payload.",
            )
        normalized: list[dict[str, Any]] = []
        required_keys = {
            "streamer_login",
            "message_id",
            "tracking_token",
            "referral_url",
            "button_label",
            "channel_id",
        }
        for item in payload:
            if not isinstance(item, dict):
                raise self._make_error(
                    status=502,
                    code="upstream_invalid_shape",
                    message="Bot internal API returned an invalid live announcement entry.",
                )
            if not required_keys.issubset(item.keys()):
                raise self._make_error(
                    status=502,
                    code="upstream_invalid_shape",
                    message="Bot internal API returned an incomplete live announcement entry.",
                )
            normalized.append(dict(item))
        return normalized

    async def healthz(self) -> dict[str, Any]:
        return self._validate_dict_payload(
            await self._request_json("GET", f"{self.api_base_path}/healthz"),
            message="Bot internal API returned an invalid health payload.",
        )

    async def get_streamers(self) -> list[dict[str, Any]]:
        payload = await self._request_json("GET", f"{self.api_base_path}/streamers")
        if isinstance(payload, list):
            return [item for item in payload if isinstance(item, dict)]
        if isinstance(payload, dict) and isinstance(payload.get("streamers"), list):
            return [item for item in payload.get("streamers", []) if isinstance(item, dict)]
        raise self._make_error(
            status=502,
            code="upstream_invalid_shape",
            message="Bot internal API returned an invalid streamers payload.",
        )

    async def add_streamer(self, login: str, *, require_link: bool = False) -> str:
        payload = await self._request_json(
            "POST",
            f"{self.api_base_path}/streamers",
            payload={"login": login, "require_link": bool(require_link)},
        )
        return self._message_or_default(payload, fallback="added")

    async def remove_streamer(self, login: str) -> str:
        normalized_login = self._normalize_login_path_segment(login)
        payload = await self._request_json(
            "DELETE",
            f"{self.api_base_path}/streamers/{normalized_login}",
        )
        return self._message_or_default(payload, fallback="removed")

    async def verify_streamer(self, login: str, *, mode: str) -> str:
        normalized_login = self._normalize_login_path_segment(login)
        payload = await self._request_json(
            "POST",
            f"{self.api_base_path}/streamers/{normalized_login}/verify",
            payload={"mode": mode},
        )
        return self._message_or_default(payload, fallback="verified")

    async def archive_streamer(self, login: str, *, mode: str) -> str:
        normalized_login = self._normalize_login_path_segment(login)
        payload = await self._request_json(
            "POST",
            f"{self.api_base_path}/streamers/{normalized_login}/archive",
            payload={"mode": mode},
        )
        return self._message_or_default(payload, fallback="updated")

    async def set_discord_flag(self, login: str, *, is_on_discord: bool) -> str:
        normalized_login = self._normalize_login_path_segment(login)
        payload = await self._request_json(
            "POST",
            f"{self.api_base_path}/streamers/{normalized_login}/discord-flag",
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
            f"{self.api_base_path}/streamers/{normalized_login}/discord-profile",
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
        return self._validate_dict_payload(
            await self._request_json(
                "GET",
                f"{self.api_base_path}/stats",
                query={
                    "hour_from": hour_from,
                    "hour_to": hour_to,
                    "streamer": streamer,
                },
            ),
            message="Bot internal API returned an invalid stats payload.",
        )

    async def get_streamer_analytics(self, login: str, *, days: int = 30) -> dict[str, Any]:
        normalized_login = self._normalize_login_path_segment(login)
        return self._validate_dict_payload(
            await self._request_json(
                "GET",
                f"{self.api_base_path}/analytics/streamer/{normalized_login}",
                query={"days": int(days)},
            ),
            message="Bot internal API returned an invalid streamer analytics payload.",
        )

    async def get_analytics_comparison(self, *, days: int = 30) -> dict[str, Any]:
        return self._validate_dict_payload(
            await self._request_json(
                "GET",
                f"{self.api_base_path}/analytics/comparison",
                query={"days": int(days)},
            ),
            message="Bot internal API returned an invalid comparison payload.",
        )

    async def get_session(self, session_id: int) -> dict[str, Any]:
        return self._validate_dict_payload(
            await self._request_json(
                "GET",
                f"{self.api_base_path}/sessions/{int(session_id)}",
            ),
            message="Bot internal API returned an invalid session payload.",
        )

    async def get_raid_auth_url(
        self,
        login: str,
        *,
        discord_user_id: str | int | None = None,
        scope_profile: str | None = None,
    ) -> str:
        query: dict[str, object] = {"login": login}
        if discord_user_id is not None:
            normalized_discord_user_id = str(discord_user_id).strip()
            if normalized_discord_user_id:
                query["discord_user_id"] = normalized_discord_user_id
        normalized_scope_profile = str(scope_profile or "").strip()
        if normalized_scope_profile:
            query["scope_profile"] = normalized_scope_profile
        payload = self._validate_dict_payload(
            await self._request_json(
                "GET",
                f"{self.api_base_path}/raid/auth-url",
                query=query,
            ),
            message="Bot internal API returned an invalid raid auth payload.",
        )
        auth_url = str(payload.get("auth_url") or "").strip()
        if not auth_url:
            raise self._make_error(
                status=502,
                code="upstream_invalid_shape",
                message="Bot internal API returned an empty raid auth URL.",
            )
        return auth_url

    async def get_raid_auth_state(self, *, discord_user_id: str | int) -> dict[str, Any]:
        normalized_discord_id = self._normalize_discord_user_id_value(discord_user_id)
        payload = await self._request_json(
            "GET",
            f"{self.api_base_path}/raid/auth-state",
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
            raise self._make_error(
                status=400,
                code="bad_request",
                message="discord_user_id or twitch_login is required.",
            )
        payload = await self._request_json(
            "GET",
            f"{self.api_base_path}/raid/block-state",
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
                f"{self.api_base_path}/raid/go-url",
                query={"state": state},
            )
        except HttpClientError as exc:
            if exc.code == "not_found":
                return None
            raise
        payload_dict = self._validate_dict_payload(
            payload,
            message="Bot internal API returned an invalid raid redirect payload.",
        )
        auth_url = str(payload_dict.get("auth_url") or "").strip()
        return auth_url or None

    async def send_raid_requirements(self, login: str) -> str:
        payload = await self._request_json(
            "POST",
            f"{self.api_base_path}/raid/requirements",
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
        return self._validate_dict_payload(
            await self._request_json(
                "POST",
                f"{self.api_base_path}/raid/oauth-callback",
                payload={"code": code, "state": state, "error": error},
            ),
            message="Bot internal API returned an invalid raid callback payload.",
        )


__all__ = ["BaseInternalHttpClient", "HttpClientError"]

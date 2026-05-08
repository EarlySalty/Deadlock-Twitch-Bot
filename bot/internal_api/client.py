"""Neutral HTTP client for the bot internal API."""

from __future__ import annotations

from typing import Any

from ..core.http_client import BaseInternalHttpClient, HttpClientError
from .contracts import IDEMPOTENCY_KEY_HEADER, INTERNAL_API_BASE_PATH, INTERNAL_TOKEN_HEADER


class InternalApiClientError(HttpClientError):
    """Safe, user-facing upstream error."""


class InternalApiClient(BaseInternalHttpClient):
    """Typed wrapper around the bot-internal HTTP API."""

    api_base_path = INTERNAL_API_BASE_PATH
    token_header = INTERNAL_TOKEN_HEADER
    error_type = InternalApiClientError

    def _map_http_error(self, status: int, payload: Any) -> InternalApiClientError:
        return self._map_http_error_common(status, payload)

    async def get_observability_snapshot(self) -> dict[str, Any]:
        return self._validate_dict_payload(
            await self._request_json("GET", f"{self.api_base_path}/debug/observability"),
            message="Bot internal API returned an invalid observability payload.",
            code="invalid_payload",
        )

    async def get_chatters_debug(self, login: str) -> dict[str, Any]:
        normalized_login = str(login or "").strip().lower().lstrip("@").lstrip("#")
        return self._validate_dict_payload(
            await self._request_json(
                "GET",
                f"{self.api_base_path}/debug/chatters/{normalized_login}",
            ),
            message="Bot internal API returned an invalid chatters debug payload.",
            code="invalid_payload",
        )

    async def get_active_live_announcements(self) -> list[dict[str, Any]]:
        payload = await self._request_json(
            "GET",
            f"{self.api_base_path}/live/active-announcements",
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

        return self._validate_dict_payload(
            await self._request_json(
                "POST",
                f"{self.api_base_path}/live/link-click",
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
            ),
            message="Bot internal API returned an invalid live link click payload.",
        )

    async def get_raid_auth_url(
        self,
        login: str,
        discord_user_id: str | int | None = None,
    ) -> str:
        return await super().get_raid_auth_url(
            login,
            discord_user_id=discord_user_id,
        )


__all__ = ["InternalApiClient", "InternalApiClientError"]

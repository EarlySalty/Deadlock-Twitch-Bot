"""HTTP client used by standalone dashboard service to reach bot internal API."""

from __future__ import annotations

from typing import Any

from ..core.http_client import BaseInternalHttpClient, HttpClientError
from ..internal_api import INTERNAL_API_BASE_PATH, INTERNAL_TOKEN_HEADER


class BotApiClientError(HttpClientError):
    """Safe, user-facing upstream error."""


class BotApiClient(BaseInternalHttpClient):
    """Typed wrapper around the bot-internal HTTP API."""

    api_base_path = INTERNAL_API_BASE_PATH
    token_header = INTERNAL_TOKEN_HEADER
    error_type = BotApiClientError

    def _map_http_error(self, status: int, payload: Any) -> BotApiClientError:
        return self._map_http_error_common(
            status,
            payload,
            preserve_server_error_code=True,
            preserve_server_message=True,
        )

    async def dispatch_eventsub_notification(
        self,
        *,
        sub_type: str,
        payload: dict[str, Any],
        message_id: str | None = None,
    ) -> dict[str, Any]:
        response = self._validate_dict_payload(
            await self._request_json(
                "POST",
                f"{self.api_base_path}/eventsub/dispatch",
                payload={
                    "sub_type": str(sub_type or "").strip(),
                    "message_id": str(message_id or "").strip() or None,
                    "payload": payload,
                },
            ),
            message="Bot internal API returned an invalid EventSub dispatch payload.",
        )
        if response.get("ok") is False:
            raise self._make_error(
                status=503,
                code="upstream_unavailable",
                message=self._sanitize_message(
                    self._extract_error_text(response),
                    fallback="Bot internal API could not dispatch the EventSub notification.",
                ),
            )
        return response


__all__ = ["BotApiClient", "BotApiClientError"]

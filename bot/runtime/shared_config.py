"""Shared configuration values for split runtimes."""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(slots=True)
class SharedRuntimeConfig:
    """Pure config values shared across both runtimes."""

    client_id: str = ""
    client_secret: str = ""
    twitch_bot_client_id: str = ""
    twitch_bot_secret: str = ""
    required_marker_default: str | None = None

    @property
    def twitch_client_id(self) -> str:
        return self.client_id

    @twitch_client_id.setter
    def twitch_client_id(self, value: str) -> None:
        self.client_id = value

    @property
    def twitch_client_secret(self) -> str:
        return self.client_secret

    @twitch_client_secret.setter
    def twitch_client_secret(self, value: str) -> None:
        self.client_secret = value


__all__ = ["SharedRuntimeConfig"]


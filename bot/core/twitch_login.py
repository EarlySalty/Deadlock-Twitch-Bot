"""Shared Twitch login normalization helpers."""

from __future__ import annotations

import re
from urllib.parse import unquote, urlsplit

TWITCH_LOGIN_RE = re.compile(r"^[a-z0-9_]{3,25}$")
_TWITCH_HOST_SUFFIX = "twitch.tv"
_RESERVED_TWITCH_PATH_SEGMENTS = frozenset(
    {
        "clip",
        "clips",
        "dashboard",
        "directory",
        "downloads",
        "friends",
        "inventory",
        "jobs",
        "login",
        "messages",
        "p",
        "payments",
        "search",
        "settings",
        "signup",
        "subscriptions",
        "turbo",
        "videos",
        "wallet",
    }
)


def normalize_twitch_login(raw: object) -> str | None:
    """Normalize a Twitch login or Twitch profile URL to a canonical login."""
    value = unquote(str(raw or "")).strip()
    if not value:
        return None

    value = value.lstrip("@").strip()
    lowered = value.lower()
    if "://" in lowered or "twitch.tv" in lowered:
        candidate = value if "://" in value else f"https://{value}"
        try:
            parts = urlsplit(candidate)
        except Exception:
            return None
        host = str(parts.netloc or "").strip().lower()
        if host and host != _TWITCH_HOST_SUFFIX and not host.endswith(f".{_TWITCH_HOST_SUFFIX}"):
            return None
        segments = [segment for segment in (parts.path or "").split("/") if segment]
        if not segments:
            return None
        value = segments[0]
        if value.lower() in _RESERVED_TWITCH_PATH_SEGMENTS:
            return None

    value = value.strip().lstrip("@").lower()
    if not TWITCH_LOGIN_RE.fullmatch(value):
        return None
    return value

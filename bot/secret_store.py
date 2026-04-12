"""Small helpers for loading secrets from keyring or environment."""

from __future__ import annotations

import os

KEYRING_SERVICE_NAME = "DeadlockBot"


def keyring_enabled() -> bool:
    """Allow keyring on Windows by default, elsewhere only via explicit opt-in."""
    raw = str(os.getenv("DEADLOCK_ENABLE_KEYRING") or "").strip().lower()
    if raw:
        return raw in {"1", "true", "yes", "on"}
    return os.name == "nt"


def read_keyring_secret(key: str) -> str:
    """Return a trimmed secret from Windows Credential Manager when available."""
    secret_key = str(key or "").strip()
    if not secret_key:
        return ""
    if not keyring_enabled():
        return ""

    try:
        import keyring  # type: ignore
    except Exception:
        return ""

    try:
        value = keyring.get_password(KEYRING_SERVICE_NAME, secret_key)
        if not value:
            value = keyring.get_password(
                f"{secret_key}@{KEYRING_SERVICE_NAME}", secret_key
            )
    except Exception:
        return ""

    return str(value or "").strip()


def _read_env_secret(key: str) -> tuple[bool, str]:
    secret_key = str(key or "").strip()
    if not secret_key or secret_key not in os.environ:
        return False, ""
    return True, str(os.getenv(secret_key) or "").strip()


def load_secret_value(
    *keys: str,
    prefer_env: bool = False,
    allow_empty_env_override: bool = False,
) -> str:
    """Return the first matching secret from keyring or environment.

    By default keyring wins over environment variables. For runtime toggles that
    need explicit environment overrides in tests or CI, callers can opt into
    `prefer_env=True`. When `allow_empty_env_override=True`, an explicitly empty
    environment variable suppresses any keyring fallback.
    """
    if prefer_env:
        for raw_key in keys:
            found, value = _read_env_secret(raw_key)
            if not found:
                continue
            if value or allow_empty_env_override:
                return value

    for raw_key in keys:
        value = read_keyring_secret(raw_key)
        if value:
            return value

    for raw_key in keys:
        _, value = _read_env_secret(raw_key)
        if value:
            return value

    return ""

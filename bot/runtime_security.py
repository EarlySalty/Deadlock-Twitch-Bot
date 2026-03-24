"""Small runtime safety helpers shared across startup surfaces."""

from __future__ import annotations

import ipaddress


def host_without_port(raw: str | None) -> str:
    value = str(raw or "").strip()
    if not value:
        return ""
    host = value.split(",", 1)[0].strip()
    if not host:
        return ""
    if host.startswith("["):
        end = host.find("]")
        if end != -1:
            host = host[1:end]
        return host.lower().rstrip(".")

    normalized = host.lower().rstrip(".")
    if not normalized:
        return ""
    if normalized.count(":") == 1:
        host_part, port_part = normalized.rsplit(":", 1)
        if host_part and port_part.isdigit():
            return host_part
    return normalized


def is_loopback_host(raw: str | None) -> bool:
    host = host_without_port(raw)
    if not host:
        return False
    if host == "localhost":
        return True
    try:
        return ipaddress.ip_address(host).is_loopback
    except ValueError:
        return False


def require_noauth_loopback_guard(*, enabled: bool, host: str | None) -> None:
    if not enabled:
        return
    if is_loopback_host(host):
        return
    raise RuntimeError(
        "Refusing to start dashboard with no-auth on a non-loopback host "
        f"({host!r}). Bind TWITCH_DASHBOARD_HOST to localhost or disable "
        "TWITCH_DASHBOARD_NOAUTH."
    )

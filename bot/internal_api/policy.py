"""Cross-cutting policy helpers for the internal API surface."""

from __future__ import annotations

import ipaddress
from ipaddress import ip_address
import secrets
from datetime import date, datetime, time
from decimal import Decimal
from typing import Any
from urllib.parse import unquote, urlsplit
from uuid import UUID

from ..core.twitch_login import normalize_twitch_login
from .contracts import PUBLIC_WEBSITE_ONBOARDING_LOGIN


def json_default(value: Any) -> Any:
    if isinstance(value, (datetime, date, time)):
        return value.isoformat()
    if isinstance(value, Decimal):
        return float(value)
    if isinstance(value, UUID):
        return str(value)
    if isinstance(value, set):
        return list(value)
    raise TypeError(f"Object of type {value.__class__.__name__} is not JSON serializable")


def compare_internal_token(presented: str | None, expected: str | None) -> bool:
    presented_value = str(presented or "").strip()
    expected_value = str(expected or "").strip()
    if not presented_value or not expected_value:
        return False
    try:
        return secrets.compare_digest(presented_value, expected_value)
    except Exception:
        return False


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

    try:
        ip_address(normalized)
        return normalized
    except ValueError:
        pass

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


def is_loopback_origin(raw_origin: str | None) -> bool:
    origin = str(raw_origin or "").strip()
    if not origin:
        return True
    try:
        parsed = urlsplit(origin)
    except Exception:
        return False
    if parsed.scheme not in {"http", "https"} or not parsed.netloc:
        return False
    if parsed.username or parsed.password:
        return False
    return is_loopback_host(parsed.hostname)


def request_peer_host(request: Any) -> str:
    remote = str(getattr(request, "remote", "") or "").strip()
    if remote:
        return remote
    transport = getattr(request, "transport", None)
    if transport is None:
        return ""
    peer = transport.get_extra_info("peername")
    if isinstance(peer, tuple) and peer:
        return str(peer[0]).strip()
    if isinstance(peer, str):
        return peer.strip()
    return ""


def is_trusted_proxy_host(
    raw: str | None,
    *,
    trusted_proxy_networks: tuple[ipaddress._BaseNetwork, ...],
) -> bool:
    host = host_without_port(raw)
    if not host:
        return False
    try:
        address = ipaddress.ip_address(host)
    except ValueError:
        return False
    return any(address in network for network in trusted_proxy_networks)


def forwarded_client_host(*, forwarded_for: str | None, x_real_ip: str | None = None) -> str:
    for candidate in str(forwarded_for or "").split(","):
        host = host_without_port(candidate)
        if host:
            return host
    return host_without_port(x_real_ip)


def effective_client_host(
    *,
    peer_host: str | None,
    forwarded_for: str | None,
    x_real_ip: str | None,
    trusted_proxy_networks: tuple[ipaddress._BaseNetwork, ...],
) -> str:
    if is_trusted_proxy_host(peer_host, trusted_proxy_networks=trusted_proxy_networks):
        forwarded_host = forwarded_client_host(forwarded_for=forwarded_for, x_real_ip=x_real_ip)
        if forwarded_host:
            return forwarded_host
    return host_without_port(peer_host)


def is_loopback_request(
    *,
    request_host: str | None,
    peer_host: str | None,
    forwarded_for: str | None = None,
    x_real_ip: str | None = None,
    trusted_proxy_networks: tuple[ipaddress._BaseNetwork, ...] = (),
) -> bool:
    if not is_loopback_host(request_host):
        return False
    client_host = effective_client_host(
        peer_host=peer_host,
        forwarded_for=forwarded_for,
        x_real_ip=x_real_ip,
        trusted_proxy_networks=trusted_proxy_networks,
    )
    return is_loopback_host(client_host)


def is_secure_request(
    *,
    peer_host: str | None,
    forwarded_proto: str | None,
    request_secure: bool,
    trusted_proxy_networks: tuple[ipaddress._BaseNetwork, ...],
) -> bool:
    if is_trusted_proxy_host(peer_host, trusted_proxy_networks=trusted_proxy_networks):
        forwarded_value = str(forwarded_proto or "").split(",", 1)[0].strip().lower()
        if forwarded_value:
            return forwarded_value == "https"
    return bool(request_secure)


def parse_allowlist_ids(
    raw: str | None,
    *,
    env_name: str | None = None,
    logger: Any | None = None,
) -> set[int] | None:
    if raw is None:
        return None

    value = str(raw).strip()
    allowed: set[int] = set()
    for token in value.replace(";", ",").split(","):
        item = token.strip()
        if not item:
            continue
        if not item.isdigit():
            if logger is not None and env_name:
                logger.warning("Ignoring invalid %s entry: %r", env_name, item)
            continue
        parsed = int(item)
        if parsed > 0:
            allowed.add(parsed)
    if not allowed and logger is not None and env_name:
        logger.warning(
            "%s configured but no valid positive IDs parsed; enabling fail-closed deny-all.",
            env_name,
        )
    return allowed


def coerce_optional_positive_int(value: Any, *, key: str) -> int | None:
    if value is None:
        return None
    if isinstance(value, bool):
        raise ValueError(f"{key} must be a positive integer")
    if isinstance(value, int):
        parsed = value
    elif isinstance(value, str):
        item = value.strip()
        if not item:
            return None
        if not item.isdigit():
            raise ValueError(f"{key} must be a positive integer")
        parsed = int(item)
    else:
        raise ValueError(f"{key} must be a positive integer")
    if parsed <= 0:
        raise ValueError(f"{key} must be a positive integer")
    return parsed


def parse_optional_int(value: str | None, *, minimum: int | None = None) -> int | None:
    raw = (value or "").strip()
    if not raw:
        return None
    try:
        parsed = int(raw)
    except ValueError as exc:
        raise ValueError("invalid integer parameter") from exc
    if minimum is not None and parsed < minimum:
        raise ValueError("integer parameter below minimum")
    return parsed


def normalize_login(raw: str) -> str | None:
    return normalize_twitch_login(raw)


def normalize_raid_auth_target(raw: str) -> str | None:
    value = unquote(str(raw or "")).strip()
    if not value:
        return None

    lowered = value.lower()
    if lowered == PUBLIC_WEBSITE_ONBOARDING_LOGIN:
        return lowered
    if lowered.startswith("discord:"):
        discord_id = lowered.split(":", 1)[1].strip()
        if discord_id.isdigit():
            return f"discord:{discord_id}"
        return None

    return normalize_login(value)


def parse_bool(value: Any, *, default: bool = False) -> bool:
    if value is None:
        return default
    if isinstance(value, bool):
        return value
    if isinstance(value, (int, float)):
        return bool(value)
    lowered = str(value).strip().lower()
    if not lowered:
        return default
    if lowered in {"1", "true", "yes", "on"}:
        return True
    if lowered in {"0", "false", "no", "off"}:
        return False
    return default


def normalize_discord_user_id(value: str | None, *, required: bool) -> str | None:
    raw = str(value or "").strip()
    if not raw:
        if required:
            raise ValueError("invalid discord_user_id")
        return None
    if not raw.isdigit():
        raise ValueError("invalid discord_user_id")
    return raw


def normalize_tracking_token(value: Any, *, required: bool) -> str | None:
    text = str(value or "").strip()
    if not text:
        if required:
            raise ValueError("invalid tracking_token")
        return None
    if len(text) > 128:
        raise ValueError("invalid tracking_token")
    return text


def normalize_text_field(
    value: Any,
    *,
    field_name: str,
    required: bool,
    max_length: int,
) -> str | None:
    text = str(value or "").replace("\r", " ").replace("\n", " ").strip()
    if not text:
        if required:
            raise ValueError(f"invalid {field_name}")
        return None
    if len(text) > max_length:
        raise ValueError(f"invalid {field_name}")
    return text


def sanitize_log_value(value: Any) -> str:
    text = "" if value is None else str(value)
    return text.replace("\r", "\\r").replace("\n", "\\n")


def safe_bad_request_detail(exc: Exception) -> str:
    text = str(exc or "").replace("\r", " ").replace("\n", " ").strip()
    if not text:
        return ""
    if len(text) > 120:
        return ""
    lowered = text.lower()
    if "://" in text:
        return ""
    if any(
        token in lowered
        for token in (
            "token",
            "secret",
            "password",
            "authorization",
            "bearer",
            "cookie",
            "session",
            "dsn",
        )
    ):
        return ""
    if "=" in text:
        return ""
    return text


def normalize_live_announcement_item(item: Any) -> dict[str, Any]:
    if not isinstance(item, dict):
        raise ValueError("active announcement item must be an object")

    streamer_login = normalize_login(str(item.get("streamer_login") or ""))
    if not streamer_login:
        raise ValueError("active announcement streamer_login is invalid")

    message_id = coerce_optional_positive_int(item.get("message_id"), key="message_id")
    if message_id is None:
        raise ValueError("active announcement message_id is invalid")

    channel_id = coerce_optional_positive_int(item.get("channel_id"), key="channel_id")
    if channel_id is None:
        raise ValueError("active announcement channel_id is invalid")

    tracking_token = normalize_tracking_token(item.get("tracking_token"), required=True)
    referral_url = normalize_text_field(
        item.get("referral_url"),
        field_name="referral_url",
        required=True,
        max_length=2000,
    )
    parsed_url = urlsplit(str(referral_url))
    if parsed_url.scheme not in {"http", "https"} or not parsed_url.netloc:
        raise ValueError("active announcement referral_url is invalid")

    button_label = normalize_text_field(
        item.get("button_label"),
        field_name="button_label",
        required=True,
        max_length=80,
    )

    return {
        "streamer_login": streamer_login,
        "message_id": int(message_id),
        "tracking_token": str(tracking_token),
        "referral_url": str(referral_url),
        "button_label": str(button_label),
        "channel_id": int(channel_id),
    }


def normalize_raid_state_payload(
    payload: Any,
    *,
    discord_user_id: str | None,
    twitch_login: str | None,
) -> dict[str, Any]:
    source = payload if isinstance(payload, dict) else {}
    normalized_discord_id = normalize_discord_user_id(
        source.get("discord_user_id"),
        required=False,
    )
    normalized_login = normalize_login(str(source.get("twitch_login") or ""))
    normalized_twitch_user_id = str(source.get("twitch_user_id") or "").strip() or None
    partner_opt_out = parse_bool(source.get("partner_opt_out"), default=False)
    token_blacklisted = parse_bool(source.get("token_blacklisted"), default=False)
    raid_blacklisted = parse_bool(source.get("raid_blacklisted"), default=False)
    blocked_default = partner_opt_out or token_blacklisted or raid_blacklisted
    return {
        "discord_user_id": normalized_discord_id or discord_user_id,
        "twitch_login": normalized_login or twitch_login,
        "twitch_user_id": normalized_twitch_user_id,
        "authorized": parse_bool(source.get("authorized"), default=False),
        "partner_opt_out": partner_opt_out,
        "token_blacklisted": token_blacklisted,
        "raid_blacklisted": raid_blacklisted,
        "blocked": parse_bool(source.get("blocked"), default=blocked_default),
    }


__all__ = [
    "coerce_optional_positive_int",
    "compare_internal_token",
    "effective_client_host",
    "forwarded_client_host",
    "host_without_port",
    "is_loopback_host",
    "is_loopback_origin",
    "is_loopback_request",
    "is_secure_request",
    "is_trusted_proxy_host",
    "json_default",
    "normalize_discord_user_id",
    "normalize_live_announcement_item",
    "normalize_login",
    "normalize_raid_auth_target",
    "normalize_raid_state_payload",
    "normalize_text_field",
    "normalize_tracking_token",
    "parse_allowlist_ids",
    "parse_bool",
    "parse_optional_int",
    "request_peer_host",
    "sanitize_log_value",
    "safe_bad_request_detail",
]

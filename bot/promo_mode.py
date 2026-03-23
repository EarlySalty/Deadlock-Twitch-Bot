"""Global admin-managed override for Twitch chat promo announcements."""

from __future__ import annotations

import string
from datetime import UTC, datetime
from typing import Any

PROMO_MODE_STANDARD = "standard"
PROMO_MODE_CUSTOM_EVENT = "custom_event"
PROMO_MODE_ALLOWED: frozenset[str] = frozenset(
    {PROMO_MODE_STANDARD, PROMO_MODE_CUSTOM_EVENT}
)
PROMO_MODE_SINGLETON_KEY = "global"
PROMO_MODE_ALLOWED_PLACEHOLDERS: frozenset[str] = frozenset({"invite"})
STREAMER_PROMO_MESSAGE_MAX_LENGTH = 500


def _issue(field: str, message: str, *, code: str = "") -> dict[str, str]:
    issue = {"field": field, "message": message}
    if code:
        issue["code"] = code
    return issue


def _row_value(row: Any, key: str, index: int, default: Any = None) -> Any:
    if row is None:
        return default
    if hasattr(row, "get"):
        return row.get(key, default)
    values = tuple(row)
    return values[index] if index < len(values) else default


def _coerce_bool(value: Any) -> bool:
    if isinstance(value, str):
        return value.strip().lower() not in {"", "0", "false", "off", "no"}
    return bool(value)


def parse_utc_datetime(raw_value: Any) -> datetime | None:
    if raw_value is None:
        return None
    if isinstance(raw_value, datetime):
        parsed = raw_value
    else:
        text = str(raw_value or "").strip()
        if not text:
            return None
        normalized = text.replace("Z", "+00:00")
        try:
            parsed = datetime.fromisoformat(normalized)
        except ValueError:
            return None
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=UTC)
    return parsed.astimezone(UTC)


def to_iso_utc(raw_value: Any) -> str | None:
    parsed = parse_utc_datetime(raw_value)
    if parsed is None:
        return None
    return parsed.isoformat(timespec="seconds")


def format_datetime_local_utc(raw_value: Any) -> str:
    parsed = parse_utc_datetime(raw_value)
    if parsed is None:
        return ""
    return parsed.strftime("%Y-%m-%dT%H:%M")


def default_global_promo_mode_config() -> dict[str, Any]:
    return {
        "mode": PROMO_MODE_STANDARD,
        "custom_message": "",
        "starts_at": None,
        "ends_at": None,
        "is_enabled": False,
        "updated_at": None,
        "updated_by": "",
    }


def normalize_global_promo_mode_config(raw_config: Any) -> dict[str, Any]:
    config = default_global_promo_mode_config()
    if not isinstance(raw_config, dict):
        return config

    raw_mode = str(raw_config.get("mode") or "").strip().lower()
    config["mode"] = raw_mode if raw_mode in PROMO_MODE_ALLOWED else PROMO_MODE_STANDARD
    config["custom_message"] = str(raw_config.get("custom_message") or "").strip()
    config["starts_at"] = to_iso_utc(raw_config.get("starts_at"))
    config["ends_at"] = to_iso_utc(raw_config.get("ends_at"))
    config["is_enabled"] = _coerce_bool(raw_config.get("is_enabled"))
    config["updated_at"] = to_iso_utc(raw_config.get("updated_at"))
    config["updated_by"] = str(raw_config.get("updated_by") or "").strip()
    return config


def _validate_template_placeholders(
    text: str,
    *,
    field: str,
    invalid_message: str,
    unsupported_message_prefix: str,
) -> tuple[list[dict[str, str]], set[str]]:
    issues: list[dict[str, str]] = []
    used_fields: set[str] = set()
    formatter = string.Formatter()
    try:
        parts = tuple(formatter.parse(text))
    except ValueError:
        return [_issue(field, invalid_message, code="invalid_placeholder")], set()

    for _literal, field_name, _format_spec, _conversion in parts:
        if field_name is None:
            continue
        root = str(field_name or "").strip().split(".", 1)[0].split("[", 1)[0]
        if not root:
            issues.append(_issue(field, invalid_message, code="invalid_placeholder"))
            continue
        used_fields.add(root)
        if root not in PROMO_MODE_ALLOWED_PLACEHOLDERS:
            issues.append(
                _issue(
                    field,
                    f"{unsupported_message_prefix} {{{root}}}. Erlaubt ist aktuell nur {{invite}}.",
                    code="invalid_placeholder",
                )
            )
    return issues, used_fields


def validate_custom_promo_message(message: Any) -> list[dict[str, str]]:
    text = str(message or "").strip()
    if not text:
        return [
            _issue(
                "custom_message",
                "Bitte einen Event-Text hinterlegen.",
                code="empty",
            )
        ]

    issues, _used_fields = _validate_template_placeholders(
        text,
        field="custom_message",
        invalid_message="Ungültiger Platzhalter im Event-Text.",
        unsupported_message_prefix="Nicht unterstützter Platzhalter",
    )
    return issues


def validate_streamer_promo_message(message: Any) -> list[dict[str, str]]:
    text = str(message or "").strip()
    if not text:
        return []

    issues: list[dict[str, str]] = []
    if len(text) > STREAMER_PROMO_MESSAGE_MAX_LENGTH:
        issues.append(
            _issue(
                "promo_message",
                (
                    "Die Promo-Nachricht darf maximal "
                    f"{STREAMER_PROMO_MESSAGE_MAX_LENGTH} Zeichen lang sein."
                ),
                code="too_long",
            )
        )

    placeholder_issues, used_fields = _validate_template_placeholders(
        text,
        field="promo_message",
        invalid_message="Ungültiger Platzhalter in der Promo-Nachricht.",
        unsupported_message_prefix="Nicht unterstützter Platzhalter",
    )
    issues.extend(placeholder_issues)

    if "invite" not in used_fields:
        issues.append(
            _issue(
                "promo_message",
                "Die Promo-Nachricht muss den Platzhalter {invite} enthalten.",
                code="missing_invite",
            )
        )

    return issues


def validate_global_promo_mode_config(raw_config: Any) -> tuple[dict[str, Any], list[dict[str, str]]]:
    config = normalize_global_promo_mode_config(raw_config)
    issues: list[dict[str, str]] = []

    raw_mode = str((raw_config or {}).get("mode") or "").strip().lower() if isinstance(raw_config, dict) else ""
    if raw_mode and raw_mode not in PROMO_MODE_ALLOWED:
        issues.append(
            {"field": "mode", "message": "Unbekannter Modus. Erlaubt sind standard und custom_event."}
        )

    starts_at_raw = (raw_config or {}).get("starts_at") if isinstance(raw_config, dict) else None
    ends_at_raw = (raw_config or {}).get("ends_at") if isinstance(raw_config, dict) else None
    if starts_at_raw not in (None, "") and parse_utc_datetime(starts_at_raw) is None:
        issues.append(
            {"field": "starts_at", "message": "Startzeit ist ungültig. Bitte UTC-ISO oder datetime-local senden."}
        )
    if ends_at_raw not in (None, "") and parse_utc_datetime(ends_at_raw) is None:
        issues.append(
            {"field": "ends_at", "message": "Endzeit ist ungültig. Bitte UTC-ISO oder datetime-local senden."}
        )

    starts_at = parse_utc_datetime(config.get("starts_at"))
    ends_at = parse_utc_datetime(config.get("ends_at"))
    if starts_at and ends_at and ends_at < starts_at:
        issues.append(
            {
                "field": "ends_at",
                "message": "Endzeit muss nach der Startzeit liegen.",
            }
        )

    if config["mode"] == PROMO_MODE_CUSTOM_EVENT:
        issues.extend(validate_custom_promo_message(config.get("custom_message")))

    return config, issues


def evaluate_global_promo_mode(
    raw_config: Any,
    *,
    now: datetime | None = None,
) -> dict[str, Any]:
    config = normalize_global_promo_mode_config(raw_config)
    now_utc = (now or datetime.now(UTC)).astimezone(UTC)
    starts_at = parse_utc_datetime(config.get("starts_at"))
    ends_at = parse_utc_datetime(config.get("ends_at"))

    status = "standard"
    is_active = False
    reason = "standard_mode"
    active_message = None

    if config["mode"] != PROMO_MODE_CUSTOM_EVENT:
        status = "standard"
        reason = "standard_mode"
    elif not config["is_enabled"]:
        status = "disabled"
        reason = "disabled"
    elif starts_at and now_utc < starts_at:
        status = "scheduled"
        reason = "before_start"
    elif ends_at and now_utc > ends_at:
        status = "expired"
        reason = "after_end"
    elif validate_custom_promo_message(config.get("custom_message")):
        status = "invalid"
        reason = "invalid_message"
    else:
        status = "active"
        reason = "active_custom_event"
        is_active = True
        active_message = str(config.get("custom_message") or "").strip()

    return {
        "config": config,
        "status": status,
        "reason": reason,
        "is_active": is_active,
        "active_message": active_message,
        "starts_at": starts_at.isoformat(timespec="seconds") if starts_at else None,
        "ends_at": ends_at.isoformat(timespec="seconds") if ends_at else None,
        "now": now_utc.isoformat(timespec="seconds"),
    }


def ensure_global_promo_mode_storage(conn: Any) -> None:
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_global_promo_modes (
            config_key TEXT PRIMARY KEY,
            mode TEXT NOT NULL DEFAULT 'standard',
            custom_message TEXT,
            starts_at TEXT,
            ends_at TEXT,
            is_enabled INTEGER NOT NULL DEFAULT 0,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_by TEXT
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_global_promo_modes_updated_at "
        "ON twitch_global_promo_modes(updated_at)"
    )


def load_global_promo_mode(conn: Any) -> dict[str, Any]:
    ensure_global_promo_mode_storage(conn)
    row = conn.execute(
        """
        SELECT mode, custom_message, starts_at, ends_at, is_enabled, updated_at, updated_by
          FROM twitch_global_promo_modes
         WHERE config_key = %s
         LIMIT 1
        """,
        (PROMO_MODE_SINGLETON_KEY,),
    ).fetchone()
    if not row:
        return default_global_promo_mode_config()
    return normalize_global_promo_mode_config(
        {
            "mode": _row_value(row, "mode", 0, PROMO_MODE_STANDARD),
            "custom_message": _row_value(row, "custom_message", 1, ""),
            "starts_at": _row_value(row, "starts_at", 2, None),
            "ends_at": _row_value(row, "ends_at", 3, None),
            "is_enabled": _row_value(row, "is_enabled", 4, 0),
            "updated_at": _row_value(row, "updated_at", 5, None),
            "updated_by": _row_value(row, "updated_by", 6, ""),
        }
    )


def save_global_promo_mode(
    conn: Any,
    *,
    config: Any,
    updated_by: str,
) -> dict[str, Any]:
    ensure_global_promo_mode_storage(conn)
    normalized, issues = validate_global_promo_mode_config(config)
    if issues:
        raise ValueError(issues[0]["message"])

    updated_at = datetime.now(UTC).isoformat(timespec="seconds")
    conn.execute(
        """
        INSERT INTO twitch_global_promo_modes (
            config_key, mode, custom_message, starts_at, ends_at, is_enabled, updated_at, updated_by
        ) VALUES (%s, %s, %s, %s, %s, %s, %s, %s)
        ON CONFLICT (config_key) DO UPDATE SET
            mode = EXCLUDED.mode,
            custom_message = EXCLUDED.custom_message,
            starts_at = EXCLUDED.starts_at,
            ends_at = EXCLUDED.ends_at,
            is_enabled = EXCLUDED.is_enabled,
            updated_at = EXCLUDED.updated_at,
            updated_by = EXCLUDED.updated_by
        """,
        (
            PROMO_MODE_SINGLETON_KEY,
            normalized["mode"],
            normalized["custom_message"] or None,
            normalized["starts_at"],
            normalized["ends_at"],
            1 if normalized["is_enabled"] else 0,
            updated_at,
            str(updated_by or "").strip() or None,
        ),
    )
    return normalize_global_promo_mode_config(
        {
            **normalized,
            "updated_at": updated_at,
            "updated_by": str(updated_by or "").strip(),
        }
    )

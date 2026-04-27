"""Key/Value-Settings fuer das Social-Media-Modul.

Persistiert in Tabelle `social_media_settings (key TEXT PK, value JSONB)`.
Ein zentraler Schalter ist `external_llm_consent`: ohne explizites `true` werden
Daten *nie* an einen externen LLM-Provider geschickt; der Dispatcher faellt auf
das lokale Ollama zurueck oder markiert den Clip als `skipped_no_local_llm`.
"""

from __future__ import annotations

import json
import logging
from typing import Any

from ..storage import readonly_connection, transaction

log = logging.getLogger("TwitchStreams.SocialMedia.Settings")

KEY_EXTERNAL_LLM_CONSENT = "external_llm_consent"
KEY_AUTO_APPROVE_YOUTUBE = "auto_approve_youtube"
KEY_AUTO_APPROVE_TIKTOK = "auto_approve_tiktok"
KEY_AUTO_APPROVE_INSTAGRAM = "auto_approve_instagram"


def _decode_json(raw: Any) -> Any:
    if raw is None:
        return None
    if isinstance(raw, (dict, list, bool, int, float)):
        return raw
    if isinstance(raw, (bytes, bytearray)):
        try:
            return json.loads(raw.decode("utf-8"))
        except Exception:
            return None
    if isinstance(raw, str):
        if not raw:
            return None
        try:
            return json.loads(raw)
        except Exception:
            return raw
    return raw


def get_setting(key: str, default: Any = None) -> Any:
    if not key:
        return default
    try:
        with readonly_connection() as conn:
            row = conn.execute(
                "SELECT value FROM social_media_settings WHERE key = %s",
                (key,),
            ).fetchone()
    except Exception:
        log.exception("Failed to read social_media_settings (key=%s)", key)
        return default
    if not row:
        return default
    raw = row["value"] if hasattr(row, "keys") else row[0]
    decoded = _decode_json(raw)
    return default if decoded is None else decoded


def set_setting(key: str, value: Any, *, updated_by: str | None = None) -> None:
    if not key:
        raise ValueError("key is required")
    payload = json.dumps(value)
    with transaction() as conn:
        conn.execute(
            """
            INSERT INTO social_media_settings (key, value, updated_at, updated_by)
            VALUES (%s, %s, CURRENT_TIMESTAMP, %s)
            ON CONFLICT (key) DO UPDATE
                SET value = EXCLUDED.value,
                    updated_at = CURRENT_TIMESTAMP,
                    updated_by = EXCLUDED.updated_by
            """,
            (key, payload, str(updated_by).strip() if updated_by else None),
        )


def external_llm_consent() -> bool:
    """True wenn der Admin explizit `external_llm_consent=true` gesetzt hat."""
    value = get_setting(KEY_EXTERNAL_LLM_CONSENT, default=False)
    return _coerce_bool(value)


def _coerce_bool(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if isinstance(value, str):
        return value.strip().lower() in {"true", "1", "yes", "on"}
    if isinstance(value, (int, float)):
        return bool(value)
    return False


def get_auto_approve_settings() -> dict[str, bool]:
    return {
        "youtube": _coerce_bool(get_setting(KEY_AUTO_APPROVE_YOUTUBE, default=False)),
        "tiktok": _coerce_bool(get_setting(KEY_AUTO_APPROVE_TIKTOK, default=False)),
        "instagram": _coerce_bool(get_setting(KEY_AUTO_APPROVE_INSTAGRAM, default=False)),
    }


def set_auto_approve_settings(
    values: dict[str, Any],
    *,
    updated_by: str | None = None,
) -> dict[str, bool]:
    normalized = {
        "youtube": _coerce_bool(values.get("youtube", False)),
        "tiktok": _coerce_bool(values.get("tiktok", False)),
        "instagram": _coerce_bool(values.get("instagram", False)),
    }
    set_setting(KEY_AUTO_APPROVE_YOUTUBE, normalized["youtube"], updated_by=updated_by)
    set_setting(KEY_AUTO_APPROVE_TIKTOK, normalized["tiktok"], updated_by=updated_by)
    set_setting(KEY_AUTO_APPROVE_INSTAGRAM, normalized["instagram"], updated_by=updated_by)
    return normalized

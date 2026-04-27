from __future__ import annotations

import json
from typing import Any

from ...storage import readonly_connection, transaction
from . import DEFAULT_STREAMER_LAYOUT, StreamerLayout


def get_streamer_layout(login: str) -> StreamerLayout | None:
    normalized_login = str(login or "").strip().lower()
    if not normalized_login:
        return None
    with readonly_connection() as conn:
        row = conn.execute(
            """
            SELECT layout_json, cam_enabled, mode
              FROM social_media_streamer_layout
             WHERE LOWER(streamer_login) = LOWER(%s)
             LIMIT 1
            """,
            (normalized_login,),
        ).fetchone()
    if not row:
        return None
    payload = _decode_json(row["layout_json"] if hasattr(row, "keys") else row[0])
    cam_enabled = row["cam_enabled"] if hasattr(row, "keys") else row[1]
    mode = row["mode"] if hasattr(row, "keys") else row[2]
    return StreamerLayout.from_mapping(payload, cam_enabled=bool(cam_enabled), mode=str(mode or "pip"))


def upsert_streamer_layout(
    login: str,
    layout: StreamerLayout,
    *,
    updated_by: str | None,
) -> None:
    normalized_login = str(login or "").strip().lower()
    if not normalized_login:
        raise ValueError("login is required")
    with transaction() as conn:
        conn.execute(
            """
            INSERT INTO social_media_streamer_layout (
                streamer_login,
                layout_json,
                cam_enabled,
                mode,
                updated_at,
                updated_by
            )
            VALUES (%s, %s, %s, %s, CURRENT_TIMESTAMP, %s)
            ON CONFLICT (streamer_login) DO UPDATE
            SET layout_json = EXCLUDED.layout_json,
                cam_enabled = EXCLUDED.cam_enabled,
                mode = EXCLUDED.mode,
                updated_at = CURRENT_TIMESTAMP,
                updated_by = EXCLUDED.updated_by
            """,
            (
                normalized_login,
                json.dumps(layout.to_layout_json()),
                layout.cam_enabled,
                layout.mode,
                str(updated_by).strip() or None,
            ),
        )


def get_clip_effective_layout(clip_db_id: int) -> StreamerLayout:
    with readonly_connection() as conn:
        row = conn.execute(
            """
            SELECT c.layout_override_json,
                   c.streamer_login,
                   l.layout_json AS streamer_layout_json,
                   l.cam_enabled AS streamer_cam_enabled,
                   l.mode AS streamer_mode
              FROM twitch_clips_social_media c
              LEFT JOIN social_media_streamer_layout l
                ON LOWER(l.streamer_login) = LOWER(c.streamer_login)
             WHERE c.id = %s
             LIMIT 1
            """,
            (clip_db_id,),
        ).fetchone()
    if not row:
        return DEFAULT_STREAMER_LAYOUT

    override_json = row["layout_override_json"] if hasattr(row, "keys") else row[0]
    if override_json:
        override_payload = _decode_json(override_json)
        return StreamerLayout.from_mapping(override_payload)

    streamer_payload = row["streamer_layout_json"] if hasattr(row, "keys") else row[2]
    if streamer_payload:
        cam_enabled = row["streamer_cam_enabled"] if hasattr(row, "keys") else row[3]
        mode = row["streamer_mode"] if hasattr(row, "keys") else row[4]
        return StreamerLayout.from_mapping(
            _decode_json(streamer_payload),
            cam_enabled=bool(cam_enabled),
            mode=str(mode or "pip"),
        )

    return DEFAULT_STREAMER_LAYOUT


def set_clip_layout_override(clip_db_id: int, layout: StreamerLayout | None) -> None:
    payload = json.dumps(layout.to_override_json()) if layout else None
    with transaction() as conn:
        conn.execute(
            """
            UPDATE twitch_clips_social_media
               SET layout_override_json = %s
             WHERE id = %s
            """,
            (payload, clip_db_id),
        )


def apply_default_layout(clip_db_id: int, streamer_login: str) -> None:
    layout = get_streamer_layout(streamer_login) or DEFAULT_STREAMER_LAYOUT
    with transaction() as conn:
        conn.execute(
            """
            UPDATE twitch_clips_social_media
               SET layout_override_json = COALESCE(layout_override_json, %s)
             WHERE id = %s
            """,
            (json.dumps(layout.to_override_json()), clip_db_id),
        )


def _decode_json(value: Any) -> dict[str, Any]:
    if isinstance(value, dict):
        return value
    if isinstance(value, str):
        return json.loads(value)
    if isinstance(value, (bytes, bytearray, memoryview)):
        return json.loads(bytes(value).decode("utf-8"))
    raise ValueError("unsupported JSON payload")

from __future__ import annotations

import logging
from collections.abc import Iterable

from ..storage import readonly_connection, transaction

log = logging.getLogger("TwitchStreams.SocialMediaRetention")

_PLATFORM_UPLOAD_COLUMNS = {
    "tiktok": "uploaded_tiktok",
    "youtube": "uploaded_youtube",
    "instagram": "uploaded_instagram",
}


def get_active_platforms_for_streamer(streamer_login: str | None) -> set[str]:
    normalized_login = str(streamer_login or "").strip().lower()
    with readonly_connection() as conn:
        rows = conn.execute(
            """
            SELECT DISTINCT platform
              FROM social_media_platform_auth
             WHERE enabled = 1
               AND (
                    LOWER(COALESCE(streamer_login, '')) = LOWER(%s)
                    OR streamer_login IS NULL
               )
            """,
            (normalized_login,),
        ).fetchall()
    return {
        str((row["platform"] if hasattr(row, "keys") else row[0]) or "").strip().lower()
        for row in rows
        if str((row["platform"] if hasattr(row, "keys") else row[0]) or "").strip().lower()
        in _PLATFORM_UPLOAD_COLUMNS
    }


def is_clip_published_on_all_active_platforms(clip_db_id: int) -> bool:
    with readonly_connection() as conn:
        row = conn.execute(
            """
            SELECT streamer_login, uploaded_tiktok, uploaded_youtube, uploaded_instagram
              FROM twitch_clips_social_media
             WHERE id = %s
             LIMIT 1
            """,
            (clip_db_id,),
        ).fetchone()
    if not row:
        return False

    streamer_login = row["streamer_login"] if hasattr(row, "keys") else row[0]
    active_platforms = get_active_platforms_for_streamer(streamer_login)
    if not active_platforms:
        return True

    for platform in active_platforms:
        column = _PLATFORM_UPLOAD_COLUMNS[platform]
        value = row[column] if hasattr(row, "keys") else None
        if not bool(value):
            return False
    return True


def refresh_clip_publication_status(clip_db_id: int) -> bool:
    published_all = is_clip_published_on_all_active_platforms(clip_db_id)
    with transaction() as conn:
        if published_all:
            conn.execute(
                """
                UPDATE twitch_clips_social_media
                   SET status = 'published_all'
                 WHERE id = %s
                   AND discarded_at IS NULL
                """,
                (clip_db_id,),
            )
        else:
            conn.execute(
                """
                UPDATE twitch_clips_social_media
                   SET status = CASE WHEN discarded_at IS NOT NULL THEN status ELSE 'pending' END
                 WHERE id = %s
                   AND status = 'published_all'
                """,
                (clip_db_id,),
            )
    return published_all


def mark_clip_discarded(clip_db_id: int) -> bool:
    with transaction() as conn:
        row = conn.execute(
            """
            UPDATE twitch_clips_social_media
               SET discarded_at = CURRENT_TIMESTAMP,
                   status = 'discarded'
             WHERE id = %s
         RETURNING id
            """,
            (clip_db_id,),
        ).fetchone()
    return bool(row)


def iter_expired_clips_for_retention(now_value) -> list[dict]:
    query_value = now_value.isoformat() if hasattr(now_value, "isoformat") else now_value
    with readonly_connection() as conn:
        rows = conn.execute(
            """
            SELECT id, clip_id, streamer_login, source_kind, upload_local_path, local_file_path,
                   retention_until, discarded_at, status
              FROM twitch_clips_social_media
             WHERE retention_until IS NOT NULL
               AND retention_until <= %s
             ORDER BY retention_until ASC, id ASC
            """,
            (query_value,),
        ).fetchall()
    return [dict(row) for row in rows]


def delete_clips_by_ids(clip_ids: Iterable[int]) -> None:
    clip_id_list = [int(clip_id) for clip_id in clip_ids]
    if not clip_id_list:
        return
    with transaction() as conn:
        for clip_id in clip_id_list:
            conn.execute("DELETE FROM twitch_clips_social_media WHERE id = %s", (clip_id,))

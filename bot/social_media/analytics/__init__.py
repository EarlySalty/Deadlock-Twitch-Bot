from __future__ import annotations

from dataclasses import dataclass
from datetime import UTC, datetime
from decimal import Decimal
from typing import Any

from ...storage import readonly_connection, transaction

BUCKETS: tuple[str, ...] = ("24h", "7d", "30d")
PLATFORMS: tuple[str, ...] = ("youtube", "tiktok", "instagram")


def utcnow() -> datetime:
    return datetime.now(UTC)


@dataclass(frozen=True)
class ClipAnalyticsSnapshot:
    clip_db_id: int
    platform: str
    bucket: str
    views: int = 0
    likes: int = 0
    comments: int = 0
    shares: int = 0
    watch_time_seconds: int | None = None
    ctr_percent: float | None = None
    engagement_rate: float | None = None
    provider: str | None = None
    synced_at: str | None = None
    next_pull_at: str | None = None


@dataclass(frozen=True)
class SocialMediaReportRecord:
    id: int
    kind: str
    streamer_login: str | None
    period_start: str
    period_end: str
    content_md: str
    model: str | None
    created_at: str | None


def list_clip_analytics(clip_db_id: int) -> list[ClipAnalyticsSnapshot]:
    with readonly_connection() as conn:
        rows = conn.execute(
            """
            SELECT clip_id, platform, bucket, views, likes, comments, shares,
                   watch_time_seconds, ctr_percent, engagement_rate, provider,
                   synced_at, next_pull_at
              FROM twitch_clips_social_analytics
             WHERE clip_id = %s
             ORDER BY platform ASC, bucket ASC
            """,
            (clip_db_id,),
        ).fetchall()
    return [_row_to_clip_analytics(row) for row in rows]


def upsert_clip_analytics(
    *,
    clip_db_id: int,
    platform: str,
    bucket: str,
    views: int = 0,
    likes: int = 0,
    comments: int = 0,
    shares: int = 0,
    watch_time_seconds: int | None = None,
    ctr_percent: float | None = None,
    engagement_rate: float | None = None,
    provider: str | None = None,
    synced_at: datetime | str | None = None,
    next_pull_at: datetime | str | None = None,
) -> ClipAnalyticsSnapshot:
    synced_value = _encode_ts(synced_at or utcnow())
    next_pull_value = _encode_ts(next_pull_at)
    with transaction() as conn:
        updated = conn.execute(
            """
            UPDATE twitch_clips_social_analytics
               SET views = %s,
                   likes = %s,
                   comments = %s,
                   shares = %s,
                   watch_time_seconds = %s,
                   ctr_percent = %s,
                   engagement_rate = %s,
                   provider = %s,
                   synced_at = %s,
                   next_pull_at = %s
             WHERE clip_id = %s
               AND platform = %s
               AND bucket = %s
            """,
            (
                int(views or 0),
                int(likes or 0),
                int(comments or 0),
                int(shares or 0),
                int(watch_time_seconds) if watch_time_seconds is not None else None,
                _round_metric(ctr_percent),
                _round_metric(engagement_rate),
                str(provider).strip() if provider else None,
                synced_value,
                next_pull_value,
                clip_db_id,
                platform,
                bucket,
            ),
        )
        if getattr(updated, "rowcount", 0) == 0:
            conn.execute(
                """
                INSERT INTO twitch_clips_social_analytics (
                    clip_id, platform, bucket, views, likes, comments, shares,
                    watch_time_seconds, ctr_percent, engagement_rate, provider,
                    synced_at, next_pull_at
                )
                VALUES (%s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s)
                """,
                (
                    clip_db_id,
                    platform,
                    bucket,
                    int(views or 0),
                    int(likes or 0),
                    int(comments or 0),
                    int(shares or 0),
                    int(watch_time_seconds) if watch_time_seconds is not None else None,
                    _round_metric(ctr_percent),
                    _round_metric(engagement_rate),
                    str(provider).strip() if provider else None,
                    synced_value,
                    next_pull_value,
                ),
            )
    return ClipAnalyticsSnapshot(
        clip_db_id=clip_db_id,
        platform=platform,
        bucket=bucket,
        views=int(views or 0),
        likes=int(likes or 0),
        comments=int(comments or 0),
        shares=int(shares or 0),
        watch_time_seconds=int(watch_time_seconds) if watch_time_seconds is not None else None,
        ctr_percent=float(_round_metric(ctr_percent)) if ctr_percent is not None else None,
        engagement_rate=(
            float(_round_metric(engagement_rate)) if engagement_rate is not None else None
        ),
        provider=str(provider).strip() if provider else None,
        synced_at=synced_value,
        next_pull_at=next_pull_value,
    )


def list_reports(
    *,
    kind: str | None = None,
    streamer_login: str | None = None,
    limit: int = 20,
) -> list[SocialMediaReportRecord]:
    where = ["1=1"]
    params: list[Any] = []
    if kind:
        where.append("kind = %s")
        params.append(kind)
    if streamer_login:
        where.append("LOWER(COALESCE(streamer_login, '')) = LOWER(%s)")
        params.append(streamer_login)
    params.append(max(1, min(int(limit), 100)))
    with readonly_connection() as conn:
        rows = conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
            f"""
            SELECT id, kind, streamer_login, period_start, period_end,
                   content_md, model, created_at
              FROM social_media_reports
             WHERE {' AND '.join(where)}
             ORDER BY period_end DESC, created_at DESC, id DESC
             LIMIT %s
            """,
            tuple(params),
        ).fetchall()
    return [_row_to_report(row) for row in rows]


def get_existing_report(
    *,
    kind: str,
    period_start: datetime,
    period_end: datetime,
    streamer_login: str | None = None,
) -> SocialMediaReportRecord | None:
    with readonly_connection() as conn:
        row = conn.execute(
            """
            SELECT id, kind, streamer_login, period_start, period_end,
                   content_md, model, created_at
              FROM social_media_reports
             WHERE kind = %s
               AND period_start = %s
               AND period_end = %s
               AND (
                   streamer_login = %s
                   OR (streamer_login IS NULL AND %s IS NULL)
               )
             ORDER BY created_at DESC, id DESC
             LIMIT 1
            """,
            (
                kind,
                _encode_ts(period_start),
                _encode_ts(period_end),
                streamer_login,
                streamer_login,
            ),
        ).fetchone()
    return _row_to_report(row) if row else None


def insert_report(
    *,
    kind: str,
    streamer_login: str | None,
    period_start: datetime,
    period_end: datetime,
    content_md: str,
    model: str | None,
) -> SocialMediaReportRecord:
    with transaction() as conn:
        row = conn.execute(
            """
            INSERT INTO social_media_reports (
                kind, streamer_login, period_start, period_end, content_md, model
            )
            VALUES (%s, %s, %s, %s, %s, %s)
            RETURNING id, kind, streamer_login, period_start, period_end,
                      content_md, model, created_at
            """,
            (
                kind,
                streamer_login,
                _encode_ts(period_start),
                _encode_ts(period_end),
                content_md,
                model,
            ),
        ).fetchone()
    if not row:
        raise RuntimeError("failed to persist social media report")
    return _row_to_report(row)


def _row_to_clip_analytics(row: Any) -> ClipAnalyticsSnapshot:
    return ClipAnalyticsSnapshot(
        clip_db_id=int(_row_get(row, "clip_id")),
        platform=str(_row_get(row, "platform")),
        bucket=str(_row_get(row, "bucket")),
        views=int(_row_get(row, "views") or 0),
        likes=int(_row_get(row, "likes") or 0),
        comments=int(_row_get(row, "comments") or 0),
        shares=int(_row_get(row, "shares") or 0),
        watch_time_seconds=_optional_int(_row_get(row, "watch_time_seconds")),
        ctr_percent=_optional_float(_row_get(row, "ctr_percent")),
        engagement_rate=_optional_float(_row_get(row, "engagement_rate")),
        provider=_optional_str(_row_get(row, "provider")),
        synced_at=_optional_str(_row_get(row, "synced_at")),
        next_pull_at=_optional_str(_row_get(row, "next_pull_at")),
    )


def _row_to_report(row: Any) -> SocialMediaReportRecord:
    return SocialMediaReportRecord(
        id=int(_row_get(row, "id")),
        kind=str(_row_get(row, "kind")),
        streamer_login=_optional_str(_row_get(row, "streamer_login")),
        period_start=str(_row_get(row, "period_start")),
        period_end=str(_row_get(row, "period_end")),
        content_md=str(_row_get(row, "content_md") or ""),
        model=_optional_str(_row_get(row, "model")),
        created_at=_optional_str(_row_get(row, "created_at")),
    )


def _row_get(row: Any, key: str) -> Any:
    if hasattr(row, "keys"):
        return row[key]
    return row[key]


def _optional_str(value: Any) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    return text or None


def _optional_int(value: Any) -> int | None:
    if value is None or value == "":
        return None
    return int(value)


def _optional_float(value: Any) -> float | None:
    if value is None or value == "":
        return None
    return float(value)


def _round_metric(value: float | None) -> Decimal | None:
    if value is None:
        return None
    return Decimal(f"{float(value):.2f}")


def _encode_ts(value: datetime | str | None) -> str | None:
    if value is None:
        return None
    if isinstance(value, datetime):
        if value.tzinfo is None:
            value = value.replace(tzinfo=UTC)
        return value.astimezone(UTC).isoformat()
    return str(value)

"""Dashboard metrics and analytics helpers for the Twitch cog."""

from __future__ import annotations

import asyncio
import json
from datetime import UTC, datetime, timedelta

from ..analytics.backend_extended import AnalyticsBackendExtended
from ..core.constants import log
from ..storage import pg as storage


def _row_to_dict(row) -> dict:
    if row is None:
        return {}
    if hasattr(row, "keys"):
        return {k: row[k] for k in row.keys()}
    try:
        return dict(row)
    except Exception:
        return {}


async def _get_monetization_stats(self) -> dict:
    """Aggregate monetization & hype train data for the last 30 days."""
    cutoff_30d = (datetime.now(UTC) - timedelta(days=30)).isoformat()

    ads: dict = {
        "total": 0,
        "auto": 0,
        "manual": 0,
        "sessions_with_ads": 0,
        "avg_duration_s": 0.0,
        "avg_viewer_drop_pct": None,
        "worst_ads": [],
    }
    hype_train: dict = {
        "total": 0,
        "avg_level": 0.0,
        "max_level": 0,
        "avg_duration_s": 0.0,
    }
    bits: dict = {"total": 0, "cheer_events": 0}
    subs: dict = {"total_events": 0, "gifted": 0}

    with storage.readonly_connection() as c:
        ad_agg = c.execute(
            """
            SELECT COUNT(*) AS total_ads,
                   SUM(CASE WHEN is_automatic IS TRUE THEN 1 ELSE 0 END) AS auto_ads,
                   AVG(duration_seconds) AS avg_duration,
                   COUNT(DISTINCT session_id) AS sessions_with_ads
              FROM twitch_ad_break_events
             WHERE started_at >= %s
            """,
            (cutoff_30d,),
        ).fetchone()
        if ad_agg:
            total = int(ad_agg["total_ads"] or 0)
            auto = int(ad_agg["auto_ads"] or 0)
            ads["total"] = total
            ads["auto"] = auto
            ads["manual"] = total - auto
            ads["sessions_with_ads"] = int(ad_agg["sessions_with_ads"] or 0)
            ads["avg_duration_s"] = float(ad_agg["avg_duration"] or 0.0)

        ad_rows = c.execute(
            """
            SELECT a.id, a.session_id, a.started_at, a.duration_seconds, a.is_automatic,
                   s.started_at AS session_start
              FROM twitch_ad_break_events a
              JOIN twitch_stream_sessions s ON s.id = a.session_id
             WHERE a.started_at >= %s
               AND a.session_id IS NOT NULL
             ORDER BY a.started_at DESC
             LIMIT 200
            """,
            (cutoff_30d,),
        ).fetchall()

        timeline_map: dict = {}
        if ad_rows:
            session_ids = list({int(r["session_id"]) for r in ad_rows if r["session_id"]})
            if session_ids:
                session_ids_json = json.dumps(session_ids)
                viewer_rows = c.execute(
                    """
                    SELECT session_id, minutes_from_start, viewer_count
                      FROM twitch_session_viewers
                     WHERE session_id IN (
                        SELECT CAST(value AS INTEGER) FROM json_each(%s)
                     )
                     ORDER BY session_id, minutes_from_start
                    """,
                    (session_ids_json,),
                ).fetchall()
                for viewer_row in viewer_rows:
                    session_id = int(viewer_row["session_id"])
                    timeline_map.setdefault(session_id, []).append(
                        (
                            float(viewer_row["minutes_from_start"] or 0),
                            int(viewer_row["viewer_count"] or 0),
                        )
                    )

        drop_pcts: list[float] = []
        worst_ads: list[dict] = []
        for ad in ad_rows:
            session_id = int(ad["session_id"] or 0)
            ad_started = ad["started_at"]
            session_start = ad["session_start"]
            duration_s = float(ad["duration_seconds"] or 30)
            try:
                ad_dt = datetime.fromisoformat(str(ad_started).replace("Z", "+00:00"))
                session_dt = datetime.fromisoformat(str(session_start).replace("Z", "+00:00"))
                minutes_into = (ad_dt - session_dt).total_seconds() / 60.0
            except Exception:
                continue
            timeline = timeline_map.get(session_id, [])
            if not timeline:
                continue
            duration_min = duration_s / 60.0
            pre_vals = [value for minute, value in timeline if (minutes_into - 5) <= minute < minutes_into]
            post_start = minutes_into + duration_min
            post_vals = [value for minute, value in timeline if post_start <= minute < (post_start + 5)]
            if not pre_vals or not post_vals:
                continue
            pre_avg = sum(pre_vals) / len(pre_vals)
            if pre_avg <= 0:
                continue
            post_avg = sum(post_vals) / len(post_vals)
            drop_pct = (post_avg - pre_avg) / pre_avg * 100.0
            drop_pcts.append(drop_pct)
            worst_ads.append(
                {
                    "started_at": str(ad_started or "")[:16],
                    "duration_s": int(duration_s),
                    "drop_pct": round(drop_pct, 1),
                    "is_automatic": bool(ad["is_automatic"]),
                }
            )

        if drop_pcts:
            ads["avg_viewer_drop_pct"] = round(sum(drop_pcts) / len(drop_pcts), 1)
        worst_ads.sort(key=lambda item: item["drop_pct"])
        ads["worst_ads"] = worst_ads[:5]

        try:
            hype_train_row = c.execute(
                """
                SELECT COUNT(*) AS total_trains,
                       AVG(level) AS avg_level,
                       MAX(level) AS max_level,
                       AVG(duration_seconds) AS avg_duration
                  FROM twitch_hype_train_events
                 WHERE started_at >= %s
                   AND ended_at IS NOT NULL
                """,
                (cutoff_30d,),
            ).fetchone()
            if hype_train_row:
                hype_train["total"] = int(hype_train_row["total_trains"] or 0)
                hype_train["avg_level"] = round(float(hype_train_row["avg_level"] or 0.0), 1)
                hype_train["max_level"] = int(hype_train_row["max_level"] or 0)
                hype_train["avg_duration_s"] = round(
                    float(hype_train_row["avg_duration"] or 0.0), 0
                )
        except Exception:
            log.debug("Hype Train query fehlgeschlagen", exc_info=True)

        try:
            bits_row = c.execute(
                """
                SELECT SUM(amount) AS total_bits, COUNT(*) AS cheer_events
                FROM twitch_bits_events
                WHERE received_at >= %s
                """,
                (cutoff_30d,),
            ).fetchone()
            if bits_row:
                bits["total"] = int(bits_row["total_bits"] or 0)
                bits["cheer_events"] = int(bits_row["cheer_events"] or 0)
        except Exception:
            log.debug("Bits query fehlgeschlagen", exc_info=True)

        try:
            subs_row = c.execute(
                """
                SELECT COUNT(*) AS total_events,
                       SUM(CASE WHEN is_gift=1 THEN 1 ELSE 0 END) AS gifted
                  FROM twitch_subscription_events
                 WHERE received_at >= %s
                """,
                (cutoff_30d,),
            ).fetchone()
            if subs_row:
                subs["total_events"] = int(subs_row["total_events"] or 0)
                subs["gifted"] = int(subs_row["gifted"] or 0)
        except Exception:
            log.debug("Subs query fehlgeschlagen", exc_info=True)

    return {
        "ads": ads,
        "hype_train": hype_train,
        "bits": bits,
        "subs": subs,
        "window_days": 30,
    }


async def _dashboard_stats(
    self,
    *,
    hour_from: int | None = None,
    hour_to: int | None = None,
    streamer: str | None = None,
) -> dict:
    stats = await self._compute_stats(
        hour_from=hour_from,
        hour_to=hour_to,
        streamer=streamer,
    )
    tracked_top = stats.get("tracked", {}).get("top", []) or []
    category_top = stats.get("category", {}).get("top", []) or []

    def _agg(items: list[dict]):
        samples = sum(int(item.get("samples") or 0) for item in items)
        uniq = len(items)
        avg_over_streamers = (
            (sum(float(item.get("avg_viewers") or 0.0) for item in items) / float(uniq))
            if uniq
            else 0.0
        )
        return samples, uniq, avg_over_streamers

    cat_samples, cat_uniq, cat_avg = _agg(category_top)
    tracked_samples, tracked_uniq, tracked_avg = _agg(tracked_top)

    stats.setdefault("tracked", {})["samples"] = tracked_samples
    stats["tracked"]["unique_streamers"] = tracked_uniq
    stats.setdefault("category", {})["samples"] = cat_samples
    stats["category"]["unique_streamers"] = cat_uniq
    stats["avg_viewers_all"] = cat_avg
    stats["avg_viewers_tracked"] = tracked_avg

    try:
        eventsub_fetcher = getattr(self, "_get_eventsub_capacity_overview", None)
        if callable(eventsub_fetcher):
            stats["eventsub"] = await eventsub_fetcher(hours=24)
    except Exception:
        log.debug("Konnte EventSub-Capacity-Overview nicht laden", exc_info=True)

    try:
        stats["monetization"] = await self._get_monetization_stats()
    except Exception:
        log.debug("Konnte Monetization-Stats nicht laden", exc_info=True)

    return stats


async def _dashboard_streamer_analytics_data(self, streamer_login: str, days: int = 30) -> dict:
    """Return analytics data for the React dashboard."""
    return await AnalyticsBackendExtended.get_comprehensive_analytics(
        streamer_login=streamer_login,
        days=days,
    )


async def _dashboard_streamer_overview(self, login: str) -> dict:
    """Fetch comprehensive stats for a single streamer."""
    login = self._normalize_login(login)
    if not login:
        return {}
    return await asyncio.to_thread(self._dashboard_streamer_overview_sync, login)


def _dashboard_streamer_overview_sync(self, login: str) -> dict:
    data = {"login": login}
    with storage.readonly_connection() as conn:
        row = conn.execute(
            """
            SELECT *
            FROM twitch_partners_all_state
            WHERE LOWER(twitch_login)=LOWER(%s) AND status='active'
            """,
            (login,),
        ).fetchone()
        if not row:
            return {}
        data["meta"] = _row_to_dict(row)

        since_30d = (datetime.now(UTC) - timedelta(days=30)).isoformat()
        aggregate = conn.execute(
            """
            SELECT COUNT(*) as total_streams,
                   SUM(duration_seconds) as total_duration,
                   AVG(avg_viewers) as avg_avg_viewers,
                   MAX(peak_viewers) as max_peak,
                   SUM(follower_delta) as total_follower_delta,
                   SUM(unique_chatters) as total_unique_chatters
              FROM twitch_stream_sessions
             WHERE streamer_login=%s
               AND started_at > %s
            """,
            (login, since_30d),
        ).fetchone()
        data["stats_30d"] = _row_to_dict(aggregate) if aggregate else {}

        sessions = conn.execute(
            """
            SELECT id, stream_id, started_at, duration_seconds,
                   avg_viewers, peak_viewers, follower_delta, stream_title
              FROM twitch_stream_sessions
             WHERE streamer_login=%s
             ORDER BY started_at DESC
             LIMIT 20
            """,
            (login,),
        ).fetchall()
        data["recent_sessions"] = [_row_to_dict(session) for session in sessions]

    return data


async def _dashboard_session_detail(self, session_id: int) -> dict:
    """Fetch deep-dive data for a single session."""
    return await asyncio.to_thread(self._dashboard_session_detail_sync, session_id)


def _dashboard_session_detail_sync(self, session_id: int) -> dict:
    data = {}
    with storage.readonly_connection() as conn:
        row = conn.execute(
            "SELECT * FROM twitch_stream_sessions WHERE id=%s",
            (session_id,),
        ).fetchone()
        if not row:
            return {}
        data["session"] = _row_to_dict(row)

        timeline = conn.execute(
            """
            SELECT minutes_from_start, viewer_count
              FROM twitch_session_viewers
             WHERE session_id=%s
             ORDER BY minutes_from_start ASC
            """,
            (session_id,),
        ).fetchall()
        data["timeline"] = [_row_to_dict(point) for point in timeline]

        top_chatters = conn.execute(
            """
            SELECT chatter_login, messages
              FROM twitch_session_chatters
             WHERE session_id=%s
             ORDER BY messages DESC
             LIMIT 10
            """,
            (session_id,),
        ).fetchall()
        data["top_chatters"] = [_row_to_dict(chatter) for chatter in top_chatters]

    return data


async def _dashboard_comparison_stats(self, days: int = 30) -> dict:
    """Fetch comparative stats: streamer vs category vs tracked set."""
    return await asyncio.to_thread(self._dashboard_comparison_stats_sync, days)


def _dashboard_comparison_stats_sync(self, days: int = 30) -> dict:
    data = {}
    since_dt = (datetime.now(UTC) - timedelta(days=days)).isoformat()
    with storage.readonly_connection() as conn:
        category_stats = conn.execute(
            """
            SELECT AVG(viewer_count) as avg_viewers, MAX(viewer_count) as peak_viewers
              FROM twitch_stats_category
             WHERE ts_utc > %s
            """,
            (since_dt,),
        ).fetchone()
        data["category"] = _row_to_dict(category_stats) if category_stats else {}

        tracked_stats = conn.execute(
            """
            SELECT AVG(viewer_count) as avg_viewers, MAX(viewer_count) as peak_viewers
              FROM twitch_stats_tracked
             WHERE ts_utc > %s
            """,
            (since_dt,),
        ).fetchone()
        data["tracked_avg"] = _row_to_dict(tracked_stats) if tracked_stats else {}

        top_streamers = conn.execute(
            """
            SELECT streamer_login, AVG(avg_viewers) as val
              FROM twitch_stream_sessions
             WHERE started_at > %s
             GROUP BY streamer_login
             ORDER BY val DESC
             LIMIT 5
            """,
            (since_dt,),
        ).fetchall()
        data["top_streamers"] = [_row_to_dict(row) for row in top_streamers]

    return data


__all__ = [
    "_dashboard_comparison_stats",
    "_dashboard_comparison_stats_sync",
    "_dashboard_session_detail",
    "_dashboard_session_detail_sync",
    "_dashboard_stats",
    "_dashboard_streamer_analytics_data",
    "_dashboard_streamer_overview",
    "_dashboard_streamer_overview_sync",
    "_get_monetization_stats",
]

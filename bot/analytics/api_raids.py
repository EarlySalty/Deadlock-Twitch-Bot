"""
Analytics API v2 - Raids Mixin.

Raid analytics: per-source-channel performance, retention curves, follow attribution.
"""

from __future__ import annotations

import logging
from datetime import UTC, datetime, timedelta
from typing import Any

from aiohttp import web

from ..core.chat_bots import build_known_chat_bot_not_in_clause
from .raid_metrics import raid_identity_key, recalculate_raid_chat_metrics
from ..storage import pg as storage
from .error_utils import analytics_internal_error_response

log = logging.getLogger("TwitchStreams.AnalyticsV2")


class _AnalyticsRaidsMixin:
    """Mixin providing raid analytics endpoints."""

    RAID_RETENTION_SAMPLE_LIMIT = 50
    RAID_METRIC_BATCH_SIZE = 500

    async def _api_v2_raid_analytics(self, request: web.Request) -> web.Response:
        """Raid analytics: per-source performance, retention curves, follow attribution."""
        self._require_v2_auth(request)
        self._require_extended_plan(request)

        streamer = request.query.get("streamer", "").strip().lower()
        days = min(max(int(request.query.get("days", "30")), 7), 365)

        if not streamer:
            return web.json_response({"error": "Streamer required"}, status=400)

        cutoff = (datetime.now(UTC) - timedelta(days=days)).isoformat()

        try:
            with storage.readonly_connection() as c:
                follower_bot_clause, follower_bot_params = build_known_chat_bot_not_in_clause(
                    column_expr="fe.follower_login",
                    placeholder="%s",
                )

                base_raids_full_rows = c.execute(
                    """
                    SELECT
                        rr.raid_id,
                        rh.from_broadcaster_login,
                        rr.viewer_count_sent,
                        rr.executed_at,
                        rr.target_session_id,
                        rr.to_broadcaster_login
                    FROM twitch_raid_retention rr
                    JOIN twitch_raid_history rh
                      ON rh.id = rr.raid_id
                     AND rh.executed_at = rr.executed_at
                    JOIN twitch_stream_sessions ss ON ss.id = rr.target_session_id
                    WHERE LOWER(ss.streamer_login) = %s
                      AND ss.started_at >= %s
                    ORDER BY ss.started_at DESC
                    """,
                    [streamer, cutoff],
                ).fetchall()

                # --- 2. Follow attribution (precise, no heuristic time window) ---
                # A follow is attributed to a raid ONLY if:
                #   - follower appeared in the session AFTER the raid timestamp
                #   - follower was NOT known before this session (first_seen_at >= session start)
                follow_rows = c.execute(
                    f"""
                    SELECT
                        fe.follower_login,
                        CASE
                            WHEN rh.executed_at IS NOT NULL
                             AND sc.first_message_at >= rh.executed_at
                             AND cr_before.chatter_login IS NULL
                                THEN 'raid'
                            ELSE 'organic'
                        END AS follow_source,
                        rh.from_broadcaster_login AS raid_source
                    FROM twitch_follow_events fe
                    JOIN twitch_stream_sessions ss
                        ON LOWER(ss.streamer_login) = LOWER(fe.streamer_login)
                       AND fe.followed_at BETWEEN ss.started_at AND COALESCE(ss.ended_at, NOW())
                    LEFT JOIN twitch_session_chatters sc
                        ON sc.session_id = ss.id
                       AND LOWER(sc.chatter_login) = LOWER(fe.follower_login)
                    LEFT JOIN twitch_raid_retention rr ON rr.target_session_id = ss.id
                    LEFT JOIN twitch_raid_history rh
                      ON rh.id = rr.raid_id
                     AND rh.executed_at = rr.executed_at
                    LEFT JOIN twitch_chatter_rollup cr_before
                        ON LOWER(cr_before.chatter_login) = LOWER(fe.follower_login)
                       AND LOWER(cr_before.streamer_login) = LOWER(fe.streamer_login)
                       AND cr_before.first_seen_at < ss.started_at
                    WHERE LOWER(fe.streamer_login) = %s
                      AND fe.followed_at >= %s
                      AND {follower_bot_clause}
                    """,
                    [streamer, cutoff, *follower_bot_params],
                ).fetchall()

                base_raids_full: list[dict[str, Any]] = []
                base_raids_sample: list[dict[str, Any]] = []
                for raid in base_raids_full_rows:
                    try:
                        raid_id = int(raid["raid_id"])
                        target_session_id = int(raid["target_session_id"])
                    except Exception:
                        continue
                    base_raids_full.append(
                        {
                            "raid_id": raid_id,
                            "from": raid["from_broadcaster_login"] or "unknown",
                            "from_login": str(raid["from_broadcaster_login"] or "").lower(),
                            "to_login": str(raid["to_broadcaster_login"] or streamer).lower(),
                            "viewers_sent": int(raid["viewer_count_sent"] or 0),
                            "executed_at": raid["executed_at"],
                            "target_session_id": target_session_id,
                        }
                    )
                    if len(base_raids_sample) < self.RAID_RETENTION_SAMPLE_LIMIT:
                        base_raids_sample.append(base_raids_full[-1])

                sample_raid_keys = {
                    key
                    for raid in base_raids_sample
                    if (key := raid_identity_key(raid.get("raid_id"), raid.get("executed_at"))) is not None
                }

                grouped_source: dict[str, dict[str, Any]] = {}
                sample_metrics: dict[tuple[int, str], dict[str, int]] = {}
                for start in range(0, len(base_raids_full), self.RAID_METRIC_BATCH_SIZE):
                    raid_batch = base_raids_full[start : start + self.RAID_METRIC_BATCH_SIZE]
                    batch_metrics = recalculate_raid_chat_metrics(
                        c,
                        raid_batch,
                    )
                    for raid in raid_batch:
                        raid_id = int(raid["raid_id"])
                        raid_key = raid_identity_key(raid_id, raid.get("executed_at"))
                        if raid_key is None:
                            continue
                        src_key = str(raid["from"] or "unknown").lower()
                        sent = int(raid["viewers_sent"] or 0)
                        metric = batch_metrics.get(raid_key, {})
                        new_chatters = int(metric.get("new_chatters", 0) or 0)
                        plus30m = int(metric.get("plus30m", 0) or 0)
                        known_from_raider = int(metric.get("known_from_raider", 0) or 0)

                        source_bucket = grouped_source.setdefault(
                            src_key,
                            {
                                "from_channel": raid["from"] or "unknown",
                                "raids_received": 0,
                                "total_viewers_sent": 0.0,
                                "total_new_chatters": 0.0,
                                "retention_ratio_sum": 0.0,
                                "retention_ratio_count": 0,
                                "overlap_ratio_sum": 0.0,
                                "overlap_ratio_count": 0,
                            },
                        )
                        source_bucket["raids_received"] += 1
                        source_bucket["total_viewers_sent"] += float(sent)
                        source_bucket["total_new_chatters"] += float(new_chatters)
                        if sent > 0:
                            source_bucket["retention_ratio_sum"] += float(plus30m) / float(sent)
                            source_bucket["retention_ratio_count"] += 1
                            source_bucket["overlap_ratio_sum"] += float(known_from_raider) / float(sent)
                            source_bucket["overlap_ratio_count"] += 1

                        if raid_key in sample_raid_keys:
                            sample_metrics[raid_key] = {
                                "plus5m": int(metric.get("plus5m", 0) or 0),
                                "plus15m": int(metric.get("plus15m", 0) or 0),
                                "plus30m": plus30m,
                                "new_chatters": new_chatters,
                            }

                follows_by_source: dict[str, int] = {}
                for follow in follow_rows:
                    if follow["follow_source"] != "raid":
                        continue
                    src_key = str(follow["raid_source"] or "").lower()
                    follows_by_source[src_key] = follows_by_source.get(src_key, 0) + 1

                per_source = []
                for src_key, source_bucket in grouped_source.items():
                    total_viewers_sent = float(source_bucket["total_viewers_sent"] or 0.0)
                    raids_received = int(source_bucket["raids_received"] or 0)
                    avg_viewers = (total_viewers_sent / raids_received) if raids_received > 0 else 0.0
                    avg_new_chatters = (
                        float(source_bucket["total_new_chatters"] or 0.0) / raids_received
                        if raids_received > 0
                        else 0.0
                    )
                    retention_count = int(source_bucket["retention_ratio_count"] or 0)
                    overlap_count = int(source_bucket["overlap_ratio_count"] or 0)
                    avg_retention_30m = (
                        float(source_bucket["retention_ratio_sum"] or 0.0) / retention_count
                        if retention_count > 0
                        else None
                    )
                    avg_known_overlap = (
                        float(source_bucket["overlap_ratio_sum"] or 0.0) / overlap_count
                        if overlap_count > 0
                        else None
                    )
                    follows_attributed = int(follows_by_source.get(src_key, 0))
                    per_source.append(
                        {
                            "from_channel": source_bucket["from_channel"] or "unknown",
                            "raids_received": raids_received,
                            "avg_viewers_sent": round(avg_viewers, 1),
                            "avg_new_chatters": round(avg_new_chatters, 1),
                            "avg_retention_30m": (
                                round(avg_retention_30m, 3) if avg_retention_30m is not None else None
                            ),
                            "follows_attributed": follows_attributed,
                            "conversion_rate": (
                                round(follows_attributed / total_viewers_sent, 3)
                                if total_viewers_sent > 0
                                else None
                            ),
                            "known_audience_overlap": (
                                round(avg_known_overlap, 3) if avg_known_overlap is not None else None
                            ),
                        }
                    )
                per_source.sort(key=lambda item: item["raids_received"], reverse=True)
                per_source = per_source[:20]

                # Follow attribution summary
                raid_follows = sum(1 for f in follow_rows if f["follow_source"] == "raid")
                organic_follows = sum(1 for f in follow_rows if f["follow_source"] == "organic")
                total_follows = len(follow_rows)

                follow_attribution = {
                    "total_follows": total_follows,
                    "raid_follows": raid_follows,
                    "organic_follows": organic_follows,
                    "raid_conversion_rate": round(raid_follows / total_follows, 3) if total_follows > 0 else None,
                } if total_follows > 0 else None

                # Retention curves per raid
                retention_curves = []
                for raid in base_raids_sample:
                    raid_id = int(raid["raid_id"])
                    raid_key = raid_identity_key(raid_id, raid.get("executed_at"))
                    if raid_key is None:
                        continue
                    sent = int(raid["viewers_sent"] or 0)
                    metric = sample_metrics.get(raid_key, {})
                    plus5m = int(metric.get("plus5m", 0) or 0)
                    plus15m = int(metric.get("plus15m", 0) or 0)
                    plus30m = int(metric.get("plus30m", 0) or 0)
                    plus5m_ratio = round(plus5m / sent, 3) if sent > 0 else 0.0
                    plus15m_ratio = round(plus15m / sent, 3) if sent > 0 else 0.0
                    plus30m_ratio = round(plus30m / sent, 3) if sent > 0 else 0.0
                    retention_curves.append(
                        {
                            "raid_id": raid_id,
                            "from": raid["from"] or "unknown",
                            "viewers_sent": sent,
                            "new_chatters": int(metric.get("new_chatters", 0) or 0),
                            "retention_curve": {
                                "plus5m": plus5m_ratio,
                                "plus15m": plus15m_ratio,
                                "plus30m": plus30m_ratio,
                            },
                        }
                    )

                # ── Incoming Raids (from twitch_raid_arrival_tracking) ──
                incoming_raid_rows = c.execute(
                    """
                    SELECT
                        rat.detected_at,
                        rat.from_broadcaster_login,
                        rat.viewer_count,
                        rat.classification,
                        rat.confirmation_signals,
                        rat.unraid_seen
                    FROM twitch_raid_arrival_tracking rat
                    WHERE LOWER(rat.to_broadcaster_login) = %s
                      AND rat.detected_at >= %s
                    ORDER BY rat.detected_at DESC
                    LIMIT 50
                    """,
                    [streamer, cutoff],
                ).fetchall()

                incoming_raids: list[dict[str, Any]] = []
                boost_values: list[float] = []
                retention_15m_values: list[float] = []

                for raid_row in incoming_raid_rows:
                    detected_at = raid_row["detected_at"]
                    from_channel = raid_row["from_broadcaster_login"] or "unknown"
                    viewers_sent = int(raid_row["viewer_count"] or 0)

                    # Find the session running at raid time
                    session_row = c.execute(
                        """
                        SELECT ss.id, ss.started_at
                        FROM twitch_stream_sessions ss
                        WHERE LOWER(ss.streamer_login) = %s
                          AND ss.started_at <= %s
                          AND (ss.ended_at IS NULL OR ss.ended_at >= %s)
                        LIMIT 1
                        """,
                        [streamer, detected_at, detected_at],
                    ).fetchone()

                    impact: dict[str, Any] = {
                        "viewers_before": None,
                        "viewers_peak_after": None,
                        "boost_pct": None,
                        "retention_5m_pct": None,
                        "retention_15m_pct": None,
                        "retention_30m_pct": None,
                        "follows_after_raid": 0,
                    }

                    if session_row:
                        session_id = int(session_row["id"])
                        session_started_at = session_row["started_at"]

                        # Calculate raid minute offset
                        if hasattr(detected_at, 'timestamp') and hasattr(session_started_at, 'timestamp'):
                            raid_minute = int((detected_at.timestamp() - session_started_at.timestamp()) / 60)
                        else:
                            from datetime import datetime as dt_cls
                            det = detected_at if isinstance(detected_at, datetime) else dt_cls.fromisoformat(str(detected_at))
                            ses = session_started_at if isinstance(session_started_at, datetime) else dt_cls.fromisoformat(str(session_started_at))
                            raid_minute = int((det.timestamp() - ses.timestamp()) / 60)

                        # Get viewer timeline for this session
                        timeline_rows = c.execute(
                            """
                            SELECT minutes_from_start, viewer_count
                            FROM twitch_session_viewers
                            WHERE session_id = %s
                            ORDER BY minutes_from_start
                            """,
                            [session_id],
                        ).fetchall()

                        if timeline_rows:
                            timeline = {int(r["minutes_from_start"]): int(r["viewer_count"]) for r in timeline_rows}

                            # viewers_before: average in [raid_minute - 3, raid_minute)
                            before_vals = [
                                v for m, v in timeline.items()
                                if (raid_minute - 3) <= m < raid_minute
                            ]
                            avg_before = sum(before_vals) / len(before_vals) if before_vals else None

                            # viewers_peak_after: max in [raid_minute, raid_minute + 5]
                            after_vals = [
                                v for m, v in timeline.items()
                                if raid_minute <= m <= (raid_minute + 5)
                            ]
                            peak_after = max(after_vals) if after_vals else None

                            if avg_before is not None and avg_before > 0 and peak_after is not None:
                                impact["viewers_before"] = round(avg_before, 1)
                                impact["viewers_peak_after"] = peak_after
                                impact["boost_pct"] = round(((peak_after - avg_before) / avg_before) * 100, 1)
                                boost_values.append(impact["boost_pct"])

                            # Retention: find closest minute in timeline
                            def _closest_viewer(target_min: int) -> int | None:
                                if not timeline:
                                    return None
                                closest = min(timeline.keys(), key=lambda m: abs(m - target_min))
                                if abs(closest - target_min) <= 2:
                                    return timeline[closest]
                                return None

                            if peak_after and peak_after > 0:
                                for offset_min, key in [(5, "retention_5m_pct"), (15, "retention_15m_pct"), (30, "retention_30m_pct")]:
                                    v = _closest_viewer(raid_minute + offset_min)
                                    if v is not None:
                                        impact[key] = round((v / peak_after) * 100, 1)

                                if impact["retention_15m_pct"] is not None:
                                    retention_15m_values.append(impact["retention_15m_pct"])

                        # Follower impact: follows within 30 minutes after raid
                        follow_count_row = c.execute(
                            """
                            SELECT COUNT(*) as follows
                            FROM twitch_follow_events
                            WHERE LOWER(streamer_login) = %s
                              AND followed_at BETWEEN %s AND %s + interval '30 minutes'
                            """,
                            [streamer, detected_at, detected_at],
                        ).fetchone()
                        if follow_count_row:
                            impact["follows_after_raid"] = int(follow_count_row["follows"] or 0)

                    incoming_raids.append({
                        "from_channel": from_channel,
                        "detected_at": detected_at.isoformat() if hasattr(detected_at, 'isoformat') else str(detected_at),
                        "viewers_sent": viewers_sent,
                        "classification": raid_row["classification"] or "unknown",
                        "unraid_seen": bool(raid_row["unraid_seen"]),
                        "impact": impact,
                    })

                # Incoming summary
                incoming_summary: dict[str, Any] | None = None
                if incoming_raids:
                    avg_viewers_received = sum(r["viewers_sent"] for r in incoming_raids) / len(incoming_raids)
                    avg_boost = (
                        round(sum(boost_values) / len(boost_values), 1)
                        if boost_values else None
                    )
                    avg_ret_15m = (
                        round(sum(retention_15m_values) / len(retention_15m_values), 1)
                        if retention_15m_values else None
                    )

                    # Best raider by avg boost
                    raider_boosts: dict[str, list[float]] = {}
                    for r in incoming_raids:
                        if r["impact"]["boost_pct"] is not None:
                            raider_boosts.setdefault(r["from_channel"], []).append(r["impact"]["boost_pct"])
                    best_raider = None
                    if raider_boosts:
                        best_raider = max(
                            raider_boosts.keys(),
                            key=lambda k: sum(raider_boosts[k]) / len(raider_boosts[k]),
                        )

                    incoming_summary = {
                        "total_raids_received": len(incoming_raids),
                        "avg_viewers_received": round(avg_viewers_received, 1),
                        "avg_boost_pct": avg_boost,
                        "avg_retention_15m": avg_ret_15m,
                        "best_raider": best_raider,
                        "raid_balance": {
                            "sent": sum(s["raids_received"] for s in per_source),
                            "received": len(incoming_raids),
                        },
                    }

                return web.json_response({
                    "per_source": per_source,
                    "follow_attribution": follow_attribution,
                    "retention_curves": retention_curves,
                    "incoming_raids": incoming_raids,
                    "incoming_summary": incoming_summary,
                    "window_days": days,
                    "dataQuality": {
                        "botFilterApplied": True,
                        "retentionCurveSampleSize": len(base_raids_sample),
                        "perSourceUsesFullWindow": True,
                        "raidMetricBatchSize": self.RAID_METRIC_BATCH_SIZE,
                    },
                })

        except Exception as exc:
            log.exception("Error in raid analytics API")
            return analytics_internal_error_response()

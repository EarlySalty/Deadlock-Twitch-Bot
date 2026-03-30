"""Synchronous loader for monetization analytics payloads."""

from __future__ import annotations

import logging
from datetime import UTC, datetime, timedelta
from typing import Any

from ..storage import pg as storage

log = logging.getLogger("TwitchStreams.AnalyticsV2")


def load_monetization_payload(*, streamer: str, days: int) -> dict[str, Any]:
    cutoff = (datetime.now(UTC) - timedelta(days=days)).isoformat()
    ads: dict[str, Any] = {
        "total": 0,
        "auto": 0,
        "manual": 0,
        "sessions_with_ads": 0,
        "avg_duration_s": 0.0,
        "avg_viewer_drop_pct": None,
        "worst_ads": [],
    }
    hype_train: dict[str, Any] = {
        "total": 0,
        "avg_level": 0.0,
        "max_level": 0,
        "avg_duration_s": 0.0,
    }
    bits: dict[str, Any] = {"total": 0, "cheer_events": 0}
    subs: dict[str, Any] = {"total_events": 0, "gifted": 0}

    with storage.readonly_connection() as conn:
        ad_agg = conn.execute(
            """
            SELECT COUNT(*) AS total_ads,
                   SUM(CASE WHEN a.is_automatic IS TRUE THEN 1 ELSE 0 END) AS auto_ads,
                   AVG(a.duration_seconds) AS avg_duration,
              COUNT(DISTINCT a.session_id) AS sessions_with_ads
              FROM twitch_ad_break_events a
              LEFT JOIN twitch_stream_sessions s ON s.id = a.session_id
             WHERE a.started_at >= %s
               AND (%s = '' OR LOWER(s.streamer_login) = %s)
            """,
            (cutoff, streamer, streamer),
        ).fetchone()
        if ad_agg:
            total = int(ad_agg["total_ads"] or 0)
            auto = int(ad_agg["auto_ads"] or 0)
            ads["total"] = total
            ads["auto"] = auto
            ads["manual"] = total - auto
            ads["sessions_with_ads"] = int(ad_agg["sessions_with_ads"] or 0)
            ads["avg_duration_s"] = round(float(ad_agg["avg_duration"] or 0.0), 1)

        ad_rows = conn.execute(
            """
            SELECT a.id, a.session_id, a.started_at, a.duration_seconds, a.is_automatic,
                   s.started_at AS session_start
              FROM twitch_ad_break_events a
              JOIN twitch_stream_sessions s ON s.id = a.session_id
             WHERE a.started_at >= %s
               AND a.session_id IS NOT NULL
               AND (%s = '' OR LOWER(s.streamer_login) = %s)
             ORDER BY a.started_at DESC
             LIMIT 200
            """,
            (cutoff, streamer, streamer),
        ).fetchall()

        timeline_map: dict[int, list[tuple[float, int]]] = {}
        if ad_rows:
            session_ids = list({int(row["session_id"]) for row in ad_rows if row["session_id"]})
            if session_ids:
                viewer_rows = conn.execute(
                    """
                    SELECT session_id, minutes_from_start, viewer_count
                      FROM twitch_session_viewers
                     WHERE session_id = ANY(%s)
                     ORDER BY session_id, minutes_from_start
                    """,
                    (session_ids,),
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
        worst_ads: list[dict[str, Any]] = []
        position_buckets: dict[str, list[float]] = {
            "early_0_30m": [],
            "mid_30_60m": [],
            "late_60_90m": [],
            "endgame_90m": [],
        }
        duration_buckets: dict[str, list[float]] = {
            "30s": [],
            "60s": [],
            "90s": [],
            "120s_plus": [],
        }
        auto_drops: list[float] = []
        manual_drops: list[float] = []
        recovery_times: list[float] = []
        duration_recovery: dict[str, list[float]] = {
            "30s": [],
            "60s": [],
            "90s": [],
            "120s_plus": [],
        }

        for ad in ad_rows:
            session_id = int(ad["session_id"] or 0)
            duration_seconds = float(ad["duration_seconds"] or 30)
            try:
                ad_dt = datetime.fromisoformat(str(ad["started_at"]).replace("Z", "+00:00"))
                session_dt = datetime.fromisoformat(str(ad["session_start"]).replace("Z", "+00:00"))
                minutes_into = (ad_dt - session_dt).total_seconds() / 60.0
            except Exception:
                continue
            timeline = timeline_map.get(session_id, [])
            if not timeline:
                continue
            duration_minutes = duration_seconds / 60.0
            pre = [value for minute, value in timeline if (minutes_into - 3) <= minute < minutes_into]
            post_start = minutes_into + duration_minutes
            post = [value for minute, value in timeline if post_start <= minute < (post_start + 2)]
            if not pre or not post:
                continue
            pre_avg = sum(pre) / len(pre)
            if pre_avg <= 0:
                continue
            drop = (pre_avg - sum(post) / len(post)) / pre_avg * 100.0
            drop_pcts.append(drop)

            recovery_min = None
            for minute, viewers in sorted(timeline, key=lambda item: item[0]):
                if minute > post_start and viewers >= pre_avg * 0.95:
                    recovery_min = round(minute - post_start, 1)
                    break
            if recovery_min is not None:
                recovery_times.append(recovery_min)
                duration_key = (
                    "30s"
                    if duration_seconds <= 35
                    else "60s"
                    if duration_seconds <= 65
                    else "90s"
                    if duration_seconds <= 100
                    else "120s_plus"
                )
                duration_recovery[duration_key].append(recovery_min)

            worst_ads.append(
                {
                    "started_at": str(ad["started_at"] or "")[:16],
                    "duration_s": int(duration_seconds),
                    "drop_pct": round(drop, 1),
                    "is_automatic": bool(ad["is_automatic"]),
                    "min_into_stream": round(minutes_into, 1),
                    "recovery_min": recovery_min,
                }
            )

            if minutes_into < 30:
                position_buckets["early_0_30m"].append(drop)
            elif minutes_into < 60:
                position_buckets["mid_30_60m"].append(drop)
            elif minutes_into < 90:
                position_buckets["late_60_90m"].append(drop)
            else:
                position_buckets["endgame_90m"].append(drop)

            if duration_seconds <= 35:
                duration_buckets["30s"].append(drop)
            elif duration_seconds <= 65:
                duration_buckets["60s"].append(drop)
            elif duration_seconds <= 100:
                duration_buckets["90s"].append(drop)
            else:
                duration_buckets["120s_plus"].append(drop)

            if ad["is_automatic"]:
                auto_drops.append(drop)
            else:
                manual_drops.append(drop)

        def _avg(values: list[float]) -> float | None:
            return round(sum(values) / len(values), 1) if values else None

        if drop_pcts:
            ads["avg_viewer_drop_pct"] = round(sum(drop_pcts) / len(drop_pcts), 1)
        worst_ads.sort(key=lambda item: item["drop_pct"], reverse=True)
        ads["worst_ads"] = worst_ads[:5]
        ads["position_impact"] = {
            bucket: {"avg_drop": _avg(drops), "count": len(drops)}
            for bucket, drops in position_buckets.items()
        }
        ads["duration_impact"] = {
            bucket: {"avg_drop": _avg(drops), "count": len(drops)}
            for bucket, drops in duration_buckets.items()
        }
        ads["auto_vs_manual"] = {
            "auto_avg_drop": _avg(auto_drops),
            "manual_avg_drop": _avg(manual_drops),
            "auto_count": len(auto_drops),
            "manual_count": len(manual_drops),
        }

        position_avgs = {
            bucket: sum(drops) / len(drops)
            for bucket, drops in position_buckets.items()
            if drops
        }
        if position_avgs:
            bucket_labels = {
                "early_0_30m": "ersten 30 Min",
                "mid_30_60m": "Min 30-60",
                "late_60_90m": "Min 60-90",
                "endgame_90m": "nach Min 90",
            }
            best_bucket = min(position_avgs, key=position_avgs.get)
            worst_bucket = max(position_avgs, key=position_avgs.get)
            ads["best_ad_time"] = (
                f"Nach {bucket_labels.get(best_bucket, best_bucket)} "
                f"(Ø -{position_avgs[best_bucket]:.1f}% statt "
                f"-{position_avgs[worst_bucket]:.1f}% {bucket_labels.get(worst_bucket, worst_bucket)})"
            )
        else:
            ads["best_ad_time"] = None

        ads["avg_recovery_min"] = round(sum(recovery_times) / len(recovery_times), 1) if recovery_times else None
        ads["recovery_by_duration"] = {
            bucket: {
                "avg_recovery_min": round(sum(values) / len(values), 1) if values else None,
                "count": len(values),
            }
            for bucket, values in duration_recovery.items()
        }

        recommendations: list[str] = []
        duration_avgs = {
            bucket: sum(drops) / len(drops)
            for bucket, drops in duration_buckets.items()
            if drops
        }
        if duration_avgs:
            duration_labels = {
                "30s": "30s",
                "60s": "60s",
                "90s": "90s",
                "120s_plus": "120s+",
            }
            best_duration = min(duration_avgs, key=duration_avgs.get)
            recommendations.append(
                f"{duration_labels[best_duration]}-Ads verursachen den geringsten Drop (Ø {duration_avgs[best_duration]:.1f}%)"
            )
        if auto_drops and manual_drops:
            auto_avg = sum(auto_drops) / len(auto_drops)
            manual_avg = sum(manual_drops) / len(manual_drops)
            if manual_avg < auto_avg * 0.7:
                recommendations.append(
                    f"Manuelle Ads verlieren {((auto_avg - manual_avg) / auto_avg * 100):.0f}% weniger Viewer als automatische"
                )
        if position_avgs:
            position_labels = {
                "early_0_30m": "in den ersten 30 Min",
                "mid_30_60m": "zwischen Min 30-60",
                "late_60_90m": "zwischen Min 60-90",
                "endgame_90m": "nach Min 90",
            }
            best_position = min(position_avgs, key=position_avgs.get)
            recommendations.append(
                f"Beste Ad-Zeit: {position_labels.get(best_position, best_position)} (Ø {position_avgs[best_position]:.1f}% Drop)"
            )
        if recovery_times:
            recommendations.append(
                f"Ø Recovery-Zeit: {sum(recovery_times) / len(recovery_times):.1f} Minuten nach Ad-Ende"
            )
        ads["recommendations"] = recommendations

        try:
            hype_train_row = conn.execute(
                """
                SELECT COUNT(*) AS total, AVG(h.level) AS avg_level,
                       MAX(h.level) AS max_level, AVG(h.duration_seconds) AS avg_dur
                  FROM twitch_hype_train_events h
                  LEFT JOIN twitch_stream_sessions s ON s.id = h.session_id
                 WHERE h.started_at >= %s
                   AND h.ended_at IS NOT NULL
                   AND (%s = '' OR LOWER(s.streamer_login) = %s)
                """,
                (cutoff, streamer, streamer),
            ).fetchone()
            if hype_train_row:
                hype_train = {
                    "total": int(hype_train_row["total"] or 0),
                    "avg_level": round(float(hype_train_row["avg_level"] or 0), 1),
                    "max_level": int(hype_train_row["max_level"] or 0),
                    "avg_duration_s": round(float(hype_train_row["avg_dur"] or 0), 0),
                }
        except Exception:
            log.debug("Hype train query failed", exc_info=True)

        try:
            bits_row = conn.execute(
                """
                SELECT SUM(amount) AS total, COUNT(*) AS events
                  FROM twitch_bits_events
                 WHERE received_at >= %s
                   AND (%s = '' OR LOWER(streamer_login) = %s)
                """,
                (cutoff, streamer, streamer),
            ).fetchone()
            if bits_row:
                bits = {
                    "total": int(bits_row["total"] or 0),
                    "cheer_events": int(bits_row["events"] or 0),
                }
        except Exception:
            log.debug("Bits query failed", exc_info=True)

        try:
            subs_row = conn.execute(
                """
                SELECT COUNT(*) AS total,
                       SUM(CASE WHEN is_gift=1 THEN 1 ELSE 0 END) AS gifted
                  FROM twitch_subscription_events
                 WHERE received_at >= %s
                   AND (%s = '' OR LOWER(streamer_login) = %s)
                """,
                (cutoff, streamer, streamer),
            ).fetchone()
            if subs_row:
                subs = {
                    "total_events": int(subs_row["total"] or 0),
                    "gifted": int(subs_row["gifted"] or 0),
                }
        except Exception:
            log.debug("Subs query failed", exc_info=True)

    return {
        "ads": ads,
        "hype_train": hype_train,
        "bits": bits,
        "subs": subs,
        "window_days": days,
    }

"""
Demo data for the public Analytics Dashboard.

Fake streamer: deadlock_de_demo
Profile: mid-tier German Deadlock streamer, 4x/week, avg ~380 viewers, ~8k followers.
All data is synthetic and does not reflect any real streamer.
"""

from __future__ import annotations

import copy
from datetime import UTC, datetime, timedelta
from typing import Any

DEMO_STREAMER = "deadlock_de_demo"
DEMO_DISPLAY_NAME = "Deadlock_DE_Demo"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _date(offset_days: int = 0) -> str:
    """Return ISO date string relative to today."""
    d = (datetime.now(UTC) - timedelta(days=offset_days)).date()
    return d.isoformat()


def _dt(offset_days: int = 0, hour: int = 19) -> str:
    d = datetime.now(UTC) - timedelta(days=offset_days)
    d = d.replace(hour=hour, minute=0, second=0, microsecond=0)
    return d.isoformat()


# ---------------------------------------------------------------------------
# Per-endpoint demo data factories
# ---------------------------------------------------------------------------


def get_auth_status() -> dict[str, Any]:
    return {
        "authenticated": True,
        "level": "partner",
        "isAdmin": False,
        "isLocalhost": False,
        "canViewAllStreamers": False,
        "twitchLogin": DEMO_STREAMER,
        "displayName": DEMO_DISPLAY_NAME,
        "demoMode": True,
        "permissions": {
            "viewAllStreamers": False,
            "viewComparison": True,
            "viewChatAnalytics": True,
            "viewOverlap": True,
        },
    }


def get_streamers() -> list[dict[str, Any]]:
    return [{"login": DEMO_STREAMER, "isPartner": True}]


def get_overview(days: int = 30) -> dict[str, Any]:
    return {
        "streamer": DEMO_STREAMER,
        "days": days,
        "scores": {
            "total": 72,
            "reach": 68,
            "retention": 78,
            "engagement": 74,
            "growth": 65,
            "monetization": 55,
            "network": 71,
        },
        "summary": {
            "avgViewers": 382,
            "peakViewers": 1087,
            "totalHoursWatched": 5342,
            "totalAirtime": 14.0,
            "followersDelta": 167,
            "followersGained": 183,
            "followersPerHour": 11.9,
            "followersGainedPerHour": 13.1,
            "retention10m": 71.2,
            "retentionReliable": True,
            "uniqueChatters": 634,
            "streamCount": 14,
            "avgViewersTrend": 6.8,
            "peakViewersTrend": 3.2,
            "followersTrend": 12.4,
            "retentionTrend": 1.9,
        },
        "sessions": _get_sessions(),
        "findings": [
            {
                "type": "pos",
                "title": "Starke Retention",
                "text": "71 % deiner Zuschauer bleiben die ersten 10 Minuten – Top 25 % der Kategorie.",
            },
            {
                "type": "pos",
                "title": "Stetiges Wachstum",
                "text": "Follower-Wachstum +12 % vs. Vormonat. Du gewinnst ~13 Follower pro Stream-Stunde.",
            },
            {
                "type": "warn",
                "title": "Späte Starts",
                "text": "Freitag-Streams starten oft nach 21:00 Uhr – die Kategorie hat dann weniger Zuschauer.",
            },
            {
                "type": "info",
                "title": "Raid-Netzwerk aktiv",
                "text": "Du hast in 30 Tagen 8 Raids gesendet und 5 empfangen.",
            },
            {
                "type": "neg",
                "title": "Wenig Samstag-Streams",
                "text": "Samstag 17:00–20:00 Uhr hat die höchste Viewer-Dichte – nur 2 Streams in dem Slot.",
            },
        ],
        "actions": [
            {
                "tag": "Schedule",
                "text": "Teste Samstag 17:00 Uhr als festen Startslot",
                "priority": "high",
            },
            {
                "tag": "Retention",
                "text": "Intro-Phase kürzen: Viele droppen in Minute 3–5",
                "priority": "medium",
            },
            {
                "tag": "Titel",
                "text": "Variiere Titel öfter – 60 % der Streams hatten identische Titel",
                "priority": "low",
            },
        ],
        "correlations": {"durationVsViewers": 0.42, "chatVsRetention": 0.61},
        "network": {"sent": 8, "received": 5, "sentViewers": 2840},
        "categoryRank": 12,
        "categoryTotal": 58,
        "audienceInsights": {
            "trends": {
                "watchTimeChange": 8.4,
                "conversionChange": None,
                "viewerReturnRate": 38.2,
                "viewerReturnChange": 4.7,
            },
            "distinctViewers": 412,
            "returnRateMethod": "distinct_rollup",
            "dataQuality": {
                "botFilterApplied": True,
                "watchTimeMethod": "real_samples",
                "watchTimeTrendAvailable": True,
                "viewerReturnTrendAvailable": True,
                "conversionTrendAvailable": False,
            },
        },
    }


def _get_sessions() -> list[dict[str, Any]]:
    sessions = []
    # 14 sessions over past 30 days, streaming Mon/Wed/Fri/Sat
    stream_days = [2, 4, 5, 7, 9, 11, 12, 14, 16, 18, 19, 21, 23, 25]
    viewer_bases = [
        410,
        390,
        365,
        420,
        375,
        395,
        445,
        360,
        385,
        400,
        430,
        370,
        355,
        410,
    ]
    peaks = [1020, 890, 780, 1087, 820, 940, 1050, 730, 860, 920, 1010, 790, 720, 960]
    durations = [
        13200,
        10800,
        14400,
        12600,
        10800,
        13500,
        14400,
        9000,
        12000,
        12600,
        13800,
        10800,
        9600,
        13200,
    ]
    followers_start = [
        8283,
        8297,
        8310,
        8323,
        8338,
        8351,
        8363,
        8379,
        8391,
        8402,
        8415,
        8430,
        8441,
        8454,
    ]

    titles = [
        "Deadlock Ranked Grind | Platin → Diamond",
        "Deadlock Ranked Grind | Platin → Diamond",
        "Ranked mit Freunden | !discord",
        "Deadlock Solo Q | Road to Diamond",
        "Deadlock Ranked Grind | Platin → Diamond",
        "Neue Patch-Analyse + Ranked | !guide",
        "Ranked mit Zuschauern | Komm rein!",
        "Deadlock Solo Q | Road to Diamond",
        "Deadlock Ranked Grind | Platin → Diamond",
        "Ranked mit Freunden | !discord",
        "Neue Patch-Analyse + Ranked | !guide",
        "Deadlock Solo Q | Road to Diamond",
        "Ranked mit Zuschauern | Komm rein!",
        "Deadlock Ranked Grind | Platin → Diamond",
    ]

    for i, (offset, base, peak, dur, fs, title) in enumerate(
        zip(stream_days, viewer_bases, peaks, durations, followers_start, titles, strict=False)
    ):
        chatters = int(base * 0.13 + 18)
        sessions.append(
            {
                "id": 1000 + i,
                "date": _date(offset),
                "startTime": _dt(offset, hour=19 + (i % 2)),
                "duration": dur,
                "startViewers": int(base * 0.7),
                "peakViewers": peak,
                "endViewers": int(base * 0.55),
                "avgViewers": base,
                "retention5m": round(83 - i * 0.4, 1),
                "retention10m": round(71 - i * 0.3, 1),
                "retention20m": round(58 - i * 0.2, 1),
                "dropoffPct": round(17 + i * 0.3, 1),
                "uniqueChatters": chatters,
                "firstTimeChatters": int(chatters * 0.28),
                "returningChatters": int(chatters * 0.72),
                "followersStart": fs,
                "followersEnd": fs + 11 + (i % 5),
                "title": title,
            }
        )
    return sessions


def get_monthly_stats() -> list[dict[str, Any]]:
    months = [
        ("März", 3, 2025,  8, 218,  610,  88,  7, 265),
        ("April", 4, 2025, 9, 248,  700,  98,  9, 288),
        ("Mai",   5, 2025, 10, 274,  780, 108, 11, 308),
        ("Juni",  6, 2025, 11, 298,  855, 118, 13, 325),
        ("Juli",  7, 2025, 12, 318,  920, 126, 14, 340),
        ("Aug",   8, 2025, 12, 336,  975, 130, 15, 354),
        ("Sep",   9, 2025, 13, 352, 1020, 134, 15, 366),
        ("Okt",  10, 2025, 13, 366, 1058, 136, 16, 376),
        ("Nov",  11, 2025, 14, 374, 1072, 138, 16, 384),
        ("Dez",  12, 2025, 12, 379, 1080, 132, 15, 388),
        ("Jan",   1, 2026, 14, 386, 1092, 140, 17, 393),
        ("Feb",   2, 2026, 15, 394, 1108, 144, 18, 401),
    ]
    result = []
    for label, month, year, streams, avg, peak, airtime, followers, chatters in months:
        result.append(
            {
                "year": year,
                "month": month,
                "monthLabel": label,
                "totalHoursWatched": round(avg * airtime, 0),
                "totalAirtime": airtime,
                "avgViewers": avg,
                "peakViewers": peak,
                "followerDelta": followers,
                "uniqueChatters": chatters,
                "streamCount": streams,
            }
        )
    return result


def get_weekday_stats() -> list[dict[str, Any]]:
    data = [
        (0, "Sonntag", 2, 2.5, 310, 650, 28),
        (1, "Montag", 3, 3.5, 372, 890, 42),
        (2, "Dienstag", 0, 0.0, 0, 0, 0),
        (3, "Mittwoch", 4, 3.2, 388, 940, 46),
        (4, "Donnerstag", 1, 2.8, 345, 780, 38),
        (5, "Freitag", 3, 3.8, 401, 1020, 48),
        (6, "Samstag", 2, 3.1, 423, 1087, 51),
    ]
    return [
        {
            "weekday": wd,
            "weekdayLabel": lbl,
            "streamCount": sc,
            "avgHours": ah,
            "avgViewers": av,
            "avgPeak": ap,
            "totalFollowers": tf,
        }
        for wd, lbl, sc, ah, av, ap, tf in data
    ]


def get_hourly_heatmap() -> list[dict[str, Any]]:
    rows = []
    for wd in range(7):
        for h in range(24):
            if 18 <= h <= 22 and wd in (1, 3, 5, 6):
                sc = 2 if wd in (5, 6) else 1
                av = 382 + (h - 18) * 15 - wd * 5
                ap = av + 180
            else:
                sc = 0
                av = 0
                ap = 0
            rows.append(
                {
                    "weekday": wd,
                    "hour": h,
                    "streamCount": sc,
                    "avgViewers": max(0, av),
                    "avgPeak": max(0, ap),
                }
            )
    return rows


def get_calendar_heatmap() -> list[dict[str, Any]]:
    today = datetime.now(UTC).date()
    rows = []
    base_date = today - timedelta(days=364)
    stream_value = 0
    for i in range(365):
        d = base_date + timedelta(days=i)
        wd = d.weekday()  # 0=Mon
        # Stream days: Mon(0), Wed(2), Fri(4), Sat(5)
        if wd in (0, 2, 4, 5) and i > 0:
            hours = round(2.8 + (hash(str(d)) % 20) / 10, 1)
            avg_v = 380 + (hash(str(d)) % 80) - 40
            stream_value = round(avg_v * hours, 0)
            sc = 1
        else:
            stream_value = 0
            hours = 0.0
            sc = 0
        rows.append(
            {
                "date": d.isoformat(),
                "value": stream_value,
                "streamCount": sc,
                "hoursWatched": hours,
            }
        )
    return rows


def get_chat_analytics() -> dict[str, Any]:
    return {
        "totalMessages": 84320,
        "totalChatterSessions": 634,
        "uniqueChatters": 634,
        "totalTrackedViewers": 910,
        "firstTimeChatters": 178,
        "returningChatters": 456,
        "messagesPerMinute": 14.2,
        "chatterReturnRate": 71.9,
        "chatPenetrationPct": 69.7,
        "chatPenetrationReliable": True,
        "messagesPer100ViewerMinutes": 482.3,
        "viewerMinutes": 17482.0,
        "legacyInteractionActivePerAvgViewer": 166.4,
        # Legacy compatibility
        "interactionRateActivePerViewer": 69.7,
        "interactionRateActivePerAvgViewer": 166.4,
        "interactionRateReliable": True,
        "activeChatters": 634,
        "activeRatio": 0.697,
        "lurkerCount": 276,
        "lurkerRatio": 0.303,
        "dataQuality": {
            "method": "real_samples",
            "coverage": 0.93,
            "sampleCount": 84320,
            "confidence": "high",
            "sessions": 16,
            "sessionsWithChat": 15,
            "chatSessionCoverage": 93.8,
            "chattersCoverage": 0.41,
            "chattersApiCoverage": 0.41,
            "passiveViewerSamples": 276,
            "viewerSampleCount": 17482,
            "viewerMinutesSource": "real_samples",
        },
        "topChatters": [
            {
                "login": "viewer_alpha",
                "totalMessages": 1840,
                "totalSessions": 13,
                "firstSeen": _date(90),
                "lastSeen": _date(2),
                "loyaltyScore": 94,
            },
            {
                "login": "deadlock_fan1",
                "totalMessages": 1290,
                "totalSessions": 11,
                "firstSeen": _date(85),
                "lastSeen": _date(4),
                "loyaltyScore": 88,
            },
            {
                "login": "proplayer_de",
                "totalMessages": 1105,
                "totalSessions": 12,
                "firstSeen": _date(70),
                "lastSeen": _date(2),
                "loyaltyScore": 86,
            },
            {
                "login": "chat_lurker",
                "totalMessages": 980,
                "totalSessions": 9,
                "firstSeen": _date(60),
                "lastSeen": _date(5),
                "loyaltyScore": 81,
            },
            {
                "login": "kappa_guy_de",
                "totalMessages": 870,
                "totalSessions": 10,
                "firstSeen": _date(55),
                "lastSeen": _date(7),
                "loyaltyScore": 79,
            },
            {
                "login": "gamerfan99",
                "totalMessages": 740,
                "totalSessions": 8,
                "firstSeen": _date(45),
                "lastSeen": _date(9),
                "loyaltyScore": 75,
            },
            {
                "login": "nachteule_gg",
                "totalMessages": 695,
                "totalSessions": 7,
                "firstSeen": _date(40),
                "lastSeen": _date(11),
                "loyaltyScore": 72,
            },
            {
                "login": "rankgrinder",
                "totalMessages": 612,
                "totalSessions": 8,
                "firstSeen": _date(35),
                "lastSeen": _date(4),
                "loyaltyScore": 70,
            },
            {
                "login": "twitch_regular",
                "totalMessages": 580,
                "totalSessions": 6,
                "firstSeen": _date(30),
                "lastSeen": _date(6),
                "loyaltyScore": 67,
            },
            {
                "login": "viewer_beta",
                "totalMessages": 520,
                "totalSessions": 5,
                "firstSeen": _date(25),
                "lastSeen": _date(8),
                "loyaltyScore": 63,
            },
        ],
        "messageTypes": [
            {"type": "text",    "count": 61420, "percentage": 72.8},
            {"type": "emote",   "count": 13490, "percentage": 16.0},
            {"type": "command", "count":  5618, "percentage":  6.7},
            {"type": "mention", "count":  2540, "percentage":  3.0},
            {"type": "link",    "count":  1252, "percentage":  1.5},
        ],
        "hourlyActivity": [
            {"hour":  0, "count":  210},
            {"hour":  1, "count":  145},
            {"hour":  2, "count":   88},
            {"hour":  3, "count":   42},
            {"hour":  4, "count":   28},
            {"hour":  5, "count":   19},
            {"hour":  6, "count":   24},
            {"hour":  7, "count":   38},
            {"hour":  8, "count":   62},
            {"hour":  9, "count":   95},
            {"hour": 10, "count":  148},
            {"hour": 11, "count":  210},
            {"hour": 12, "count":  285},
            {"hour": 13, "count":  342},
            {"hour": 14, "count":  398},
            {"hour": 15, "count":  465},
            {"hour": 16, "count":  612},
            {"hour": 17, "count":  845},
            {"hour": 18, "count": 1240},
            {"hour": 19, "count": 1820},
            {"hour": 20, "count": 2340},
            {"hour": 21, "count": 2180},
            {"hour": 22, "count": 1650},
            {"hour": 23, "count":  920},
        ],
    }


def get_viewer_overlap() -> list[dict[str, Any]]:
    peers = [
        ("deadlock_de_1", 142, 340, 0.418),
        ("deadlock_de_2", 118, 280, 0.421),
        ("deadlock_de_3", 95, 260, 0.365),
        ("fps_master_de", 87, 390, 0.223),
        ("ranked_grinder", 74, 310, 0.239),
        ("pro_viewer_de", 68, 420, 0.162),
        ("casual_gamer", 55, 210, 0.262),
        ("deadlock_fan", 49, 180, 0.272),
    ]
    return [
        {
            "streamerA": DEMO_STREAMER,
            "streamerB": b,
            "sharedChatters": sc,
            "totalChattersA": 634,
            "totalChattersB": tc,
            "overlapPercentage": round(op * 100, 1),
        }
        for b, sc, tc, op in peers
    ]


def get_tag_analysis() -> list[dict[str, Any]]:
    return [
        {
            "tagName": "Deadlock",
            "usageCount": 14,
            "avgViewers": 382,
            "avgRetention10m": 71.2,
            "avgFollowerGain": 13.1,
        },
        {
            "tagName": "Deutsch",
            "usageCount": 14,
            "avgViewers": 382,
            "avgRetention10m": 71.2,
            "avgFollowerGain": 13.1,
        },
        {
            "tagName": "FPS",
            "usageCount": 12,
            "avgViewers": 375,
            "avgRetention10m": 70.8,
            "avgFollowerGain": 12.4,
        },
        {
            "tagName": "PC",
            "usageCount": 14,
            "avgViewers": 382,
            "avgRetention10m": 71.2,
            "avgFollowerGain": 13.1,
        },
        {
            "tagName": "Ranked",
            "usageCount": 10,
            "avgViewers": 398,
            "avgRetention10m": 73.4,
            "avgFollowerGain": 14.2,
        },
        {
            "tagName": "Controller",
            "usageCount": 4,
            "avgViewers": 355,
            "avgRetention10m": 68.1,
            "avgFollowerGain": 10.8,
        },
        {
            "tagName": "Shooter",
            "usageCount": 8,
            "avgViewers": 371,
            "avgRetention10m": 70.1,
            "avgFollowerGain": 12.0,
        },
    ]


def _tag_analysis_extended() -> list[dict[str, Any]]:
    base = get_tag_analysis()
    extras = [
        ("up", 8.4, "19:00-22:00", 3.5, 4),
        ("up", 5.1, "18:00-21:00", 3.4, 4),
        ("stable", 1.2, "19:00-22:00", 3.4, 6),
        ("up", 3.3, "18:00-22:00", 3.5, 3),
        ("up", 12.7, "19:00-22:00", 3.6, 2),
        ("down", -4.2, "20:00-23:00", 3.2, 12),
        ("stable", 0.8, "19:00-22:00", 3.3, 5),
    ]
    result = []
    for tag, (trend, tv, slot, dur, rank) in zip(base, extras, strict=False):
        result.append(
            {
                **tag,
                "trend": trend,
                "trendValue": tv,
                "bestTimeSlot": slot,
                "avgStreamDuration": dur,
                "categoryRank": rank,
            }
        )
    return result


def get_tag_analysis_extended() -> list[dict[str, Any]]:
    return _tag_analysis_extended()


def _title_performance() -> list[dict[str, Any]]:
    return [
        {
            "title": "Deadlock Ranked Grind | Platin → Diamond",
            "usageCount": 6,
            "avgViewers": 398,
            "avgRetention10m": 73.1,
            "avgFollowerGain": 14.2,
            "peakViewers": 1087,
            "keywords": ["Ranked", "Platin", "Diamond", "Grind"],
        },
        {
            "title": "Neue Patch-Analyse + Ranked | !guide",
            "usageCount": 2,
            "avgViewers": 412,
            "avgRetention10m": 74.8,
            "avgFollowerGain": 15.6,
            "peakViewers": 1040,
            "keywords": ["Patch", "Analyse", "Guide", "Ranked"],
        },
        {
            "title": "Ranked mit Freunden | !discord",
            "usageCount": 2,
            "avgViewers": 377,
            "avgRetention10m": 69.8,
            "avgFollowerGain": 12.1,
            "peakViewers": 940,
            "keywords": ["Ranked", "Discord", "Freunde"],
        },
        {
            "title": "Ranked mit Zuschauern | Komm rein!",
            "usageCount": 2,
            "avgViewers": 390,
            "avgRetention10m": 72.3,
            "avgFollowerGain": 13.4,
            "peakViewers": 970,
            "keywords": ["Ranked", "Zuschauer"],
        },
        {
            "title": "Deadlock Solo Q | Road to Diamond",
            "usageCount": 3,
            "avgViewers": 368,
            "avgRetention10m": 68.9,
            "avgFollowerGain": 11.8,
            "peakViewers": 890,
            "keywords": ["Solo", "Diamond", "Road"],
        },
    ]


def get_title_performance() -> list[dict[str, Any]]:
    return _title_performance()


def get_rankings(metric: str = "viewers") -> list[dict[str, Any]]:
    data = {
        "viewers": [
            (1, "deadlock_de_top1", 892, "up", 12.4),
            (2, "fps_king_de", 721, "up", 4.1),
            (3, "ranked_master_de", 612, "same", 0.0),
            (4, "deadlock_de_2", 521, "down", -3.2),
            (5, "casual_fps_de", 487, "up", 6.8),
            (6, "fps_nerd_de", 445, "up", 2.1),
            (7, "deadlock_de_3", 430, "down", -1.4),
            (8, "pro_gamer_de", 408, "same", 0.0),
            (9, DEMO_STREAMER, 382, "up", 6.8),
            (10, "new_streamer_de", 371, "up", 18.3),
        ],
        "growth": [
            (1, "new_streamer_de", 42.1, "up", 8.3),
            (2, "deadlock_de_top1", 18.7, "up", 2.1),
            (3, DEMO_STREAMER, 12.4, "up", 1.2),
            (4, "fps_king_de", 10.8, "up", 0.4),
            (5, "ranked_master_de", 8.2, "same", 0.0),
            (6, "casual_fps_de", 7.4, "up", 1.1),
            (7, "fps_nerd_de", 5.1, "down", -0.3),
            (8, "deadlock_de_2", 3.8, "down", -1.2),
            (9, "deadlock_de_3", 2.1, "same", 0.0),
            (10, "pro_gamer_de", -0.4, "down", -2.1),
        ],
        "retention": [
            (1, "ranked_master_de", 78.4, "up", 1.2),
            (2, "deadlock_de_top1", 76.1, "up", 0.8),
            (3, "fps_king_de", 74.9, "same", 0.0),
            (4, DEMO_STREAMER, 71.2, "up", 1.9),
            (5, "casual_fps_de", 69.4, "up", 0.4),
            (6, "fps_nerd_de", 68.8, "down", -0.3),
            (7, "pro_gamer_de", 67.2, "same", 0.0),
            (8, "deadlock_de_2", 65.9, "down", -1.1),
            (9, "deadlock_de_3", 64.1, "up", 0.2),
            (10, "new_streamer_de", 61.8, "up", 3.4),
        ],
        "chat": [
            (1, "deadlock_de_top1", 21.4, "up", 2.8),
            (2, "fps_king_de", 18.9, "up", 1.1),
            (3, "casual_fps_de", 17.2, "same", 0.0),
            (4, DEMO_STREAMER, 14.2, "up", 1.4),
            (5, "ranked_master_de", 13.8, "down", -0.2),
            (6, "fps_nerd_de", 12.4, "up", 0.8),
            (7, "deadlock_de_2", 11.9, "same", 0.0),
            (8, "pro_gamer_de", 11.2, "down", -0.4),
            (9, "deadlock_de_3", 10.8, "up", 0.1),
            (10, "new_streamer_de", 9.4, "up", 2.2),
        ],
    }
    rows = data.get(metric, data["viewers"])
    return [
        {"rank": rank, "login": login, "value": value, "trend": trend, "trendValue": tv}
        for rank, login, value, trend, tv in rows
    ]


def get_category_comparison() -> dict[str, Any]:
    return {
        "yourStats": {
            "avgViewers": 382,
            "peakViewers": 1087,
            "retention10m": 71.2,
            "chatHealth": 74,
        },
        "categoryAvg": {
            "avgViewers": 284,
            "peakViewers": 780,
            "retention10m": 63.4,
            "chatHealth": 61,
        },
        "percentiles": {
            "avgViewers": 79,
            "peakViewers": 82,
            "retention10m": 74,
            "chatHealth": 77,
        },
        "categoryRank": 12,
        "categoryTotal": 58,
    }


def _watch_time_distribution() -> dict[str, Any]:
    return {
        "under5min": 12.4,
        "min5to15": 18.7,
        "min15to30": 21.3,
        "min30to60": 26.8,
        "over60min": 20.8,
        "avgWatchTime": 38.4,
        "medianWatchTime": 29.1,
        "sessionCount": 14,
        "previous": {
            "under5min": 14.1,
            "min5to15": 19.8,
            "min15to30": 22.1,
            "min30to60": 25.2,
            "over60min": 18.8,
            "avgWatchTime": 35.2,
            "medianWatchTime": 26.8,
            "sessionCount": 13,
        },
        "deltas": {
            "under5min": -1.7,
            "min5to15": -1.1,
            "min15to30": -0.8,
            "min30to60": 1.6,
            "over60min": 2.0,
            "avgWatchTime": 3.2,
        },
    }


def get_watch_time_distribution() -> dict[str, Any]:
    return _watch_time_distribution()


def _follower_funnel() -> dict[str, Any]:
    return {
        "uniqueViewers": 1840,
        "returningViewers": 680,
        "newFollowers": 183,
        "netFollowerDelta": 167,
        "conversionRate": 9.95,
        "avgTimeToFollow": 18.4,
        "followersBySource": {"organic": 124, "raids": 41, "hosts": 8, "other": 10},
    }


def get_follower_funnel() -> dict[str, Any]:
    return _follower_funnel()


def get_audience_insights() -> dict[str, Any]:
    return {
        "trends": {
            "watchTimeChange": 8.4,
            "conversionChange": None,
            "viewerReturnRate": 38.2,
            "viewerReturnChange": 4.7,
        },
        "distinctViewers": 412,
        "returnRateMethod": "distinct_rollup",
        "dataQuality": {
            "botFilterApplied": True,
            "watchTimeMethod": "real_samples",
            "watchTimeTrendAvailable": True,
            "viewerReturnTrendAvailable": True,
            "conversionTrendAvailable": False,
        },
    }


def get_audience_demographics() -> dict[str, Any]:
    return {
        "estimatedRegions": [
            {"region": "DE", "percentage": 58.4},
            {"region": "AT", "percentage": 12.1},
            {"region": "CH", "percentage": 8.7},
            {"region": "EU (andere)", "percentage": 14.2},
            {"region": "Sonstige", "percentage": 6.6},
        ],
        "viewerTypes": [
            {"label": "Loyale Stamm-Viewer", "percentage": 38.2},
            {"label": "Wiederkehrende Viewer", "percentage": 29.4},
            {"label": "Neue Viewer", "percentage": 21.8},
            {"label": "Zufällige Besucher", "percentage": 10.6},
        ],
        "activityPattern": "weekend-heavy",
        "primaryLanguage": "German",
        "languageConfidence": 92.0,
        "peakActivityHours": [19, 20, 21, 22],
        "chatPenetrationPct": 68.5,
        "chatPenetrationReliable": True,
        "messagesPer100ViewerMinutes": 476.8,
        "viewerMinutes": 17120.0,
        "legacyInteractionActivePerAvgViewer": 161.2,
        "interactiveRate": 68.5,
        "interactionRateActivePerViewer": 68.5,
        "interactionRateActivePerAvgViewer": 161.2,
        "interactionRateReliable": True,
        "loyaltyScore": 72,
        "timezone": "Europe/Berlin",
        "dataQuality": {
            "confidence": "high",
            "sessions": 16,
            "method": "real_samples",
            "peakMethod": "real_samples",
            "coverage": 0.67,
            "sampleCount": 812,
            "peakSessionCount": 16,
            "peakSessionsWithActivity": 13,
            "interactiveSampleCount": 623,
            "interactionCoverage": 0.39,
            "chattersCoverage": 0.39,
            "chattersApiCoverage": 0.39,
            "passiveViewerSamples": 287,
            "viewerSampleCount": 17120,
            "viewerMinutesSource": "real_samples",
            "sessionsWithChat": 15,
            "chatSessionCoverage": 93.8,
        },
    }


def get_viewer_timeline(days: int = 30) -> list[dict[str, Any]]:
    result = []
    today = datetime.now(UTC)
    for i in range(min(days, 30), 0, -1):
        dt = today - timedelta(days=i)
        wd = dt.weekday()
        if wd in (0, 2, 4, 5):
            avg = 382 + (hash(str(dt.date())) % 60) - 30
            peak = avg + 180 + (hash(str(dt.date()) + "p") % 100)
            mn = max(50, avg - 120)
        else:
            avg = 0
            peak = 0
            mn = 0
        result.append(
            {
                "timestamp": dt.replace(hour=20, minute=0, second=0, microsecond=0).isoformat(),
                "avgViewers": avg,
                "peakViewers": peak,
                "minViewers": mn,
                "samples": 4 if avg > 0 else 0,
            }
        )
    return result


def get_category_leaderboard() -> dict[str, Any]:
    entries = [
        (1, "deadlock_de_top1", 892, 2140, True),
        (2, "fps_king_de", 721, 1820, True),
        (3, "ranked_master_de", 612, 1540, True),
        (4, "deadlock_de_2", 521, 1290, True),
        (5, "casual_fps_de", 487, 1180, False),
        (6, "fps_nerd_de", 445, 1090, True),
        (7, "deadlock_de_3", 430, 1060, False),
        (8, "pro_gamer_de", 408, 1010, True),
        (9, DEMO_STREAMER, 382, 940, True),
        (10, "new_streamer_de", 371, 910, False),
        (11, "fr_player", 348, 870, False),
        (12, "en_streamer", 332, 840, False),
    ]
    return {
        "leaderboard": [
            {
                "rank": r,
                "streamer": s,
                "avgViewers": av,
                "peakViewers": pv,
                "isPartner": ip,
                "isYou": s == DEMO_STREAMER,
            }
            for r, s, av, pv, ip in entries
        ],
        "totalStreamers": 58,
        "yourRank": 9,
    }


def get_monetization() -> dict[str, Any]:
    return {
        "ads": {
            "total": 38,
            "auto": 22,
            "manual": 16,
            "sessions_with_ads": 11,
            "avg_duration_s": 31.4,
            "avg_viewer_drop_pct": 8.2,
            "worst_ads": [
                {
                    "started_at": _dt(5, 20),
                    "duration_s": 90,
                    "drop_pct": 18.4,
                    "is_automatic": True,
                },
                {
                    "started_at": _dt(12, 21),
                    "duration_s": 60,
                    "drop_pct": 14.1,
                    "is_automatic": False,
                },
                {
                    "started_at": _dt(19, 20),
                    "duration_s": 60,
                    "drop_pct": 12.8,
                    "is_automatic": True,
                },
            ],
        },
        "hype_train": {
            "total": 4,
            "avg_level": 2.8,
            "max_level": 4,
            "avg_duration_s": 312,
        },
        "bits": {"total": 28400, "cheer_events": 47},
        "subs": {"total_events": 312, "gifted": 84},
        "window_days": 30,
    }


def get_category_timings() -> dict[str, Any]:
    hourly = []
    for h in range(24):
        if 17 <= h <= 23:
            median = 380 + (h - 17) * 20
            p25 = median - 80
            p75 = median + 120
            sc = 12
            samp = 48
        elif 12 <= h <= 16:
            median = 220
            p25 = 160
            p75 = 290
            sc = 6
            samp = 18
        else:
            median = None
            p25 = None
            p75 = None
            sc = 0
            samp = 0
        hourly.append(
            {
                "hour": h,
                "median": median,
                "p25": p25,
                "p75": p75,
                "streamer_count": sc,
                "sample_count": samp,
            }
        )
    weekly_data = [
        (0, "Montag", 410, 320, 530),
        (1, "Dienstag", 380, 300, 490),
        (2, "Mittwoch", 420, 340, 560),
        (3, "Donnerstag", 360, 280, 470),
        (4, "Freitag", 480, 390, 640),
        (5, "Samstag", 520, 420, 680),
        (6, "Sonntag", 390, 310, 510),
    ]
    weekly = [
        {
            "weekday": wd,
            "label": lbl,
            "median": med,
            "p25": p25,
            "p75": p75,
            "streamer_count": 24,
            "sample_count": 96,
        }
        for wd, lbl, med, p25, p75 in weekly_data
    ]
    return {
        "hourly": hourly,
        "weekly": weekly,
        "total_streamers": 58,
        "window_days": 30,
        "method": "median",
    }


def get_category_activity_series() -> dict[str, Any]:
    hourly = []
    for h in range(24):
        if 17 <= h <= 23:
            cat_avg = 320 + (h - 17) * 18
            tr_avg = 382 + (h - 17) * 12
            cat_peak = cat_avg + 240
            tr_peak = tr_avg + 200
            c_samp = 380
            t_samp = 48
        elif 12 <= h <= 16:
            cat_avg = 180
            tr_avg = 0
            cat_peak = 280
            tr_peak = 0
            c_samp = 120
            t_samp = 0
        else:
            cat_avg = 0
            tr_avg = 0
            cat_peak = 0
            tr_peak = 0
            c_samp = 0
            t_samp = 0
        hourly.append(
            {
                "hour": h,
                "label": f"{h:02d}:00",
                "categoryAvg": cat_avg or None,
                "trackedAvg": tr_avg or None,
                "categoryPeak": cat_peak or None,
                "trackedPeak": tr_peak or None,
                "categorySamples": c_samp,
                "trackedSamples": t_samp,
            }
        )
    weekly_data = [
        (0, "Mo", 380, 372, 890, 860),
        (1, "Di", 340, None, 780, None),
        (2, "Mi", 390, 388, 920, 940),
        (3, "Do", 320, 345, 740, 780),
        (4, "Fr", 450, 401, 1040, 1020),
        (5, "Sa", 480, 423, 1120, 1087),
        (6, "So", 360, 310, 840, 650),
    ]
    weekly = [
        {
            "weekday": wd,
            "label": lbl,
            "categoryAvg": ca,
            "trackedAvg": ta,
            "categoryPeak": cp,
            "trackedPeak": tp,
            "categorySamples": 200,
            "trackedSamples": 14 if ta else 0,
        }
        for wd, lbl, ca, ta, cp, tp in weekly_data
    ]
    return {"hourly": hourly, "weekly": weekly, "windowDays": 30, "source": "mixed"}


def get_coaching() -> dict[str, Any]:
    return {
        "streamer": DEMO_STREAMER,
        "days": 30,
        "empty": False,
        "efficiency": {
            "viewerHoursPerStreamHour": 27.4,
            "categoryAvg": 21.8,
            "topPerformers": [
                {"streamer": "deadlock_de_top1", "ratio": 42.1},
                {"streamer": "fps_king_de", "ratio": 36.8},
                {"streamer": DEMO_STREAMER, "ratio": 27.4},
            ],
            "percentile": 74,
            "totalStreamHours": 194.8,
            "totalViewerHours": 5342,
        },
        "titleAnalysis": {
            "yourTitles": [
                {
                    "title": "Deadlock Ranked Grind | Platin → Diamond",
                    "avgViewers": 398,
                    "peakViewers": 1087,
                    "chatters": 52,
                    "usageCount": 6,
                },
                {
                    "title": "Neue Patch-Analyse + Ranked | !guide",
                    "avgViewers": 412,
                    "peakViewers": 1040,
                    "chatters": 55,
                    "usageCount": 2,
                },
                {
                    "title": "Ranked mit Zuschauern | Komm rein!",
                    "avgViewers": 390,
                    "peakViewers": 970,
                    "chatters": 50,
                    "usageCount": 2,
                },
            ],
            "categoryTopTitles": [
                {
                    "title": "Deadlock HIGH ELO | !discord",
                    "streamer": "deadlock_de_top1",
                    "avgViewers": 892,
                },
                {
                    "title": "Ranked Push | Platin → Diamond",
                    "streamer": "fps_king_de",
                    "avgViewers": 721,
                },
            ],
            "yourMissingPatterns": ["!discord im Titel", "Rank-Fortschritt sichtbar"],
            "topPerformerPatterns": [
                "!discord im Titel",
                "HIGH ELO",
                "Rank-Ziel explizit",
            ],
            "varietyPct": 35.7,
            "uniqueTitleCount": 5,
            "totalSessionCount": 14,
            "avgPeerVarietyPct": 42.1,
            "peerVariety": [
                {
                    "streamer": "deadlock_de_top1",
                    "uniqueTitles": 9,
                    "totalSessions": 16,
                    "varietyPct": 56.3,
                },
                {
                    "streamer": DEMO_STREAMER,
                    "uniqueTitles": 5,
                    "totalSessions": 14,
                    "varietyPct": 35.7,
                },
            ],
        },
        "scheduleOptimizer": {
            "sweetSpots": [
                {
                    "weekday": 5,
                    "hour": 17,
                    "categoryViewers": 1840,
                    "competitors": 4,
                    "opportunityScore": 88,
                },
                {
                    "weekday": 6,
                    "hour": 17,
                    "categoryViewers": 1920,
                    "competitors": 5,
                    "opportunityScore": 84,
                },
                {
                    "weekday": 3,
                    "hour": 19,
                    "categoryViewers": 1480,
                    "competitors": 3,
                    "opportunityScore": 78,
                },
            ],
            "yourCurrentSlots": [
                {"weekday": 0, "hour": 19, "count": 3},
                {"weekday": 2, "hour": 19, "count": 4},
                {"weekday": 4, "hour": 20, "count": 3},
                {"weekday": 5, "hour": 20, "count": 2},
            ],
            "competitionHeatmap": [],
        },
        "durationAnalysis": {
            "buckets": [
                {
                    "label": "< 2h",
                    "streamCount": 1,
                    "avgViewers": 310,
                    "avgChatters": 32,
                    "avgRetention5m": 78.1,
                    "efficiencyRatio": 18.4,
                },
                {
                    "label": "2-3h",
                    "streamCount": 3,
                    "avgViewers": 362,
                    "avgChatters": 44,
                    "avgRetention5m": 82.4,
                    "efficiencyRatio": 24.8,
                },
                {
                    "label": "3-4h",
                    "streamCount": 7,
                    "avgViewers": 388,
                    "avgChatters": 49,
                    "avgRetention5m": 84.2,
                    "efficiencyRatio": 27.9,
                },
                {
                    "label": "4-5h",
                    "streamCount": 3,
                    "avgViewers": 395,
                    "avgChatters": 51,
                    "avgRetention5m": 83.8,
                    "efficiencyRatio": 28.4,
                },
                {
                    "label": "> 5h",
                    "streamCount": 0,
                    "avgViewers": 0,
                    "avgChatters": 0,
                    "avgRetention5m": 0.0,
                    "efficiencyRatio": 0.0,
                },
            ],
            "optimalLabel": "3-4h",
            "currentAvgHours": 3.4,
            "correlation": 0.42,
        },
        "crossCommunity": {
            "totalUniqueChatters": 634,
            "chatterSources": [
                {
                    "sourceStreamer": "deadlock_de_top1",
                    "sharedChatters": 142,
                    "percentage": 22.4,
                },
                {
                    "sourceStreamer": "deadlock_de_2",
                    "sharedChatters": 118,
                    "percentage": 18.6,
                },
                {
                    "sourceStreamer": "fps_king_de",
                    "sharedChatters": 95,
                    "percentage": 15.0,
                },
            ],
            "isolatedChatters": 187,
            "isolatedPercentage": 29.5,
            "ecosystemSummary": "Gut vernetzt im deutschen Deadlock-Ökosystem. ~70 % deiner Chatter schauen auch andere Partner.",
        },
        "tagOptimization": {
            "yourTags": [
                {
                    "tags": "Deadlock, Deutsch, Ranked",
                    "avgViewers": 398,
                    "usageCount": 10,
                },
                {"tags": "Deadlock, Deutsch, FPS", "avgViewers": 375, "usageCount": 4},
            ],
            "categoryBestTags": [
                {
                    "tags": "Deadlock, Deutsch, HIGH ELO",
                    "avgViewers": 620,
                    "streamerCount": 3,
                },
                {
                    "tags": "Deadlock, Ranked, !discord",
                    "avgViewers": 540,
                    "streamerCount": 5,
                },
            ],
            "missingHighPerformers": ["HIGH ELO", "!discord im Tag"],
            "underperformingTags": ["Controller"],
        },
        "retentionCoaching": {
            "your5mRetention": 83.7,
            "category5mRetention": 76.2,
            "yourViewerCurve": [
                {"minute": m, "avgViewerPct": max(40, 100 - m * 2.1 - (m > 5) * 3)}
                for m in range(0, 31, 5)
            ],
            "topPerformerCurve": [
                {"minute": m, "avgViewerPct": max(50, 100 - m * 1.6 - (m > 5) * 2)}
                for m in range(0, 31, 5)
            ],
            "criticalDropoffMinute": 4,
        },
        "doubleStreamDetection": {
            "detected": False,
            "count": 0,
            "occurrences": [],
            "singleDayAvg": 382,
            "doubleDayAvg": 0,
        },
        "chatConcentration": {
            "totalChatters": 634,
            "totalMessages": 84320,
            "msgsPerChatter": 133,
            "loyaltyBuckets": {
                "Einmalig (1-2)": {"count": 178, "pct": 28.1, "messages": 267},
                "Gelegentlich (3-9)": {"count": 212, "pct": 33.4, "messages": 8480},
                "Regelmäßig (10-29)": {"count": 148, "pct": 23.3, "messages": 20720},
                "Stamm-Viewer (30+)": {"count": 96, "pct": 15.1, "messages": 54853},
            },
            "topChatters": [
                {
                    "login": "viewer_alpha",
                    "messages": 1840,
                    "sessions": 13,
                    "sharePct": 2.18,
                    "cumulativePct": 2.18,
                },
                {
                    "login": "deadlock_fan1",
                    "messages": 1290,
                    "sessions": 11,
                    "sharePct": 1.53,
                    "cumulativePct": 3.71,
                },
            ],
            "concentrationIndex": 0.48,
            "top1Pct": 6.8,
            "top3Pct": 14.2,
            "ownOneTimerPct": 28.1,
            "avgPeerOneTimerPct": 32.4,
        },
        "raidNetwork": {
            "totalSent": 8,
            "totalReceived": 5,
            "totalSentViewers": 2840,
            "totalReceivedViewers": 1620,
            "avgSentViewers": 355,
            "avgReceivedViewers": 324,
            "reciprocityRatio": 0.625,
            "mutualPartners": 3,
            "totalPartners": 6,
            "partners": [
                {
                    "login": "deadlock_de_1",
                    "sentCount": 3,
                    "sentAvgViewers": 380,
                    "receivedCount": 2,
                    "receivedAvgViewers": 340,
                    "reciprocity": "mutual",
                    "balance": 1,
                },
                {
                    "login": "fps_king_de",
                    "sentCount": 2,
                    "sentAvgViewers": 350,
                    "receivedCount": 2,
                    "receivedAvgViewers": 310,
                    "reciprocity": "mutual",
                    "balance": 0,
                },
                {
                    "login": "deadlock_de_3",
                    "sentCount": 2,
                    "sentAvgViewers": 340,
                    "receivedCount": 0,
                    "receivedAvgViewers": 0,
                    "reciprocity": "sentOnly",
                    "balance": 2,
                },
                {
                    "login": "casual_fps_de",
                    "sentCount": 1,
                    "sentAvgViewers": 360,
                    "receivedCount": 1,
                    "receivedAvgViewers": 290,
                    "reciprocity": "mutual",
                    "balance": 0,
                },
                {
                    "login": "fps_nerd_de",
                    "sentCount": 0,
                    "sentAvgViewers": 0,
                    "receivedCount": 2,
                    "receivedAvgViewers": 310,
                    "reciprocity": "receivedOnly",
                    "balance": -2,
                },
            ],
        },
        "peerComparison": {
            "ownData": {
                "login": DEMO_STREAMER,
                "sessions": 14,
                "avgViewers": 382,
                "maxPeak": 1087,
                "avgHours": 3.4,
                "avgChatters": 45,
                "retention5m": 83.7,
                "totalHours": 47.6,
                "followsGained": 183,
                "uniqueTitles": 5,
                "titleVariety": 35.7,
            },
            "ownRank": 9,
            "totalStreamers": 58,
            "similarPeers": [
                {
                    "login": "fps_nerd_de",
                    "sessions": 13,
                    "avgViewers": 445,
                    "maxPeak": 1090,
                    "avgHours": 3.2,
                    "avgChatters": 48,
                    "retention5m": 84.1,
                    "totalHours": 41.6,
                    "followsGained": 145,
                    "uniqueTitles": 6,
                    "titleVariety": 46.2,
                },
                {
                    "login": "deadlock_de_3",
                    "sessions": 15,
                    "avgViewers": 430,
                    "maxPeak": 1060,
                    "avgHours": 3.5,
                    "avgChatters": 46,
                    "retention5m": 82.9,
                    "totalHours": 52.5,
                    "followsGained": 138,
                    "uniqueTitles": 5,
                    "titleVariety": 33.3,
                },
            ],
            "aspirationalPeers": [
                {
                    "login": "deadlock_de_top1",
                    "sessions": 16,
                    "avgViewers": 892,
                    "maxPeak": 2140,
                    "avgHours": 4.1,
                    "avgChatters": 98,
                    "retention5m": 88.4,
                    "totalHours": 65.6,
                    "followsGained": 412,
                    "uniqueTitles": 9,
                    "titleVariety": 56.3,
                },
            ],
            "metricsRanked": {
                "avgViewers": {"rank": 9, "total": 58, "value": 382},
                "retention5m": {"rank": 6, "total": 58, "value": 83.7},
                "growth": {"rank": 3, "total": 58, "value": 12.4},
                "chatHealth": {"rank": 10, "total": 58, "value": 74},
            },
            "gapToNext": {
                "login": "pro_gamer_de",
                "avgViewersDiff": 26,
                "chatDiff": 3,
                "retentionDiff": 0.6,
            },
        },
        "competitionDensity": {
            "hourly": [
                {
                    "hour": h,
                    "activeStreamers": max(0, 18 - abs(h - 20) * 2),
                    "avgViewers": max(0, 380 - abs(h - 20) * 40),
                    "avgPeak": max(0, 920 - abs(h - 20) * 80),
                    "opportunityScore": max(0, 60 - abs(h - 17) * 8),
                    "yourData": {
                        "count": 1,
                        "avgViewers": 382,
                        "avgPeak": 940,
                        "avgChatters": 45,
                    }
                    if h in (19, 20)
                    else None,
                }
                for h in range(24)
            ],
            "weekly": [
                {
                    "weekday": wd,
                    "weekdayLabel": lbl,
                    "activeStreamers": sc,
                    "avgViewers": av,
                    "yourData": {"count": yc, "avgViewers": ya, "avgPeak": yp} if yc else None,
                }
                for wd, lbl, sc, av, yc, ya, yp in [
                    (0, "Montag", 14, 310, 3, 372, 890),
                    (1, "Dienstag", 12, 280, 0, 0, 0),
                    (2, "Mittwoch", 16, 330, 4, 388, 940),
                    (3, "Donnerstag", 11, 260, 1, 345, 780),
                    (4, "Freitag", 18, 380, 3, 401, 1020),
                    (5, "Samstag", 20, 420, 2, 423, 1087),
                    (6, "Sonntag", 10, 280, 2, 310, 650),
                ]
            ],
            "sweetSpots": [
                {
                    "hour": 17,
                    "activeStreamers": 8,
                    "avgViewers": 1840,
                    "avgPeak": 2100,
                    "opportunityScore": 88,
                    "yourData": None,
                },
                {
                    "hour": 18,
                    "activeStreamers": 10,
                    "avgViewers": 1920,
                    "avgPeak": 2240,
                    "opportunityScore": 82,
                    "yourData": None,
                },
            ],
        },
        "recommendations": [
            {
                "priority": "high",
                "category": "Schedule",
                "icon": "⏰",
                "title": "Früher starten: Samstag 17:00 testen",
                "description": "Samstag 17–19 Uhr hat die niedrigste Konkurrenz-Dichte aber hohe Viewer-Nachfrage in der Kategorie.",
                "estimatedImpact": "+15–25 % Avg Viewer",
                "evidence": "12 Streams in diesem Slot haben 21 % mehr Avg Viewer als deine aktuellen Samstag-Starts.",
            },
            {
                "priority": "medium",
                "category": "Titel",
                "icon": "✏️",
                "title": "!discord in Titel aufnehmen",
                "description": "Top-Performer nutzen !discord-Referenz im Titel. Zeigt Community-Stärke und konvertiert besser.",
                "estimatedImpact": "+8–12 % Follower-Conversion",
                "evidence": "Streamer mit !discord im Titel haben 18 % höhere Conversion-Rate.",
            },
            {
                "priority": "medium",
                "category": "Retention",
                "icon": "📈",
                "title": "Erste 5 Minuten straffen",
                "description": "Kritischer Dropoff in Minute 3–5. Kürze Intro, starte früher mit Gameplay.",
                "estimatedImpact": "+3–5 % Retention",
                "evidence": "Deine 5m-Retention ist gut (83 %), aber Minute 3–5 zeigt messbaren Einbruch.",
            },
            {
                "priority": "low",
                "category": "Titel",
                "icon": "🎯",
                "title": "Mehr Titel-Variation einsetzen",
                "description": "35 % Titel-Variety vs. 42 % Kategorie-Durchschnitt. Abwechslung signalisiert frischen Content.",
                "estimatedImpact": "+5 % Neue Viewer",
                "evidence": "Peers mit 50 %+ Variety gewinnen 22 % mehr Erst-Viewer pro Stream.",
            },
        ],
        "aiSummary": (
            "**Deadlock_DE_Demo** ist ein gut etablierter Mid-Tier-Streamer im deutschen Deadlock-Ökosystem. "
            "Deine Retention (71 % bei 10 min) liegt 12 % über dem Kategorie-Durchschnitt – ein starkes Fundament. "
            "Das größte Wachstumspotenzial liegt im **Schedule-Timing**: Samstag 17:00 Uhr hat aktuell wenig Konkurrenz, "
            "aber hohe Viewer-Nachfrage. Dein Raid-Netzwerk ist aktiv und Community-gesund. "
            "Kurzfristig: **!discord in Titel aufnehmen** und **frühere Samstag-Starts testen**."
        ),
    }


def get_lurker_analysis() -> dict:
    return {
        "dataAvailable": True,
        "regularLurkers": [
            {"login": "silent_watcher_42", "lurkSessions": 18, "firstSeen": "2025-10-01T18:00:00", "lastSeen": "2025-12-28T21:00:00"},
            {"login": "nightowl_lurk", "lurkSessions": 15, "firstSeen": "2025-09-15T19:00:00", "lastSeen": "2025-12-30T22:00:00"},
            {"login": "bgstream_pro", "lurkSessions": 12, "firstSeen": "2025-10-10T17:00:00", "lastSeen": "2025-12-25T20:00:00"},
            {"login": "couch_gaming", "lurkSessions": 11, "firstSeen": "2025-11-01T20:00:00", "lastSeen": "2025-12-29T21:00:00"},
            {"login": "lazy_sunday_fan", "lurkSessions": 10, "firstSeen": "2025-10-20T15:00:00", "lastSeen": "2025-12-27T16:00:00"},
            {"login": "silent_fan_de", "lurkSessions": 9, "firstSeen": "2025-11-05T18:00:00", "lastSeen": "2025-12-26T19:00:00"},
            {"login": "zapperghost", "lurkSessions": 8, "firstSeen": "2025-09-01T20:00:00", "lastSeen": "2025-12-20T21:00:00"},
            {"login": "background_andy", "lurkSessions": 7, "firstSeen": "2025-10-14T17:00:00", "lastSeen": "2025-12-22T18:00:00"},
            {"login": "muted_observer", "lurkSessions": 6, "firstSeen": "2025-11-10T19:00:00", "lastSeen": "2025-12-24T20:00:00"},
            {"login": "shadow_viewer99", "lurkSessions": 5, "firstSeen": "2025-11-20T18:00:00", "lastSeen": "2025-12-15T19:00:00"},
        ],
        "lurkerStats": {"ratio": 0.342, "avgSessions": 7.4, "totalLurkers": 127, "totalViewers": 371},
        "conversionStats": {"rate": 0.083, "eligible": 84, "converted": 7},
    }


def get_raid_retention() -> dict:
    return {
        "dataAvailable": True,
        "summary": {"avgRetentionPct": 41.2, "avgConversionPct": 18.7, "totalNewChatters": 43, "raidCount": 5},
        "raids": [
            {"raidId": 101, "toBroadcaster": "competitive_player_eu", "viewersSent": 140, "executedAt": "2025-12-28T22:15:00", "chattersAt5m": 85, "chattersAt15m": 71, "chattersAt30m": 62, "retention30mPct": 44.3, "newChatters": 12, "chatterConversionPct": 19.4, "knownFromRaider": 28},
            {"raidId": 102, "toBroadcaster": "game_pro_streamer", "viewersSent": 95, "executedAt": "2025-12-21T21:30:00", "chattersAt5m": 55, "chattersAt15m": 43, "chattersAt30m": 37, "retention30mPct": 38.9, "newChatters": 8, "chatterConversionPct": 21.6, "knownFromRaider": 15},
            {"raidId": 103, "toBroadcaster": "deadlock_esports", "viewersSent": 210, "executedAt": "2025-12-14T20:00:00", "chattersAt5m": 130, "chattersAt15m": 98, "chattersAt30m": 82, "retention30mPct": 39.0, "newChatters": 18, "chatterConversionPct": 22.0, "knownFromRaider": 45},
            {"raidId": 104, "toBroadcaster": "relaxed_gamer_de", "viewersSent": 70, "executedAt": "2025-12-07T23:00:00", "chattersAt5m": 38, "chattersAt15m": 31, "chattersAt30m": 28, "retention30mPct": 40.0, "newChatters": 5, "chatterConversionPct": 17.9, "knownFromRaider": 10},
            {"raidId": 105, "toBroadcaster": "fps_master_twitch", "viewersSent": 180, "executedAt": "2025-11-30T21:45:00", "chattersAt5m": 110, "chattersAt15m": 88, "chattersAt30m": 76, "retention30mPct": 42.2, "newChatters": 14, "chatterConversionPct": 18.4, "knownFromRaider": 38},
        ],
    }


def get_viewer_profiles() -> dict:
    return {
        "dataAvailable": True,
        "profiles": {"exclusive": 87, "loyalMulti": 112, "casual": 134, "explorer": 23, "passive": 127, "total": 483},
        "exclusivityDistribution": [
            {"streamerCount": 1, "viewerCount": 87},
            {"streamerCount": 2, "viewerCount": 68},
            {"streamerCount": 3, "viewerCount": 44},
            {"streamerCount": 4, "viewerCount": 31},
            {"streamerCount": 5, "viewerCount": 24},
            {"streamerCount": 6, "viewerCount": 18},
            {"streamerCount": 7, "viewerCount": 12},
            {"streamerCount": 8, "viewerCount": 9},
            {"streamerCount": 10, "viewerCount": 7},
            {"streamerCount": 15, "viewerCount": 4},
            {"streamerCount": 20, "viewerCount": 3},
        ],
    }


def get_audience_sharing() -> dict:
    return {
        "dataAvailable": True,
        "current": [
            {"streamer": "competitive_player_eu", "sharedViewers": 142, "inflow": 18, "outflow": 7, "jaccardSimilarity": 0.21},
            {"streamer": "game_pro_streamer", "sharedViewers": 98, "inflow": 12, "outflow": 14, "jaccardSimilarity": 0.15},
            {"streamer": "deadlock_esports", "sharedViewers": 76, "inflow": 9, "outflow": 5, "jaccardSimilarity": 0.11},
            {"streamer": "fps_master_twitch", "sharedViewers": 54, "inflow": 22, "outflow": 3, "jaccardSimilarity": 0.09},
            {"streamer": "relaxed_gamer_de", "sharedViewers": 41, "inflow": 4, "outflow": 11, "jaccardSimilarity": 0.07},
            {"streamer": "speedrun_king", "sharedViewers": 33, "inflow": 3, "outflow": 8, "jaccardSimilarity": 0.05},
            {"streamer": "nightly_grind", "sharedViewers": 28, "inflow": 7, "outflow": 2, "jaccardSimilarity": 0.04},
            {"streamer": "chill_de_stream", "sharedViewers": 19, "inflow": 5, "outflow": 6, "jaccardSimilarity": 0.03},
        ],
        "timeline": [
            {"month": "2025-09", "streamer": "competitive_player_eu", "sharedViewers": 88},
            {"month": "2025-09", "streamer": "game_pro_streamer", "sharedViewers": 61},
            {"month": "2025-09", "streamer": "deadlock_esports", "sharedViewers": 45},
            {"month": "2025-10", "streamer": "competitive_player_eu", "sharedViewers": 104},
            {"month": "2025-10", "streamer": "game_pro_streamer", "sharedViewers": 79},
            {"month": "2025-10", "streamer": "deadlock_esports", "sharedViewers": 58},
            {"month": "2025-11", "streamer": "competitive_player_eu", "sharedViewers": 121},
            {"month": "2025-11", "streamer": "game_pro_streamer", "sharedViewers": 88},
            {"month": "2025-11", "streamer": "deadlock_esports", "sharedViewers": 67},
            {"month": "2025-12", "streamer": "competitive_player_eu", "sharedViewers": 142},
            {"month": "2025-12", "streamer": "game_pro_streamer", "sharedViewers": 98},
            {"month": "2025-12", "streamer": "deadlock_esports", "sharedViewers": 76},
        ],
        "totalUniqueViewers": 483,
        "dataQuality": {"months": 4, "minSharedFilter": 3},
    }


# ---------------------------------------------------------------------------
# Viewer directory + detail + segments (Demo-only endpoints)
# ---------------------------------------------------------------------------

_VIEWER_CHANNEL_POOL = [
    "deadlock_de_top1",
    "fps_king_de",
    "ranked_master_de",
    "casual_fps_de",
    "pro_gamer_de",
    "late_night_deadlock",
    "stratlab_live",
    "scrim_hub_de",
    "aimcoach_tv",
    "duoq_lounge",
    "meta_patchtalk",
]


def _checksum(value: str) -> int:
    return sum(ord(ch) for ch in value)


def _viewer_timestamp(days_ago: int, hour: int = 20) -> str:
    dt = datetime.now(UTC) - timedelta(days=max(0, days_ago))
    dt = dt.replace(hour=hour, minute=0, second=0, microsecond=0)
    return dt.isoformat()


def _viewer_category(
    total_sessions: int,
    total_messages: int,
    first_seen_days: int,
    last_seen_days: int,
) -> str:
    if first_seen_days <= 14 and total_sessions <= 3:
        return "new"
    if total_messages <= 0:
        return "lurker"
    msgs_per_session = total_messages / max(1, total_sessions)
    if total_sessions >= 18 and msgs_per_session >= 20 and last_seen_days <= 14:
        return "dedicated"
    if total_sessions >= 8 and msgs_per_session >= 6:
        return "regular"
    return "casual"


def _make_top_channels(login: str, other_channels: int) -> list[str]:
    if other_channels <= 0:
        return []
    start = _checksum(login) % len(_VIEWER_CHANNEL_POOL)
    out: list[str] = []
    for i in range(other_channels):
        candidate = _VIEWER_CHANNEL_POOL[(start + i) % len(_VIEWER_CHANNEL_POOL)]
        if candidate not in out:
            out.append(candidate)
    return out


def _make_viewer_entry(
    *,
    login: str,
    total_sessions: int,
    total_messages: int,
    first_seen_days: int,
    last_seen_days: int,
    other_channels: int,
    top_other_channels: list[str] | None = None,
) -> dict[str, Any]:
    first_seen_days = max(first_seen_days, last_seen_days + 2)
    first_seen = _viewer_timestamp(first_seen_days, hour=18)
    last_seen = _viewer_timestamp(last_seen_days, hour=20)
    category = _viewer_category(total_sessions, total_messages, first_seen_days, last_seen_days)
    top_channels = top_other_channels if top_other_channels is not None else _make_top_channels(login, other_channels)
    avg_messages = (
        round(total_messages / max(1, total_sessions), 1) if total_messages > 0 else 0
    )
    return {
        "login": login,
        "totalSessions": total_sessions,
        "totalMessages": total_messages,
        "firstSeen": first_seen,
        "lastSeen": last_seen,
        "daysSinceLastSeen": last_seen_days,
        "otherChannels": max(0, other_channels),
        "topOtherChannels": top_channels,
        "category": category,
        "avgMessagesPerSession": avg_messages,
        "isLurker": total_messages <= 0,
    }


def _seed_viewers() -> list[dict[str, Any]]:
    return [
        _make_viewer_entry(
            login="alpha_shotcaller",
            total_sessions=42,
            total_messages=2210,
            first_seen_days=310,
            last_seen_days=1,
            other_channels=4,
        ),
        _make_viewer_entry(
            login="patchnotes_pete",
            total_sessions=36,
            total_messages=1740,
            first_seen_days=280,
            last_seen_days=0,
            other_channels=3,
        ),
        _make_viewer_entry(
            login="fragqueen_de",
            total_sessions=29,
            total_messages=1265,
            first_seen_days=250,
            last_seen_days=2,
            other_channels=5,
        ),
        _make_viewer_entry(
            login="duoq_daniel",
            total_sessions=22,
            total_messages=742,
            first_seen_days=190,
            last_seen_days=5,
            other_channels=2,
        ),
        _make_viewer_entry(
            login="silent_observer77",
            total_sessions=17,
            total_messages=0,
            first_seen_days=220,
            last_seen_days=12,
            other_channels=1,
        ),
        _make_viewer_entry(
            login="rerun_lurker",
            total_sessions=14,
            total_messages=0,
            first_seen_days=205,
            last_seen_days=33,
            other_channels=0,
        ),
        _make_viewer_entry(
            login="newcomer_fps",
            total_sessions=2,
            total_messages=8,
            first_seen_days=7,
            last_seen_days=1,
            other_channels=1,
        ),
        _make_viewer_entry(
            login="latequeue_mia",
            total_sessions=19,
            total_messages=356,
            first_seen_days=132,
            last_seen_days=17,
            other_channels=4,
        ),
        _make_viewer_entry(
            login="coach_clipper",
            total_sessions=11,
            total_messages=289,
            first_seen_days=94,
            last_seen_days=4,
            other_channels=2,
        ),
        _make_viewer_entry(
            login="prime_subber",
            total_sessions=27,
            total_messages=1118,
            first_seen_days=188,
            last_seen_days=6,
            other_channels=3,
        ),
        _make_viewer_entry(
            login="bg_music_only",
            total_sessions=9,
            total_messages=0,
            first_seen_days=144,
            last_seen_days=41,
            other_channels=2,
        ),
        _make_viewer_entry(
            login="scrimcaller_neo",
            total_sessions=31,
            total_messages=1330,
            first_seen_days=212,
            last_seen_days=3,
            other_channels=4,
        ),
    ]


def _generated_viewers() -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for i in range(1, 61):
        login = f"demo_viewer_{i:02d}"
        total_sessions = 1 + (i * 7) % 42
        last_seen_days = (i * 3) % 57
        first_seen_days = min(360, last_seen_days + 20 + (i * 5) % 180)
        if i % 10 == 0:
            total_sessions = 1 + (i % 3)
            first_seen_days = 2 + (i % 8)
            last_seen_days = i % 5
        if i % 9 == 0:
            total_messages = 0
        else:
            total_messages = total_sessions * (2 + (i * 5) % 22)
        other_channels = (i * 2 + 1) % 6
        rows.append(
            _make_viewer_entry(
                login=login,
                total_sessions=total_sessions,
                total_messages=total_messages,
                first_seen_days=first_seen_days,
                last_seen_days=last_seen_days,
                other_channels=other_channels,
            )
        )
    return rows


def _all_demo_viewers() -> list[dict[str, Any]]:
    return _seed_viewers() + _generated_viewers()


def get_viewer_directory(
    sort: str = "sessions",
    order: str = "desc",
    filter_type: str = "all",
    search: str = "",
    page: int = 1,
    per_page: int = 50,
) -> dict[str, Any]:
    page = max(1, int(page))
    per_page = min(100, max(10, int(per_page)))
    search = (search or "").strip().lower()

    viewers = _all_demo_viewers()
    total_viewers = len(viewers)
    total_active = sum(1 for v in viewers if v["daysSinceLastSeen"] <= 14)
    total_lurkers = sum(1 for v in viewers if v["isLurker"])
    total_exclusive = sum(1 for v in viewers if v["otherChannels"] == 0)
    total_shared = total_viewers - total_exclusive
    avg_sessions = (
        round(sum(v["totalSessions"] for v in viewers) / total_viewers, 1)
        if total_viewers
        else 0
    )
    avg_other = (
        round(sum(v["otherChannels"] for v in viewers) / total_viewers, 1)
        if total_viewers
        else 0
    )

    if filter_type == "active":
        viewers = [v for v in viewers if v["daysSinceLastSeen"] <= 14]
    elif filter_type == "lurker":
        viewers = [v for v in viewers if v["isLurker"]]
    elif filter_type == "exclusive":
        viewers = [v for v in viewers if v["otherChannels"] == 0]
    elif filter_type == "shared":
        viewers = [v for v in viewers if v["otherChannels"] > 0]
    elif filter_type == "new":
        viewers = [v for v in viewers if v["category"] == "new"]
    elif filter_type == "churned":
        viewers = [v for v in viewers if v["daysSinceLastSeen"] > 30]

    if search:
        viewers = [v for v in viewers if search in str(v["login"]).lower()]

    sort_map = {
        "sessions": "totalSessions",
        "messages": "totalMessages",
        "last_seen": "daysSinceLastSeen",
        "other_channels": "otherChannels",
        "first_seen": "firstSeen",
    }
    sort_key = sort_map.get(sort, "totalSessions")
    reverse = order == "desc"
    if sort == "last_seen":
        reverse = order == "asc"
    viewers = sorted(viewers, key=lambda item: item.get(sort_key, 0), reverse=reverse)

    filtered_total = len(viewers)
    start = (page - 1) * per_page
    end = start + per_page
    return {
        "viewers": viewers[start:end],
        "total": filtered_total,
        "page": page,
        "perPage": per_page,
        "summary": {
            "totalViewers": total_viewers,
            "activeViewers": total_active,
            "lurkers": total_lurkers,
            "exclusiveViewers": total_exclusive,
            "sharedViewers": total_shared,
            "avgSessionsPerViewer": avg_sessions,
            "avgOtherChannels": avg_other,
        },
    }


def _fallback_viewer(login: str) -> dict[str, Any]:
    seed = _checksum(login)
    total_sessions = 4 + (seed % 18)
    total_messages = total_sessions * (3 + (seed % 13))
    return _make_viewer_entry(
        login=login,
        total_sessions=total_sessions,
        total_messages=total_messages,
        first_seen_days=70 + (seed % 220),
        last_seen_days=seed % 21,
        other_channels=seed % 4,
    )


def _resolve_viewer(login: str) -> dict[str, Any]:
    login_l = login.strip().lower()
    for v in _all_demo_viewers():
        if str(v["login"]).lower() == login_l:
            return v
    return _fallback_viewer(login_l or "unknown_viewer")


def get_viewer_detail(login: str) -> dict[str, Any]:
    viewer = _resolve_viewer(login)
    seed = _checksum(str(viewer["login"]))

    activity: list[dict[str, Any]] = []
    for idx in range(30):
        days_ago = (30 - idx) * 3
        sessions = 1 if (idx + seed) % 4 != 0 else 0
        if viewer["totalSessions"] >= 20 and idx % 5 == 0:
            sessions += 1
        if viewer["isLurker"]:
            messages = 0
        else:
            drift = ((seed // 3 + idx * 7) % 9) - 2
            base_msgs = max(2.0, float(viewer["avgMessagesPerSession"]) * 0.65)
            messages = max(0, int(sessions * base_msgs + drift))
        activity.append({"date": _date(days_ago), "sessions": sessions, "messages": messages})

    cross_channel: list[dict[str, Any]] = []
    for i, channel in enumerate(viewer["topOtherChannels"][:6]):
        sessions = max(1, int(viewer["totalSessions"] * (0.45 - 0.06 * i)))
        if viewer["isLurker"]:
            messages = 0
        else:
            messages = int(sessions * max(1.4, float(viewer["avgMessagesPerSession"]) * (0.75 - 0.08 * i)))
        overlap = "before" if i % 3 == 0 else ("after" if i % 3 == 1 else "unknown")
        cross_channel.append(
            {
                "streamer": channel,
                "sessions": sessions,
                "messages": max(0, messages),
                "firstSeen": _viewer_timestamp(min(360, viewer["daysSinceLastSeen"] + 90 + i * 9), hour=18),
                "lastSeen": _viewer_timestamp(min(60, viewer["daysSinceLastSeen"] + i * 4), hour=20),
                "overlap": overlap,
            }
        )

    base_hour = 16 + (seed % 7)
    peak_hours = sorted(
        {
            base_hour % 24,
            (base_hour + 2 + seed % 3) % 24,
            (base_hour + 5) % 24,
        }
    )
    weekday_names = [
        "Sonntag",
        "Montag",
        "Dienstag",
        "Mittwoch",
        "Donnerstag",
        "Freitag",
        "Samstag",
    ]
    most_active_day = weekday_names[seed % 7]

    if viewer["totalSessions"] < 4:
        trend = "insufficient_data"
    else:
        trend = ("increasing", "decreasing", "stable")[seed % 3]

    personality: dict[str, Any] | None = None
    if not viewer["isLurker"]:
        base = max(3, int(float(viewer["avgMessagesPerSession"])))
        distribution = {
            "Game-Related": base * 3 + seed % 12,
            "Reaction": base * 2 + (seed // 2) % 9,
            "Question": base + seed % 7,
            "Greeting": base // 2 + 2 + seed % 5,
            "Engagement": base * 2 + (seed // 5) % 8,
            "Command": base + (seed // 7) % 6,
            "Other": max(1, base // 2 + seed % 4),
        }
        personality = {"primary": max(distribution, key=distribution.get), "distribution": distribution}

    payload: dict[str, Any] = {
        "login": viewer["login"],
        "overview": {
            "totalSessions": viewer["totalSessions"],
            "totalMessages": viewer["totalMessages"],
            "firstSeen": viewer["firstSeen"],
            "lastSeen": viewer["lastSeen"],
            "category": viewer["category"],
            "isLurker": viewer["isLurker"],
        },
        "activityTimeline": activity,
        "crossChannelPresence": cross_channel,
        "chatPatterns": {
            "peakHours": peak_hours,
            "avgMessagesPerSession": viewer["avgMessagesPerSession"],
            "mostActiveDay": most_active_day,
            "messagesTrend": trend,
        },
    }
    if personality:
        payload["personality"] = personality
    return payload


def get_viewer_segments() -> dict[str, Any]:
    viewers = _all_demo_viewers()
    total = len(viewers)
    segment_names = ("dedicated", "regular", "casual", "lurker", "new")
    buckets: dict[str, list[dict[str, Any]]] = {name: [] for name in segment_names}
    for viewer in viewers:
        cat = str(viewer["category"])
        if cat not in buckets:
            cat = "casual"
        buckets[cat].append(viewer)

    segments: dict[str, Any] = {}
    for name in segment_names:
        chunk = buckets[name]
        count = len(chunk)
        segments[name] = {
            "count": count,
            "pct": round((count / total) * 100, 1) if total else 0,
            "avgMessages": round(sum(v["totalMessages"] for v in chunk) / max(1, count), 1)
            if count
            else 0,
            "avgSessions": round(sum(v["totalSessions"] for v in chunk) / max(1, count), 1)
            if count
            else 0,
        }

    at_risk = []
    recently_churned = 0
    for viewer in viewers:
        engaged = viewer["totalSessions"] >= 3 and viewer["totalMessages"] > 0
        last_seen = viewer["daysSinceLastSeen"]
        if engaged and 14 < last_seen <= 45:
            at_risk.append(
                {
                    "login": viewer["login"],
                    "sessions": viewer["totalSessions"],
                    "messages": viewer["totalMessages"],
                    "daysSinceLastSeen": last_seen,
                    "category": viewer["category"],
                    "recentlySeenAt": viewer["topOtherChannels"][:2],
                }
            )
        elif engaged and last_seen > 45:
            recently_churned += 1

    at_risk.sort(key=lambda v: (v["sessions"] * 2 + v["messages"]), reverse=True)

    exclusive = sum(1 for v in viewers if v["otherChannels"] == 0)
    avg_other = round(sum(v["otherChannels"] for v in viewers) / max(1, total), 1)
    top_shared_counts: dict[str, int] = {}
    for viewer in viewers:
        for channel in viewer["topOtherChannels"][:3]:
            top_shared_counts[channel] = top_shared_counts.get(channel, 0) + 1
    top_shared = sorted(top_shared_counts.items(), key=lambda item: item[1], reverse=True)[:10]
    top_shared_payload = []
    for channel, count in top_shared:
        direction = _demo_shared_channel_direction(channel)
        top_shared_payload.append(
            {"streamer": channel, "sharedCount": count, "direction": direction}
        )

    return {
        "segments": segments,
        "churnRisk": {
            "atRisk": len(at_risk),
            "recentlyChurned": recently_churned,
            "atRiskViewers": at_risk[:20],
        },
        "crossChannelStats": {
            "exclusiveViewersPct": round((exclusive / total) * 100, 1) if total else 0,
            "avgOtherChannels": avg_other,
            "topSharedChannels": top_shared_payload,
        },
    }


def _demo_shared_channel_direction(channel: str) -> str:
    directions = ("incoming", "outgoing", "unknown", "bidirectional")
    checksum = sum(ord(ch) for ch in str(channel or "").lower())
    return directions[checksum % len(directions)]


def _profile_viewer_pool(spec: dict[str, Any]) -> list[dict[str, Any]]:
    base = _all_demo_viewers()
    target = max(24, int(spec.get("viewer_population_target", len(base)) or len(base)))
    session_factor = max(0.35, float(spec.get("activity_factor", 1.0) or 1.0))
    chat_factor = max(0.0, float(spec.get("chat_factor", 1.0) or 1.0))
    other_factor = max(0.35, float(spec.get("other_channels_factor", 1.0) or 1.0))

    adjusted: list[dict[str, Any]] = []
    for index, viewer in enumerate(base):
        login = str(viewer["login"])
        last_seen_days = int(viewer["daysSinceLastSeen"])
        session_bias = 0.86 + (index % 5) * 0.05
        message_bias = 0.80 + (index % 7) * 0.04
        total_sessions = max(1, int(round(float(viewer["totalSessions"]) * session_factor * session_bias)))
        if viewer["isLurker"]:
            total_messages = 0
        else:
            total_messages = max(
                0,
                int(round(float(viewer["totalMessages"]) * chat_factor * message_bias)),
            )
        other_channels = max(0, int(round(float(viewer["otherChannels"]) * other_factor)))
        first_seen_days = min(360, last_seen_days + 24 + (index * 9) % 210)
        adjusted.append(
            _make_viewer_entry(
                login=login,
                total_sessions=total_sessions,
                total_messages=total_messages,
                first_seen_days=first_seen_days,
                last_seen_days=last_seen_days,
                other_channels=other_channels,
            )
        )

    if target <= len(adjusted):
        return adjusted[:target]

    prefix = str(spec.get("login", "demo"))[:3]
    out = list(adjusted)
    clone_index = 0
    while len(out) < target:
        source = adjusted[clone_index % len(adjusted)]
        multiplier = 0.62 + (clone_index % 6) * 0.08
        last_seen_days = (int(source["daysSinceLastSeen"]) + clone_index * 3) % 58
        total_sessions = max(1, int(round(float(source["totalSessions"]) * multiplier)))
        if source["isLurker"]:
            total_messages = 0
        else:
            total_messages = max(0, int(round(float(source["totalMessages"]) * multiplier)))
        other_channels = max(
            0,
            int(round(float(source["otherChannels"]) * (0.9 + (clone_index % 3) * 0.15))),
        )
        first_seen_days = min(360, last_seen_days + 18 + (clone_index * 11) % 210)
        out.append(
            _make_viewer_entry(
                login=f"{source['login']}_{prefix}{clone_index + 1:03d}",
                total_sessions=total_sessions,
                total_messages=total_messages,
                first_seen_days=first_seen_days,
                last_seen_days=last_seen_days,
                other_channels=other_channels,
            )
        )
        clone_index += 1
    return out


def build_demo_viewer_directory(
    spec: dict[str, Any],
    *,
    sort: str = "sessions",
    order: str = "desc",
    filter_type: str = "all",
    search: str = "",
    page: int = 1,
    per_page: int = 50,
) -> dict[str, Any]:
    page = max(1, int(page))
    per_page = min(100, max(10, int(per_page)))
    search = (search or "").strip().lower()

    viewers = _profile_viewer_pool(spec)
    total_viewers = len(viewers)
    total_active = sum(1 for v in viewers if v["daysSinceLastSeen"] <= 14)
    total_lurkers = sum(1 for v in viewers if v["isLurker"])
    total_exclusive = sum(1 for v in viewers if v["otherChannels"] == 0)
    total_shared = total_viewers - total_exclusive
    avg_sessions = (
        round(sum(v["totalSessions"] for v in viewers) / total_viewers, 1)
        if total_viewers
        else 0
    )
    avg_other = (
        round(sum(v["otherChannels"] for v in viewers) / total_viewers, 1)
        if total_viewers
        else 0
    )

    filtered = viewers
    if filter_type == "active":
        filtered = [v for v in filtered if v["daysSinceLastSeen"] <= 14]
    elif filter_type == "lurker":
        filtered = [v for v in filtered if v["isLurker"]]
    elif filter_type == "exclusive":
        filtered = [v for v in filtered if v["otherChannels"] == 0]
    elif filter_type == "shared":
        filtered = [v for v in filtered if v["otherChannels"] > 0]
    elif filter_type == "new":
        filtered = [v for v in filtered if v["category"] == "new"]
    elif filter_type == "churned":
        filtered = [v for v in filtered if v["daysSinceLastSeen"] > 30]

    if search:
        filtered = [v for v in filtered if search in str(v["login"]).lower()]

    sort_map = {
        "sessions": "totalSessions",
        "messages": "totalMessages",
        "last_seen": "daysSinceLastSeen",
        "other_channels": "otherChannels",
        "first_seen": "firstSeen",
    }
    sort_key = sort_map.get(sort, "totalSessions")
    reverse = order == "desc"
    if sort == "last_seen":
        reverse = order == "asc"
    filtered = sorted(filtered, key=lambda item: item.get(sort_key, 0), reverse=reverse)

    start = (page - 1) * per_page
    end = start + per_page
    return {
        "viewers": filtered[start:end],
        "total": len(filtered),
        "page": page,
        "perPage": per_page,
        "summary": {
            "totalViewers": total_viewers,
            "activeViewers": total_active,
            "lurkers": total_lurkers,
            "exclusiveViewers": total_exclusive,
            "sharedViewers": total_shared,
            "avgSessionsPerViewer": avg_sessions,
            "avgOtherChannels": avg_other,
        },
    }


def _fallback_viewer_for_profile(spec: dict[str, Any], login: str) -> dict[str, Any]:
    seed = _checksum(login)
    session_factor = max(0.35, float(spec.get("activity_factor", 1.0) or 1.0))
    chat_factor = max(0.0, float(spec.get("chat_factor", 1.0) or 1.0))
    other_factor = max(0.35, float(spec.get("other_channels_factor", 1.0) or 1.0))
    total_sessions = max(1, int(round((4 + seed % 18) * session_factor)))
    total_messages = 0 if seed % 7 == 0 else int(round(total_sessions * (3 + seed % 13) * chat_factor))
    return _make_viewer_entry(
        login=login,
        total_sessions=total_sessions,
        total_messages=total_messages,
        first_seen_days=70 + (seed % 220),
        last_seen_days=seed % 21,
        other_channels=max(0, int(round((seed % 4) * other_factor))),
    )


def build_demo_viewer_detail(spec: dict[str, Any], login: str) -> dict[str, Any]:
    viewer_pool = _profile_viewer_pool(spec)
    viewer = next(
        (item for item in viewer_pool if str(item["login"]).lower() == login.strip().lower()),
        _fallback_viewer_for_profile(spec, login.strip().lower() or "unknown_viewer"),
    )
    seed = _checksum(str(viewer["login"]))

    activity: list[dict[str, Any]] = []
    for idx in range(30):
        days_ago = (30 - idx) * 3
        sessions = 1 if (idx + seed) % 4 != 0 else 0
        if viewer["totalSessions"] >= 20 and idx % 5 == 0:
            sessions += 1
        if viewer["isLurker"]:
            messages = 0
        else:
            drift = ((seed // 3 + idx * 7) % 9) - 2
            base_msgs = max(2.0, float(viewer["avgMessagesPerSession"]) * 0.65)
            messages = max(0, int(sessions * base_msgs + drift))
        activity.append({"date": _date(days_ago), "sessions": sessions, "messages": messages})

    cross_channel: list[dict[str, Any]] = []
    for i, channel in enumerate(viewer["topOtherChannels"][:6]):
        sessions = max(1, int(viewer["totalSessions"] * (0.45 - 0.06 * i)))
        if viewer["isLurker"]:
            messages = 0
        else:
            messages = int(
                sessions
                * max(1.4, float(viewer["avgMessagesPerSession"]) * (0.75 - 0.08 * i))
            )
        overlap = "before" if i % 3 == 0 else ("after" if i % 3 == 1 else "unknown")
        cross_channel.append(
            {
                "streamer": channel,
                "sessions": sessions,
                "messages": max(0, messages),
                "firstSeen": _viewer_timestamp(
                    min(360, viewer["daysSinceLastSeen"] + 90 + i * 9), hour=18
                ),
                "lastSeen": _viewer_timestamp(
                    min(60, viewer["daysSinceLastSeen"] + i * 4), hour=20
                ),
                "overlap": overlap,
            }
        )

    base_hour = 16 + (seed % 7)
    peak_hours = sorted(
        {
            base_hour % 24,
            (base_hour + 2 + seed % 3) % 24,
            (base_hour + 5) % 24,
        }
    )
    weekday_names = [
        "Sonntag",
        "Montag",
        "Dienstag",
        "Mittwoch",
        "Donnerstag",
        "Freitag",
        "Samstag",
    ]
    most_active_day = weekday_names[seed % 7]
    trend = "insufficient_data" if viewer["totalSessions"] < 4 else ("increasing", "decreasing", "stable")[seed % 3]

    payload: dict[str, Any] = {
        "login": viewer["login"],
        "overview": {
            "totalSessions": viewer["totalSessions"],
            "totalMessages": viewer["totalMessages"],
            "firstSeen": viewer["firstSeen"],
            "lastSeen": viewer["lastSeen"],
            "category": viewer["category"],
            "isLurker": viewer["isLurker"],
        },
        "activityTimeline": activity,
        "crossChannelPresence": cross_channel,
        "chatPatterns": {
            "peakHours": peak_hours,
            "avgMessagesPerSession": viewer["avgMessagesPerSession"],
            "mostActiveDay": most_active_day,
            "messagesTrend": trend,
        },
    }
    if not viewer["isLurker"]:
        base = max(3, int(float(viewer["avgMessagesPerSession"])))
        distribution = {
            "Game-Related": base * 3 + seed % 12,
            "Reaction": base * 2 + (seed // 2) % 9,
            "Question": base + seed % 7,
            "Greeting": base // 2 + 2 + seed % 5,
            "Engagement": base * 2 + (seed // 5) % 8,
            "Command": base + (seed // 7) % 6,
            "Other": max(1, base // 2 + seed % 4),
        }
        payload["personality"] = {
            "primary": max(distribution, key=distribution.get),
            "distribution": distribution,
        }
    return payload


def build_demo_viewer_segments(spec: dict[str, Any]) -> dict[str, Any]:
    viewers = _profile_viewer_pool(spec)
    total = len(viewers)
    segment_names = ("dedicated", "regular", "casual", "lurker", "new")
    buckets: dict[str, list[dict[str, Any]]] = {name: [] for name in segment_names}
    for viewer in viewers:
        cat = str(viewer["category"])
        if cat not in buckets:
            cat = "casual"
        buckets[cat].append(viewer)

    segments: dict[str, Any] = {}
    for name in segment_names:
        chunk = buckets[name]
        count = len(chunk)
        segments[name] = {
            "count": count,
            "pct": round((count / total) * 100, 1) if total else 0,
            "avgMessages": round(sum(v["totalMessages"] for v in chunk) / max(1, count), 1) if count else 0,
            "avgSessions": round(sum(v["totalSessions"] for v in chunk) / max(1, count), 1) if count else 0,
        }

    at_risk = []
    recently_churned = 0
    for viewer in viewers:
        engaged = viewer["totalSessions"] >= 3 and viewer["totalMessages"] > 0
        last_seen = viewer["daysSinceLastSeen"]
        if engaged and 14 < last_seen <= 45:
            at_risk.append(
                {
                    "login": viewer["login"],
                    "sessions": viewer["totalSessions"],
                    "messages": viewer["totalMessages"],
                    "daysSinceLastSeen": last_seen,
                    "category": viewer["category"],
                    "recentlySeenAt": viewer["topOtherChannels"][:2],
                }
            )
        elif engaged and last_seen > 45:
            recently_churned += 1
    at_risk.sort(key=lambda v: (v["sessions"] * 2 + v["messages"]), reverse=True)

    exclusive = sum(1 for v in viewers if v["otherChannels"] == 0)
    avg_other = round(sum(v["otherChannels"] for v in viewers) / max(1, total), 1)
    top_shared_counts: dict[str, int] = {}
    for viewer in viewers:
        for channel in viewer["topOtherChannels"][:3]:
            top_shared_counts[channel] = top_shared_counts.get(channel, 0) + 1
    top_shared = sorted(top_shared_counts.items(), key=lambda item: item[1], reverse=True)[:10]
    top_shared_payload = []
    for channel, count in top_shared:
        direction = _demo_shared_channel_direction(channel)
        top_shared_payload.append(
            {"streamer": channel, "sharedCount": count, "direction": direction}
        )

    return {
        "segments": segments,
        "churnRisk": {
            "atRisk": len(at_risk),
            "recentlyChurned": recently_churned,
            "atRiskViewers": at_risk[:20],
        },
        "crossChannelStats": {
            "exclusiveViewersPct": round((exclusive / total) * 100, 1) if total else 0,
            "avgOtherChannels": avg_other,
            "topSharedChannels": top_shared_payload,
        },
    }


def build_demo_viewer_profiles(spec: dict[str, Any]) -> dict[str, Any]:
    viewers = _profile_viewer_pool(spec)
    exclusive = sum(1 for v in viewers if v["otherChannels"] == 0)
    explorer = sum(1 for v in viewers if v["otherChannels"] >= 5)
    loyal_multi = sum(
        1
        for v in viewers
        if not v["isLurker"] and v["otherChannels"] in {2, 3, 4} and v["totalSessions"] >= 8
    )
    passive = sum(1 for v in viewers if v["isLurker"])
    casual = max(0, len(viewers) - exclusive - loyal_multi - explorer - passive)
    distribution: dict[int, int] = {}
    for viewer in viewers:
        bucket = max(1, int(viewer["otherChannels"]) + 1)
        distribution[bucket] = distribution.get(bucket, 0) + 1

    return {
        "dataAvailable": True,
        "profiles": {
            "exclusive": exclusive,
            "loyalMulti": loyal_multi,
            "casual": casual,
            "explorer": explorer,
            "passive": passive,
            "total": len(viewers),
        },
        "exclusivityDistribution": [
            {"streamerCount": count, "viewerCount": distribution[count]}
            for count in sorted(distribution)
        ],
    }


# ---------------------------------------------------------------------------
# Labor / Experimental demo data
# ---------------------------------------------------------------------------

_EXP_GAME_BASE = [
    {
        "game": "Deadlock",
        "sessions30": 24,
        "avgViewers": 388.0,
        "peakViewers": 1087,
        "avgDurationMin": 196.0,
        "avgFollowerDelta": 12.9,
    },
    {
        "game": "Just Chatting",
        "sessions30": 13,
        "avgViewers": 344.0,
        "peakViewers": 612,
        "avgDurationMin": 74.0,
        "avgFollowerDelta": 5.8,
    },
    {
        "game": "Counter-Strike 2",
        "sessions30": 11,
        "avgViewers": 312.0,
        "peakViewers": 702,
        "avgDurationMin": 138.0,
        "avgFollowerDelta": 7.1,
    },
    {
        "game": "VALORANT",
        "sessions30": 9,
        "avgViewers": 286.0,
        "peakViewers": 650,
        "avgDurationMin": 152.0,
        "avgFollowerDelta": 6.4,
    },
    {
        "game": "Apex Legends",
        "sessions30": 7,
        "avgViewers": 254.0,
        "peakViewers": 574,
        "avgDurationMin": 161.0,
        "avgFollowerDelta": 5.0,
    },
    {
        "game": "Marvel Rivals",
        "sessions30": 6,
        "avgViewers": 302.0,
        "peakViewers": 588,
        "avgDurationMin": 126.0,
        "avgFollowerDelta": 6.9,
    },
    {
        "game": "The Finals",
        "sessions30": 5,
        "avgViewers": 228.0,
        "peakViewers": 470,
        "avgDurationMin": 118.0,
        "avgFollowerDelta": 4.1,
    },
    {
        "game": "Helldivers 2",
        "sessions30": 4,
        "avgViewers": 214.0,
        "peakViewers": 451,
        "avgDurationMin": 109.0,
        "avgFollowerDelta": 3.6,
    },
]


def _exp_scale(days: int) -> float:
    return max(0.25, min(2.4, max(1, days) / 30.0))


def get_exp_game_breakdown(days: int = 30) -> list[dict[str, Any]]:
    scale = _exp_scale(days)
    rows = []
    for idx, game in enumerate(_EXP_GAME_BASE):
        sessions = max(1, int(round(game["sessions30"] * scale)))
        viewer_bias = ((idx * 7 + days) % 9) - 4
        rows.append(
            {
                "game": game["game"],
                "sessions": sessions,
                "avgViewers": round(game["avgViewers"] + viewer_bias * 1.6, 1),
                "peakViewers": int(round(game["peakViewers"] + viewer_bias * 9)),
                "avgDurationMin": round(game["avgDurationMin"] + (((days + idx * 5) % 11) - 5) * 1.2, 1),
                "avgFollowerDelta": round(game["avgFollowerDelta"] + (((idx + days) % 5) - 2) * 0.3, 1),
            }
        )
    rows.sort(key=lambda r: r["avgViewers"], reverse=True)
    return rows


def get_exp_overview(days: int = 30) -> dict[str, Any]:
    rows = get_exp_game_breakdown(days)
    total_sessions = sum(int(r["sessions"]) for r in rows)
    games_played = len([r for r in rows if int(r["sessions"]) > 0])
    weighted_viewers = sum(float(r["avgViewers"]) * int(r["sessions"]) for r in rows)
    avg_viewers = round(weighted_viewers / max(1, total_sessions), 1)
    best_game = max(rows, key=lambda r: r["avgViewers"]) if rows else None
    return {
        "totalSessions": total_sessions,
        "gamesPlayed": games_played,
        "avgViewers": avg_viewers,
        "bestGame": best_game["game"] if best_game else "",
        "bestGameAvgViewers": best_game["avgViewers"] if best_game else 0.0,
    }


def get_exp_game_transitions(days: int = 30) -> list[dict[str, Any]]:
    scale = _exp_scale(days)
    base = [
        ("Deadlock", "Just Chatting", 9, 402.0, -48.0),
        ("Just Chatting", "Deadlock", 8, 336.0, 62.0),
        ("Deadlock", "Counter-Strike 2", 6, 391.0, -23.0),
        ("Counter-Strike 2", "Deadlock", 6, 318.0, 44.0),
        ("Deadlock", "Marvel Rivals", 5, 384.0, -17.0),
        ("Marvel Rivals", "Deadlock", 4, 303.0, 31.0),
        ("Deadlock", "VALORANT", 4, 377.0, -35.0),
        ("VALORANT", "Just Chatting", 3, 279.0, 22.0),
        ("Apex Legends", "Deadlock", 3, 248.0, 58.0),
        ("Helldivers 2", "Deadlock", 2, 206.0, 41.0),
    ]
    rows = []
    for from_game, to_game, count30, before, delta in base:
        count = max(1, int(round(count30 * scale)))
        rows.append(
            {
                "fromGame": from_game,
                "toGame": to_game,
                "count": count,
                "avgViewersBefore": round(before, 1),
                "avgViewersAfter": round(before + delta, 1),
                "viewerDelta": round(delta, 1),
            }
        )
    rows.sort(key=lambda r: r["count"], reverse=True)
    return rows


def get_exp_growth_curves(days: int = 30) -> list[dict[str, Any]]:
    rows = get_exp_game_breakdown(days)[:6]
    out: list[dict[str, Any]] = []
    for row in rows:
        game = str(row["game"])
        avg = float(row["avgViewers"])
        peak = max(avg * 1.08, float(row["peakViewers"]) * 0.72)
        peak_minute = 45 + (_checksum(game) % 35)
        decay = max(0.18, avg / 520.0)
        for minute in range(0, 241, 15):
            if minute <= peak_minute:
                progress = minute / max(1, peak_minute)
                viewers = avg * 0.68 + (peak - avg * 0.68) * progress
            else:
                viewers = peak - (minute - peak_minute) * decay
            viewers = max(avg * 0.55, viewers)
            wobble = ((_checksum(game) + minute) % 9) - 4
            viewers = round(viewers + wobble * 0.7, 1)
            sample_count = max(6, int(row["sessions"]) * (1 + max(0, 240 - minute) // 60))
            out.append(
                {
                    "game": game,
                    "minuteFromStart": minute,
                    "avgViewers": viewers,
                    "sampleCount": sample_count,
                }
            )
    out.sort(key=lambda r: (r["game"], r["minuteFromStart"]))
    return out


# ---------------------------------------------------------------------------
# Multi-profile demo fixture layer
# ---------------------------------------------------------------------------

DEMO_FIXTURE_VERSION = "earlysalty-snapshot-2026-03-v2"
DEFAULT_DEMO_PROFILE = "midcore_live"

_DEMO_PROFILE_SPECS: dict[str, dict[str, Any]] = {
    "smallquest_tv": {
        "display_name": "SmallQuest TV",
        "viewer_factor": 0.14,
        "peak_factor": 0.20,
        "chat_factor": 0.34,
        "follower_factor": 0.21,
        "monetization_factor": 0.18,
        "ads_factor": 0.28,
        "hype_factor": 0.35,
        "activity_factor": 0.74,
        "duration_factor": 0.88,
        "other_channels_factor": 0.72,
        "viewer_population_target": 34,
        "retention_shift": 10.8,
        "engagement_pct_factor": 1.28,
        "rank_position": 10,
        "category_rank": 43,
        "category_total": 64,
        "overview_scores": {
            "total": 63,
            "reach": 29,
            "retention": 86,
            "engagement": 79,
            "growth": 36,
            "monetization": 21,
            "network": 41,
        },
        "findings": [
            {"type": "pos", "title": "Enge Stamm-Community", "text": "Ein kleiner Kanal mit ueberdurchschnittlicher Bindung: ein grosser Teil der Viewer bleibt frueh aktiv und kommt wieder."},
            {"type": "pos", "title": "Chat reagiert schnell", "text": "Interaktive Formate loesen trotz geringer Reichweite verhaeltnismaessig viele Chat-Signale aus."},
            {"type": "warn", "title": "Zu wenig Discovery", "text": "Der Kanal lebt stark von Wiederkehrern, aber neue Zuschauerstroeme sind noch zu klein und unregelmaessig."},
            {"type": "info", "title": "Jeder Slot zaehlt", "text": "Schon kleine Timing- oder Titel-Aenderungen sind in dieser Groessenordnung sofort messbar."},
            {"type": "neg", "title": "Monetization fast nur eventgetrieben", "text": "Support-Spitzen entstehen punktuell, aber noch nicht als verlaesslicher wiederkehrender Kanal."},
        ],
        "actions": [
            {"tag": "Reach", "text": "Plane pro Woche einen Discovery-Stream mit klarerem Hook fuer neue Viewer statt nur fuer die Stamm-Community.", "priority": "high"},
            {"tag": "Consistency", "text": "Halte zwei feste Slots ueber mehrere Wochen konstant, damit der kleine Datensatz nicht verrauscht.", "priority": "high"},
            {"tag": "Community", "text": "Bewahre die enge Interaktion, aber verpacke sie in einfachere, sofort erkennbare Titel-Hooks.", "priority": "medium"},
        ],
        "session_titles": [
            "After Work Grind | Chat spielt mit",
            "Small Stream, Big Calls | Ranked",
            "Patch-Talk + Duo Queue",
            "Road to 100 Avg | Community Night",
        ],
        "ai_focus": "kleiner Community-Kanal mit hoher Bindung",
    },
    "midcore_live": {
        "display_name": "MidCore Live",
        "viewer_factor": 1.00,
        "peak_factor": 1.12,
        "chat_factor": 1.04,
        "follower_factor": 1.02,
        "monetization_factor": 1.06,
        "ads_factor": 1.05,
        "hype_factor": 1.08,
        "activity_factor": 1.02,
        "duration_factor": 1.01,
        "other_channels_factor": 1.00,
        "viewer_population_target": 92,
        "retention_shift": 2.1,
        "engagement_pct_factor": 1.02,
        "rank_position": 6,
        "category_rank": 13,
        "category_total": 64,
        "overview_scores": {
            "total": 77,
            "reach": 71,
            "retention": 74,
            "engagement": 73,
            "growth": 76,
            "monetization": 61,
            "network": 72,
        },
        "findings": [
            {"type": "pos", "title": "Balanced Mid-Tier-Profil", "text": "Der Kanal liefert als mittlere Groessenklasse klare Signale in fast allen Tabs ohne Extreme zu verstecken."},
            {"type": "pos", "title": "Stabiles Wachstum", "text": "Follower-, Viewer- und Chat-Signale wachsen breit genug, um neue Tests belastbar auszuwerten."},
            {"type": "warn", "title": "Noch keine klare Dominanz", "text": "Gute Gesamtperformance, aber kein einzelner Hebel ist so stark, dass er den Kanal allein nach oben zieht."},
            {"type": "info", "title": "Vergleichs- und Ranking-Tab sind aussagekraeftig", "text": "Die Groessenordnung liegt mitten im Wettbewerbsfeld und eignet sich gut fuer Benchmarks."},
            {"type": "neg", "title": "Zu viele solide statt starke Slots", "text": "Viele Streams performen ordentlich, aber zu wenige erreichen wiederholt echte Ausreisser nach oben."},
        ],
        "actions": [
            {"tag": "Growth", "text": "Waehle ein klares Hero-Format pro Woche und optimiere dieses aggressiver als den restlichen Schedule.", "priority": "high"},
            {"tag": "Retention", "text": "Kuerze schwache Intros und verlagere den ersten echten Value-Moment in die ersten 10 Minuten.", "priority": "medium"},
            {"tag": "Programming", "text": "Trenne stabile Core-Slots von Test-Slots klarer, damit Ursachen in den Daten sauber lesbar bleiben.", "priority": "medium"},
        ],
        "session_titles": [
            "Prime Time Grind | Ranked + Review",
            "Meta Check | Patch, Queue, Calls",
            "Mid-Tier Push | Heute Peak knacken",
            "Ranked Session | Fokus + Community",
        ],
        "ai_focus": "mittlerer Growth-Kanal mit ausgewogenen Signalen",
    },
    "megaarena_gg": {
        "display_name": "MegaArena GG",
        "viewer_factor": 7.60,
        "peak_factor": 8.80,
        "chat_factor": 5.20,
        "follower_factor": 7.10,
        "monetization_factor": 8.60,
        "ads_factor": 2.20,
        "hype_factor": 3.10,
        "activity_factor": 1.58,
        "duration_factor": 1.24,
        "other_channels_factor": 1.34,
        "viewer_population_target": 280,
        "retention_shift": -8.7,
        "engagement_pct_factor": 0.82,
        "rank_position": 1,
        "category_rank": 2,
        "category_total": 64,
        "overview_scores": {
            "total": 89,
            "reach": 97,
            "retention": 61,
            "engagement": 78,
            "growth": 94,
            "monetization": 96,
            "network": 91,
        },
        "findings": [
            {"type": "pos", "title": "Massive Reichweite", "text": "Das grosse Profil demonstriert klar, wie Peaks, Wachstum und Revenue auf einem deutlich groesseren Niveau zusammenspielen."},
            {"type": "pos", "title": "Revenue-Engine aktiv", "text": "Bits, Subs, Hype und Ads sind nicht nur Begleitsignale, sondern ein eigener, stark sichtbarer Leistungskanal."},
            {"type": "warn", "title": "Retention kostet auf Skalenniveau", "text": "Bei hoher Reichweite schlagen auch kleine Halteverluste sofort in absoluten Viewer-Zahlen durch."},
            {"type": "info", "title": "Viewer- und Audience-Tabs zeigen echte Breite", "text": "Grosse Segmente, mehr Overlap und deutlich mehr Churn-/Lifecycle-Signale machen den Kanal sichtbar anders als Mid- und Small-Tier."},
            {"type": "neg", "title": "Ops-Risiko in der Session-Mitte", "text": "Bei grossem Volumen werden schwache Mid-Stream-Passagen, zu lange Ads oder unklare Switches sofort teuer."},
        ],
        "actions": [
            {"tag": "Retention", "text": "Zerlege lange Sessions in klar erkennbare Akte und setze den zweiten Peak bewusst statt zufaellig.", "priority": "high"},
            {"tag": "Revenue", "text": "Optimiere Ads, Hype- und Gift-Momente auf minimale Reibung pro Zuschauerblock statt auf maximale Haeufigkeit.", "priority": "high"},
            {"tag": "Operations", "text": "Plane Category- oder Segment-Wechsel so, dass Zuschauerstrom und Moderation nicht gleichzeitig kippen.", "priority": "medium"},
        ],
        "session_titles": [
            "Mega Queue Night | Peak Push",
            "Main Stage Grind | Ranked Marathon",
            "Arena Callouts | Patch + Queue",
            "Big Session Energy | Road to #1",
        ],
        "ai_focus": "grosser Reach- und Revenue-Kanal",
    },
}

ALLOWED_DEMO_PROFILES: tuple[str, ...] = tuple(_DEMO_PROFILE_SPECS.keys())


def _resolve_demo_profile(requested_streamer: str | None = None) -> dict[str, Any]:
    normalized = str(requested_streamer or "").strip().lower()
    login = normalized if normalized in _DEMO_PROFILE_SPECS else DEFAULT_DEMO_PROFILE
    spec = dict(_DEMO_PROFILE_SPECS[login])
    spec["login"] = login
    return spec


def build_demo_streamers() -> list[dict[str, Any]]:
    return [{"login": login, "isPartner": True} for login in ALLOWED_DEMO_PROFILES]


def build_demo_auth_status(*, streamer: str | None = None) -> dict[str, Any]:
    spec = _resolve_demo_profile(streamer)
    return {
        "authenticated": True,
        "level": "partner",
        "authLevel": "partner",
        "demoMode": True,
        "isAdmin": False,
        "isLocalhost": False,
        "canViewAllStreamers": False,
        "twitchLogin": spec["login"],
        "displayName": spec["display_name"],
        "plan": None,
        "permissions": {
            "viewAllStreamers": False,
            "viewComparison": True,
            "viewChatAnalytics": True,
            "viewOverlap": True,
        },
    }


def _profile_rank_scale(spec: dict[str, Any], metric: str, value: int | float) -> float:
    if metric == "viewers":
        return float(spec.get("viewer_factor", 1.0) or 1.0)
    if metric == "growth":
        return float(spec.get("follower_factor", 1.0) or 1.0)
    if metric == "retention":
        baseline = max(1.0, float(value))
        adjusted = max(18.0, min(98.0, baseline + float(spec.get("retention_shift", 0.0) or 0.0)))
        return adjusted / baseline
    return float(spec.get("chat_factor", 1.0) or 1.0)


def build_demo_rankings(spec: dict[str, Any], metric: str) -> list[dict[str, Any]]:
    rows = _copy_payload(get_rankings(metric))
    for row in rows:
        if str(row.get("login", "")).lower() != DEMO_STREAMER:
            continue
        row["login"] = spec["login"]
        scale = _profile_rank_scale(spec, metric, float(row.get("value", 0) or 0))
        row["value"] = _round_like(float(row.get("value", 0) or 0), float(row.get("value", 0) or 0) * scale)
        row["trendValue"] = _round_like(
            float(row.get("trendValue", 0) or 0),
            float(row.get("trendValue", 0) or 0) * max(0.6, min(scale, 3.0)),
        )
        break

    rows.sort(key=lambda item: float(item.get("value", 0) or 0), reverse=True)
    for index, row in enumerate(rows, start=1):
        row["rank"] = index
    return rows


def _slice_limit(rows: list[Any], limit: int) -> list[Any]:
    safe_limit = max(1, min(int(limit or 1), len(rows))) if rows else 0
    return rows[:safe_limit] if safe_limit else []


def _build_demo_category_rows(spec: dict[str, Any], days: int) -> list[dict[str, Any]]:
    overview = build_demo_payload("overview", streamer=spec["login"], days=days)
    summary = overview.get("summary", {}) if isinstance(overview, dict) else {}
    total_streamers = max(12, int(spec.get("category_total", 64) or 64))
    desired_rank = max(1, min(total_streamers, int(spec.get("category_rank", 1) or 1)))
    target_avg = max(12, int(summary.get("avgViewers", 0) or 0))
    target_peak = max(target_avg + 25, int(summary.get("peakViewers", 0) or 0))
    peak_ratio = max(1.4, float(target_peak) / float(max(1, target_avg)))
    up_step = max(3, int(round(max(4.0, target_avg * 0.06))))
    down_slots = max(1, total_streamers - desired_rank)
    down_step = max(1, int((target_avg - 12) / max(1, down_slots)))

    rows: list[dict[str, Any]] = []
    for rank in range(1, total_streamers + 1):
        if rank < desired_rank:
            avg_viewers = target_avg + up_step * (desired_rank - rank)
        elif rank > desired_rank:
            avg_viewers = max(12, target_avg - down_step * (rank - desired_rank))
        else:
            avg_viewers = target_avg

        peak_viewers = max(avg_viewers + 25, int(round(avg_viewers * peak_ratio)))
        is_partner = rank <= max(6, int(round(total_streamers * 0.58))) or rank % 5 in {1, 2}
        rows.append(
            {
                "rank": rank,
                "streamer": f"deadlock_rank_{rank:02d}",
                "avgViewers": avg_viewers,
                "peakViewers": peak_viewers,
                "isPartner": is_partner,
                "isYou": False,
            }
        )

    target_row = rows[desired_rank - 1]
    target_row["streamer"] = spec["login"]
    target_row["avgViewers"] = target_avg
    target_row["peakViewers"] = target_peak
    target_row["isPartner"] = True
    target_row["isYou"] = True
    return rows


def build_demo_category_leaderboard(
    spec: dict[str, Any],
    *,
    days: int = 30,
    limit: int = 25,
    sort: str = "avg",
    exclude_external: bool = False,
) -> dict[str, Any]:
    leaderboard = _build_demo_category_rows(spec, days)
    if exclude_external:
        leaderboard = [
            row
            for row in leaderboard
            if row.get("isPartner") or int(row.get("avgViewers", 0) or 0) <= 100
        ]

    sort_key = "peakViewers" if str(sort).strip().lower() == "peak" else "avgViewers"
    leaderboard.sort(key=lambda item: float(item.get(sort_key, 0) or 0), reverse=True)

    your_rank = 1
    for index, row in enumerate(leaderboard, start=1):
        row["rank"] = index
        if row.get("isYou"):
            your_rank = index

    limited_rows = _slice_limit(leaderboard, min(max(1, limit), len(leaderboard)))
    return {
        "leaderboard": limited_rows,
        "totalStreamers": len(leaderboard),
        "yourRank": your_rank,
    }


def build_demo_category_comparison(spec: dict[str, Any]) -> dict[str, Any]:
    payload = _copy_payload(get_category_comparison())
    your_stats = payload.get("yourStats", {})
    percentiles = payload.get("percentiles", {})
    your_stats["avgViewers"] = int(round(float(your_stats.get("avgViewers", 0) or 0) * float(spec.get("viewer_factor", 1.0))))
    your_stats["peakViewers"] = int(round(float(your_stats.get("peakViewers", 0) or 0) * float(spec.get("peak_factor", 1.0))))
    your_stats["retention10m"] = round(
        max(15.0, min(98.0, float(your_stats.get("retention10m", 0) or 0) + float(spec.get("retention_shift", 0.0)))),
        1,
    )
    your_stats["chatHealth"] = int(round(max(12.0, min(99.0, float(your_stats.get("chatHealth", 0) or 0) * float(spec.get("engagement_pct_factor", 1.0))))))
    category_rank = int(spec.get("category_rank", payload.get("categoryRank", 1)))
    category_total = int(spec.get("category_total", payload.get("categoryTotal", 58)))
    rank_percentile = max(
        1,
        min(99, int(round(((category_total - category_rank) / max(1, category_total - 1)) * 100))),
    )
    percentiles["avgViewers"] = rank_percentile
    percentiles["peakViewers"] = max(1, min(99, rank_percentile + 4))
    percentiles["retention10m"] = max(10, min(99, int(round(float(percentiles.get("retention10m", 0) or 0) + float(spec.get("retention_shift", 0.0))))))
    percentiles["chatHealth"] = max(10, min(99, int(round(float(percentiles.get("chatHealth", 0) or 0) * float(spec.get("engagement_pct_factor", 1.0))))))
    payload["categoryRank"] = category_rank
    payload["categoryTotal"] = category_total
    return payload


def build_demo_monetization(spec: dict[str, Any]) -> dict[str, Any]:
    payload = _copy_payload(get_monetization())
    ads_factor = float(spec.get("ads_factor", spec.get("activity_factor", 1.0)) or 1.0)
    hype_factor = float(spec.get("hype_factor", spec.get("activity_factor", 1.0)) or 1.0)
    money_factor = float(spec.get("monetization_factor", 1.0) or 1.0)

    ads = payload.get("ads", {})
    ads["total"] = int(round(float(ads.get("total", 0) or 0) * ads_factor))
    ads["auto"] = int(round(float(ads.get("auto", 0) or 0) * ads_factor))
    ads["manual"] = max(0, ads["total"] - ads["auto"])
    ads["sessions_with_ads"] = int(round(float(ads.get("sessions_with_ads", 0) or 0) * float(spec.get("activity_factor", 1.0))))
    ads["avg_viewer_drop_pct"] = round(
        max(1.2, min(34.0, float(ads.get("avg_viewer_drop_pct", 0) or 0) - float(spec.get("retention_shift", 0.0)) * 0.35)),
        1,
    )

    hype_train = payload.get("hype_train", {})
    hype_train["total"] = int(round(float(hype_train.get("total", 0) or 0) * hype_factor))
    hype_train["avg_level"] = round(max(1.0, min(5.0, float(hype_train.get("avg_level", 0) or 0) * (0.72 + hype_factor * 0.18))), 1)
    hype_train["max_level"] = int(round(max(float(hype_train.get("avg_level", 0) or 0), float(hype_train.get("max_level", 0) or 0) * (0.7 + hype_factor * 0.15))))

    bits = payload.get("bits", {})
    bits["total"] = int(round(float(bits.get("total", 0) or 0) * money_factor))
    bits["cheer_events"] = int(round(float(bits.get("cheer_events", 0) or 0) * max(0.4, money_factor * 0.52)))

    subs = payload.get("subs", {})
    subs["total_events"] = int(round(float(subs.get("total_events", 0) or 0) * money_factor))
    subs["gifted"] = int(round(float(subs.get("gifted", 0) or 0) * max(0.5, money_factor * 0.78)))
    return payload


def _copy_payload(data: Any) -> Any:
    return copy.deepcopy(data)


def _replace_demo_strings(value: Any, spec: dict[str, Any]) -> Any:
    if isinstance(value, dict):
        return {key: _replace_demo_strings(item, spec) for key, item in value.items()}
    if isinstance(value, list):
        return [_replace_demo_strings(item, spec) for item in value]
    if isinstance(value, str):
        return (
            value.replace(DEMO_STREAMER, spec["login"])
            .replace(DEMO_DISPLAY_NAME, spec["display_name"])
        )
    return value


def _round_like(original: int | float, new_value: float) -> int | float:
    if isinstance(original, bool):
        return original
    if isinstance(original, int):
        return int(round(new_value))
    return round(new_value, 1)


def _scale_ratio(value: int | float, factor: float) -> int | float:
    if isinstance(value, int) and not isinstance(value, bool):
        return max(0, int(round(float(value) * factor)))
    return round(max(0.0, float(value) * factor), 1)


def _transform_numeric(value: int | float, key: str, spec: dict[str, Any]) -> int | float:
    lower = key.lower()
    if lower.endswith("id") or lower in {
        "id",
        "raidid",
        "rank",
        "yourrank",
        "weekday",
        "month",
        "year",
        "hour",
        "number",
        "page",
        "perpage",
        "window_days",
        "windowdays",
        "days",
        "minutefromstart",
    }:
        return value

    if "retention" in lower:
        adjusted = max(0.0, min(100.0, float(value) + float(spec["retention_shift"])))
        return _round_like(value, adjusted)
    if "dropoff" in lower:
        adjusted = max(0.0, min(100.0, float(value) - float(spec["retention_shift"]) * 0.7))
        return _round_like(value, adjusted)
    if any(token in lower for token in ("pct", "percentage", "ratio", "rate", "share")):
        upper_bound = 1.0 if float(value) <= 1.0 else 100.0
        adjusted = max(0.0, min(upper_bound, float(value) * float(spec["engagement_pct_factor"])))
        return _round_like(value, adjusted)
    if any(token in lower for token in ("message", "chatter", "chat", "lurker", "interaction", "engagement")):
        return _scale_ratio(value, float(spec["chat_factor"]))
    if any(token in lower for token in ("gift", "bit", "sub", "ads", "hype", "monet", "revenue", "payout", "cheer")):
        return _scale_ratio(value, float(spec["monetization_factor"]))
    if any(token in lower for token in ("follower", "follow", "growth")):
        return _scale_ratio(value, float(spec["follower_factor"]))
    if any(token in lower for token in ("viewer", "peak", "hourswatched", "watchtime")):
        factor = float(spec["peak_factor"]) if "peak" in lower else float(spec["viewer_factor"])
        return _scale_ratio(value, factor)
    if any(token in lower for token in ("duration", "airtime", "hours", "minute")):
        return _scale_ratio(value, float(spec["duration_factor"]))
    if any(token in lower for token in ("session", "streamcount", "sample", "count", "total", "streams", "gamesplayed")):
        return _scale_ratio(value, float(spec["activity_factor"]))
    if "score" in lower:
        return _scale_ratio(value, 0.98 + (float(spec["viewer_factor"]) - 1.0) * 0.35)
    return value


def _transform_demo_payload(value: Any, spec: dict[str, Any], key: str = "") -> Any:
    if isinstance(value, dict):
        return {item_key: _transform_demo_payload(item, spec, item_key) for item_key, item in value.items()}
    if isinstance(value, list):
        return [_transform_demo_payload(item, spec, key) for item in value]
    if isinstance(value, bool) or value is None:
        return value
    if isinstance(value, (int, float)):
        return _transform_numeric(value, key, spec)
    if isinstance(value, str):
        return _replace_demo_strings(value, spec)
    return value


def _apply_profile_overrides(kind: str, payload: Any, spec: dict[str, Any]) -> Any:
    if kind == "rankings" and isinstance(payload, list):
        for row in payload:
            if isinstance(row, dict) and str(row.get("login", "")).lower() == spec["login"]:
                row["rank"] = spec["rank_position"]
                break
        return payload

    if not isinstance(payload, dict):
        return payload

    if kind == "overview":
        payload["streamer"] = spec["login"]
        payload["scores"] = dict(spec["overview_scores"])
        payload["categoryRank"] = spec["category_rank"]
        payload["categoryTotal"] = spec.get("category_total", 64)
        payload["findings"] = _copy_payload(spec["findings"])
        payload["actions"] = _copy_payload(spec["actions"])
        for index, session in enumerate(payload.get("sessions", [])):
            if isinstance(session, dict):
                session["title"] = spec["session_titles"][index % len(spec["session_titles"])]
    elif kind == "coaching":
        payload["streamer"] = spec["login"]
        title_analysis = payload.get("titleAnalysis")
        if isinstance(title_analysis, dict):
            for index, row in enumerate(title_analysis.get("yourTitles", [])):
                if isinstance(row, dict):
                    row["title"] = spec["session_titles"][index % len(spec["session_titles"])]
    return payload


def _base_demo_payload(
    kind: str,
    *,
    days: int,
    months: int,
    limit: int,
    metric: str,
    login: str,
    sort: str,
    order: str,
    filter_type: str,
    search: str,
    page: int,
    per_page: int,
    source: str,
    exclude_external: bool,
) -> Any:
    if kind == "overview":
        return get_overview(days)
    if kind == "monthly-stats":
        stats = get_monthly_stats()
        return stats[-max(1, min(months, len(stats))):]
    if kind == "weekly-stats":
        return get_weekday_stats()
    if kind == "hourly-heatmap":
        return get_hourly_heatmap()
    if kind == "calendar-heatmap":
        return get_calendar_heatmap()
    if kind == "chat-analytics":
        return get_chat_analytics()
    if kind == "viewer-overlap":
        return _slice_limit(get_viewer_overlap(), limit)
    if kind == "tag-analysis":
        return _slice_limit(get_tag_analysis(), limit)
    if kind == "tag-analysis-extended":
        return _slice_limit(get_tag_analysis_extended(), limit)
    if kind == "title-performance":
        return _slice_limit(get_title_performance(), limit)
    if kind == "rankings":
        return _slice_limit(get_rankings(metric), limit)
    if kind == "category-comparison":
        return get_category_comparison()
    if kind == "watch-time-distribution":
        return get_watch_time_distribution()
    if kind == "follower-funnel":
        return get_follower_funnel()
    if kind == "audience-insights":
        return get_audience_insights()
    if kind == "audience-demographics":
        return get_audience_demographics()
    if kind == "viewer-timeline":
        return get_viewer_timeline(days)
    if kind == "category-leaderboard":
        return get_category_leaderboard()
    if kind == "coaching":
        return get_coaching()
    if kind == "monetization":
        return get_monetization()
    if kind == "category-timings":
        payload = get_category_timings()
        if isinstance(payload, dict):
          payload["source"] = source if source in {"category", "tracked"} else "category"
          payload["windowDays"] = min(max(days, 7), 365)
        return payload
    if kind == "category-activity-series":
        payload = get_category_activity_series()
        if isinstance(payload, dict):
            payload["windowDays"] = min(max(days, 7), 365)
            payload["source"] = source if source in {"category", "tracked"} else "category"
        return payload
    if kind == "lurker-analysis":
        return get_lurker_analysis()
    if kind == "raid-retention":
        return get_raid_retention()
    if kind == "viewer-directory":
        return get_viewer_directory(
            sort=sort,
            order=order,
            filter_type=filter_type,
            search=search,
            page=page,
            per_page=per_page,
        )
    if kind == "viewer-detail":
        return get_viewer_detail(login)
    if kind == "viewer-segments":
        return get_viewer_segments()
    if kind == "viewer-profiles":
        return get_viewer_profiles()
    if kind == "audience-sharing":
        return get_audience_sharing()
    if kind == "exp-overview":
        return get_exp_overview(days)
    if kind == "exp-game-breakdown":
        return get_exp_game_breakdown(days)
    if kind == "exp-game-transitions":
        return get_exp_game_transitions(days)
    if kind == "exp-growth-curves":
        return get_exp_growth_curves(days)
    raise KeyError(f"Unknown demo fixture kind: {kind}")


def build_demo_payload(
    kind: str,
    *,
    streamer: str | None = None,
    days: int = 30,
    months: int = 12,
    limit: int = 20,
    metric: str = "viewers",
    login: str = "",
    sort: str = "sessions",
    order: str = "desc",
    filter_type: str = "all",
    search: str = "",
    page: int = 1,
    per_page: int = 50,
    source: str = "category",
    exclude_external: bool = False,
) -> Any:
    spec = _resolve_demo_profile(streamer)
    if kind == "rankings":
        return _slice_limit(build_demo_rankings(spec, metric), limit)
    if kind == "category-leaderboard":
        return build_demo_category_leaderboard(
            spec,
            days=days,
            limit=limit,
            sort=sort,
            exclude_external=exclude_external,
        )
    if kind == "category-comparison":
        return build_demo_category_comparison(spec)
    if kind == "monetization":
        return build_demo_monetization(spec)
    if kind == "viewer-directory":
        return build_demo_viewer_directory(
            spec,
            sort=sort,
            order=order,
            filter_type=filter_type,
            search=search,
            page=page,
            per_page=per_page,
        )
    if kind == "viewer-detail":
        return build_demo_viewer_detail(spec, login)
    if kind == "viewer-segments":
        return build_demo_viewer_segments(spec)
    if kind == "viewer-profiles":
        return build_demo_viewer_profiles(spec)

    base_payload = _base_demo_payload(
        kind,
        days=days,
        months=months,
        limit=limit,
        metric=metric,
        login=login,
        sort=sort,
        order=order,
        filter_type=filter_type,
        search=search,
        page=page,
        per_page=per_page,
        source=source,
        exclude_external=exclude_external,
    )
    payload = _copy_payload(base_payload)
    payload = _replace_demo_strings(payload, spec)
    payload = _transform_demo_payload(payload, spec)
    payload = _apply_profile_overrides(kind, payload, spec)
    return payload


def _demo_ai_points(
    spec: dict[str, Any],
    *,
    days: int,
    game_filter: str,
    snapshot: dict[str, Any],
) -> list[dict[str, Any]]:
    avg_viewers = int(snapshot.get("avgViewers", 0) or 0)
    peak_viewers = int(snapshot.get("peakViewers", 0) or 0)
    followers = int(snapshot.get("followersGained", 0) or 0)
    retention = round(float(snapshot.get("avgRetention10m", 0) or 0), 1)
    dropoff = round(float(snapshot.get("avgDropoffPct", 0) or 0), 1)
    chatters = int(snapshot.get("avgChatters", 0) or 0)
    stream_count = int(snapshot.get("streamCount", 0) or 0)
    mode_label = "Deadlock-Fokus" if game_filter == "deadlock" else "Gesamtprogramm"

    profile_focus = {
        "smallquest_tv": "Nutze die hohe Naehe zur Community, aber baue gezielt wiederholbare Discovery-Hooks ein.",
        "midcore_live": "Wandle die solide Mid-Tier-Basis in einen klaren Hero-Slot mit wiederkehrenden Peak-Signalen um.",
        "megaarena_gg": "Sichere Reichweite operativ ab, damit Scale nicht an Retention, Ads oder Session-Struktur verloren geht.",
    }[spec["login"]]

    return [
        {
            "number": 1,
            "priority": "kritisch",
            "title": "Opening-Fenster schaerfen",
            "analysis": f"Im {mode_label} liegen {avg_viewers} Ø Viewer und {retention}% 10-Min-Retention nah genug zusammen, um den Einstieg klar zu optimieren. Der Peak von {peak_viewers} zeigt, dass Reichweite vorhanden ist, aber zu viel Momentum noch nach dem Start verschenkt wird.",
            "action": "Baue in den ersten 8 Minuten einen klaren Hook, einen sichtbaren Session-Plan und den ersten messbaren Payoff ein.",
            "expectedImpact": "Hoehere Fruehbindung und sauberere Conversion der Peak-Wellen in stabile Durchschnittswerte.",
        },
        {
            "number": 2,
            "priority": "kritisch",
            "title": "Prime-Slot fokussieren",
            "analysis": f"Mit {stream_count} Sessions auf {days} Tage ist genug Datenmasse da, um den besten Prime-Slot enger zu schneiden. {profile_focus}",
            "action": "Lasse nur einen klaren Hauptslot unangetastet und fuehre Experimente isoliert an einem zweiten Wochentag durch.",
            "expectedImpact": "Weniger Rauschen im Zeitplan und schneller sichtbare Ursache-Wirkung bei Slot-Aenderungen.",
        },
        {
            "number": 3,
            "priority": "kritisch",
            "title": "Hook auf Zielsignal ausrichten",
            "analysis": f"Der Kanal gewinnt im betrachteten Zeitraum {followers} Follower, aber die Differenz zwischen Peak ({peak_viewers}) und Chat-Tiefe ({chatters} aktive Chatter im Schnitt) zeigt ungenutztes Conversion-Potenzial.",
            "action": "Definiere pro Format genau ein Zielsignal: Follows, aktive Chat-Teilnahme oder Session-Retention, und mappe Hook, CTA und Mid-Stream-Event darauf.",
            "expectedImpact": "Staerkere Uebersetzung von Reichweite in wiederkehrende Community- und Growth-Signale.",
        },
        {
            "number": 4,
            "priority": "hoch",
            "title": "Session-Mitte absichern",
            "analysis": f"Die aktuelle Dropoff-Rate von {dropoff}% ist beherrschbar, aber sie zeigt, wo das Format nach dem ersten Peak an Klarheit verliert. Gerade bei guten Startwerten kostet eine diffuse Mittelphase unverhaeltnismaessig viel Durchschnittsreichweite.",
            "action": "Plane zur Halbzeit ein wiederkehrendes Segment mit klarer Erwartung: Review, Community-Event, Challenge oder Switch.",
            "expectedImpact": "Stabilerer Mittelteil und weniger abrupte Zuschauerabbrueche nach dem ersten Hoch.",
        },
        {
            "number": 5,
            "priority": "hoch",
            "title": "Titel-Set reduzieren",
            "analysis": "Die aktuellen Formate funktionieren, aber ihre Hooks sind noch zu breit gestreut. Ein kleineres, konsequent iteriertes Titel-Set macht Unterschiede schneller sichtbar und verbessert den Lerneffekt pro Session.",
            "action": "Arbeite fuer 3 Wochen mit drei festen Titel-Frameworks und aendere pro Stream nur ein Element.",
            "expectedImpact": "Messbarere CTR-/Retention-Learnings statt Bauchgefuehl ueber einzelne Ausreisser.",
        },
        {
            "number": 6,
            "priority": "hoch",
            "title": "Chat-Momentum gezielt nutzen",
            "analysis": f"{chatters} durchschnittliche aktive Chatter sind genug, um soziale Dynamik sichtbar zu steuern. Das Signal ist stark genug, dass Format- und CTA-Aenderungen kurzfristig im Chat lesbar werden.",
            "action": "Setze pro Stream zwei klar definierte Interaktionspunkte mit Poll, Call oder Community-Entscheidung und tracke deren Effekt auf Viewer-Haltekurve.",
            "expectedImpact": "Hoehere Beteiligung pro Viewer und bessere Lesbarkeit der wirksamen Community-Momente.",
        },
        {
            "number": 7,
            "priority": "hoch",
            "title": "Netzwerk-Effekte planbar machen",
            "analysis": "Das Netzwerk-Signal ist stark genug, um nicht nur Zufall zu sein. Reichweite und Wiederkehr profitieren am meisten, wenn Partner-Slots bewusst zur eigenen Formatstruktur passen.",
            "action": "Raids und Cross-Community-Formate auf dieselben Themenfenster legen, in denen der eigene Kanal ohnehin ueberdurchschnittlich gut haelt.",
            "expectedImpact": "Weniger Streuverlust bei externem Traffic und bessere Wiederkehrraten nach Partner-Kontakten.",
        },
        {
            "number": 8,
            "priority": "mittel",
            "title": "Monetization nicht isoliert denken",
            "analysis": "Support-Signale funktionieren am besten, wenn sie wie ein organischer Teil des Formats wirken. Einzelne Revenue-Peaks ohne Formatanker sind schwer wiederholbar.",
            "action": "Koppele Support-Momente an wiederkehrende Segmente und formuliere ihren Nutzen fuer den Stream klarer aus.",
            "expectedImpact": "Planbarere Monetization-Spitzen ohne Bruch im Zuschauererlebnis.",
        },
        {
            "number": 9,
            "priority": "mittel",
            "title": "Viewer-Rueckkehr sichtbar machen",
            "analysis": "Der Datensatz ist breit genug, um Wiederkehr nicht nur global, sondern pro Format zu lesen. Genau dort liegen die stabilsten Qualitaetssignale jenseits von Einzelpeaks.",
            "action": "Vergleiche jede Woche zwei Formate ueber Rueckkehr, Chat-Aktivierung und Follower-pro-Stunde statt nur ueber Peak-Viewer.",
            "expectedImpact": "Bessere Priorisierung der Formate, die nachhaltig tragen statt nur kurzfristig ziehen.",
        },
        {
            "number": 10,
            "priority": "mittel",
            "title": "Experiment-Takt verkuerzen",
            "analysis": "Der Kanal hat genug Kontrast in den Daten, um schneller zu lernen. Lange Iterationszyklen kosten vor allem dort, wo gute Signale bereits vorhanden sind.",
            "action": "Lege einen 14-Tage-Rhythmus fuer kleine, dokumentierte Experimente fest und bewerte jedes davon ueber dieselben drei Kernmetriken.",
            "expectedImpact": "Schnelleres Lernen mit weniger Zufall und saubereren Produktentscheidungen pro Format.",
        },
    ]


def build_demo_ai_analysis(
    *,
    streamer: str | None = None,
    days: int = 30,
    game_filter: str = "all",
) -> dict[str, Any]:
    spec = _resolve_demo_profile(streamer)
    overview = build_demo_payload("overview", streamer=spec["login"], days=days)
    summary = dict(overview.get("summary", {}))
    sessions = [
        session for session in overview.get("sessions", []) if isinstance(session, dict)
    ]
    avg_dropoff = (
        round(
            sum(float(session.get("dropoffPct", 0) or 0) for session in sessions)
            / max(1, len(sessions)),
            1,
        )
        if sessions
        else 18.0
    )
    stream_count = max(1, int(summary.get("streamCount", len(sessions)) or len(sessions) or 1))
    analysis_id = 9000 + ALLOWED_DEMO_PROFILES.index(spec["login"]) * 100 + min(max(days, 7), 365)
    if game_filter == "deadlock":
        summary["streamCount"] = max(4, int(round(stream_count * 0.68)))
        summary["totalAirtime"] = round(float(summary.get("totalAirtime", summary.get("totalHours", 0)) or 0) * 0.72, 1)
        summary["avgViewers"] = int(round(float(summary.get("avgViewers", 0) or 0) * 1.06))
        summary["peakViewers"] = int(round(float(summary.get("peakViewers", 0) or 0) * 1.08))
        summary["followersGained"] = int(round(float(summary.get("followersGained", 0) or 0) * 0.76))
        avg_dropoff = round(avg_dropoff * 0.92, 1)

    snapshot_streams = max(1, int(summary.get("streamCount", stream_count) or stream_count))
    snapshot = {
        "streamCount": snapshot_streams,
        "totalHours": round(float(summary.get("totalAirtime", summary.get("totalHours", 0)) or 0), 1),
        "avgViewers": int(round(float(summary.get("avgViewers", 0) or 0))),
        "peakViewers": int(round(float(summary.get("peakViewers", 0) or 0))),
        "followersGained": int(round(float(summary.get("followersGained", 0) or 0))),
        "avgRetention10m": round(float(summary.get("retention10m", 0) or 0), 1),
        "avgDropoffPct": avg_dropoff,
        "avgChatters": int(round(float(summary.get("uniqueChatters", 0) or 0) / snapshot_streams)),
    }

    points = _demo_ai_points(spec, days=days, game_filter=game_filter, snapshot=snapshot)
    return {
        "id": analysis_id,
        "streamer": spec["login"],
        "days": min(max(days, 7), 365),
        "gameFilter": "deadlock" if game_filter == "deadlock" else "all",
        "generatedAt": _viewer_timestamp(1 + ALLOWED_DEMO_PROFILES.index(spec["login"]) * 2, hour=11),
        "points": points,
        "dataSnapshot": snapshot,
    }


def build_demo_ai_history(*, streamer: str | None = None, limit: int = 20) -> list[dict[str, Any]]:
    spec = _resolve_demo_profile(streamer)
    presets = (
        (14, "deadlock", 2, 9),
        (30, "all", 7, 11),
        (90, "all", 19, 14),
    )
    out: list[dict[str, Any]] = []
    for index, (days, game_filter, days_ago, hour) in enumerate(presets, start=1):
        entry = build_demo_ai_analysis(streamer=spec["login"], days=days, game_filter=game_filter)
        points = list(entry.get("points", []))
        entry["id"] = 7000 + ALLOWED_DEMO_PROFILES.index(spec["login"]) * 100 + index
        entry["generatedAt"] = _viewer_timestamp(days_ago, hour=hour)
        entry["model"] = "claude-opus-4-6"
        entry["kritischCount"] = sum(1 for point in points if point.get("priority") == "kritisch")
        entry["hochCount"] = sum(1 for point in points if point.get("priority") == "hoch")
        entry["mittelCount"] = sum(1 for point in points if point.get("priority") == "mittel")
        out.append(entry)
    out.sort(key=lambda item: str(item.get("generatedAt", "")), reverse=True)
    return out[: max(1, min(limit, 50))]

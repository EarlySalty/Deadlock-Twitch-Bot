"""
Analytics API v2 - Chat Deep Analysis Mixin.

Chat hype detection, content analysis (hero mentions, topics, sentiment),
social graph (@mentions, conversation hubs), and viewer personality profiles.
"""

from __future__ import annotations

import asyncio
import logging
import math
import re
from datetime import UTC, datetime, timedelta

from aiohttp import web

from ..core.chat_bots import build_known_chat_bot_not_in_clause
from ..storage import pg as storage
from .chat_social_graph_loader import load_chat_social_graph_payload
from .error_utils import analytics_internal_error_response
from .raw_chat_status import build_raw_chat_status

log = logging.getLogger("TwitchStreams.AnalyticsV2")

# ── Deadlock Hero Names + Aliases ──

DEADLOCK_HEROES: dict[str, list[str]] = {
    # Original roster
    "abrams": ["abrams"],
    "bebop": ["bebop"],
    "dynamo": ["dynamo"],
    "grey_talon": ["grey talon", "talon", "gt"],
    "haze": ["haze"],
    "infernus": ["infernus", "inf"],
    "ivy": ["ivy"],
    "kelvin": ["kelvin"],
    "lady_geist": ["lady geist", "geist"],
    "lash": ["lash"],
    "mcginnis": ["mcginnis"],
    "mirage": ["mirage"],
    "mo_krill": ["mo & krill", "mo and krill", "mo krill", "mokrill"],
    "paradox": ["paradox"],
    "pocket": ["pocket"],
    "seven": ["seven"],
    "shiv": ["shiv"],
    "vindicta": ["vindicta", "vindi"],
    "viscous": ["viscous"],
    "warden": ["warden"],
    "wraith": ["wraith"],
    "yamato": ["yamato", "yama"],
    "calico": ["calico"],
    "holliday": ["holliday", "holiday"],
    # Newer heroes
    "apollo": ["apollo"],
    "billy": ["billy"],
    "celeste": ["celeste"],
    "doorman": ["doorman"],
    "drifter": ["drifter"],
    "graves": ["graves"],
    "mina": ["mina"],
    "paige": ["paige"],
    "rem": ["rem"],
    "silver": ["silver"],
    "sinclair": ["sinclair"],
    "venator": ["venator"],
    "victor": ["victor"],
    "vyper": ["vyper", "viper"],
}

# Pre-compile: alias → hero_key, longest-first for greedy matching
_ALIAS_TO_HERO: list[tuple[str, str]] = []
for _hero, _aliases in DEADLOCK_HEROES.items():
    for _alias in sorted(_aliases, key=len, reverse=True):
        _ALIAS_TO_HERO.append((_alias.lower(), _hero))

# ── Topic Keywords ──

TOPIC_KEYWORDS: dict[str, list[str]] = {
    "builds": [
        "build", "item", "weapon", "spirit", "vitality", "flex slot",
        "tesla", "headshot", "lifestrike", "majestic", "mystic",
        "soul", "ability", "active item", "component", "upgrade",
    ],
    "ranked": [
        "rank", "mmr", "elo", "ranked", "competitive", "placement",
        "rank up", "derank",
        # Deadlock rank tiers
        "initiate", "seeker", "alchemist", "arcanist", "ritualist",
        "emissary", "archon", "oracle", "phantom", "ascendant", "eternus",
    ],
    "meta": [
        "meta", "nerf", "buff", "patch", "broken", "op", "overpowered", "underpowered",
        "rework", "hotfix", "s-tier", "s tier", "a-tier", "comp", "balance",
        "update", "changelog", "patchnotes",
    ],
    "gameplay": [
        "push", "gank", "lane", "jungle", "tower", "urn",
        "soul orb", "mid boss", "patron", "guardian", "walker",
        "rejuv", "zipline", "teleport", "shrine", "flex",
        "last hit", "deny", "farm", "creep", "troop",
    ],
    "backseat": [
        # English directives
        "you should", "you need to", "just buy", "just play", "just pick",
        "pick him", "pick her", "don't buy", "don't pick", "don't play",
        "play safe", "play aggressive", "go left", "go right", "go back",
        "buy this", "try this", "swap to", "switch to", "sell that",
        "why didn't you", "why don't you", "you could have",
        "should have", "shouldve", "shoulda",
        # German directives
        "kauf", "nimm", "spiel", "geh", "mach", "probier",
        "du musst", "du solltest", "warum kaufst", "warum spielst",
        "hättest du", "kauf dir", "nimm dir", "versuch mal",
        "spiel mal", "geh mal", "nicht kaufen", "nicht spielen",
        "hör auf", "lass das",
    ],
}

# Backseat phrases are checked separately — they're imperative patterns
# that indicate the chat is "coaching" the streamer
BACKSEAT_PHRASES = (
    "you should", "you need to", "just buy", "just play", "just pick",
    "don't buy", "don't pick", "play safe", "play aggressive",
    "why didn't you", "why don't you", "you could have",
    "should have", "shouldve",
    "du musst", "du solltest", "warum kaufst", "warum spielst",
    "hättest du", "kauf dir", "nimm dir", "versuch mal",
    "spiel mal", "geh mal", "nicht kaufen", "hör auf", "lass das",
)

# Messages about socials/channel/meta-chat (non gameplay).
SOCIAL_MARKERS = (
    "discord", "youtube", "tiktok", "instagram", "social",
    "follow", "abo", "sub", "community",
    "raid", "clip", "danke", "thanks", "thx",
)

# Emote-heavy or hype one-liners that otherwise often land in "other".
REACTION_TOKENS = frozenset({
    "gg", "wp", "lul", "kekw", "xd", "xdd", "kappa",
    "pog", "pogchamp", "poggers", "peepo", "catjam",
    "nice", "geil", "krass", "banger", "lol", "lmao", "ggs",
    "omg", "wtf", "insane", "crazy", "hype", "letsgo", "sadge",
    "rip", "damn", "woah", "huh", "haha", "hahaha",
    "notlikethis", "shruge", "heyguys", "cheerstothat",
    "uff", "wow", "nah", "safe",
})

REACTION_PHRASES = (
    "let's go",
    "lets go",
    "<3",
    ":d",
    ":(",
    ":o",
    "skill issue",
    "all good",
)

EMOTE_PREFIXES = (
    "dhalu",
    "frag",
    "peepo",
    "kitty",
    "owo",
    "uwu",
    "xdgara",
    "seems",
)

EMOTE_SUFFIXES = (
    "cheer",
    "hype",
    "love",
    "clap",
    "lul",
    "dance",
    "jam",
    "hug",
)

SMALLTALK_TOKENS = frozenset({
    "ja", "jaa", "jaaa", "nein", "nö", "ne", "yes", "yep",
    "ok", "okay", "jo", "klar", "mhm", "doch", "ah", "oh", "oha",
    "stimmt", "wieder", "zusammen",
    "true", "sure", "no", "same", "check", "achso", "stark", "na",
    "genau", "man",
})

GREETING_TOKENS = frozenset({
    "servus", "nabend", "abend", "nacht", "hallo", "hey", "hi",
    "hello", "bye", "gn8", "huhu", "moooin", "moin", "wb", "back", "o7",
})

GREETING_PHRASES = (
    "guten morgen",
    "guten abend",
    "gute nacht",
    "bye bye",
    "wie läufts",
)

# ── Sentiment Wordlists (Twitch-culture-aware) ──

POSITIVE_WORDS = frozenset({
    # English Twitch
    "gg", "nice", "pog", "pogchamp", "poggers", "pogu", "goat",
    "clutch", "insane", "clean", "sick", "cracked", "huge", "based",
    "godlike", "gigachad", "hype", "letsgo", "wp",
    "legendary", "amazing", "incredible", "beautiful", "perfect",
    "dope", "fire", "lit", "banger", "chad", "king", "queen",
    "mvp", "carry", "diff", "outplay", "outplayed",
    # Emote-words (typed as text)
    "catjam", "peped", "pepejam", "widepeepo", "feelsgood",
    "kreygasm", "trihard", "kappa", "jebaited",
    # German positive
    "geil", "krass", "stark", "mega", "hammer", "bombe",
    "gott", "legende", "ehre", "ehrenmann", "digga",
    "danke", "liebe", "super", "genial", "wahnsinn",
    "gönnung", "bruder", "alter", "heftig",
    # Gratitude / community
    "thanks", "thx", "love", "ily", "hearts",
})

NEGATIVE_WORDS = frozenset({
    # English Twitch
    "trash", "garbage", "boring", "cringe", "bad", "worst", "hate",
    "toxic", "throw", "throwing", "troll", "report",
    "lost", "washed", "dog", "bot", "yikes", "braindead", "griefing", "ez",
    "dogwater", "clown", "noob", "feeder", "feeding", "inting",
    "useless", "terrible", "awful", "pathetic", "disgusting",
    "cope", "copium", "ratio", "salty", "tilted", "malding",
    "pepega", "omegalul", "weirdchamp", "hasmods",
    # Game-specific frustration
    "broken", "bugged", "unfair", "unbalanced", "cheater", "hacker",
    "smurf", "smurfing", "griefer", "afk", "ragequit",
    # German negative
    "mies", "schlecht", "langweilig", "nervig", "müll",
    "schrott", "kacke", "scheisse", "mist", "grottig",
    "peinlich", "lächerlich", "asozial", "hurensohn", "spast",
    "behindert", "dumm", "kack", "dreck",
})

# Multi-word phrases — checked via substring before tokenizing
POSITIVE_PHRASES = (
    "lets go", "let's go", "gg wp", "well played", "so good", "so sick",
    "no way", "holy shit", "my goat", "too good", "what a play",
    "so clean", "big brain", "guter stream", "geiler stream",
    "macht spass", "feier ich",
)
NEGATIVE_PHRASES = (
    "so bad", "too bad", "skill issue", "get good", "git gud",
    "so boring", "dead game", "touch grass", "no shot",
    "kein skill", "macht keinen spass", "ist kaputt",
)

# Short tokens (1-2 chars) that are too ambiguous for substring matching.
# Only counted when they appear as an isolated token in split().
SHORT_POSITIVE = frozenset({"w", "dw", "gg"})
SHORT_NEGATIVE = frozenset({"l", "f", "ff", "nah"})

_WORD_RE = re.compile(r"[a-z0-9äöüß_+#']+")


def _detect_heroes(content_lower: str) -> list[str]:
    """Return list of hero keys mentioned in a message."""
    found: list[str] = []
    for alias, hero in _ALIAS_TO_HERO:
        if alias in content_lower and hero not in found:
            found.append(hero)
    return found


def _detect_topics(content_lower: str) -> list[str]:
    """Return list of topic categories matched in a message."""
    topics: list[str] = []
    for topic, keywords in TOPIC_KEYWORDS.items():
        if any(kw in content_lower for kw in keywords):
            topics.append(topic)
    return topics


def _tokenize_words(content_lower: str) -> list[str]:
    """Tokenize lowercased chat content into simple alnum/emote-friendly tokens."""
    return _WORD_RE.findall(content_lower)


def _is_reaction_message(content_lower: str, tokens: list[str] | None = None) -> bool:
    """Heuristic for short emote/hype messages."""
    stripped = content_lower.strip()
    words = tokens or _tokenize_words(content_lower)

    if stripped in {"?", "??", "!", "!!"}:
        return True

    if any(phrase in content_lower for phrase in REACTION_PHRASES):
        return True

    # Emoji-only / symbol-only messages (no alnum tokens) are pure reaction chat.
    if not words and stripped:
        return True

    return any(
        token in REACTION_TOKENS
        or token.startswith(EMOTE_PREFIXES)
        or token.endswith(EMOTE_SUFFIXES)
        or token.startswith("xd")
        or token.startswith("haha")
        for token in words
    )


def _is_command_message(content_lower: str) -> bool:
    """Detect bot/chat command style messages."""
    return content_lower.strip().startswith("!")


def _is_greeting_message(content_lower: str, tokens: list[str] | None = None) -> bool:
    """Heuristic for greeting/goodbye messages."""
    words = tokens or _tokenize_words(content_lower)
    if any(phrase in content_lower for phrase in GREETING_PHRASES):
        return True
    return any(token in GREETING_TOKENS for token in words)


def _is_social_message(content_lower: str) -> bool:
    """Detect social/channel/meta-chat markers."""
    return any(marker in content_lower for marker in SOCIAL_MARKERS)


def _is_smalltalk_message(content_lower: str, tokens: list[str] | None = None) -> bool:
    """Detect short acknowledgements and lightweight banter."""
    words = tokens or _tokenize_words(content_lower)
    if len(words) <= 4 and any(token in SMALLTALK_TOKENS for token in words):
        return True

    alpha_words = [
        token for token in words
        if any(("a" <= ch <= "z") or ch in "äöüß" for ch in token)
    ]
    return 1 <= len(alpha_words) <= 2


def _looks_like_community_message(content_lower: str, tokens: list[str] | None = None) -> bool:
    """Heuristic for community/chat/social messages that don't hit game topics."""
    words = tokens or _tokenize_words(content_lower)

    alpha_words = [
        token for token in words
        if any(("a" <= ch <= "z") or ch in "äöüß" for ch in token)
    ]

    # Multi-word discussion often belongs to stream/community chatter.
    if len(alpha_words) >= 4:
        return True

    # Short Q&A messages should not stay in "other".
    if "?" in content_lower and len(alpha_words) >= 2:
        return True

    return False


def _score_sentiment(content_lower: str) -> int:
    """Score a single chat message as positive (+1), negative (-1) or neutral (0).

    Strategy:
    1. Multi-word phrases checked via substring first (e.g. "lets go", "gg wp")
    2. Tokenize into words via split()
    3. Short ambiguous tokens ("w", "l", "f") only count as isolated tokens
    4. Regular words matched against the frozensets
    5. Count pos vs neg — majority wins, tie → neutral
    """
    if not content_lower or not content_lower.strip():
        return 0

    pos = 0
    neg = 0

    # 1) Multi-word phrases (substring match)
    for phrase in POSITIVE_PHRASES:
        if phrase in content_lower:
            pos += 1
    for phrase in NEGATIVE_PHRASES:
        if phrase in content_lower:
            neg += 1

    # 2) Tokenize
    tokens = content_lower.split()

    for token in tokens:
        # 3) Short tokens — only match as isolated word
        if token in SHORT_POSITIVE:
            pos += 1
            continue
        if token in SHORT_NEGATIVE:
            neg += 1
            continue

        # 4) Regular words (skip very short to avoid false positives)
        if len(token) < 2:
            continue
        if token in POSITIVE_WORDS:
            pos += 1
        elif token in NEGATIVE_WORDS:
            neg += 1

    # 5) Majority wins
    if pos > neg:
        return 1
    if neg > pos:
        return -1
    return 0


def _pearson_r(xs: list[float], ys: list[float]) -> float:
    """Compute Pearson correlation coefficient between two series."""
    n = len(xs)
    if n < 3 or len(ys) != n:
        return 0.0
    mx = sum(xs) / n
    my = sum(ys) / n
    num = sum((x - mx) * (y - my) for x, y in zip(xs, ys))
    dx = math.sqrt(sum((x - mx) ** 2 for x in xs))
    dy = math.sqrt(sum((y - my) ** 2 for y in ys))
    if dx == 0 or dy == 0:
        return 0.0
    return num / (dx * dy)


def _interpret_r(r: float) -> str:
    """Human-readable interpretation of Pearson r."""
    ar = abs(r)
    if ar >= 0.7:
        return "strong_positive" if r > 0 else "strong_negative"
    if ar >= 0.4:
        return "moderate_positive" if r > 0 else "moderate_negative"
    if ar >= 0.2:
        return "weak_positive" if r > 0 else "weak_negative"
    return "none"


def _load_chat_hype_timeline_payload(
    *,
    streamer: str,
    session_id_raw: str,
) -> tuple[int, dict[str, object] | None, int]:
    with storage.readonly_connection() as conn:
        bot_clause, bot_params = build_known_chat_bot_not_in_clause(
            column_expr="m.chatter_login",
            placeholder="%s",
        )

        if session_id_raw:
            try:
                session_id = int(session_id_raw)
            except ValueError:
                return 400, {"error": "Invalid session_id"}, 0
        else:
            row = conn.execute(
                """
            SELECT id FROM twitch_stream_sessions
                WHERE LOWER(streamer_login) = %s
                ORDER BY started_at DESC LIMIT 1
            """,
                (streamer,),
            ).fetchone()
            if not row:
                return 404, {"error": "No sessions found"}, 0
            session_id = row[0]

        sess = conn.execute(
            """
            SELECT id, streamer_login, started_at, duration_seconds, stream_title
            FROM twitch_stream_sessions WHERE id = %s
            """,
            (session_id,),
        ).fetchone()
        if not sess:
            return 404, {"error": "Session not found"}, 0

        session_start = sess[2]
        duration = sess[3] or 0
        title = sess[4] or ""

        mpm_rows = conn.execute(
            f"""
            SELECT
                time_bucket('1 minute', m.message_ts) AS bucket,
                COUNT(*) AS messages,
                COUNT(DISTINCT m.chatter_login) AS unique_chatters
            FROM twitch_chat_messages m
            WHERE m.session_id = %s
              AND m.chatter_login IS NOT NULL
              AND {bot_clause}
            GROUP BY bucket
            ORDER BY bucket
            """,
            (session_id, *bot_params),
        ).fetchall()

        viewer_rows = conn.execute(
            """
            SELECT minutes_from_start, viewer_count
            FROM twitch_session_viewers
            WHERE session_id = %s
            ORDER BY minutes_from_start
            """,
            (session_id,),
        ).fetchall()
        viewer_map: dict[int, int] = {}
        for viewer_row in viewer_rows:
            minute_raw = viewer_row[0]
            viewers_raw = viewer_row[1]
            if minute_raw is None or viewers_raw is None:
                continue
            try:
                minute = int(minute_raw)
                viewers = max(0, int(viewers_raw))
            except (TypeError, ValueError):
                continue
            if minute < 0:
                continue
            viewer_map[minute] = viewers

        timeline: list[dict[str, object]] = []
        msg_counts: list[int] = []

        for row in mpm_rows:
            bucket_ts = row[0]
            msgs = int(row[1])
            chatters = int(row[2])
            if hasattr(bucket_ts, "timestamp") and hasattr(session_start, "timestamp"):
                minute = int((bucket_ts.timestamp() - session_start.timestamp()) / 60)
            else:
                minute = len(timeline)

            viewers = viewer_map.get(minute, 0)
            if viewers == 0:
                for offset in range(-2, 3):
                    nearby_viewers = viewer_map.get(minute + offset, 0)
                    if nearby_viewers > 0:
                        viewers = nearby_viewers
                        break

            timeline.append(
                {
                    "minute": minute,
                    "messages": msgs,
                    "chatters": chatters,
                    "viewers": viewers,
                    "isSpike": False,
                }
            )
            msg_counts.append(msgs)

        avg_mpm = sum(msg_counts) / len(msg_counts) if msg_counts else 0
        peak_mpm = max(msg_counts) if msg_counts else 0
        spikes: list[dict[str, object]] = []

        threshold = max(avg_mpm * 2, 3)
        for entry in timeline:
            if int(entry["messages"]) >= threshold:
                entry["isSpike"] = True
                multiplier = round(int(entry["messages"]) / avg_mpm, 1) if avg_mpm > 0 else 0
                spikes.append(
                    {
                        "minute": entry["minute"],
                        "messages": entry["messages"],
                        "multiplier": multiplier,
                    }
                )

        spikes.sort(key=lambda spike: int(spike["messages"]), reverse=True)

        paired_minutes: list[tuple[float, float]] = []
        for entry in timeline:
            if int(entry["viewers"]) > 0:
                paired_minutes.append((float(entry["messages"]), float(entry["viewers"])))

        chat_vals = [pair[0] for pair in paired_minutes]
        viewer_vals = [pair[1] for pair in paired_minutes]
        r_val = _pearson_r(chat_vals, viewer_vals)

        chat_leads = False
        lag_minutes = 0
        if len(timeline) >= 10:
            best_lag_r = abs(r_val)
            for lag in range(1, 11):
                lagged_chat = chat_vals[:-lag] if lag < len(chat_vals) else []
                lagged_view = viewer_vals[lag:] if lag < len(viewer_vals) else []
                if len(lagged_chat) >= 5:
                    lagged_r = abs(_pearson_r(lagged_chat, lagged_view))
                    if lagged_r > best_lag_r + 0.05:
                        best_lag_r = lagged_r
                        lag_minutes = lag
                        chat_leads = True

        recent_rows = conn.execute(
            """
            SELECT s.id, DATE(s.started_at), s.stream_title
            FROM twitch_stream_sessions s
            WHERE LOWER(s.streamer_login) = %s
              AND s.id != %s
            ORDER BY s.started_at DESC
            LIMIT 10
            """,
            (streamer, session_id),
        ).fetchall()

        recent_sessions: list[dict[str, object]] = []
        for recent_row in recent_rows:
            recent_mpm = conn.execute(
                f"""
                SELECT COUNT(*) AS total,
                       COUNT(*) * 1.0 / GREATEST(1,
                           EXTRACT(EPOCH FROM MAX(m.message_ts) - MIN(m.message_ts)) / 60
                       ) AS avg_mpm
                FROM twitch_chat_messages m
                WHERE m.session_id = %s
                  AND {bot_clause}
                """,
                (recent_row[0], *bot_params),
            ).fetchone()
            recent_sessions.append(
                {
                    "id": recent_row[0],
                    "date": str(recent_row[1]),
                    "title": recent_row[2] or "",
                    "avgMPM": round(float(recent_mpm[1]) if recent_mpm and recent_mpm[1] else 0, 1),
                    "peakMPM": 0,
                }
            )

        raw_chat_status = build_raw_chat_status(
            conn,
            streamer,
            session_ids=[int(session_id)],
        )

    return (
        200,
        {
            "sessionId": session_id,
            "sessionTitle": title,
            "startedAt": session_start.isoformat() if hasattr(session_start, "isoformat") else str(session_start),
            "duration": duration,
            "avgMPM": round(avg_mpm, 1),
            "peakMPM": peak_mpm,
            "timeline": timeline,
            "spikes": spikes[:20],
            "correlation": {
                "chatViewerR": round(r_val, 2),
                "interpretation": _interpret_r(r_val),
                "chatLeadsViewers": chat_leads,
                "lagMinutes": lag_minutes,
            },
            "recentSessions": recent_sessions,
            "rawChatStatus": raw_chat_status,
        },
        session_id,
    )


class _AnalyticsChatDeepMixin:
    """Mixin providing chat deep-analysis endpoints."""

    # ─────────────────────────────────────────────────────────────────────
    # Endpoint 1: /twitch/api/v2/chat-hype-timeline
    # ─────────────────────────────────────────────────────────────────────

    async def _api_v2_chat_hype_timeline(self, request: web.Request) -> web.Response:
        """Chat velocity + viewer overlay per session, spike detection, correlation."""
        self._require_v2_auth(request)
        self._require_extended_plan(request)

        streamer = request.query.get("streamer", "").strip().lower()
        if not streamer:
            return web.json_response({"error": "Streamer required"}, status=400)

        session_id_raw = request.query.get("session_id", "").strip()

        try:
            status, payload, _session_id = await asyncio.to_thread(
                _load_chat_hype_timeline_payload,
                streamer=streamer,
                session_id_raw=session_id_raw,
            )
            return web.json_response(payload, status=status)

        except web.HTTPException:
            raise
        except Exception:
            log.exception("Error in chat-hype-timeline API")
            return analytics_internal_error_response()

    # ─────────────────────────────────────────────────────────────────────
    # Endpoint 2: /twitch/api/v2/chat-content-analysis
    # ─────────────────────────────────────────────────────────────────────

    def _load_chat_content_analysis_payload_sync(
        self,
        *,
        streamer: str,
        days: int,
    ) -> dict[str, object]:
        cutoff = (datetime.now(UTC) - timedelta(days=days)).isoformat()
        with storage.readonly_connection() as conn:
            bot_clause, bot_params = build_known_chat_bot_not_in_clause(
                column_expr="m.chatter_login",
                placeholder="%s",
            )

            rows = conn.execute(
                f"""
                SELECT
                    m.message_ts,
                    m.content,
                    m.chatter_login
                FROM twitch_chat_messages m
                JOIN twitch_stream_sessions s ON s.id = m.session_id
                WHERE LOWER(s.streamer_login) = %s
                  AND m.message_ts >= %s
                  AND m.content IS NOT NULL
                  AND m.content != ''
                  AND {bot_clause}
                ORDER BY m.message_ts
                """,
                (streamer, cutoff, *bot_params),
            ).fetchall()

            hero_counts: dict[str, int] = {}
            topic_counts = {
                "heroes": 0,
                "builds": 0,
                "ranked": 0,
                "meta": 0,
                "gameplay": 0,
                "backseat": 0,
                "commands": 0,
                "social": 0,
                "smalltalk": 0,
                "greeting": 0,
                "community": 0,
                "reaction": 0,
                "other": 0,
            }
            sentiment_buckets: dict[str, dict[str, int]] = {}
            total_positive = 0
            total_negative = 0
            backseat_count = 0
            backseat_examples: list[str] = []
            depth_reaction = 0
            depth_short = 0
            depth_discussion = 0

            for row in rows:
                ts = row[0]
                content = row[1]
                content_lower = content.lower()

                heroes = _detect_heroes(content_lower)
                for hero in heroes:
                    hero_counts[hero] = hero_counts.get(hero, 0) + 1

                topics = _detect_topics(content_lower)
                matched_any = False
                if heroes:
                    topic_counts["heroes"] += 1
                    matched_any = True
                for topic in topics:
                    topic_counts[topic] = topic_counts.get(topic, 0) + 1
                    matched_any = True

                is_backseat = any(phrase in content_lower for phrase in BACKSEAT_PHRASES)
                if is_backseat:
                    topic_counts["backseat"] += 1
                    matched_any = True
                    backseat_count += 1
                    if len(backseat_examples) < 10:
                        example = content[:80] + ("..." if len(content) > 80 else "")
                        backseat_examples.append(example)

                if not matched_any:
                    tokens = _tokenize_words(content_lower)
                    if _is_reaction_message(content_lower, tokens):
                        topic_counts["reaction"] += 1
                        matched_any = True
                    elif _is_greeting_message(content_lower, tokens):
                        topic_counts["greeting"] += 1
                        matched_any = True
                    elif _is_command_message(content_lower):
                        topic_counts["commands"] += 1
                        matched_any = True
                    elif _is_social_message(content_lower):
                        topic_counts["social"] += 1
                        matched_any = True
                    elif _is_smalltalk_message(content_lower, tokens):
                        topic_counts["smalltalk"] += 1
                        matched_any = True
                    elif _looks_like_community_message(content_lower, tokens):
                        topic_counts["community"] += 1
                        matched_any = True

                if not matched_any:
                    topic_counts["other"] += 1

                word_count = len(content.split())
                if word_count <= 3:
                    depth_reaction += 1
                elif word_count <= 10:
                    depth_short += 1
                else:
                    depth_discussion += 1

                score = _score_sentiment(content_lower)
                if hasattr(ts, "strftime"):
                    minute = ts.minute - (ts.minute % 15)
                    bucket_key = ts.strftime(f"%Y-%m-%dT%H:{minute:02d}")
                else:
                    bucket_key = "unknown"

                if bucket_key not in sentiment_buckets:
                    sentiment_buckets[bucket_key] = {"positive": 0, "negative": 0, "neutral": 0}
                if score > 0:
                    sentiment_buckets[bucket_key]["positive"] += 1
                    total_positive += 1
                elif score < 0:
                    sentiment_buckets[bucket_key]["negative"] += 1
                    total_negative += 1
                else:
                    sentiment_buckets[bucket_key]["neutral"] += 1

            total_hero_mentions = sum(hero_counts.values())
            hero_mentions = sorted(
                [
                    {
                        "hero": hero,
                        "count": count,
                        "pct": round(count / total_hero_mentions * 100, 1)
                        if total_hero_mentions
                        else 0,
                    }
                    for hero, count in hero_counts.items()
                ],
                key=lambda hero: hero["count"],
                reverse=True,
            )

            sentiment_timeline = sorted(
                [
                    {
                        "bucket": bucket,
                        "positive": vals["positive"],
                        "negative": vals["negative"],
                        "score": round(
                            (vals["positive"] - vals["negative"])
                            / max(1, vals["positive"] + vals["negative"]),
                            2,
                        ),
                    }
                    for bucket, vals in sentiment_buckets.items()
                ],
                key=lambda sample: sample["bucket"],
            )

            total_analyzed = len(rows)
            scored_total = total_positive + total_negative
            overall_score = (
                round((total_positive - total_negative) / max(1, scored_total), 2)
                if scored_total > 0
                else 0
            )

            if len(sentiment_timeline) >= 4:
                mid = len(sentiment_timeline) // 2
                first_scores = [sample["score"] for sample in sentiment_timeline[:mid]]
                second_scores = [sample["score"] for sample in sentiment_timeline[mid:]]
                first_avg = sum(first_scores) / len(first_scores) if first_scores else 0
                second_avg = sum(second_scores) / len(second_scores) if second_scores else 0
                if second_avg > first_avg + 0.1:
                    trend = "rising"
                elif first_avg > second_avg + 0.1:
                    trend = "falling"
                else:
                    trend = "stable"
            else:
                trend = "insufficient_data"

            label = (
                "positiv"
                if overall_score > 0.2
                else "negativ"
                if overall_score < -0.2
                else "neutral"
            )
            depth_total = depth_reaction + depth_short + depth_discussion

            def _depth_pct(value: int) -> float:
                return round(value / max(1, depth_total) * 100, 1)

            backseat_pct = round(backseat_count / max(1, total_analyzed) * 100, 1)
            raw_chat_status = build_raw_chat_status(
                conn,
                streamer,
                since_date=cutoff,
            )

        return {
            "heroMentions": hero_mentions[:25],
            "topicBreakdown": topic_counts,
            "sentimentTimeline": sentiment_timeline,
            "overallSentiment": {
                "score": overall_score,
                "label": label,
                "trend": trend,
                "totalAnalyzed": total_analyzed,
                "positiveCount": total_positive,
                "negativeCount": total_negative,
            },
            "backseat": {
                "count": backseat_count,
                "pct": backseat_pct,
                "examples": backseat_examples,
            },
            "engagementDepth": {
                "reaction": depth_reaction,
                "reactionPct": _depth_pct(depth_reaction),
                "short": depth_short,
                "shortPct": _depth_pct(depth_short),
                "discussion": depth_discussion,
                "discussionPct": _depth_pct(depth_discussion),
                "total": depth_total,
                "avgWordCount": round(sum(len(row[1].split()) for row in rows) / max(1, len(rows)), 1),
            },
            "rawChatStatus": raw_chat_status,
        }

    async def _api_v2_chat_content_analysis(self, request: web.Request) -> web.Response:
        """Hero mentions, topic breakdown, sentiment trend over time period."""
        self._require_v2_auth(request)
        self._require_extended_plan(request)

        streamer = request.query.get("streamer", "").strip().lower()
        if not streamer:
            return web.json_response({"error": "Streamer required"}, status=400)

        try:
            days = int(request.query.get("days", "30"))
        except ValueError:
            days = 30
        days = min(365, max(1, days))

        try:
            payload = await asyncio.to_thread(
                self._load_chat_content_analysis_payload_sync,
                streamer=streamer,
                days=days,
            )
            return web.json_response(payload)

        except web.HTTPException:
            raise
        except Exception as exc:
            log.exception("Error in chat-content-analysis API")
            return analytics_internal_error_response()

    # ─────────────────────────────────────────────────────────────────────
    # Endpoint 3: /twitch/api/v2/chat-social-graph
    # ─────────────────────────────────────────────────────────────────────

    async def _api_v2_chat_social_graph(self, request: web.Request) -> web.Response:
        """@Mention network: hubs, top pairs, distribution."""
        self._require_v2_auth(request)
        self._require_extended_plan(request)

        streamer = request.query.get("streamer", "").strip().lower()
        if not streamer:
            return web.json_response({"error": "Streamer required"}, status=400)

        try:
            days = int(request.query.get("days", "30"))
        except ValueError:
            days = 30
        days = min(365, max(1, days))

        try:
            payload = await asyncio.to_thread(
                load_chat_social_graph_payload,
                streamer=streamer,
                days=days,
            )
            return web.json_response(payload)

        except web.HTTPException:
            raise
        except Exception as exc:
            log.exception("Error in chat-social-graph API")
            return analytics_internal_error_response()

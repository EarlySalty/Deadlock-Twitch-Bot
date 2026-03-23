import logging
import re

from ..core.chat_bots import KNOWN_CHAT_BOTS

# ---------------------------------------------------------------------------
# Optional twitchio dependency
# ---------------------------------------------------------------------------
try:
    from twitchio import eventsub
    from twitchio import web as twitchio_web
    from twitchio.ext import commands as twitchio_commands

    _ = (eventsub, twitchio_web, twitchio_commands)

    TWITCHIO_AVAILABLE = True
except ImportError:
    TWITCHIO_AVAILABLE = False
    eventsub = None
    twitchio_web = None
    twitchio_commands = None
    log = logging.getLogger("TwitchStreams.ChatBot")
    log.warning(
        "twitchio nicht installiert. Twitch Chat Bot wird nicht verfügbar sein. "
        "Installation: pip install twitchio"
    )

# Join Twitch chat even when the stream is offline.
# Intentionally hardcoded to True (no ENV toggle) to keep commands wie !ping
# verfügbar auch bei offline Streams.
CHAT_JOIN_OFFLINE: bool = True

# Whitelist für bekannte legitime Bots (keine Spam-Prüfung)
WHITELISTED_BOTS = set(KNOWN_CHAT_BOTS)

SPAM_PHRASES = (
    "Best viewers streamboo.com",
    "Best viewers streamboo .com",
    "Best viewers streamboo com",
    "Best viewers smmtop32.online",
    "Best viewers smmtop32 .online",
    "Best viewers smmtop32 online",
    "Best viewers on",
    "Best viewers",
    "B̟est viewers",
    "Cheap Viewers",
    "Ch͟eap viewers",
    "(remove the space)",
    "Cool overlay \N{THUMBS UP SIGN} Honestly, it\N{RIGHT SINGLE QUOTATION MARK}s so hard to get found on the directory lately. I have small tips on beating the algorithm. Mind if I send you an share?",
    "Mind if I send you an share",
    " Viewers https://smmbest5.online",
    "Viewers smmbest4.online",
    "Viewers streamboo .com",
    "Viewers smmhype12.ru",
    "Viewers smmhype1.ru",
    "Viewers smmhype",
    "viewers on streamboo .com (remove the space)",
    "Hey friend I really enjoy your content so I give you a follow I'd love to be a friend and of you feel free to Add me on Discord",
)
# Entferne "viewer" und "viewers" aus den Fragmenten - zu allgemein und führt zu False Positives
SPAM_FRAGMENTS = (
    "best viewers",  # Nur die Kombination ist verdächtig
    "cheap viewers",  # Nur die Kombination ist verdächtig
    "streamboo.com",
    "streamboo .com",
    "streamboo com",
    "streamboo",
    "smmtop32.online",
    "smmtop32 .online",
    "smmtop32 online",
    "smmtop32",
    "remove the space",
    "cool overlay",
    "get found on the directory",
    "beating the algorithm",
    "d!sc",
    "smmbest4.online",
    "smmbest5.online",
    "rookie",
    "smmhype12.ru",
    "smmhype1.ru",
    "smmhype",
    "topsmm3.ru",
    "topsmm3 .ru",
    "topsmm3 ru",
    "topsmm3",
    "promnow.ru",
    "promnow ru",
    "promnow",
    "top viewers",
    "prmxy",
    "prmup",
)
SPAM_MIN_MATCHES = 3

# ---------------------------------------------------------------------------
# Periodische Chat-Promos
# ---------------------------------------------------------------------------
PROMO_MESSAGES_CATEGORIZED: dict[str, list[str]] = {
    "generic": [
        "heyo! Falls ihr bock habt auf Deadlock und noch eine deutsche Community sucht – schau gerne mal vorbei: {invite}",
        "Hey! Noch eine deutsche Deadlock-Community am suchen? Wir sind hier: {invite} 🎮",
        "Falls du noch eine deutsche Deadlock-Community sucht – schau doch mal vorbei: {invite}",
    ],
    "competitive": [
        "Kein Bock mehr auf Solo-Queue-Grief? Such dir feste Mates in unserer Community! {invite} 🔫",
        "Schon den neuesten Meta-Build ausprobiert? Tausch dich mit anderen Pros aus: {invite}",
        "MMR-Grind ist hart, aber im Team macht's mehr Spaß. Hier findest du die deutsche Deadlock-Community: {invite}",
        "Du willst deine Lane-Phase verbessern? Tipps & Tricks gibt's bei uns auf Discord: {invite}",
    ],
    "community": [
        "Bock auf Inhouses oder kleine Turniere? Wir organisieren regelmäßig Events! Schau doch mal vorbei: {invite} 🏆",
        "Noch auf der suche nach Mates für die nächste Runde Deadlock? In unserer Community wirst du fündig! {invite}",
        "Dein suchen hat ein Ende, falls du nach dem Stream noch Mates zum Zocken suchst, schau bei uns vorbei: {invite}",
    ],
    "growth": [
        "Deadlock ist komplex – wir helfen dir beim Einstieg! Guides und mehr bei uns auf dem Discord: {invite}",
        "Neu in Deadlock? Keine Sorge, unsere Community hat die besten Tipps für Einsteiger: {invite} 📚",
    ],
    "hype": [
        "Willkommen an alle neuen Gesichter! 🎮 Wenn ihr Bock auf Deadlock habt, schaut gerne bei unserer Community vorbei: {invite}",

    ]
}

# Flache Liste für Abwärtskompatibilität (wird von secrets.choice verwendet, wenn kein Grund angegeben ist)
PROMO_MESSAGES: list[str] = [
    msg for category in PROMO_MESSAGES_CATEGORIZED.values() for msg in category
]

PROMO_DISCORD_INVITE: str = "https://discord.gg/z5TfVHuQq2"
_PROMO_INTERVAL_MIN: int = 30

# Promo-Activity (ohne ENV; hier direkt konfigurieren)
_PROMO_ACTIVITY_ENABLED: bool = True
PROMO_CHANNEL_ALLOWLIST: set[str] = set()
PROMO_ACTIVITY_WINDOW_MIN: int = 8
PROMO_ACTIVITY_MIN_MSGS: int = 5
PROMO_ACTIVITY_MIN_CHATTERS: int = 1
PROMO_ACTIVITY_MIN_RAW_MSGS_SINCE_PROMO: int = 16
PROMO_ACTIVITY_TARGET_MPM: float = 3.0
PROMO_ACTIVITY_CHATTER_DEDUP_SEC: int = (
    30  # derselbe Chatter zählt höchstens einmal alle x Sekunden
)
_PROMO_COOLDOWN_MIN: int = 30
_PROMO_COOLDOWN_MAX: int = 120
PROMO_OVERALL_COOLDOWN_MIN: int = 60
PROMO_ATTEMPT_COOLDOWN_MIN: int = 5
PROMO_IGNORE_COMMANDS: bool = True
PROMO_LOOP_INTERVAL_SEC: int = 60

# Periodischer Fallback: wenn Chat still ist, aber Viewer über "normal" liegen
PROMO_VIEWER_SPIKE_ENABLED: bool = True
PROMO_VIEWER_SPIKE_COOLDOWN_MIN: int = 90
PROMO_VIEWER_SPIKE_MIN_CHAT_SILENCE_SEC: int = 300
PROMO_VIEWER_SPIKE_MIN_RATIO: float = 1.10
PROMO_VIEWER_SPIKE_MIN_DELTA: int = 2
PROMO_VIEWER_SPIKE_MIN_SESSIONS: int = 3
PROMO_VIEWER_SPIKE_SESSION_SAMPLE_LIMIT: int = 20
PROMO_VIEWER_SPIKE_STATS_SAMPLE_LIMIT: int = 240
PROMO_VIEWER_SPIKE_MIN_STATS_SAMPLES: int = 40

_PROMO_INTERVAL_MIN = max(1, int(_PROMO_INTERVAL_MIN))
_PROMO_ACTIVITY_ENABLED = bool(_PROMO_ACTIVITY_ENABLED)
if _PROMO_COOLDOWN_MAX < _PROMO_COOLDOWN_MIN:
    _PROMO_COOLDOWN_MAX = _PROMO_COOLDOWN_MIN
if _PROMO_COOLDOWN_MAX < _PROMO_INTERVAL_MIN:
    _PROMO_COOLDOWN_MAX = _PROMO_INTERVAL_MIN

# Promotion-Konfiguration auf sinnvolle Grenzwerte normalisieren.
PROMO_IGNORE_COMMANDS = bool(PROMO_IGNORE_COMMANDS)
PROMO_LOOP_INTERVAL_SEC = max(1, int(PROMO_LOOP_INTERVAL_SEC))
PROMO_VIEWER_SPIKE_ENABLED = bool(PROMO_VIEWER_SPIKE_ENABLED)
PROMO_VIEWER_SPIKE_COOLDOWN_MIN = max(0, int(PROMO_VIEWER_SPIKE_COOLDOWN_MIN))
PROMO_VIEWER_SPIKE_MIN_CHAT_SILENCE_SEC = max(0, int(PROMO_VIEWER_SPIKE_MIN_CHAT_SILENCE_SEC))
PROMO_VIEWER_SPIKE_MIN_RATIO = max(1.0, float(PROMO_VIEWER_SPIKE_MIN_RATIO))
PROMO_VIEWER_SPIKE_MIN_DELTA = max(0, int(PROMO_VIEWER_SPIKE_MIN_DELTA))
PROMO_VIEWER_SPIKE_MIN_SESSIONS = max(1, int(PROMO_VIEWER_SPIKE_MIN_SESSIONS))
PROMO_VIEWER_SPIKE_SESSION_SAMPLE_LIMIT = max(1, int(PROMO_VIEWER_SPIKE_SESSION_SAMPLE_LIMIT))
PROMO_VIEWER_SPIKE_STATS_SAMPLE_LIMIT = max(1, int(PROMO_VIEWER_SPIKE_STATS_SAMPLE_LIMIT))
PROMO_VIEWER_SPIKE_MIN_STATS_SAMPLES = max(1, int(PROMO_VIEWER_SPIKE_MIN_STATS_SAMPLES))
if _PROMO_ACTIVITY_ENABLED and not PROMO_MESSAGES:
    _PROMO_ACTIVITY_ENABLED = False
if PROMO_VIEWER_SPIKE_MIN_SESSIONS > PROMO_VIEWER_SPIKE_SESSION_SAMPLE_LIMIT:
    PROMO_VIEWER_SPIKE_MIN_SESSIONS = PROMO_VIEWER_SPIKE_SESSION_SAMPLE_LIMIT
if PROMO_VIEWER_SPIKE_MIN_STATS_SAMPLES > PROMO_VIEWER_SPIKE_STATS_SAMPLE_LIMIT:
    PROMO_VIEWER_SPIKE_MIN_STATS_SAMPLES = PROMO_VIEWER_SPIKE_STATS_SAMPLE_LIMIT
if PROMO_VIEWER_SPIKE_MIN_DELTA == 0 and PROMO_VIEWER_SPIKE_MIN_RATIO <= 1.0:
    PROMO_VIEWER_SPIKE_MIN_DELTA = 1

# Öffentliche, normalisierte Werte für andere Module.
PROMO_INTERVAL_MIN: int = _PROMO_INTERVAL_MIN
PROMO_ACTIVITY_ENABLED: bool = _PROMO_ACTIVITY_ENABLED
PROMO_COOLDOWN_MIN: int = _PROMO_COOLDOWN_MIN
PROMO_COOLDOWN_MAX: int = _PROMO_COOLDOWN_MAX

# ---------------------------------------------------------------------------
# Deadlock Zugangsfragen (Invite-Only Hinweise)
# ---------------------------------------------------------------------------
DEADLOCK_INVITE_REPLY: str = (
    "Wenn du einen Zugang benötigst, schau gerne auf unserem Discord vorbei, "
    "dort bekommst du eine Einladung und Hilfe beim Einstieg :) {invite}"
)
_INVITE_QUESTION_CHANNEL_COOLDOWN_SEC: int = 120
_INVITE_QUESTION_USER_COOLDOWN_SEC: int = 3600
_INVITE_QUESTION_RE = re.compile(
    r"\b(wie|wo|wann|wieso|warum|woher)\b"
    r"|\b(kann|darf)\s+man\b"
    r"|\b(kann|kannst|konnte|koennte|könnte|darf|darfst)\s+(man|ich|du)\b"
    r"|\b(bekomm|krieg|erhalt)\w*\s+(man|ich)\b",
    re.IGNORECASE,
)
INVITE_QUESTION_CHANNEL_COOLDOWN_SEC: int = _INVITE_QUESTION_CHANNEL_COOLDOWN_SEC
INVITE_QUESTION_USER_COOLDOWN_SEC: int = _INVITE_QUESTION_USER_COOLDOWN_SEC
INVITE_QUESTION_RE = _INVITE_QUESTION_RE

INVITE_ACCESS_RE = re.compile(
    r"\b(spielen|spiel|play|zock\w*|zugang|einlad\w*|invit\w*|beta|key|access|ea|early\s*access|reinkomm\w*|rankomm\w*)\b",
    re.IGNORECASE,
)
INVITE_STRONG_ACCESS_RE = re.compile(
    r"\b(zugang|einlad\w*|invit\w*|beta|key|access|ea|early\s*access|reinkomm\w*|rankomm\w*)\b",
    re.IGNORECASE,
)
INVITE_GAME_CONTEXT_RE = re.compile(
    r"\b(game|spiel|play|zock\w*)\b",
    re.IGNORECASE,
)
INVITE_JOIN_RE = re.compile(
    r"\b("
    r"anschlie(?:ss|ß)\w*"
    r"|mit\s*(?:spiel\w*|zock\w*)"
    r"|mitspiel\w*"
    r"|mitzock\w*"
    r")\b",
    re.IGNORECASE,
)

# ---------------------------------------------------------------------------
# Streamer-Pläne / Abonnements (zukünftiges Feature, noch inaktiv)
# ---------------------------------------------------------------------------
# Globaler Schalter: Auf True setzen, sobald Pläne offiziell angeboten werden.
# Solange False: kein Einfluss auf den Bot – die streamer_plans-Tabelle in der
# DB existiert bereits und kann manuell befüllt werden.
#
# Verfügbare Plan-Features (werden nur geprüft wenn SUBSCRIPTION_PLANS_ENABLED=True):
#   promo_disabled  – Chat-Promos werden für diesen Streamer nicht gesendet
SUBSCRIPTION_PLANS_ENABLED: bool = True

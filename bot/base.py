"""Base implementation shared across the Twitch cog mixins."""

from __future__ import annotations

import asyncio
import inspect
import ipaddress
import json
import logging
import os
import re
import socket
import time
from collections.abc import Coroutine
from datetime import UTC, datetime
from typing import Any
from urllib.parse import urlparse

from aiohttp import web
from discord import Forbidden, Guild, HTTPException
from discord.ext import commands

try:
    from bot_core.boot_profile import log_event
except Exception:  # pragma: no cover - fallback if master package not in path
    def log_event(step: str, duration: float, detail: str | None = None) -> None:  # type: ignore
        return

from . import storage
from .api.token_manager import TwitchBotTokenManager
from .api.twitch_api import TwitchAPI
from .chat.bot import TWITCHIO_AVAILABLE, create_twitch_chat_bot, load_bot_tokens
from .chat.constants import CHAT_JOIN_OFFLINE
from .chat.irc_lurker_tracker import IRCLurkerTracker
from .core.constants import (
    POLL_INTERVAL_SECONDS,
    TWITCH_ALERT_CHANNEL_ID,
    TWITCH_ALERT_MENTION,
    TWITCH_CATEGORY_SAMPLE_LIMIT,
    TWITCH_DASHBOARD_HOST,
    TWITCH_DASHBOARD_NOAUTH,
    TWITCH_DASHBOARD_PORT,
    TWITCH_INTERNAL_API_HOST,
    TWITCH_INTERNAL_API_PORT,
    TWITCH_LANGUAGE,
    TWITCH_LOG_EVERY_N_TICKS,
    TWITCH_NOTIFY_CHANNEL_ID,
    TWITCH_RAID_REDIRECT_URI,
    TWITCH_REQUIRED_DISCORD_MARKER,
    TWITCH_TARGET_GAME_NAME,
    log,
)
from .internal_api import InternalApiRunner
from .raid.manager import RaidBot
from .raid import partner_scores as partner_raid_scores
from .reload_manager import LoopSpec, SubsystemDef, TwitchReloadManager
from .secret_store import load_secret_value


def _parse_env_bool(name: str, default: bool) -> bool:
    raw = (os.getenv(name) or "").strip().lower()
    if not raw:
        return default
    if raw in {"1", "true", "yes", "on"}:
        return True
    if raw in {"0", "false", "no", "off"}:
        return False
    return default


def _parse_env_int(name: str, default: int) -> int:
    raw = (os.getenv(name) or "").strip()
    if not raw:
        return default
    try:
        return int(raw)
    except ValueError:
        return default


def _parse_env_csv(name: str, default: tuple[str, ...] = ()) -> tuple[str, ...]:
    raw = str(os.getenv(name) or "").strip()
    source = raw.split(",") if raw else list(default)
    normalized: list[str] = []
    seen: set[str] = set()
    for item in source:
        value = str(item or "").strip().lower().lstrip("#")
        if not value or value in seen:
            continue
        seen.add(value)
        normalized.append(value)
    return tuple(normalized)


def _observability_flow_id(prefix: str) -> str:
    normalized = str(prefix or "flow").strip().lower() or "flow"
    return f"{normalized}-{int(time.time() * 1000)}"


def _observability_sample(values: object, *, limit: int = 8) -> list[str]:
    if isinstance(values, dict):
        source = values.keys()
    elif isinstance(values, (set, list, tuple)):
        source = values
    else:
        return []
    normalized = [
        str(value or "").strip().lower().lstrip("#")
        for value in source
        if str(value or "").strip()
    ]
    return sorted(dict.fromkeys(normalized))[:limit]


def _observability_value(value: object, *, limit: int = 240) -> str:
    def _convert(obj: object) -> object:
        if obj is None:
            return None
        if isinstance(obj, datetime):
            return obj.isoformat()
        if isinstance(obj, set):
            return sorted(str(item) for item in obj)
        if isinstance(obj, (list, tuple)):
            return [_convert(item) for item in obj]
        if isinstance(obj, dict):
            return {str(key): _convert(val) for key, val in obj.items()}
        if isinstance(obj, (str, int, float, bool)):
            return obj
        return str(obj)

    normalized = _convert(value)
    if isinstance(normalized, str):
        text = normalized.replace("\r", " ").replace("\n", " ").strip()
    else:
        text = json.dumps(normalized, sort_keys=True, ensure_ascii=True, separators=(",", ":"))
    return f"{text[:limit]}..." if len(text) > limit else text


def _observability_fields(**fields: object) -> str:
    parts = []
    for key in sorted(fields):
        value = fields[key]
        if value is None:
            continue
        parts.append(f"{str(key).strip()}={_observability_value(value)}")
    return " ".join(parts)


class TwitchBaseCog(commands.Cog):
    """Handle shared initialisation, shutdown and utility helpers."""

    def __init__(self, bot: commands.Bot):
        super().__init__()
        self.bot = bot

        # Diagnose: Welche Keys sind da?
        twitch_keys = [k for k in os.environ.keys() if k.startswith("TWITCH_")]
        log.debug("Detected Twitch Keys in ENV: %s", ", ".join(twitch_keys))

        # 🔒 Secrets nur aus ENV (nicht hardcoden!)
        # TWITCH_CLIENT_ID/SECRET sind für die Haupt-App (Raids, Dashboard)
        self.client_id = load_secret_value("TWITCH_CLIENT_ID")
        self.client_secret = load_secret_value("TWITCH_CLIENT_SECRET")

        # TWITCH_BOT_CLIENT_ID ist speziell für den Chat-Bot (Fallback auf Haupt-App)
        self._twitch_bot_client_id: str = (
            load_secret_value("TWITCH_BOT_CLIENT_ID") or self.client_id
        )

        # Bot-Secret laden: 1. Spezieller Key, 2. Fallback auf Haupt-Secret (wenn ID identisch)
        bot_secret_env = load_secret_value("TWITCH_BOT_CLIENT_SECRET")
        if bot_secret_env:
            self._twitch_bot_secret = bot_secret_env
        elif self._twitch_bot_client_id == self.client_id:
            self._twitch_bot_secret = self.client_secret
        else:
            self._twitch_bot_secret = ""

        # Runtime attributes initialised even if the cog is disabled
        self.api: TwitchAPI | None
        self._web: web.AppRunner | None = None
        self._web_app: web.Application | None = None
        self._category_id: str | None = None
        self._language_filters = self._parse_language_filters(TWITCH_LANGUAGE)
        self._tick_count = 0
        self._log_every_n = max(1, int(TWITCH_LOG_EVERY_N_TICKS or 1))
        self._category_sample_limit = max(50, int(TWITCH_CATEGORY_SAMPLE_LIMIT or 400))
        self._poll_interval_seconds = max(5, min(3600, int(POLL_INTERVAL_SECONDS or 15)))
        self._poll_interval_resync_interval_seconds = 60.0
        self._poll_interval_last_sync_monotonic = 0.0
        self._poll_interval_last_error_log_at = 0.0
        self._poll_interval_last_invalid_value: str | None = None
        self._poll_interval_settings_table = "twitch_global_settings"
        self._poll_interval_settings_key = "poll_interval_seconds"
        self._admin_polling_interval_seconds = self._poll_interval_seconds
        self._active_sessions: dict[str, int] = {}
        self._notify_channel_id = int(TWITCH_NOTIFY_CHANNEL_ID or 0)
        self._alert_channel_id = int(TWITCH_ALERT_CHANNEL_ID or 0)
        self._alert_mention = TWITCH_ALERT_MENTION or ""
        self._invite_codes: dict[int, set[str]] = {}
        self._twl_command: commands.Command | None = None
        self._target_game_name = (TWITCH_TARGET_GAME_NAME or "").strip()
        self._target_game_lower = self._target_game_name.lower()
        self.partner_raid_score_service = partner_raid_scores

        # Dashboard/Auth (aus Config-Header)
        self._dashboard_token = load_secret_value("TWITCH_DASHBOARD_TOKEN") or None
        self._dashboard_noauth = _parse_env_bool(
            "TWITCH_DASHBOARD_NOAUTH",
            bool(TWITCH_DASHBOARD_NOAUTH),
        )
        env_dashboard_host = (os.getenv("TWITCH_DASHBOARD_HOST") or "").strip()
        default_dashboard_host = TWITCH_DASHBOARD_HOST or "127.0.0.1"
        self._dashboard_host = env_dashboard_host or default_dashboard_host
        try:
            if ipaddress.ip_address(self._dashboard_host).is_unspecified:
                log.warning(
                    "TWITCH_DASHBOARD_HOST resolves to an unspecified address; keep this behind auth/reverse proxy."
                )
        except ValueError:
            log.warning("TWITCH_DASHBOARD_HOST is not a valid IP; using it as-is: %s", self._dashboard_host)
        self._dashboard_port = _parse_env_int("TWITCH_DASHBOARD_PORT", int(TWITCH_DASHBOARD_PORT))
        embedded_env = (os.getenv("TWITCH_DASHBOARD_EMBEDDED", "") or "").strip().lower()
        self._dashboard_embedded = embedded_env not in {"0", "false", "no", "off"}
        if not self._dashboard_embedded:
            log.info(
                "TWITCH_DASHBOARD_EMBEDDED disabled - assuming external reverse proxy serves the dashboard"
            )
        self._partner_dashboard_token = load_secret_value("TWITCH_PARTNER_TOKEN") or None
        self._dashboard_auth_redirect_uri = (
            os.getenv("TWITCH_DASHBOARD_AUTH_REDIRECT_URI") or ""
        ).strip() or "https://twitch.earlysalty.com/twitch/auth/callback"
        self._dashboard_session_ttl = max(
            6 * 3600,
            _parse_env_int("TWITCH_DASHBOARD_SESSION_TTL_SEC", 6 * 3600),
        )
        self._legacy_stats_url = (os.getenv("TWITCH_LEGACY_STATS_URL") or "").strip() or None
        self._required_marker_default = TWITCH_REQUIRED_DISCORD_MARKER or None
        self._internal_api_runner: InternalApiRunner | None = None
        self._experimental_irc_lurker_channels = set(
            _parse_env_csv(
                "TWITCH_EXPERIMENTAL_IRC_LURKER_CHANNELS",
                default=("earlysalty",),
            )
        )
        self._experimental_irc_lurker_enabled = False
        self._irc_lurker_tracker: IRCLurkerTracker | None = None

        # Internal API for split dashboard deployments
        self._internal_api_token = (
            load_secret_value(
                "TWITCH_INTERNAL_API_TOKEN",
                prefer_env=True,
                allow_empty_env_override=True,
            )
            or None
        )
        env_internal_host = (os.getenv("TWITCH_INTERNAL_API_HOST") or "").strip()
        default_internal_host = TWITCH_INTERNAL_API_HOST or "127.0.0.1"
        self._internal_api_host = env_internal_host or default_internal_host
        try:
            if ipaddress.ip_address(self._internal_api_host).is_unspecified:
                log.warning(
                    "TWITCH_INTERNAL_API_HOST resolves to an unspecified address; keep it private."
                )
        except ValueError:
            log.warning(
                "TWITCH_INTERNAL_API_HOST is not a valid IP; using it as-is: %s",
                self._internal_api_host,
            )
        self._internal_api_port = _parse_env_int(
            "TWITCH_INTERNAL_API_PORT",
            int(TWITCH_INTERNAL_API_PORT),
        )
        self._internal_api_runner = InternalApiRunner(
            host=self._internal_api_host,
            port=self._internal_api_port,
            token=self._internal_api_token,
            add_cb=getattr(self, "_dashboard_add", None),
            remove_cb=getattr(self, "_dashboard_remove", None),
            list_cb=getattr(self, "_dashboard_list", None),
            stats_cb=getattr(self, "_dashboard_stats", None),
            verify_cb=getattr(self, "_dashboard_verify", None),
            archive_cb=getattr(self, "_dashboard_archive", None),
            discord_flag_cb=getattr(self, "_dashboard_set_discord_flag", None),
            discord_profile_cb=getattr(self, "_dashboard_save_discord_profile", None),
            streamer_analytics_cb=getattr(self, "_dashboard_streamer_analytics_data", None),
            comparison_cb=getattr(self, "_dashboard_comparison_stats", None),
            session_cb=getattr(self, "_dashboard_session_detail", None),
            raid_auth_url_cb=getattr(self, "_dashboard_raid_auth_url", None),
            raid_auth_state_cb=getattr(self, "_integration_raid_auth_state", None),
            raid_block_state_cb=getattr(self, "_integration_raid_block_state", None),
            raid_go_url_cb=getattr(self, "_dashboard_raid_go_url", None),
            raid_requirements_cb=getattr(self, "_dashboard_raid_requirements", None),
            raid_oauth_callback_cb=getattr(self, "_dashboard_raid_oauth_callback", None),
            live_active_announcements_cb=getattr(
                self,
                "_dashboard_live_active_announcements",
                None,
            ),
            live_link_click_cb=getattr(self, "_dashboard_live_link_click", None),
            observability_snapshot_cb=getattr(
                self,
                "_internal_observability_snapshot",
                None,
            ),
            chatters_debug_cb=getattr(self, "_internal_chatters_debug", None),
        )

        # EventSub Webhook Handler – früh initialisieren damit er sowohl im Dashboard
        # als auch in _start_eventsub_listener verfügbar ist.
        _webhook_secret = load_secret_value("TWITCH_WEBHOOK_SECRET")
        if _webhook_secret:
            try:
                from .monitoring.eventsub_webhook import EventSubWebhookHandler

                self._eventsub_webhook_handler = EventSubWebhookHandler(
                    secret=_webhook_secret,
                    logger=log,
                )
                # Webhook-Basis-URL aus dem Auth-Redirect-URI ableiten
                _parsed_redirect = urlparse(self._dashboard_auth_redirect_uri)
                self._webhook_base_url: str | None = (
                    f"{_parsed_redirect.scheme}://{_parsed_redirect.netloc}"
                    if _parsed_redirect.netloc
                    else None
                )
                self._webhook_secret: str | None = _webhook_secret
                log.debug(
                    "EventSub Webhook Handler initialisiert (base_url=%s)",
                    self._webhook_base_url,
                )
            except Exception:
                log.exception("EventSub Webhook Handler konnte nicht initialisiert werden")
                self._eventsub_webhook_handler = None
                self._webhook_base_url = None
                self._webhook_secret = None
        else:
            log.info(
                "TWITCH_WEBHOOK_SECRET nicht gesetzt – EventSub Webhook deaktiviert, "
                "WebSocket-Fallback wird verwendet."
            )
            self._eventsub_webhook_handler = None
            self._webhook_base_url = None
            self._webhook_secret = None

        if not self.client_id:
            log.error(
                "TWITCH_CLIENT_ID not configured; Twitch features will be limited or disabled."
            )
            self.api = None
            # Wir machen hier nicht 'return', damit der Chat-Bot (der seine eigene ID hat) evtl. trotzdem starten kann.
        else:
            if not self.client_secret:
                log.warning(
                    "TWITCH_CLIENT_SECRET missing. API calls and Raids will fail, but Chat Bot might work."
                )
                self.api = None
            else:
                self.api = TwitchAPI(self.client_id, self.client_secret)

        if self.api:
            # Rehydrate offene Streams/Sessions nach einem Neustart
            self._spawn_bg_task(self._startup_db_warmup(), "twitch.db_warmup")

        # Raid-Bot initialisieren
        self._raid_bot: RaidBot | None = None
        self._twitch_chat_bot = None
        self._periodic_channel_join_task: asyncio.Task | None = None
        bot_token, bot_refresh_token, _ = load_bot_tokens(log_missing=False)
        self._twitch_bot_token: str | None = bot_token
        self._twitch_bot_refresh_token: str | None = bot_refresh_token
        env_bot_client_id = os.getenv("TWITCH_BOT_CLIENT_ID", "").strip()
        self._twitch_bot_client_id = (
            env_bot_client_id or self._twitch_bot_client_id or self.client_id
        )
        if not self._twitch_bot_secret:
            env_bot_secret = os.getenv("TWITCH_BOT_CLIENT_SECRET", "").strip()
            if env_bot_secret:
                self._twitch_bot_secret = env_bot_secret
            elif self._twitch_bot_client_id == self.client_id:
                self._twitch_bot_secret = self.client_secret
            else:
                self._twitch_bot_secret = None
        self._bot_token_manager: TwitchBotTokenManager | None = None
        if self._twitch_bot_client_id:
            self._bot_token_manager = TwitchBotTokenManager(
                self._twitch_bot_client_id,
                (self._twitch_bot_secret or self.client_secret or ""),
            )

        # Redirect-URL: Priorität 1: ENV/Tresor, Priorität 2: Constant
        redirect_uri = os.getenv("TWITCH_RAID_REDIRECT_URI", "").strip() or TWITCH_RAID_REDIRECT_URI
        self._raid_redirect_uri = redirect_uri

        if self.api:
            try:
                session = self.api.get_http_session()
                self._raid_bot = RaidBot(
                    client_id=self.client_id,
                    client_secret=self.client_secret,
                    redirect_uri=redirect_uri,
                    session=session,
                )
                self._raid_bot.partner_raid_score_service = partner_raid_scores
                self._raid_bot.set_discord_bot(self.bot)
                self._raid_bot.set_cog(self)  # For dynamic EventSub subscriptions
                log.debug("Raid-Bot initialisiert (redirect_uri: %s)", redirect_uri)

                # Twitch Chat Bot starten (falls Token vorhanden)
                if self._twitch_bot_token:
                    self._spawn_bg_task(self._init_twitch_chat_bot(), "twitch.chat_bot")
                else:
                    log.info(
                        "Twitch Chat Bot nicht verfuegbar (kein Token gesetzt). "
                        "Setze TWITCH_BOT_TOKEN oder TWITCH_BOT_TOKEN_FILE, um den Chat-Bot zu aktivieren."
                    )
            except Exception:
                log.exception("Fehler beim Initialisieren des Raid-Bots")
                self._raid_bot = None
        else:
            log.warning("Raid-Bot und Chat-Bot deaktiviert, da TWITCH_CLIENT_ID/SECRET fehlen.")

        # Background tasks
        sync_poll_interval = getattr(self, "_sync_poll_interval_from_storage", None)
        if callable(sync_poll_interval):
            try:
                sync_poll_interval(force=True, startup=True)
            except Exception:
                log.debug("Persistiertes Polling-Intervall konnte vor Loop-Start nicht geladen werden", exc_info=True)
        self.poll_streams.start()
        # invites_refresh.start() → DEAKTIVIERT: On-Demand statt periodisch
        self._spawn_bg_task(self._ensure_category_id(), "twitch.ensure_category_id")
        self._spawn_bg_task(self._load_invite_codes_from_db(), "twitch.load_invites")
        self._spawn_bg_task(self._start_internal_api(), "twitch.start_internal_api")
        if self._dashboard_embedded:
            self._spawn_bg_task(self._start_dashboard(), "twitch.start_dashboard")
        else:
            log.info("Skipping internal Twitch dashboard server startup")
        self._spawn_bg_task(self._refresh_all_invites(), "twitch.refresh_all_invites")
        # NUR EINEN EventSub Listener starten (konsolidiert stream.online + stream.offline)
        self._spawn_bg_task(self._start_eventsub_listener(), "twitch.eventsub")
        # Beim Start fehlende user_ids in twitch_streamers nachfüllen
        if self.api:
            self._spawn_bg_task(self._sync_missing_user_ids(), "twitch.sync_user_ids")
            self._spawn_bg_task(self._scout_deadlock_channels(), "twitch.scout_deadlock")

        # Persistente Views und Session-Rehydrate nach Bot-Ready erledigen
        self._spawn_bg_task(self._register_views_after_ready(), "twitch.views_warmup")

        # Social Media Clip Management
        self.clip_manager = None
        self.clip_fetcher = None
        self.upload_worker = None
        if self.api:
            from .social_media.clip_fetcher import ClipFetcher
            from .social_media.clip_manager import ClipManager
            from .social_media.upload_worker import UploadWorker

            self.clip_manager = ClipManager(twitch_api=self.api)
            self.clip_fetcher = ClipFetcher(bot, self.api, self.clip_manager)
            self.upload_worker = UploadWorker(bot, self.clip_manager)
            log.info(
                "Social Media Clip Management initialized (ClipManager + ClipFetcher + UploadWorker)"
            )

        # Subsystem hot-reload manager
        self._reload_manager = TwitchReloadManager(self)
        self._reload_manager.register(SubsystemDef(
            name="analytics",
            display_name="Analytics",
            modules=["bot.analytics.mixin"],
            loops=[
                LoopSpec("collect_analytics_data"),
                LoopSpec("collect_chatters_data"),
                LoopSpec("compute_raid_retention"),
            ],
            hot_reloadable=True,
        ))
        self._reload_manager.register(SubsystemDef(
            name="community",
            display_name="Community",
            modules=["bot.community.admin", "bot.community.leaderboard", "bot.community.partner_recruit"],
            loops=[],
            hot_reloadable=True,
        ))
        self._reload_manager.register(SubsystemDef(
            name="social",
            display_name="Social Media",
            modules=["bot.social_media.clip_fetcher", "bot.social_media.clip_manager", "bot.social_media.upload_worker"],
            loops=[],
            hot_reloadable=True,
            teardown_hook="_reload_social_teardown",
            startup_hook="_reload_social_startup",
        ))
        self._reload_manager.register(SubsystemDef(
            name="monitoring",
            display_name="Monitoring",
            modules=["bot.monitoring.monitoring", "bot.monitoring.eventsub_mixin", "bot.monitoring.sessions_mixin", "bot.monitoring.embeds_mixin"],
            loops=[
                LoopSpec("poll_streams"),
                LoopSpec("invites_refresh"),
            ],
            hot_reloadable=False,
        ))
        self._reload_manager.register(SubsystemDef(
            name="chat",
            display_name="Chat Bot",
            modules=["bot.chat.bot", "bot.chat.commands", "bot.chat.connection"],
            loops=[],
            hot_reloadable=False,
        ))
        self._reload_manager.register(SubsystemDef(
            name="dashboard",
            display_name="Dashboard",
            modules=["bot.dashboard.mixin", "bot.dashboard.server_v2", "bot.dashboard.routes_mixin"],
            loops=[],
            hot_reloadable=False,
        ))
        self._reload_manager.register(SubsystemDef(
            name="raid",
            display_name="Raid",
            modules=["bot.raid.mixin", "bot.raid.manager", "bot.raid.commands", "bot.raid.auth"],
            loops=[],
            hot_reloadable=False,
        ))
        log.debug("Subsystem reload manager ready (%d subsystems)", len(self._reload_manager.get_all_names()))

    @staticmethod
    def _sample_observability_items(values: object, *, limit: int = 10) -> list[str]:
        if not isinstance(values, (list, tuple, set)):
            return []
        sampled: list[str] = []
        for value in values:
            text = str(value or "").strip()
            if not text:
                continue
            sampled.append(text)
            if len(sampled) >= limit:
                break
        return sampled

    def _log_chat_bot_lifecycle_event(
        self,
        *,
        flow_id: str,
        event: str,
        level: int = logging.INFO,
        **fields: object,
    ) -> None:
        payload = {
            "flow_id": str(flow_id or "").strip() or None,
            "event": str(event or "").strip() or "unknown",
            "chat_bot_available": bool(getattr(self, "_twitch_chat_bot", None)),
            "bot_token_manager_available": bool(getattr(self, "_bot_token_manager", None)),
            **fields,
        }
        log.log(level, "%s %s", str(payload["event"]), _observability_fields(**payload))
        storage.insert_observability_event(
            flow_type="chat_runtime",
            flow_id=str(payload.get("flow_id") or ""),
            step=str(payload.get("event") or "unknown"),
            decision=str(payload.get("event") or "unknown"),
            entity_login=None,
            entity_id=None,
            details=payload,
        )

    async def _internal_observability_snapshot(self) -> dict[str, Any]:
        chat_bot = getattr(self, "_twitch_chat_bot", None)
        raid_bot = getattr(self, "_raid_bot", None)
        analytics_snapshot = None
        irc_lurker_snapshot = None

        chat_snapshot = None
        if chat_bot and hasattr(chat_bot, "get_observability_snapshot"):
            try:
                maybe_chat_snapshot = chat_bot.get_observability_snapshot()
                if inspect.isawaitable(maybe_chat_snapshot):
                    maybe_chat_snapshot = await maybe_chat_snapshot
                if isinstance(maybe_chat_snapshot, dict):
                    chat_snapshot = maybe_chat_snapshot
            except Exception:
                log.debug("Observability snapshot: chat bot snapshot failed", exc_info=True)

        raid_snapshot = None
        if raid_bot and hasattr(raid_bot, "get_observability_snapshot"):
            try:
                maybe_raid_snapshot = raid_bot.get_observability_snapshot()
                if inspect.isawaitable(maybe_raid_snapshot):
                    maybe_raid_snapshot = await maybe_raid_snapshot
                if isinstance(maybe_raid_snapshot, dict):
                    raid_snapshot = maybe_raid_snapshot
            except Exception:
                log.debug("Observability snapshot: raid bot snapshot failed", exc_info=True)

        analytics_getter = getattr(self, "get_analytics_observability_snapshot", None)
        if callable(analytics_getter):
            try:
                maybe_analytics_snapshot = analytics_getter()
                if inspect.isawaitable(maybe_analytics_snapshot):
                    maybe_analytics_snapshot = await maybe_analytics_snapshot
                if isinstance(maybe_analytics_snapshot, dict):
                    analytics_snapshot = maybe_analytics_snapshot
            except Exception:
                log.debug("Observability snapshot: analytics snapshot failed", exc_info=True)

        irc_lurker_tracker = getattr(self, "_irc_lurker_tracker", None)
        if irc_lurker_tracker and hasattr(irc_lurker_tracker, "get_observability_snapshot"):
            try:
                maybe_irc_snapshot = irc_lurker_tracker.get_observability_snapshot()
                if inspect.isawaitable(maybe_irc_snapshot):
                    maybe_irc_snapshot = await maybe_irc_snapshot
                if isinstance(maybe_irc_snapshot, dict):
                    irc_lurker_snapshot = maybe_irc_snapshot
            except Exception:
                log.debug("Observability snapshot: IRC lurker snapshot failed", exc_info=True)

        last_followers_diagnostic = None
        if isinstance(analytics_snapshot, dict):
            last_followers_diagnostic = analytics_snapshot.get("lastFollowersDiagnostic")
        if last_followers_diagnostic is None and isinstance(raid_snapshot, dict):
            last_followers_diagnostic = raid_snapshot.get("lastAnalyticsFollowersDiagnostic")

        return {
            "generatedAt": datetime.now(UTC).isoformat(timespec="seconds"),
            "pollIntervalSeconds": self._poll_interval_seconds,
            "activeSessionCount": len(getattr(self, "_active_sessions", {}) or {}),
            "activeSessionSample": self._sample_observability_items(
                sorted((getattr(self, "_active_sessions", {}) or {}).keys())
            ),
            "chatRuntimeAvailable": bool(chat_bot),
            "raidRuntimeAvailable": bool(raid_bot),
            "analyticsRuntimeAvailable": bool(
                isinstance(analytics_snapshot, dict)
                and analytics_snapshot.get("runtimeAvailable")
            ),
            "botTokenManagerAvailable": bool(getattr(self, "_bot_token_manager", None)),
            "chatBotAvailable": bool(chat_bot),
            "ircLurkerExperimentEnabled": bool(self._experimental_irc_lurker_enabled),
            "ircLurkerRuntimeAvailable": bool(irc_lurker_tracker),
            "ircLurkerExperimentChannels": sorted(
                str(channel or "").strip().lower().lstrip("#")
                for channel in (getattr(self, "_experimental_irc_lurker_channels", set()) or set())
                if str(channel or "").strip()
            ),
            "lastChattersDiagnostic": (
                analytics_snapshot.get("lastChattersDiagnostic")
                if isinstance(analytics_snapshot, dict)
                else None
            ),
            "lastFollowersDiagnostic": last_followers_diagnostic,
            "lastAnalyticsDecisionSample": (
                analytics_snapshot.get("lastDecisionSample")
                if isinstance(analytics_snapshot, dict)
                else None
            ),
            "chat": chat_snapshot,
            "raid": raid_snapshot,
            "analytics": analytics_snapshot,
            "ircLurker": irc_lurker_snapshot,
        }

    async def _internal_chatters_debug(self, login: str) -> dict[str, Any]:
        normalized_login = str(login or "").strip().lower().lstrip("#")
        if not normalized_login:
            raise ValueError("login is required")

        current_session_id: int | None = None
        current_user_id: str | None = None
        is_live = False
        try:
            with storage.get_conn() as conn:
                row = conn.execute(
                    """
                    SELECT twitch_user_id, active_session_id, is_live
                    FROM twitch_live_state
                    WHERE LOWER(streamer_login) = ?
                    LIMIT 1
                    """,
                    (normalized_login,),
                ).fetchone()
            if row is not None:
                current_user_id = str(
                    row["twitch_user_id"] if hasattr(row, "keys") else row[0] or ""
                ).strip() or None
                current_session_id = int(
                    row["active_session_id"] if hasattr(row, "keys") else row[1]
                ) if (row["active_session_id"] if hasattr(row, "keys") else row[1]) is not None else None
                is_live = bool(row["is_live"] if hasattr(row, "keys") else row[2])
        except Exception:
            log.debug("Chatters debug: live state lookup failed for %s", normalized_login, exc_info=True)

        runtime_state: dict[str, Any] = {}
        runtime_builder = getattr(self, "_build_analytics_runtime_state", None)
        if callable(runtime_builder):
            try:
                runtime_state = dict(runtime_builder(normalized_login))
            except Exception:
                log.debug("Chatters debug: runtime builder failed for %s", normalized_login, exc_info=True)

        bot_token, bot_id, bot_scopes, bot_diagnostics = (None, None, set(), {})
        resolver = getattr(self, "_resolve_bot_chatters_fallback", None)
        if callable(resolver):
            try:
                bot_token, bot_id, bot_scopes, bot_diagnostics = await resolver(
                    normalized_login,
                    allow_untracked=True,
                )
            except Exception:
                log.debug("Chatters debug: bot fallback resolution failed for %s", normalized_login, exc_info=True)

        streamer_token_present = False
        streamer_scope_state = "absent"
        if getattr(self, "_raid_bot", None) and self.api is not None:
            try:
                session = self.api.get_http_session()
                token_result = await self._raid_bot.auth_manager.get_valid_token_for_login(
                    normalized_login,
                    session,
                )
                if token_result:
                    auth_user_id, auth_token = token_result
                    current_user_id = current_user_id or str(auth_user_id or "").strip() or None
                    streamer_token_present = bool(str(auth_token or "").strip())
                    streamer_scopes = {
                        str(scope).strip().lower()
                        for scope in self._raid_bot.auth_manager.get_scopes(auth_user_id)
                        if str(scope).strip()
                    }
                    scope_helper = getattr(self, "_scope_presence_state", None)
                    if callable(scope_helper):
                        streamer_scope_state = scope_helper(
                            scopes=streamer_scopes,
                            required_scope="moderator:read:chatters",
                            token_available=streamer_token_present,
                        )
            except Exception:
                log.debug("Chatters debug: streamer token lookup failed for %s", normalized_login, exc_info=True)

        snapshot = await self._internal_observability_snapshot()
        last_chatters_diagnostic = (
            snapshot.get("lastChattersDiagnostic")
            if isinstance(snapshot, dict)
            else None
        )
        last_decision_matches_login = (
            isinstance(last_chatters_diagnostic, dict)
            and str(last_chatters_diagnostic.get("login") or "").strip().lower() == normalized_login
        )
        return {
            "login": normalized_login,
            "currentUserId": current_user_id,
            "currentSessionId": current_session_id,
            "isLive": is_live,
            "runtimeState": runtime_state,
            "botTokenPresent": bool(bot_token),
            "botId": str(bot_id or "").strip() or None,
            "botScopeState": (
                str(bot_diagnostics.get("bot_scope_present") or "unknown")
                if isinstance(bot_diagnostics, dict)
                else "unknown"
            ),
            "botDiagnostics": bot_diagnostics if isinstance(bot_diagnostics, dict) else {},
            "streamerTokenPresent": streamer_token_present,
            "streamerScopeState": streamer_scope_state,
            "lastDecisionRecord": last_chatters_diagnostic if last_decision_matches_login else None,
            "lastApiStatus": (
                last_chatters_diagnostic.get("http_status")
                if last_decision_matches_login and isinstance(last_chatters_diagnostic, dict)
                else None
            ),
            "observabilitySnapshotSample": {
                "lastAnalyticsDecisionSample": (
                    snapshot.get("lastAnalyticsDecisionSample")
                    if isinstance(snapshot, dict)
                    else None
                ),
                "chatBotAvailable": (
                    snapshot.get("chatBotAvailable")
                    if isinstance(snapshot, dict)
                    else None
                ),
                "botTokenManagerAvailable": (
                    snapshot.get("botTokenManagerAvailable")
                    if isinstance(snapshot, dict)
                    else None
                ),
            },
        }

    async def _reload_social_teardown(self) -> None:
        """Stop ClipFetcher and UploadWorker before hot-reloading social modules."""
        if self.clip_fetcher:
            try:
                self.clip_fetcher.cog_unload()
                self.clip_fetcher = None
            except Exception:
                log.exception("_reload_social_teardown: ClipFetcher stop failed")
        if self.upload_worker:
            try:
                self.upload_worker.cog_unload()
                self.upload_worker = None
            except Exception:
                log.exception("_reload_social_teardown: UploadWorker stop failed")

    async def _reload_social_startup(self) -> None:
        """Re-create ClipFetcher and UploadWorker after hot-reloading social modules."""
        if not self.api:
            log.warning("_reload_social_startup: no Twitch API — skipping social workers")
            return
        try:
            from .social_media.clip_fetcher import ClipFetcher
            from .social_media.clip_manager import ClipManager
            from .social_media.upload_worker import UploadWorker

            self.clip_manager = ClipManager(twitch_api=self.api)
            self.clip_fetcher = ClipFetcher(self.bot, self.api, self.clip_manager)
            self.upload_worker = UploadWorker(self.bot, self.clip_manager)
            log.info("_reload_social_startup: social workers restarted")
        except Exception:
            log.exception("_reload_social_startup: failed to restart social workers")

    async def _scout_deadlock_channels(self):
        """Periodically scout for live German Deadlock streams and join them.
        Also cleans up monitored channels that are no longer playing Deadlock.
        """
        await self.bot.wait_until_ready()

        # Initial delay to let other things startup
        await asyncio.sleep(60)

        while True:
            scout_flow_id = _observability_flow_id("scout")
            scout_summary: dict[str, object] = {
                "flow_id": scout_flow_id,
                "new_logins": [],
                "heal_logins": [],
                "heal_reasons": {},
                "to_remove": [],
                "streams_seen": 0,
                "current_deadlock_logins_count": 0,
                "existing_monitored_count": 0,
                "chat_runtime_monitored_count": 0,
                "ready_check_available": False,
                "set_monitored_channels_ok": None,
                "join_channels_ok": None,
                "part_channels_ok": None,
            }
            try:
                if not self.api:
                    log.warning("Scout: Twitch API not available, skipping.")
                    await asyncio.sleep(300)
                    continue

                # Ensure we have the Game ID
                if not self._category_id:
                    self._category_id = await self._ensure_category_id()

                if not self._category_id:
                    log.warning("Scout: Could not resolve Game ID for Deadlock, skipping.")
                    await asyncio.sleep(300)
                    continue

                # --- 1. Find NEW targets ---
                # Fetch live streams (language='de', game_id=Deadlock)
                streams = await self.api.get_streams_for_game(
                    game_id=self._category_id,
                    game_name=self._target_game_name,
                    language="de",
                    limit=100,
                )
                scout_summary["streams_seen"] = len(streams)

                current_deadlock_logins = {
                    s.get("user_login", "").lower() for s in streams if s.get("user_login")
                }
                scout_summary["current_deadlock_logins_count"] = len(current_deadlock_logins)
                new_logins = []
                now = datetime.now(UTC).isoformat(timespec="seconds")

                with storage.get_conn() as conn:
                    # Get currently monitored
                    existing_monitored = {
                        row[0].lower()
                        for row in conn.execute(
                            "SELECT twitch_login FROM twitch_streamers WHERE is_monitored_only = 1"
                        ).fetchall()
                    }
                    scout_summary["existing_monitored_count"] = len(existing_monitored)

                    for s in streams:
                        login = s.get("user_login", "").lower()
                        if not login:
                            continue

                        # Only add if not already tracked (as partner or monitor)
                        exists = conn.execute(
                            "SELECT 1 FROM twitch_streamers WHERE twitch_login = ?",
                            (login,),
                        ).fetchone()

                        if not exists:
                            conn.execute(
                                """
                                INSERT INTO twitch_streamers (twitch_login, twitch_user_id, is_monitored_only, created_at)
                                VALUES (?, ?, 1, ?)
                                """,
                                (login, s.get("user_id"), now),
                            )
                            new_logins.append(login)
                    scout_summary["new_logins"] = list(new_logins)

                    conn.commit()

                if new_logins:
                    await self._prime_monitored_only_sessions(
                        streams=streams,
                        logins=new_logins,
                    )

                # --- 2. Cleanup OLD targets ---
                # Remove monitored channels that are NO LONGER in the live Deadlock list.
                # This covers: Offline, Switched Game, Removed 'de' tag.
                to_remove = []
                for login in existing_monitored:
                    if login not in current_deadlock_logins:
                        to_remove.append(login)
                scout_summary["to_remove"] = list(to_remove)

                if to_remove:
                    with storage.get_conn() as conn:
                        for login in to_remove:
                            # Finalize open sessions before deleting
                            try:
                                conn.execute(
                                    """
                                    UPDATE twitch_stream_sessions
                                    SET ended_at = CURRENT_TIMESTAMP,
                                        duration_seconds = EXTRACT(EPOCH FROM (CURRENT_TIMESTAMP - started_at))::int,
                                        notes = COALESCE(notes || '; ', '') || 'auto-closed: scout-removed'
                                    WHERE streamer_login = ? AND ended_at IS NULL
                                    """,
                                    (login,),
                                )
                            except Exception:
                                log.debug("Scout: session cleanup failed for %s", login, exc_info=True)
                            # Clean up stale live_state
                            try:
                                conn.execute(
                                    "DELETE FROM twitch_live_state WHERE streamer_login = ?",
                                    (login,),
                                )
                            except Exception:
                                log.debug("Scout: live_state cleanup failed for %s", login, exc_info=True)
                            storage.delete_streamer(conn, login)
                        conn.commit()
                    log.info(
                        "Scout: Removing %d monitored channels (no longer Deadlock/DE/Live): %s",
                        len(to_remove),
                        ", ".join(to_remove[:10]),
                    )

                # --- 3. Sync Chat Bot ---
                chat_bot = getattr(self, "_twitch_chat_bot", None)
                heal_logins: list[str] = []
                heal_reasons: dict[str, str] = {}
                if chat_bot:
                    ready_check = getattr(chat_bot, "is_channel_subscription_ready", None)
                    monitored_runtime = getattr(chat_bot, "_monitored_streamers", None)
                    runtime_monitored = {
                        str(login or "").strip().lower()
                        for login in monitored_runtime
                    } if isinstance(monitored_runtime, set) else set()
                    scout_summary["chat_runtime_monitored_count"] = len(runtime_monitored)
                    scout_summary["ready_check_available"] = callable(ready_check)
                    for login in sorted(current_deadlock_logins.intersection(existing_monitored)):
                        if login in new_logins or login in to_remove:
                            continue
                        is_ready = False
                        if callable(ready_check):
                            try:
                                is_ready = bool(ready_check(login))
                            except Exception:
                                log.debug(
                                    "Scout: readiness check failed for %s",
                                    login,
                                    exc_info=True,
                                )
                        else:
                            is_ready = login in runtime_monitored
                        if not is_ready:
                            heal_logins.append(login)
                            heal_reasons[login] = (
                                "subscription_not_ready"
                                if login in runtime_monitored and callable(ready_check)
                                else "missing_runtime_membership"
                            )
                    scout_summary["heal_logins"] = list(heal_logins)
                    scout_summary["heal_reasons"] = heal_reasons
                if chat_bot:
                    join_targets: list[str] = []
                    for login in [*new_logins, *heal_logins]:
                        if login and login not in join_targets:
                            join_targets.append(login)

                    # Join new or heal missing runtime subscriptions
                    if join_targets:
                        if new_logins:
                            log.info("Scout: Joining %d new channels", len(new_logins))
                        if heal_logins:
                            log.info(
                                "Scout: Rejoining %d monitored channels missing from chat runtime",
                                len(heal_logins),
                            )
                        set_monitored_channels = getattr(chat_bot, "set_monitored_channels", None)
                        if callable(set_monitored_channels):
                            try:
                                set_monitored_channels(join_targets)
                                scout_summary["set_monitored_channels_ok"] = True
                            except Exception:
                                scout_summary["set_monitored_channels_ok"] = False
                                log.debug(
                                    "Scout: set_monitored_channels failed",
                                    exc_info=True,
                                )

                        join_channels = getattr(chat_bot, "join_channels", None)
                        if callable(join_channels):
                            await join_channels(join_targets)
                            scout_summary["join_channels_ok"] = True
                        else:
                            join_single = getattr(chat_bot, "join", None)
                            if callable(join_single):
                                joined = 0
                                for login in join_targets:
                                    try:
                                        if await join_single(login):
                                            joined += 1
                                    except Exception:
                                        log.debug(
                                            "Scout: fallback join failed for %s",
                                            login,
                                            exc_info=True,
                                        )
                                log.warning(
                                    "Scout: chat bot has no join_channels; fallback join used (%d/%d).",
                                    joined,
                                    len(join_targets),
                                )
                                scout_summary["join_channels_ok"] = joined == len(join_targets)
                            else:
                                log.warning(
                                    "Scout: chat bot has neither join_channels nor join; cannot join %d channels.",
                                    len(join_targets),
                                )
                                scout_summary["join_channels_ok"] = False

                    # Leave old
                    if to_remove:
                        part_channels = getattr(chat_bot, "part_channels", None)
                        if callable(part_channels):
                            log.info("Scout: Leaving %d channels", len(to_remove))
                            await part_channels(to_remove)
                            scout_summary["part_channels_ok"] = True
                        else:
                            monitored = getattr(chat_bot, "_monitored_streamers", None)
                            if isinstance(monitored, set):
                                for login in to_remove:
                                    monitored.discard(str(login).strip().lower())
                            channel_ids = getattr(chat_bot, "_channel_ids", None)
                            if isinstance(channel_ids, dict):
                                for login in to_remove:
                                    channel_ids.pop(str(login).strip().lower(), None)
                            log.info(
                                "Scout: part_channels not available; removed %d channels from local monitor cache.",
                                len(to_remove),
                            )
                            scout_summary["part_channels_ok"] = False

                log.info(
                    "scout_cycle_summary %s",
                    _observability_fields(
                        **scout_summary,
                        new_logins_sample=_observability_sample(scout_summary["new_logins"]),
                        heal_logins_sample=_observability_sample(scout_summary["heal_logins"]),
                        to_remove_sample=_observability_sample(scout_summary["to_remove"]),
                    ),
                )
                storage.insert_observability_event(
                    flow_type="scout",
                    flow_id=scout_flow_id,
                    step="cycle_complete",
                    decision="ok",
                    details=dict(scout_summary),
                )

            except Exception:
                scout_summary["error"] = "exception"
                log.warning(
                    "scout_cycle_summary %s",
                    _observability_fields(
                        **scout_summary,
                        new_logins_sample=_observability_sample(scout_summary["new_logins"]),
                        heal_logins_sample=_observability_sample(scout_summary["heal_logins"]),
                        to_remove_sample=_observability_sample(scout_summary["to_remove"]),
                    ),
                )
                storage.insert_observability_event(
                    flow_type="scout",
                    flow_id=scout_flow_id,
                    step="cycle_complete",
                    decision="exception",
                    details=dict(scout_summary),
                )
                log.exception("Scout: Error during Deadlock channel scouting")

            # Run every 5 minutes
            await asyncio.sleep(300)

    async def _prime_monitored_only_sessions(
        self,
        *,
        streams: list[dict[str, object]],
        logins: list[str],
    ) -> None:
        """Create sessions for freshly discovered monitored-only channels before chat joins them."""
        if not logins:
            return

        ensure_stream_session = getattr(self, "_ensure_stream_session", None)
        if not callable(ensure_stream_session):
            return

        streams_by_login: dict[str, dict[str, object]] = {}
        for stream in streams:
            login = str(stream.get("user_login") or "").strip().lower()
            if login:
                streams_by_login[login] = stream

        primed = 0
        for login in logins:
            stream = streams_by_login.get(str(login or "").strip().lower())
            if not stream:
                continue

            try:
                session_id = await ensure_stream_session(
                    login=str(login).strip().lower(),
                    stream=stream,
                    previous_state={},
                    twitch_user_id=str(stream.get("user_id") or "").strip() or None,
                )
            except Exception:
                log.debug(
                    "Scout: monitored-only session bootstrap failed for %s",
                    login,
                    exc_info=True,
                )
                continue

            if session_id is not None:
                primed += 1

        if primed and primed != len(logins):
            log.debug(
                "Scout: primed monitored-only sessions for %d/%d new channels.",
                primed,
                len(logins),
            )

    def _register_persistent_raid_auth_views(self) -> None:
        """Registriert persistente RaidAuthGenerateViews für alle Streamer in der DB.
        Muss bei Bot-Start aufgerufen werden damit Buttons nach Neustart funktionieren."""
        from .raid.views import RaidAuthGenerateView

        try:
            with storage.get_conn() as conn:
                rows = conn.execute(
                    "SELECT twitch_login FROM twitch_raid_auth WHERE twitch_login IS NOT NULL"
                ).fetchall()
            count = 0
            for row in rows:
                login = (
                    str(row[0] if not hasattr(row, "keys") else row["twitch_login"]).strip().lower()
                )
                if login:
                    self.bot.add_view(RaidAuthGenerateView(twitch_login=login))
                    count += 1
            log.debug("Persistente RaidAuthViews registriert: %d Streamer", count)
        except Exception:
            log.exception("Fehler beim Registrieren persistenter RaidAuthViews")

    async def _startup_db_warmup(self) -> None:
        """Lightweight Warmup: DB-Verbindung + Active Sessions erst nach Bot-Ready herstellen."""
        try:
            await self.bot.wait_until_ready()
        except Exception:
            log.debug("Warmup wait_until_ready fehlgeschlagen", exc_info=True)
            return

        t0 = time.perf_counter()
        try:
            self._rehydrate_active_sessions()
            duration = time.perf_counter() - t0
            log_event("twitch.db_warmup", duration, "rehydrate_active_sessions")
            log.debug("Warmup: _rehydrate_active_sessions in %.3fs", duration)
        except Exception:
            log.debug("Warmup: aktive Sessions konnten nicht rehydriert werden", exc_info=True)

    async def _register_views_after_ready(self) -> None:
        """Register persistent views after the bot is ready to avoid blocking startup."""
        try:
            await self.bot.wait_until_ready()
        except Exception:
            log.debug("View-Warmup wait_until_ready fehlgeschlagen", exc_info=True)
            return

        t0 = time.perf_counter()
        try:
            self._register_persistent_raid_auth_views()
            duration = time.perf_counter() - t0
            log_event("twitch.views_warmup", duration, "raid_auth_views")
            log.debug("Warmup: RaidAuthViews registriert in %.3fs", duration)
        except Exception:
            log.debug("Warmup: RaidAuthViews konnten nicht registriert werden", exc_info=True)

    # -------------------------------------------------------
    # Lifecycle
    # -------------------------------------------------------
    async def cog_unload(self):
        """Ensure background resources are torn down when the cog is removed.

        Shutdown-Reihenfolge (wichtig!):
        1. Tasks/Loops canceln
        2. EventSub WebSocket Listeners stoppen
        3. Chat Bot sauber beenden (inklusive Port-Freigabe)
        4. Token Manager cleanup
        5. Dashboard stoppen (inklusive Port-Freigabe)
        6. Internal API stoppen
        7. Raid Bot cleanup
        8. API Session schließen
        9. Commands deregistrieren
        """
        log.info("Twitch Cog Unload gestartet – fahre alle Ressourcen herunter...")

        # 1. Background Loops canceln
        loops = (self.poll_streams, self.invites_refresh)
        for lp in loops:
            try:
                if lp.is_running():
                    lp.cancel()
                    log.debug("Loop gecancelt: %r", lp)
            except Exception:
                log.exception("Konnte Loop nicht canceln: %r", lp)

        # 1.5 Social Media Workers stoppen
        if self.clip_fetcher:
            try:
                self.clip_fetcher.cog_unload()
                log.debug("ClipFetcher gecancelt")
            except Exception:
                log.exception("Konnte ClipFetcher nicht canceln")

        if self.upload_worker:
            try:
                self.upload_worker.cog_unload()
                log.debug("UploadWorker gecancelt")
            except Exception:
                log.exception("Konnte UploadWorker nicht canceln")

        try:
            await self._cancel_periodic_channel_join_task()
        except Exception:
            log.exception("Konnte periodic channel maintenance task nicht stoppen")

        if self._irc_lurker_tracker:
            try:
                await self._irc_lurker_tracker.stop()
                log.debug("Experimental IRC Lurker Tracker gestoppt")
            except Exception:
                log.exception("Konnte Experimental IRC Lurker Tracker nicht stoppen")
            finally:
                self._irc_lurker_tracker = None

        # 2. EventSub: Webhook Handler hat keinen persistenten Zustand der explizit
        #    gestoppt werden muss. Etwaige Background-Tasks (dispatch) werden beim
        #    asyncio-Shutdown automatisch gecancelt.
        log.debug("EventSub Webhook: kein expliziter Teardown nötig")

        # 3. Twitch Chat Bot sauber beenden (Port 4343 freigeben)
        if self._twitch_chat_bot:
            log.info("Beende Twitch Chat Bot...")
            try:
                # TwitchIO Bot hat keine explizite shutdown-Methode in 3.x,
                # aber close() schließt die WebSocket-Session und den Adapter.
                if hasattr(self._twitch_chat_bot, "close"):
                    await self._twitch_chat_bot.close()
                    log.debug("Chat Bot close() abgeschlossen")

                # Warte explizit auf Port-Freigabe (4343)
                adapter = getattr(self._twitch_chat_bot, "adapter", None)
                if adapter:
                    adapter_host = getattr(adapter, "_host", "127.0.0.1")
                    adapter_port = int(getattr(adapter, "_port", 4343))

                    # Gebe dem Adapter Zeit zum Herunterfahren
                    await asyncio.sleep(2.0)

                    # Prüfe ob der Port frei ist
                    for retry in range(10):  # Max 10 Sekunden warten
                        can_bind, _ = await self._can_bind_port_async(adapter_host, adapter_port)
                        if can_bind:
                            log.info(
                                "Chat Bot Adapter Port %s:%s erfolgreich freigegeben",
                                adapter_host,
                                adapter_port,
                            )
                            break
                        if retry < 9:
                            log.debug(
                                "Warte auf Port-Freigabe %s:%s... (%d/10)",
                                adapter_host,
                                adapter_port,
                                retry + 1,
                            )
                            await asyncio.sleep(1.0)
                        else:
                            log.warning(
                                "Port %s:%s nach 10s noch belegt – fahre trotzdem fort",
                                adapter_host,
                                adapter_port,
                            )

                log.info("Twitch Chat Bot beendet")
            except Exception:
                log.exception("Twitch Chat Bot shutdown fehlgeschlagen")

        # 4. Token Manager cleanup
        if self._bot_token_manager:
            try:
                await self._bot_token_manager.cleanup()
                log.debug("Bot Token Manager cleanup abgeschlossen")
            except Exception:
                log.exception("Twitch Bot Token Manager shutdown fehlgeschlagen")

        # 5. Dashboard stoppen (Port 8765 freigeben)
        if self._web:
            log.info("Stoppe Twitch Dashboard...")
            try:
                await self._stop_dashboard()

                # Warte explizit auf Port-Freigabe (8765)
                dashboard_port = self._dashboard_port
                dashboard_host = self._dashboard_host

                # Gebe dem Dashboard Zeit zum Herunterfahren
                await asyncio.sleep(2.0)

                # Prüfe ob der Port frei ist
                for retry in range(10):  # Max 10 Sekunden warten
                    can_bind, _ = await self._can_bind_port_async(dashboard_host, dashboard_port)
                    if can_bind:
                        log.info(
                            "Dashboard Port %s:%s erfolgreich freigegeben",
                            dashboard_host,
                            dashboard_port,
                        )
                        break
                    if retry < 9:
                        log.debug(
                            "Warte auf Port-Freigabe %s:%s... (%d/10)",
                            dashboard_host,
                            dashboard_port,
                            retry + 1,
                        )
                        await asyncio.sleep(1.0)
                    else:
                        log.warning(
                            "Port %s:%s nach 10s noch belegt – fahre trotzdem fort",
                            dashboard_host,
                            dashboard_port,
                        )

                log.info("Twitch Dashboard gestoppt")
            except Exception:
                log.exception("Dashboard shutdown fehlgeschlagen")

        # 6. Internal API stoppen
        if self._internal_api_runner and self._internal_api_runner.is_running:
            log.info("Stoppe interne Twitch API...")
            try:
                await self._stop_internal_api()
            except Exception:
                log.exception("Internal API shutdown fehlgeschlagen")

        # 7. RaidBot Cleanup
        if self._raid_bot:
            try:
                await self._raid_bot.cleanup()
                log.debug("RaidBot cleanup abgeschlossen")
            except Exception:
                log.exception("RaidBot cleanup fehlgeschlagen")

        # 8. API Session schließen (mit Grace Period für laufende Requests)
        if self.api is not None:
            log.info("Schließe Twitch API Session...")
            try:
                # Warte kurz damit laufende Requests abgeschlossen werden können
                await asyncio.sleep(1.0)
                await self.api.aclose()
                log.info("Twitch API Session geschlossen")
            except asyncio.CancelledError as exc:
                log.debug("Schließen der TwitchAPI-Session abgebrochen: %s", exc)
                raise
            except Exception:
                log.exception("TwitchAPI-Session konnte nicht geschlossen werden")

        # 9. Commands deregistrieren
        try:
            if self._twl_command is not None:
                existing = self.bot.get_command(self._twl_command.name)
                if existing is self._twl_command:
                    self.bot.remove_command(self._twl_command.name)
                    log.debug("!twl Command deregistriert")
        except Exception:
            log.exception("Konnte !twl-Command nicht deregistrieren")
        finally:
            self._twl_command = None

        # Finale Pause damit alle async Tasks sauber beendet werden können
        await asyncio.sleep(0.5)
        log.info("Twitch Cog Unload abgeschlossen")

    def set_prefix_command(self, command: commands.Command) -> None:
        """Speichert die Referenz auf den dynamisch registrierten Prefix-Command."""
        self._twl_command = command

    async def _start_internal_api(self) -> None:
        runner = self._internal_api_runner
        if runner is None:
            return
        try:
            await runner.start()
            if not runner.is_running:
                log.error(
                    "Konnte interne Twitch API nicht starten%s",
                    f": {runner.last_start_error}" if runner.last_start_error else "",
                )
        except Exception:
            log.exception("Konnte interne Twitch API nicht starten")

    async def _stop_internal_api(self) -> None:
        runner = self._internal_api_runner
        if runner is None:
            return
        await runner.stop()

    def _spawn_bg_task(self, coro: Coroutine[Any, Any, Any], name: str) -> None:
        """Start a background coroutine without relying on Bot.loop (removed in d.py 2.4)."""
        try:
            asyncio.create_task(coro, name=name)
        except RuntimeError as exc:
            log.error("Cannot start background task %s (no running loop yet): %s", name, exc)
        except Exception:
            log.exception("Failed to start background task %s", name)

    def _ensure_periodic_channel_join_task(self) -> asyncio.Task | None:
        """Start the periodic chat channel maintenance loop at most once."""
        existing = getattr(self, "_periodic_channel_join_task", None)
        if existing is not None and not existing.done():
            return existing
        try:
            task = asyncio.create_task(
                self._periodic_channel_join(), name="twitch.chat_bot.join_channels"
            )
        except RuntimeError as exc:
            log.error(
                "Cannot start background task %s (no running loop yet): %s",
                "twitch.chat_bot.join_channels",
                exc,
            )
            return None
        except Exception:
            log.exception("Failed to start background task %s", "twitch.chat_bot.join_channels")
            return None
        self._periodic_channel_join_task = task
        return task

    async def _cancel_periodic_channel_join_task(self) -> None:
        """Cancel the periodic chat channel maintenance loop if it is running."""
        task = getattr(self, "_periodic_channel_join_task", None)
        self._periodic_channel_join_task = None
        if task is None or task.done():
            return
        task.cancel()
        try:
            await task
        except asyncio.CancelledError:
            log.debug("Periodic channel maintenance task cancelled")
        except Exception:
            log.debug("Periodic channel maintenance task failed during shutdown", exc_info=True)

    async def _ensure_experimental_irc_lurker_tracker_started(self) -> bool:
        """
        Start the experimental IRC lurker tracker as a secondary presence source.

        This is intentionally optional and does not replace Helix `Get Chatters`.
        """
        if not bool(getattr(self, "_experimental_irc_lurker_enabled", False)):
            return False

        existing = getattr(self, "_irc_lurker_tracker", None)
        if existing is not None and getattr(existing, "running", False):
            return True

        allowlist = sorted(
            str(channel or "").strip().lower().lstrip("#")
            for channel in (getattr(self, "_experimental_irc_lurker_channels", set()) or set())
            if str(channel or "").strip()
        )
        if not allowlist:
            return False

        token_manager = getattr(self, "_bot_token_manager", None)
        token = str(getattr(token_manager, "access_token", "") or "").strip()
        bot_login = str(getattr(token_manager, "bot_login", "") or "").strip().lower()
        bot_scopes = {
            str(scope).strip().lower()
            for scope in (getattr(token_manager, "scopes", set()) or set())
            if str(scope).strip()
        }
        if not token or not bot_login:
            self._log_chat_bot_lifecycle_event(
                flow_id=_observability_flow_id("irc-lurker"),
                event="irc_lurker_experiment_skipped",
                level=logging.WARNING,
                experimental=True,
                reason="missing_bot_token_or_login",
                token_present=bool(token),
                bot_login_present=bool(bot_login),
            )
            return False
        if bot_scopes and "user:read:chat" not in bot_scopes:
            self._log_chat_bot_lifecycle_event(
                flow_id=_observability_flow_id("irc-lurker"),
                event="irc_lurker_experiment_skipped",
                level=logging.WARNING,
                experimental=True,
                reason="missing_user_read_chat_scope",
                bot_scopes=sorted(bot_scopes),
            )
            return False

        tracker = IRCLurkerTracker(
            self._twitch_bot_client_id,
            token,
            nick=bot_login,
        )
        await tracker.start()
        self._irc_lurker_tracker = tracker
        self._log_chat_bot_lifecycle_event(
            flow_id=_observability_flow_id("irc-lurker"),
            event="irc_lurker_experiment_started",
            experimental=True,
            source_role="secondary_presence_source",
            allowlist=allowlist,
            bot_login=bot_login,
            bot_scopes=sorted(bot_scopes),
        )
        log.info(
            "Experimental IRC Lurker Tracker gestartet: zweite Presence-Quelle neben Helix Chatters "
            "(bot_login=%s, channels=%s)",
            bot_login,
            ",".join(allowlist),
        )
        return True

    async def _sync_experimental_irc_lurker_tracker_channels(self) -> dict[str, object]:
        """Mirror current chat-bot runtime channels into the experimental IRC tracker."""
        tracker = getattr(self, "_irc_lurker_tracker", None)
        chat_bot = getattr(self, "_twitch_chat_bot", None)
        if tracker is None or chat_bot is None:
            return {"tracked": 0, "untracked": 0, "runtime_sources": []}

        runtime_sources: list[str] = []
        target_channels: set[str] = set()
        allowlist = {
            str(channel or "").strip().lower().lstrip("#")
            for channel in (getattr(self, "_experimental_irc_lurker_channels", set()) or set())
            if str(channel or "").strip()
        }

        monitored = getattr(chat_bot, "_monitored_streamers", None)
        if isinstance(monitored, set) and monitored:
            runtime_sources.append("monitored_streamers")
            target_channels.update(
                str(channel or "").strip().lower().lstrip("#")
                for channel in monitored
                if str(channel or "").strip()
            )

        subscription_types = getattr(chat_bot, "_channel_subscription_types", None)
        if isinstance(subscription_types, dict) and subscription_types:
            runtime_sources.append("channel_subscription_types")
            target_channels.update(
                str(channel or "").strip().lower().lstrip("#")
                for channel in subscription_types.keys()
                if str(channel or "").strip()
            )

        channel_ids = getattr(chat_bot, "_channel_ids", None)
        if isinstance(channel_ids, dict) and channel_ids:
            runtime_sources.append("channel_ids")
            target_channels.update(
                str(channel or "").strip().lower().lstrip("#")
                for channel in channel_ids.keys()
                if str(channel or "").strip()
            )

        initial_channels = getattr(chat_bot, "_initial_channels", None)
        if not target_channels and isinstance(initial_channels, list) and initial_channels:
            runtime_sources.append("initial_channels_bootstrap")
            target_channels.update(
                str(channel or "").strip().lower().lstrip("#")
                for channel in initial_channels
                if str(channel or "").strip()
            )

        if allowlist:
            runtime_sources.append("forced_allowlist")
            target_channels.update(allowlist)

        if allowlist:
            target_channels = {channel for channel in target_channels if channel in allowlist}

        current_channels = set(getattr(tracker, "channels", set()) or set())
        to_track = sorted(channel for channel in target_channels if channel not in current_channels)
        to_untrack = sorted(channel for channel in current_channels if channel not in target_channels)

        classifier = getattr(chat_bot, "_is_partner_channel_for_chat_tracking", None)
        for channel in to_track:
            mode = "partner"
            if callable(classifier):
                try:
                    if not bool(classifier(channel)):
                        mode = "category"
                except Exception:
                    log.debug("IRC experiment: channel classification failed for %s", channel, exc_info=True)
            await tracker.track_channel(channel, mode=mode)

        for channel in to_untrack:
            await tracker.untrack_channel(channel)

        if to_track or to_untrack:
            log.info(
                "Experimental IRC Lurker Tracker synchronisiert "
                "(tracked=%d, untracked=%d, runtime_sources=%s)",
                len(to_track),
                len(to_untrack),
                ",".join(runtime_sources) or "-",
            )

        return {
            "tracked": len(to_track),
            "untracked": len(to_untrack),
            "runtime_sources": runtime_sources,
        }

    # -------------------------------------------------------
    # DB-Helpers / Guild-Setup / Invites
    # -------------------------------------------------------
    def _set_channel(self, guild_id: int, channel_id: int) -> None:
        with storage.get_conn() as c:
            c.execute(
                "INSERT INTO twitch_guild_settings (guild_id, notify_channel_id) "
                "VALUES (?, ?) "
                "ON CONFLICT (guild_id) DO UPDATE SET notify_channel_id = EXCLUDED.notify_channel_id",
                (int(guild_id), int(channel_id)),
            )
        if self._notify_channel_id == 0:
            self._notify_channel_id = int(channel_id)

    async def _refresh_all_invites(self):
        """Alle Guild-Einladungen sammeln (für Link-Checks/Partner-Validierung sinnvoll)."""
        try:
            await self.bot.wait_until_ready()
        except Exception:
            log.exception("wait_until_ready fehlgeschlagen")
            return

        guilds = list(self.bot.guilds)
        if not guilds:
            return

        # Delay zwischen Guilds einbauen um Rate Limits zu vermeiden
        delay_between_guilds = max(2.0, 30.0 / len(guilds))  # Minimum 2s, verteilt über 30s

        for i, guild in enumerate(guilds):
            try:
                await self._refresh_guild_invites(guild)
                # Warte zwischen Guilds, außer beim letzten
                if i < len(guilds) - 1:
                    await asyncio.sleep(delay_between_guilds)
            except Exception:
                log.exception("Einladungen für Guild %s fehlgeschlagen", guild.id)

    async def _load_invite_codes_from_db(self):
        """Load cached invite codes from database on startup."""
        try:
            await self.bot.wait_until_ready()
        except Exception:
            log.exception("wait_until_ready fehlgeschlagen")
            return

        try:
            with storage.get_conn() as conn:
                rows = conn.execute(
                    "SELECT guild_id, invite_code FROM discord_invite_codes"
                ).fetchall()

            if not rows:
                log.info(
                    "Keine Invite-Codes in DB gefunden - werden beim ersten Gebrauch abgerufen"
                )
                return

            # Gruppiere nach Guild
            by_guild: dict[int, set[str]] = {}
            for guild_id, code in rows:
                if guild_id not in by_guild:
                    by_guild[guild_id] = set()
                by_guild[guild_id].add(code)

            # Lade in RAM-Cache
            for guild_id, codes in by_guild.items():
                self._invite_codes[guild_id] = codes

            total_codes = sum(len(codes) for codes in by_guild.values())
            log.debug(
                "Invite-Codes aus DB geladen: %s Guilds, %s Codes gesamt",
                len(by_guild),
                total_codes,
            )
        except Exception:
            log.exception("Konnte Invite-Codes nicht aus DB laden")

    async def _sync_missing_user_ids(self):
        """Beim Start fehlende twitch_user_id in twitch_streamers nachfüllen.

        Strategie:
          1. Aus twitch_raid_auth übernehmen (kein API-Call noetig).
          2. Verbleibende per Twitch-API (Helix /users) auflösen.
        Wird nur beim Hochfahren ausgeführt – neue Einträge bekommen
        ihre user_id bereits beim Anlegen in _cmd_add / _dashboard_save_discord_profile.
        """
        try:
            await self.bot.wait_until_ready()
        except Exception:
            log.exception("wait_until_ready in _sync_missing_user_ids fehlgeschlagen")
            return

        # --- Phase 1: Sync aus raid_auth (offline, instant) ---
        try:
            with storage.get_conn() as conn:
                conn.execute("""
                    UPDATE twitch_streamers
                    SET twitch_user_id = (
                        SELECT tra.twitch_user_id
                        FROM twitch_raid_auth tra
                        WHERE LOWER(tra.twitch_login) = LOWER(twitch_streamers.twitch_login)
                    )
                    WHERE twitch_user_id IS NULL
                      AND EXISTS (
                          SELECT 1 FROM twitch_raid_auth tra
                          WHERE LOWER(tra.twitch_login) = LOWER(twitch_streamers.twitch_login)
                            AND tra.twitch_user_id IS NOT NULL
                      )
                """)
                synced = conn.execute("SELECT changes()").fetchone()[0]
            if synced:
                log.info(
                    "_sync_missing_user_ids: %d user_ids aus raid_auth übernommen",
                    synced,
                )
        except Exception:
            log.exception("_sync_missing_user_ids: Phase 1 (raid_auth) fehlgeschlagen")

        # --- Phase 2: Rest per API auflösen ---
        try:
            with storage.get_conn() as conn:
                rows = conn.execute(
                    "SELECT twitch_login FROM twitch_streamers WHERE twitch_user_id IS NULL"
                ).fetchall()
            missing = [row[0] for row in rows]
        except Exception:
            log.exception("_sync_missing_user_ids: Konnte fehlende Logins nicht laden")
            return

        if not missing:
            log.debug("_sync_missing_user_ids: alle user_ids vorhanden, nichts zu tun")
            return

        log.info(
            "_sync_missing_user_ids: %d Logins ohne user_id, frage Twitch API ab",
            len(missing),
        )

        try:
            # get_users gibt ein Dict {login: {id, login, ...}} zurück
            users = await self.api.get_users(missing)
        except Exception:
            log.exception("_sync_missing_user_ids: API-Aufruf fehlgeschlagen")
            return

        if not users:
            log.warning(
                "_sync_missing_user_ids: API hat keine Ergebnisse für %s zurückgegeben",
                missing,
            )
            return

        try:
            with storage.get_conn() as conn:
                for login, user_data in users.items():
                    uid = user_data.get("id")
                    if uid:
                        conn.execute(
                            "UPDATE twitch_streamers SET twitch_user_id = ? "
                            "WHERE LOWER(twitch_login) = LOWER(?) AND twitch_user_id IS NULL",
                            (uid, login),
                        )
            log.info("_sync_missing_user_ids: %d user_ids per API aktualisiert", len(users))
        except Exception:
            log.exception("_sync_missing_user_ids: DB-Update nach API-Aufruf fehlgeschlagen")

        # --- Abschliessender Bericht ---
        try:
            with storage.get_conn() as conn:
                still_missing = conn.execute(
                    "SELECT twitch_login FROM twitch_streamers WHERE twitch_user_id IS NULL"
                ).fetchall()
            if still_missing:
                log.warning(
                    "_sync_missing_user_ids: %d Logins konnten nicht aufgelöst werden: %s",
                    len(still_missing),
                    [r[0] for r in still_missing],
                )
            else:
                log.info("_sync_missing_user_ids: alle user_ids erfolgreich gesetzt")
        except Exception:
            log.debug(
                "_sync_missing_user_ids: Abschliessender Check fehlgeschlagen",
                exc_info=True,
            )

    async def _refresh_guild_invites(self, guild: Guild):
        codes: set[str] = set()
        max_retries = 3
        retry_delay = 5.0  # Initial 5 Sekunden

        for attempt in range(max_retries):
            try:
                invites = await guild.invites()
                for inv in invites:
                    if inv.code:
                        codes.add(inv.code)
                break  # Erfolg, Schleife verlassen
            except Forbidden:
                log.warning("Fehlende Berechtigung, um Invites von Guild %s zu lesen", guild.id)
                break  # Keine Retries bei Permission-Fehler
            except HTTPException as e:
                if attempt < max_retries - 1 and "429" in str(e):  # Rate Limit
                    wait_time = retry_delay * (2**attempt)  # Exponential backoff
                    log.warning(
                        "Rate Limit bei Invite-Refresh für Guild %s - warte %s Sekunden (Versuch %s/%s)",
                        guild.id,
                        wait_time,
                        attempt + 1,
                        max_retries,
                    )
                    await asyncio.sleep(wait_time)
                else:
                    # Letzter Versuch oder anderer Fehler - loggen und abbrechen
                    if "429" in str(e):
                        log.error(
                            "HTTP-Fehler beim Abruf der Invites für Guild %s nach %s Versuchen - überspringe",
                            guild.id,
                            max_retries,
                        )
                    else:
                        log.exception("HTTP-Fehler beim Abruf der Invites für Guild %s", guild.id)
                    break

        # Cache im RAM
        self._invite_codes[guild.id] = codes

        # Persistiere in DB für spätere Verwendung
        if codes:
            try:
                from datetime import datetime

                now = datetime.now(UTC).isoformat(timespec="seconds")
                with storage.get_conn() as conn:
                    # Lösche alte Codes die nicht mehr existieren
                    existing = {
                        row[0]
                        for row in conn.execute(
                            "SELECT invite_code FROM discord_invite_codes WHERE guild_id = ?",
                            (guild.id,),
                        ).fetchall()
                    }

                    to_remove = existing - codes
                    if to_remove:
                        for invite_code in to_remove:
                            conn.execute(
                                "DELETE FROM discord_invite_codes WHERE guild_id = ? AND invite_code = ?",
                                (guild.id, invite_code),
                            )

                    # Füge neue hinzu oder update last_seen_at
                    for code in codes:
                        conn.execute(
                            """INSERT INTO discord_invite_codes (guild_id, invite_code, created_at, last_seen_at)
                               VALUES (?, ?, ?, ?)
                               ON CONFLICT(guild_id, invite_code) 
                               DO UPDATE SET last_seen_at = ?""",
                            (guild.id, code, now, now, now),
                        )
                    conn.commit()
                    log.debug(
                        "Invite-Codes für Guild %s in DB gespeichert: %s",
                        guild.id,
                        len(codes),
                    )
            except Exception:
                log.exception("Konnte Invite-Codes nicht in DB speichern für Guild %s", guild.id)

    async def _init_twitch_chat_bot(self):
        """Initialisiert den Twitch Chat Bot für Raid-Commands."""
        flow_id = _observability_flow_id("chat-bot-init")
        try:
            self._log_chat_bot_lifecycle_event(
                flow_id=flow_id,
                event="chat_bot_init_started",
                token_manager_ready_before_chat_bot=bool(self._bot_token_manager),
            )
            await self.bot.wait_until_ready()
            if not self._raid_bot:
                log.info("Raid-Bot nicht verfügbar, überspringe Twitch Chat Bot")
                return
            if not TWITCHIO_AVAILABLE:
                log.info("twitchio nicht installiert; Twitch Chat Bot wird übersprungen.")
                return

            token = self._twitch_bot_token
            refresh_token = self._twitch_bot_refresh_token

            if not token:
                token, refresh_from_store, _ = load_bot_tokens(log_missing=False)
                refresh_token = refresh_token or refresh_from_store

            refresh_env = os.getenv("TWITCH_BOT_REFRESH_TOKEN", "").strip() or None
            if refresh_env:
                refresh_token = refresh_env

            if not token:
                log.info(
                    "Twitch Chat Bot nicht verfuegbar (kein Token gesetzt). "
                    "Setze TWITCH_BOT_TOKEN oder TWITCH_BOT_TOKEN_FILE, um den Chat-Bot zu aktivieren."
                )
                return
            self._twitch_bot_token = token
            self._twitch_bot_refresh_token = refresh_token
            if self._bot_token_manager is None and self._twitch_bot_client_id:
                self._bot_token_manager = TwitchBotTokenManager(
                    self._twitch_bot_client_id,
                    (self._twitch_bot_secret or self.client_secret or ""),
                )
            if self._bot_token_manager is not None:
                self._log_chat_bot_lifecycle_event(
                    flow_id=flow_id,
                    event="token_manager_ready_before_chat_bot",
                    token_manager_ready_before_chat_bot=True,
                )

            self._twitch_chat_bot = await create_twitch_chat_bot(
                client_id=self._twitch_bot_client_id,
                client_secret=self._twitch_bot_secret
                or "",  # TwitchIO mag None manchmal nicht, Empty String ist sicherer
                redirect_uri=self._raid_redirect_uri,
                raid_bot=self._raid_bot,
                bot_token=token,
                bot_refresh_token=refresh_token,
                log_missing=False,
                token_manager=self._bot_token_manager,
            )

            if self._twitch_chat_bot:
                self._log_chat_bot_lifecycle_event(
                    flow_id=flow_id,
                    event="chat_bot_init_ready",
                    token_manager_ready_before_chat_bot=bool(self._bot_token_manager),
                    start_with_adapter_pending=True,
                )
                if self._bot_token_manager:
                    self._twitch_bot_token = (
                        self._bot_token_manager.access_token or self._twitch_bot_token
                    )
                    self._twitch_bot_refresh_token = (
                        self._bot_token_manager.refresh_token or self._twitch_bot_refresh_token
                    )
                try:
                    if hasattr(self._twitch_chat_bot, "set_discord_bot"):
                        invite_channel_id = self._notify_channel_id or None
                        self._twitch_chat_bot.set_discord_bot(
                            self.bot,
                            invite_channel_id=invite_channel_id,
                        )
                except Exception:
                    log.debug("Konnte Discord-Bot nicht an Chat-Bot binden", exc_info=True)
                # Bot im Hintergrund laufen lassen
                start_with_adapter = await self._should_start_chat_adapter()
                if hasattr(self._twitch_chat_bot, "configure_managed_start"):
                    self._twitch_chat_bot.configure_managed_start(
                        with_adapter=start_with_adapter,
                        load_tokens=False,
                        save_tokens=False,
                    )
                start_coro = self._twitch_chat_bot.start(
                    with_adapter=start_with_adapter,
                    load_tokens=False,  # vermeidet kaputte .tio.tokens.json ohne scope
                    save_tokens=False,
                )
                asyncio.create_task(start_coro, name="twitch.chat_bot.start")
                self._log_chat_bot_lifecycle_event(
                    flow_id=flow_id,
                    event="chat_bot_start_scheduled",
                    start_with_adapter=start_with_adapter,
                )
                log.info(
                    "Twitch Chat Bot gestartet (Web Adapter: %s)",
                    "on" if start_with_adapter else "off",
                )

                # Verknüpfe Chat-Bot mit Raid-Bot für Recruitment-Messages
                if self._raid_bot:
                    self._raid_bot.set_chat_bot(self._twitch_chat_bot)
                    log.debug("Chat-Bot mit Raid-Bot verknüpft für Recruitment-Messages")

                # Periodisch neue Partner-Channels joinen
                self._ensure_periodic_channel_join_task()
                if await self._ensure_experimental_irc_lurker_tracker_started():
                    try:
                        await self._sync_experimental_irc_lurker_tracker_channels()
                    except Exception:
                        log.exception(
                            "Experimental IRC Lurker Tracker initial sync failed"
                        )

        except Exception:
            self._log_chat_bot_lifecycle_event(
                flow_id=flow_id,
                event="chat_bot_start_failed",
                level=logging.ERROR,
            )
            log.exception("Fehler beim Initialisieren des Twitch Chat Bots")

    async def _periodic_channel_join(self):
        """Joint periodisch neue Partner-Channels und räumt Offline-Channels auf."""
        if not self._twitch_chat_bot:
            return

        await self.bot.wait_until_ready()
        await asyncio.sleep(60)  # Initial delay

        while True:
            try:
                if hasattr(self._twitch_chat_bot, "join_partner_channels"):
                    await self._twitch_chat_bot.join_partner_channels()
                await self._cleanup_offline_channels()
                if getattr(self, "_irc_lurker_tracker", None) is not None:
                    await self._sync_experimental_irc_lurker_tracker_channels()
            except Exception:
                log.exception("Fehler in periodic channel maintenance")

            await asyncio.sleep(1800)  # Alle 30 Minuten prüfen

    async def _cleanup_offline_channels(self):
        """Verlässt Channels von Partnern, die offline sind (übersprungen wenn Offline-Joins aktiv)."""
        chat_bot = getattr(self, "_twitch_chat_bot", None)
        if not chat_bot:
            return
        # Wenn Offline-Joins erlaubt sind, nicht aus Channels austreten
        if CHAT_JOIN_OFFLINE:
            return

        monitored = {login.lower() for login in getattr(chat_bot, "_monitored_streamers", set())}
        if not monitored:
            return

        offline_logins: list[str] = []
        offline_ids: dict[str, str] = {}

        try:
            with storage.get_conn() as conn:
                rows = []
                for login in monitored:
                    row = conn.execute(
                        """
                        SELECT s.twitch_login, l.is_live, s.twitch_user_id
                          FROM twitch_streamers s
                          LEFT JOIN twitch_live_state l ON s.twitch_user_id = l.twitch_user_id
                         WHERE LOWER(s.twitch_login) = ?
                        """,
                        (login,),
                    ).fetchone()
                    if row is not None:
                        rows.append(row)

            for row in rows:
                login = str(row["twitch_login"] if hasattr(row, "keys") else row[0]).strip().lower()
                is_live = row["is_live"] if hasattr(row, "keys") else row[1]
                user_id = str(row["twitch_user_id"] if hasattr(row, "keys") else row[2]).strip()
                if not login:
                    continue
                if bool(is_live):
                    continue
                offline_logins.append(login)
                if user_id:
                    offline_ids[login] = user_id
        except Exception:
            log.debug("Cleanup: konnte Live-Status nicht laden", exc_info=True)
            return

        if not offline_logins:
            return

        offline_id_set = set(offline_ids.values())
        unsubscribed = 0

        try:
            subs_result = chat_bot.fetch_eventsub_subscriptions()
            # TwitchIO liefert je nach Version ein awaitable, das einen HTTPAsyncIterator zurückgibt.
            if inspect.isawaitable(subs_result):
                subs_result = await subs_result

            subs_list = []

            async def _consume_async_iter(source) -> bool:
                if source is None:
                    return False
                if hasattr(source, "__aiter__"):
                    async for sub in source:
                        subs_list.append(sub)
                    return True
                if hasattr(source, "__anext__"):
                    while True:
                        try:
                            sub = await source.__anext__()
                        except StopAsyncIteration:
                            break
                        subs_list.append(sub)
                    return True
                return False

            if subs_result is None:
                log.warning("Cleanup: fetch_eventsub_subscriptions returned None")
            else:
                handled = await _consume_async_iter(subs_result)
                if not handled:
                    # TwitchIO 3.x gibt EventsubSubscriptions zurück – Einträge in .subscriptions
                    inner = getattr(subs_result, "subscriptions", None)
                    if inner is not None:
                        handled = await _consume_async_iter(inner)
                        if not handled:
                            try:
                                subs_list.extend(list(inner))
                            except TypeError:
                                log.warning(
                                    "Cleanup: fetch_eventsub_subscriptions returned unexpected subscriptions type: %s",
                                    type(inner),
                                )
                    else:
                        try:
                            subs_list.extend(list(subs_result))
                        except TypeError:
                            log.warning(
                                "Cleanup: fetch_eventsub_subscriptions returned unexpected type: %s",
                                type(subs_result),
                            )

            for sub in subs_list:
                try:
                    sub_type = getattr(sub, "type", "") or getattr(sub, "subscription_type", "")
                    if sub_type != "channel.chat.message":
                        continue
                    condition = getattr(sub, "condition", None)
                    broadcaster_id = ""
                    if isinstance(condition, dict):
                        broadcaster_id = str(
                            condition.get("broadcaster_user_id")
                            or condition.get("broadcaster_id")
                            or ""
                        ).strip()
                    else:
                        broadcaster_id = str(
                            getattr(condition, "broadcaster_user_id", "")
                            or getattr(condition, "broadcaster_id", "")
                            or ""
                        ).strip()

                    if not broadcaster_id or broadcaster_id not in offline_id_set:
                        continue

                    sub_id = (
                        getattr(sub, "id", None)
                        or getattr(sub, "subscription_id", None)
                        or getattr(sub, "uuid", None)
                    )
                    if sub_id:
                        try:
                            await chat_bot.delete_eventsub_subscription(sub_id)
                            unsubscribed += 1
                        except Exception:
                            log.debug(
                                "Cleanup: konnte EventSub-Subscription %s nicht löschen",
                                sub_id,
                                exc_info=True,
                            )
                except Exception:
                    log.debug(
                        "Cleanup: Fehler beim Prüfen von EventSub-Subscriptions",
                        exc_info=True,
                    )
        except Exception:
            log.debug("Cleanup: konnte EventSub-Subscriptions nicht abrufen", exc_info=True)

        for login in offline_logins:
            chat_bot._monitored_streamers.discard(login)

        log.info(
            "Cleanup: %d offline Channels entfernt (unsubscribed: %d)",
            len(offline_logins),
            unsubscribed,
        )

    async def _should_start_chat_adapter(self) -> bool:
        """Decide whether to start the TwitchIO web adapter (avoids port collisions)."""
        override = (os.getenv("TWITCH_CHAT_ADAPTER") or "").strip().lower()
        if override in {"0", "false", "off", "no"}:
            log.info("Twitch Chat Web Adapter deaktiviert per TWITCH_CHAT_ADAPTER.")
            return False

        bot = self._twitch_chat_bot
        adapter = getattr(bot, "adapter", None)
        if adapter is None:
            return False

        host = getattr(adapter, "_host", "localhost")
        port_raw = getattr(adapter, "_port", 4343)
        try:
            port = int(port_raw)
        except Exception:
            port = 4343

        can_bind, error = await self._can_bind_port_async(host, port)
        if not can_bind:
            log.warning(
                "Twitch Chat Web Adapter Port %s auf %s bereits belegt (%s) - starte ohne Adapter (Webhooks/OAuth ausgeschaltet).",
                port,
                host,
                error or "address already in use",
            )
        return can_bind

    @staticmethod
    async def _can_bind_port_async(host: str, port: int) -> tuple[bool, str | None]:
        """Try binding to the given host/port with retries; return False if something is already listening."""
        max_retries = 5
        retry_delay = 0.5
        last_error: str | None = None

        for attempt in range(max_retries):
            try:
                families = [
                    info[0] for info in socket.getaddrinfo(host, port, type=socket.SOCK_STREAM)
                ]
            except Exception as exc:
                families = [socket.AF_INET]
                last_error = str(exc)

            success = False
            seen = set()
            for family in families or [socket.AF_INET]:
                if family in seen:
                    continue
                seen.add(family)
                try:
                    with socket.socket(family, socket.SOCK_STREAM) as sock:
                        sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
                        sock.bind((host, port))
                    success = True
                    break
                except OSError as exc:
                    last_error = str(exc)
                    continue

            if success:
                return True, None

            if attempt < max_retries - 1:
                log.debug(
                    "Port %s:%s belegt, versuche es erneut in %ss... (Versuch %s/%s)",
                    host,
                    port,
                    retry_delay,
                    attempt + 1,
                    max_retries,
                )
                await asyncio.sleep(retry_delay)
                retry_delay *= 2
            else:
                break

        return False, last_error

    @staticmethod
    def _can_bind_port(host: str, port: int) -> tuple[bool, str | None]:
        """Synchronous version for compatibility (if needed), but prefers async version."""
        # For compatibility we keep the sync one but the async one should be used where possible
        try:
            families = [info[0] for info in socket.getaddrinfo(host, port, type=socket.SOCK_STREAM)]
        except Exception as exc:
            families = [socket.AF_INET]
            last_error = str(exc)

        seen = set()
        for family in families or [socket.AF_INET]:
            if family in seen:
                continue
            seen.add(family)
            try:
                with socket.socket(family, socket.SOCK_STREAM) as sock:
                    sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
                    sock.bind((host, port))
                return True, None
            except OSError as exc:
                last_error = str(exc)
                continue
        return False, last_error

    async def _send_alert_message(self, message: str) -> None:
        """Send a warning to the configured alert channel (Discord)."""
        channel_id = int(getattr(self, "_alert_channel_id", 0) or 0)
        if not channel_id:
            return
        content = f"{self._alert_mention} {message}".strip() if self._alert_mention else message
        try:
            channel = self.bot.get_channel(channel_id)
            if channel is None:
                channel = await self.bot.fetch_channel(channel_id)
            if channel is None or not hasattr(channel, "send"):
                return
            await channel.send(content=content)
        except (Forbidden, HTTPException):
            log.debug("Konnte Alert nicht senden (Discord-Zugriff verweigert).", exc_info=True)
        except Exception:
            log.debug("Konnte Alert nicht senden.", exc_info=True)

    # -------------------------------------------------------
    # Utils
    # -------------------------------------------------------
    @staticmethod
    def _normalize_login(raw: str) -> str:
        login = (raw or "").strip()
        if not login:
            return ""
        login = login.split("?")[0].split("#")[0].strip()
        lowered = login.lower()
        if "twitch.tv" in lowered:
            if "//" not in login:
                login = f"https://{login}"
            try:
                parsed = urlparse(login)
            except Exception:
                return ""
            path = (parsed.path or "").strip("/")
            if path:
                login = path.split("/")[0]
            else:
                return ""
        login = login.strip().lstrip("@")
        login = re.sub(r"[^a-z0-9_]", "", login.lower())
        return login

    @staticmethod
    def _parse_language_filters(raw: str | None) -> list[str] | None:
        """Allow TWITCH_LANGUAGE to define multiple comma/whitespace separated codes."""
        value = (raw or "").strip()
        if not value:
            return None
        tokens = [tok.strip().lower() for tok in re.split(r"[,\s;|]+", value) if tok.strip()]
        if not tokens:
            return None
        if any(tok in {"*", "any", "all"} for tok in tokens):
            return None
        seen: list[str] = []
        for tok in tokens:
            if tok not in seen:
                seen.append(tok)
        return seen or None

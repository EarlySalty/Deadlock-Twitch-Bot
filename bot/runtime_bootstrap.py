"""Runtime/bootstrap orchestration for the Twitch cog."""

from __future__ import annotations

import asyncio
import contextlib
import ipaddress
import os
from typing import Any
from urllib.parse import urlparse

from .api.token_manager import TwitchBotTokenManager
from .api.twitch_api import TwitchAPI
from .chat.bot import load_bot_tokens
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
from .raid import partner_scores as partner_raid_scores
from .raid.manager import RaidBot
from .reload_manager import LoopSpec, SubsystemDef, TwitchReloadManager
from .secret_store import load_secret_value
from .storage import pg as storage_pg


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


class TwitchRuntimeBootstrap:
    """Own the cog's runtime setup stages explicitly."""

    def __init__(self, cog: Any) -> None:
        self._cog = cog

    def configure_runtime(self) -> None:
        cog = self._cog

        twitch_keys = [key for key in os.environ.keys() if key.startswith("TWITCH_")]
        log.debug("Detected Twitch Keys in ENV: %s", ", ".join(twitch_keys))

        cog.client_id = load_secret_value("TWITCH_CLIENT_ID")
        cog.client_secret = load_secret_value("TWITCH_CLIENT_SECRET")
        cog._twitch_bot_client_id = load_secret_value("TWITCH_BOT_CLIENT_ID") or cog.client_id

        bot_secret_env = load_secret_value("TWITCH_BOT_CLIENT_SECRET")
        if bot_secret_env:
            cog._twitch_bot_secret = bot_secret_env
        elif cog._twitch_bot_client_id == cog.client_id:
            cog._twitch_bot_secret = cog.client_secret
        else:
            cog._twitch_bot_secret = ""

        cog.api = None
        cog._web = None
        cog._web_app = None
        cog._category_id = None
        cog._language_filters = cog._parse_language_filters(TWITCH_LANGUAGE)
        cog._tick_count = 0
        cog._log_every_n = max(1, int(TWITCH_LOG_EVERY_N_TICKS or 1))
        cog._category_sample_limit = max(50, int(TWITCH_CATEGORY_SAMPLE_LIMIT or 400))
        cog._poll_interval_seconds = max(5, min(3600, int(POLL_INTERVAL_SECONDS or 15)))
        cog._poll_interval_resync_interval_seconds = 60.0
        cog._poll_interval_last_sync_monotonic = 0.0
        cog._poll_interval_last_error_log_at = 0.0
        cog._poll_interval_last_invalid_value = None
        cog._poll_interval_settings_table = "twitch_global_settings"
        cog._poll_interval_settings_key = "poll_interval_seconds"
        cog._admin_polling_interval_seconds = cog._poll_interval_seconds
        cog._active_sessions = {}
        cog._notify_channel_id = int(TWITCH_NOTIFY_CHANNEL_ID or 0)
        cog._alert_channel_id = int(TWITCH_ALERT_CHANNEL_ID or 0)
        cog._alert_mention = TWITCH_ALERT_MENTION or ""
        cog._invite_codes = {}
        cog._twl_command = None
        cog._target_game_name = (TWITCH_TARGET_GAME_NAME or "").strip()
        cog._target_game_lower = cog._target_game_name.lower()
        cog.partner_raid_score_service = partner_raid_scores
        cog._managed_bg_tasks = set()
        cog._runtime_started = False
        cog._runtime_start_lock = asyncio.Lock()

        cog._dashboard_token = load_secret_value("TWITCH_DASHBOARD_TOKEN") or None
        cog._dashboard_noauth = _parse_env_bool(
            "TWITCH_DASHBOARD_NOAUTH",
            bool(TWITCH_DASHBOARD_NOAUTH),
        )
        env_dashboard_host = (os.getenv("TWITCH_DASHBOARD_HOST") or "").strip()
        default_dashboard_host = TWITCH_DASHBOARD_HOST or "127.0.0.1"
        cog._dashboard_host = env_dashboard_host or default_dashboard_host
        try:
            if ipaddress.ip_address(cog._dashboard_host).is_unspecified:
                log.warning(
                    "TWITCH_DASHBOARD_HOST resolves to an unspecified address; keep this behind auth/reverse proxy."
                )
        except ValueError:
            log.warning(
                "TWITCH_DASHBOARD_HOST is not a valid IP; using it as-is: %s",
                cog._dashboard_host,
            )
        cog._dashboard_port = _parse_env_int("TWITCH_DASHBOARD_PORT", int(TWITCH_DASHBOARD_PORT))
        embedded_env = (os.getenv("TWITCH_DASHBOARD_EMBEDDED", "") or "").strip().lower()
        cog._dashboard_embedded = embedded_env not in {"0", "false", "no", "off"}
        if not cog._dashboard_embedded:
            log.info(
                "TWITCH_DASHBOARD_EMBEDDED disabled - assuming external reverse proxy serves the dashboard"
            )
        cog._partner_dashboard_token = load_secret_value("TWITCH_PARTNER_TOKEN") or None
        cog._dashboard_auth_redirect_uri = (
            os.getenv("TWITCH_DASHBOARD_AUTH_REDIRECT_URI") or ""
        ).strip() or "https://twitch.earlysalty.com/twitch/auth/callback"
        cog._dashboard_session_ttl = max(
            6 * 3600,
            _parse_env_int("TWITCH_DASHBOARD_SESSION_TTL_SEC", 6 * 3600),
        )
        cog._legacy_stats_url = (os.getenv("TWITCH_LEGACY_STATS_URL") or "").strip() or None
        cog._required_marker_default = TWITCH_REQUIRED_DISCORD_MARKER or None
        cog._internal_api_runner = None
        cog._experimental_irc_lurker_channels = set(
            _parse_env_csv(
                "TWITCH_EXPERIMENTAL_IRC_LURKER_CHANNELS",
                default=("earlysalty",),
            )
        )
        cog._experimental_irc_lurker_enabled = False
        cog._irc_lurker_tracker = None

        cog._internal_api_token = (
            load_secret_value(
                "TWITCH_INTERNAL_API_TOKEN",
                prefer_env=True,
                allow_empty_env_override=True,
            )
            or None
        )
        env_internal_host = (os.getenv("TWITCH_INTERNAL_API_HOST") or "").strip()
        default_internal_host = TWITCH_INTERNAL_API_HOST or "127.0.0.1"
        cog._internal_api_host = env_internal_host or default_internal_host
        try:
            if ipaddress.ip_address(cog._internal_api_host).is_unspecified:
                log.warning(
                    "TWITCH_INTERNAL_API_HOST resolves to an unspecified address; keep it private."
                )
        except ValueError:
            log.warning(
                "TWITCH_INTERNAL_API_HOST is not a valid IP; using it as-is: %s",
                cog._internal_api_host,
            )
        cog._internal_api_port = _parse_env_int(
            "TWITCH_INTERNAL_API_PORT",
            int(TWITCH_INTERNAL_API_PORT),
        )

        cog._raid_bot = None
        cog._twitch_chat_bot = None
        cog._periodic_channel_join_task = None
        cog._twitch_bot_token = None
        cog._twitch_bot_refresh_token = None
        cog._bot_token_manager = None
        cog._raid_redirect_uri = ""

        cog.clip_manager = None
        cog.clip_fetcher = None
        cog.upload_worker = None
        cog._reload_manager = None

    def wire_runtime_dependencies(self) -> None:
        cog = self._cog

        # Runtime dependencies such as raid auth, token handling and social-media
        # workers touch PostgreSQL during construction, so storage must be ready
        # before we instantiate them.
        storage_pg.prepare_runtime_storage()

        cog._internal_api_runner = InternalApiRunner(
            host=cog._internal_api_host,
            port=cog._internal_api_port,
            token=cog._internal_api_token,
            add_cb=getattr(cog, "_dashboard_add", None),
            remove_cb=getattr(cog, "_dashboard_remove", None),
            list_cb=getattr(cog, "_dashboard_list", None),
            stats_cb=getattr(cog, "_dashboard_stats", None),
            verify_cb=getattr(cog, "_dashboard_verify", None),
            archive_cb=getattr(cog, "_dashboard_archive", None),
            discord_flag_cb=getattr(cog, "_dashboard_set_discord_flag", None),
            discord_profile_cb=getattr(cog, "_dashboard_save_discord_profile", None),
            streamer_analytics_cb=getattr(cog, "_dashboard_streamer_analytics_data", None),
            comparison_cb=getattr(cog, "_dashboard_comparison_stats", None),
            session_cb=getattr(cog, "_dashboard_session_detail", None),
            raid_auth_url_cb=getattr(cog, "_dashboard_raid_auth_url", None),
            raid_auth_state_cb=getattr(cog, "_integration_raid_auth_state", None),
            raid_block_state_cb=getattr(cog, "_integration_raid_block_state", None),
            raid_go_url_cb=getattr(cog, "_dashboard_raid_go_url", None),
            raid_requirements_cb=getattr(cog, "_dashboard_raid_requirements", None),
            raid_oauth_callback_cb=getattr(cog, "_dashboard_raid_oauth_callback", None),
            live_active_announcements_cb=getattr(
                cog,
                "_dashboard_live_active_announcements",
                None,
            ),
            live_link_click_cb=getattr(cog, "_dashboard_live_link_click", None),
            observability_snapshot_cb=getattr(
                cog,
                "_internal_observability_snapshot",
                None,
            ),
            chatters_debug_cb=getattr(cog, "_internal_chatters_debug", None),
        )

        webhook_secret = load_secret_value("TWITCH_WEBHOOK_SECRET")
        if webhook_secret:
            try:
                from .monitoring.eventsub_webhook import EventSubWebhookHandler

                cog._eventsub_webhook_handler = EventSubWebhookHandler(
                    secret=webhook_secret,
                    logger=log,
                )
                parsed_redirect = urlparse(cog._dashboard_auth_redirect_uri)
                cog._webhook_base_url = (
                    f"{parsed_redirect.scheme}://{parsed_redirect.netloc}"
                    if parsed_redirect.netloc
                    else None
                )
                cog._webhook_secret = webhook_secret
                log.debug(
                    "EventSub Webhook Handler initialisiert (base_url=%s)",
                    cog._webhook_base_url,
                )
            except Exception:
                log.exception("EventSub Webhook Handler konnte nicht initialisiert werden")
                cog._eventsub_webhook_handler = None
                cog._webhook_base_url = None
                cog._webhook_secret = None
        else:
            log.info(
                "TWITCH_WEBHOOK_SECRET nicht gesetzt – EventSub Webhook deaktiviert, "
                "WebSocket-Fallback wird verwendet."
            )
            cog._eventsub_webhook_handler = None
            cog._webhook_base_url = None
            cog._webhook_secret = None

        if not cog.client_id:
            log.error(
                "TWITCH_CLIENT_ID not configured; Twitch features will be limited or disabled."
            )
            cog.api = None
        elif not cog.client_secret:
            log.warning(
                "TWITCH_CLIENT_SECRET missing. API calls and Raids will fail, but Chat Bot might work."
            )
            cog.api = None
        else:
            cog.api = TwitchAPI(cog.client_id, cog.client_secret)

        bot_token, bot_refresh_token, _ = load_bot_tokens(log_missing=False)
        cog._twitch_bot_token = bot_token
        cog._twitch_bot_refresh_token = bot_refresh_token
        env_bot_client_id = os.getenv("TWITCH_BOT_CLIENT_ID", "").strip()
        cog._twitch_bot_client_id = env_bot_client_id or cog._twitch_bot_client_id or cog.client_id
        if not cog._twitch_bot_secret:
            env_bot_secret = os.getenv("TWITCH_BOT_CLIENT_SECRET", "").strip()
            if env_bot_secret:
                cog._twitch_bot_secret = env_bot_secret
            elif cog._twitch_bot_client_id == cog.client_id:
                cog._twitch_bot_secret = cog.client_secret
            else:
                cog._twitch_bot_secret = None
        if cog._twitch_bot_client_id:
            cog._bot_token_manager = TwitchBotTokenManager(
                cog._twitch_bot_client_id,
                (cog._twitch_bot_secret or cog.client_secret or ""),
            )

        cog._raid_redirect_uri = (
            os.getenv("TWITCH_RAID_REDIRECT_URI", "").strip() or TWITCH_RAID_REDIRECT_URI
        )

        if cog.api:
            try:
                session = cog.api.get_http_session()
                cog._raid_bot = RaidBot(
                    client_id=cog.client_id,
                    client_secret=cog.client_secret,
                    redirect_uri=cog._raid_redirect_uri,
                    session=session,
                )
                cog._raid_bot.partner_raid_score_service = partner_raid_scores
                cog._raid_bot.set_discord_bot(cog.bot)
                cog._raid_bot.set_cog(cog)
                log.debug("Raid-Bot initialisiert (redirect_uri: %s)", cog._raid_redirect_uri)
            except Exception:
                log.exception("Fehler beim Initialisieren des Raid-Bots")
                cog._raid_bot = None
        else:
            log.warning("Raid-Bot und Chat-Bot deaktiviert, da TWITCH_CLIENT_ID/SECRET fehlen.")

        if cog.api:
            from .social_media.clip_fetcher import ClipFetcher
            from .social_media.clip_manager import ClipManager
            from .social_media.upload_worker import UploadWorker

            cog.clip_manager = ClipManager(twitch_api=cog.api)
            cog.clip_fetcher = ClipFetcher(cog.bot, cog.api, cog.clip_manager)
            cog.upload_worker = UploadWorker(cog.bot, cog.clip_manager)
            log.info(
                "Social Media Clip Management initialized (ClipManager + ClipFetcher + UploadWorker)"
            )

        self._register_reload_manager()

    def _register_reload_manager(self) -> None:
        cog = self._cog
        cog._reload_manager = TwitchReloadManager(cog)
        cog._reload_manager.register(
            SubsystemDef(
                name="analytics",
                display_name="Analytics",
                modules=["bot.analytics.mixin"],
                loops=[
                    LoopSpec("collect_analytics_data"),
                    LoopSpec("collect_chatters_data"),
                    LoopSpec("compute_raid_retention"),
                ],
                hot_reloadable=True,
            )
        )
        cog._reload_manager.register(
            SubsystemDef(
                name="community",
                display_name="Community",
                modules=[
                    "bot.community.admin",
                    "bot.community.leaderboard",
                    "bot.community.partner_recruit",
                ],
                loops=[],
                hot_reloadable=True,
            )
        )
        cog._reload_manager.register(
            SubsystemDef(
                name="social",
                display_name="Social Media",
                modules=[
                    "bot.social_media.clip_fetcher",
                    "bot.social_media.clip_manager",
                    "bot.social_media.upload_worker",
                ],
                loops=[],
                hot_reloadable=True,
                teardown_hook="_reload_social_teardown",
                startup_hook="_reload_social_startup",
            )
        )
        cog._reload_manager.register(
            SubsystemDef(
                name="monitoring",
                display_name="Monitoring",
                modules=[
                    "bot.monitoring.monitoring",
                    "bot.monitoring.eventsub_mixin",
                    "bot.monitoring.sessions_mixin",
                    "bot.monitoring.embeds_mixin",
                ],
                loops=[
                    LoopSpec("poll_streams"),
                    LoopSpec("invites_refresh"),
                ],
                hot_reloadable=False,
            )
        )
        cog._reload_manager.register(
            SubsystemDef(
                name="chat",
                display_name="Chat Bot",
                modules=["bot.chat.bot", "bot.chat.commands", "bot.chat.connection"],
                loops=[],
                hot_reloadable=False,
            )
        )
        cog._reload_manager.register(
            SubsystemDef(
                name="dashboard",
                display_name="Dashboard",
                modules=["bot.dashboard.mixin", "bot.dashboard.server_v2", "bot.dashboard.routes_mixin"],
                loops=[],
                hot_reloadable=False,
            )
        )
        cog._reload_manager.register(
            SubsystemDef(
                name="raid",
                display_name="Raid",
                modules=["bot.raid.mixin", "bot.raid.manager", "bot.raid.commands", "bot.raid.auth"],
                loops=[],
                hot_reloadable=False,
            )
        )
        log.debug(
            "Subsystem reload manager ready (%d subsystems)",
            len(cog._reload_manager.get_all_names()),
        )

    async def start_runtime(self) -> None:
        cog = self._cog
        start_lock = getattr(cog, "_runtime_start_lock", None)
        if start_lock is None:
            start_lock = asyncio.Lock()
            cog._runtime_start_lock = start_lock

        async with start_lock:
            if getattr(cog, "_runtime_started", False):
                return

            started_poll_streams = False
            try:
                if cog.api:
                    await asyncio.to_thread(storage_pg.prepare_runtime_storage)
                    cog._spawn_bg_task(cog._startup_db_warmup(), "twitch.db_warmup")

                if cog._raid_bot and cog._twitch_bot_token:
                    cog._spawn_bg_task(cog._init_twitch_chat_bot(), "twitch.chat_bot")
                elif cog.api:
                    log.info(
                        "Twitch Chat Bot nicht verfuegbar (kein Token gesetzt). "
                        "Setze TWITCH_BOT_TOKEN oder TWITCH_BOT_TOKEN_FILE, um den Chat-Bot zu aktivieren."
                    )

                sync_poll_interval = getattr(cog, "_sync_poll_interval_from_storage", None)
                if callable(sync_poll_interval):
                    try:
                        sync_poll_interval(force=True, startup=True)
                    except Exception:
                        log.debug(
                            "Persistiertes Polling-Intervall konnte vor Loop-Start nicht geladen werden",
                            exc_info=True,
                        )

                if not cog.poll_streams.is_running():
                    cog.poll_streams.start()
                    started_poll_streams = True

                cog._spawn_bg_task(cog._ensure_category_id(), "twitch.ensure_category_id")
                cog._spawn_bg_task(cog._load_invite_codes_from_db(), "twitch.load_invites")
                cog._spawn_bg_task(cog._start_internal_api(), "twitch.start_internal_api")
                if cog._dashboard_embedded:
                    cog._spawn_bg_task(cog._start_dashboard(), "twitch.start_dashboard")
                else:
                    log.info("Skipping internal Twitch dashboard server startup")
                cog._spawn_bg_task(cog._refresh_all_invites(), "twitch.refresh_all_invites")
                cog._spawn_bg_task(cog._start_eventsub_listener(), "twitch.eventsub")
                if cog.api:
                    cog._spawn_bg_task(cog._sync_missing_user_ids(), "twitch.sync_user_ids")
                    cog._spawn_bg_task(cog._scout_deadlock_channels(), "twitch.scout_deadlock")
                cog._spawn_bg_task(cog._register_views_after_ready(), "twitch.views_warmup")
            except Exception:
                if started_poll_streams and cog.poll_streams.is_running():
                    with contextlib.suppress(Exception):
                        cog.poll_streams.cancel()
                cancel_tasks = getattr(cog, "_cancel_managed_bg_tasks", None)
                if callable(cancel_tasks):
                    with contextlib.suppress(Exception):
                        await cancel_tasks()
                raise
            else:
                cog._runtime_started = True

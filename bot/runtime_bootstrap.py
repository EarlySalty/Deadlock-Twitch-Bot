"""Runtime/bootstrap orchestration for the Twitch cog."""

from __future__ import annotations

import asyncio
import contextlib
import ipaddress
import os
import socket
from typing import Any
from urllib.parse import urlparse

from .api.token_manager import TwitchBotTokenManager
from .api.twitch_api import TwitchAPI
from .chat.bot import load_bot_tokens
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
from .internal_api import InternalApiCallbacks, InternalApiRunner
from .raid import partner_scores as partner_raid_scores
from .raid.manager import RaidBot
from .reload_manager import LoopSpec, SubsystemDef, TwitchReloadManager
from .runtime.dashboard_runtime import (
    DashboardRuntimeContainer,
)
from .runtime.contracts import ensure_bot_runtime_container
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


class BotRuntimeBootstrap:
    """Own the bot runtime setup stages explicitly."""

    def __init__(self, cog: Any) -> None:
        self._cog = cog

    def configure_runtime(self) -> None:
        cog = self._cog
        runtime = ensure_bot_runtime_container(cog)
        config = runtime.config
        state = runtime.state
        services = runtime.services

        twitch_keys = [key for key in os.environ.keys() if key.startswith("TWITCH_")]
        log.debug("Detected Twitch Keys in ENV: %s", ", ".join(twitch_keys))

        config.client_id = load_secret_value("TWITCH_CLIENT_ID")
        config.client_secret = load_secret_value("TWITCH_CLIENT_SECRET")
        config.twitch_bot_client_id = (
            load_secret_value("TWITCH_BOT_CLIENT_ID") or config.client_id
        )

        bot_secret_env = load_secret_value("TWITCH_BOT_CLIENT_SECRET")
        if bot_secret_env:
            config.twitch_bot_secret = bot_secret_env
        elif config.twitch_bot_client_id == config.client_id:
            config.twitch_bot_secret = config.client_secret
        else:
            config.twitch_bot_secret = ""

        services.api = None
        state.category_id = None
        config.language_filters = cog._parse_language_filters(TWITCH_LANGUAGE)
        state.tick_count = 0
        config.log_every_n = max(1, int(TWITCH_LOG_EVERY_N_TICKS or 1))
        config.category_sample_limit = max(50, int(TWITCH_CATEGORY_SAMPLE_LIMIT or 400))
        config.poll_interval_seconds = max(5, min(3600, int(POLL_INTERVAL_SECONDS or 15)))
        config.poll_interval_resync_interval_seconds = 60.0
        state.poll_interval_last_sync_monotonic = 0.0
        state.poll_interval_last_error_log_at = 0.0
        state.poll_interval_last_invalid_value = None
        config.poll_interval_settings_table = "twitch_global_settings"
        config.poll_interval_settings_key = "poll_interval_seconds"
        state.admin_polling_interval_seconds = config.poll_interval_seconds
        state.active_sessions = {}
        config.notify_channel_id = int(TWITCH_NOTIFY_CHANNEL_ID or 0)
        config.alert_channel_id = int(TWITCH_ALERT_CHANNEL_ID or 0)
        config.alert_mention = TWITCH_ALERT_MENTION or ""
        state.invite_codes = {}
        services.twl_command = None
        config.target_game_name = (TWITCH_TARGET_GAME_NAME or "").strip()
        config.target_game_lower = config.target_game_name.lower()
        services.partner_raid_score_service = partner_raid_scores
        state.managed_bg_tasks = set()
        state.runtime_started = False
        state.runtime_start_lock = asyncio.Lock()
        state.runtime_stop_lock = state.runtime_start_lock

        self._configure_dashboard_compat_attrs()
        config.experimental_irc_lurker_channels = set(
            _parse_env_csv(
                "TWITCH_EXPERIMENTAL_IRC_LURKER_CHANNELS",
                default=("earlysalty",),
            )
        )
        state.experimental_irc_lurker_enabled = False
        state.irc_lurker_tracker = None

        config.internal_api_token = (
            load_secret_value(
                "TWITCH_INTERNAL_API_TOKEN",
                prefer_env=True,
                allow_empty_env_override=True,
            )
            or None
        )
        env_internal_host = (os.getenv("TWITCH_INTERNAL_API_HOST") or "").strip()
        default_internal_host = TWITCH_INTERNAL_API_HOST or "127.0.0.1"
        config.internal_api_host = env_internal_host or default_internal_host
        try:
            if ipaddress.ip_address(config.internal_api_host).is_unspecified:
                log.warning(
                    "TWITCH_INTERNAL_API_HOST resolves to an unspecified address; keep it private."
                )
        except ValueError:
            log.warning(
                "TWITCH_INTERNAL_API_HOST is not a valid IP; using it as-is: %s",
                config.internal_api_host,
            )
        config.internal_api_port = _parse_env_int(
            "TWITCH_INTERNAL_API_PORT",
            int(TWITCH_INTERNAL_API_PORT),
        )

        services.raid_bot = None
        services.twitch_chat_bot = None
        state.periodic_channel_join_task = None
        services.twitch_bot_token = None
        services.twitch_bot_refresh_token = None
        services.bot_token_manager = None
        config.raid_redirect_uri = ""

        services.clip_manager = None
        services.clip_fetcher = None
        services.upload_worker = None
        services.social_media_approval_worker = None
        services.social_media_retention_worker = None
        services.social_media_enrichment_worker = None
        services.social_media_insights_worker = None
        services.social_media_report_dispatcher = None
        services.reload_manager = None

    def wire_runtime_dependencies(self) -> None:
        cog = self._cog
        runtime = ensure_bot_runtime_container(cog)
        config = runtime.config
        services = runtime.services

        # Runtime dependencies such as raid auth, token handling and social-media
        # workers touch PostgreSQL during construction, so storage must be ready
        # before we instantiate them.
        storage_pg.prepare_runtime_storage()

        services.internal_api_runner = InternalApiRunner(
            host=config.internal_api_host,
            port=config.internal_api_port,
            token=config.internal_api_token,
            callbacks=InternalApiCallbacks(
                add=getattr(cog, "_dashboard_add", None),
                remove=getattr(cog, "_dashboard_remove", None),
                streamers=getattr(cog, "_dashboard_list", None),
                stats=getattr(cog, "_dashboard_stats", None),
                verify=getattr(cog, "_dashboard_verify", None),
                archive=getattr(cog, "_dashboard_archive", None),
                discord_flag=getattr(cog, "_dashboard_set_discord_flag", None),
                discord_profile=getattr(cog, "_dashboard_save_discord_profile", None),
                streamer_analytics=getattr(cog, "_dashboard_streamer_analytics_data", None),
                comparison=getattr(cog, "_dashboard_comparison_stats", None),
                session=getattr(cog, "_dashboard_session_detail", None),
                raid_auth_url=getattr(cog, "_dashboard_raid_auth_url", None),
                raid_auth_state=getattr(cog, "_integration_raid_auth_state", None),
                raid_block_state=getattr(cog, "_integration_raid_block_state", None),
                raid_go_url=getattr(cog, "_dashboard_raid_go_url", None),
                raid_requirements=getattr(cog, "_dashboard_raid_requirements", None),
                raid_oauth_callback=getattr(cog, "_dashboard_raid_oauth_callback", None),
                live_active_announcements=getattr(
                    cog,
                    "_dashboard_live_active_announcements",
                    None,
                ),
                live_link_click=getattr(cog, "_dashboard_live_link_click", None),
                observability_snapshot=getattr(
                    cog,
                    "_internal_observability_snapshot",
                    None,
                ),
                chatters_debug=getattr(cog, "_internal_chatters_debug", None),
                eventsub_dispatch=getattr(cog, "_internal_eventsub_dispatch", None),
                eventsub_processing_debug=getattr(
                    cog,
                    "_internal_eventsub_processing_debug",
                    None,
                ),
                eventsub_processing_requeue=getattr(
                    cog,
                    "_internal_eventsub_processing_requeue",
                    None,
                ),
            ),
        )

        webhook_secret = load_secret_value("TWITCH_WEBHOOK_SECRET")
        if webhook_secret:
            try:
                from .monitoring.eventsub_webhook import EventSubWebhookHandler
                from .monitoring.eventsub_state_store import EventSubStateStore

                services.eventsub_webhook_handler = EventSubWebhookHandler(
                    secret=webhook_secret,
                    logger=log,
                    synchronous_notifications=True,
                    state_store=EventSubStateStore(logger=log),
                )
                parsed_redirect = urlparse(
                    str(getattr(cog, "_dashboard_auth_redirect_uri", "") or "")
                )
                services.webhook_base_url = (
                    f"{parsed_redirect.scheme}://{parsed_redirect.netloc}"
                    if parsed_redirect.netloc
                    else None
                )
                services.webhook_secret = webhook_secret
                log.debug(
                    "EventSub Webhook Handler initialisiert (base_url=%s)",
                    services.webhook_base_url,
                )
            except Exception:
                log.exception("EventSub Webhook Handler konnte nicht initialisiert werden")
                services.eventsub_webhook_handler = None
                services.webhook_base_url = None
                services.webhook_secret = None
        else:
            log.info(
                "TWITCH_WEBHOOK_SECRET nicht gesetzt – EventSub Webhook deaktiviert, "
                "WebSocket-Fallback wird verwendet."
            )
            services.eventsub_webhook_handler = None
            services.webhook_base_url = None
            services.webhook_secret = None

        if not config.client_id:
            log.error(
                "TWITCH_CLIENT_ID not configured; Twitch features will be limited or disabled."
            )
            services.api = None
        elif not config.client_secret:
            log.warning(
                "TWITCH_CLIENT_SECRET missing. API calls and Raids will fail, but Chat Bot might work."
            )
            services.api = None
        else:
            services.api = TwitchAPI(config.client_id, config.client_secret)

        bot_token, bot_refresh_token, _ = load_bot_tokens(log_missing=False)
        services.twitch_bot_token = bot_token
        services.twitch_bot_refresh_token = bot_refresh_token
        env_bot_client_id = os.getenv("TWITCH_BOT_CLIENT_ID", "").strip()
        config.twitch_bot_client_id = env_bot_client_id or config.twitch_bot_client_id or config.client_id
        if not config.twitch_bot_secret:
            env_bot_secret = os.getenv("TWITCH_BOT_CLIENT_SECRET", "").strip()
            if env_bot_secret:
                config.twitch_bot_secret = env_bot_secret
            elif config.twitch_bot_client_id == config.client_id:
                config.twitch_bot_secret = config.client_secret
            else:
                config.twitch_bot_secret = None
        if config.twitch_bot_client_id:
            services.bot_token_manager = TwitchBotTokenManager(
                config.twitch_bot_client_id,
                (config.twitch_bot_secret or config.client_secret or ""),
            )

        config.raid_redirect_uri = (
            os.getenv("TWITCH_RAID_REDIRECT_URI", "").strip() or TWITCH_RAID_REDIRECT_URI
        )

        if services.api:
            try:
                session = services.api.get_http_session()
                services.raid_bot = RaidBot(
                    client_id=config.client_id,
                    client_secret=config.client_secret,
                    redirect_uri=config.raid_redirect_uri,
                    session=session,
                )
                services.raid_bot.partner_raid_score_service = partner_raid_scores
                services.raid_bot.set_discord_bot(cog.bot)
                services.raid_bot.set_cog(cog)
                cleanup_task = services.raid_bot.start()
                if cleanup_task is None:
                    raise RuntimeError("RaidBot lifecycle start failed")
                log.debug("Raid-Bot initialisiert (redirect_uri: %s)", config.raid_redirect_uri)
            except Exception:
                log.exception("Fehler beim Initialisieren des Raid-Bots")
                services.raid_bot = None
        else:
            log.warning("Raid-Bot und Chat-Bot deaktiviert, da TWITCH_CLIENT_ID/SECRET fehlen.")

        self._ensure_social_media_workers()

        self._register_reload_manager()

    def _configure_dashboard_compat_attrs(self) -> None:
        """Populate legacy dashboard attributes until callers finish migrating."""

        cog = self._cog
        setattr(cog, "_dashboard_token", load_secret_value("TWITCH_DASHBOARD_TOKEN") or None)
        setattr(
            cog,
            "_dashboard_noauth",
            _parse_env_bool(
                "TWITCH_DASHBOARD_NOAUTH",
                bool(TWITCH_DASHBOARD_NOAUTH),
            ),
        )
        env_dashboard_host = (os.getenv("TWITCH_DASHBOARD_HOST") or "").strip()
        default_dashboard_host = TWITCH_DASHBOARD_HOST or "127.0.0.1"
        setattr(cog, "_dashboard_host", env_dashboard_host or default_dashboard_host)
        try:
            if ipaddress.ip_address(getattr(cog, "_dashboard_host")).is_unspecified:
                log.warning(
                    "TWITCH_DASHBOARD_HOST resolves to an unspecified address; keep this behind auth/reverse proxy."
                )
        except ValueError:
            log.warning(
                "TWITCH_DASHBOARD_HOST is not a valid IP; using it as-is: %s",
                getattr(cog, "_dashboard_host"),
            )
        setattr(
            cog,
            "_dashboard_port",
            _parse_env_int("TWITCH_DASHBOARD_PORT", int(TWITCH_DASHBOARD_PORT)),
        )
        embedded_env = (os.getenv("TWITCH_DASHBOARD_EMBEDDED", "") or "").strip().lower()
        setattr(cog, "_dashboard_embedded", embedded_env not in {"0", "false", "no", "off"})
        if not bool(getattr(cog, "_dashboard_embedded", True)):
            log.info(
                "TWITCH_DASHBOARD_EMBEDDED disabled - assuming external reverse proxy serves the dashboard"
            )
        setattr(cog, "_partner_dashboard_token", load_secret_value("TWITCH_PARTNER_TOKEN") or None)
        setattr(
            cog,
            "_dashboard_auth_redirect_uri",
            (os.getenv("TWITCH_DASHBOARD_AUTH_REDIRECT_URI") or "").strip()
            or "https://deutsche-deadlock-community.de/callback/twitch",
        )
        setattr(
            cog,
            "_dashboard_session_ttl",
            max(6 * 3600, _parse_env_int("TWITCH_DASHBOARD_SESSION_TTL_SEC", 6 * 3600)),
        )
        setattr(
            cog,
            "_legacy_stats_url",
            (os.getenv("TWITCH_LEGACY_STATS_URL") or "").strip() or None,
        )
        setattr(cog, "_required_marker_default", TWITCH_REQUIRED_DISCORD_MARKER or None)

    def _ensure_social_media_workers(self) -> None:
        cog = self._cog
        services = ensure_bot_runtime_container(cog).services
        if not services.api:
            return
        if (
            services.clip_manager is not None
            and services.clip_fetcher is not None
            and services.upload_worker is not None
            and services.social_media_approval_worker is not None
            and services.social_media_retention_worker is not None
            and services.social_media_enrichment_worker is not None
            and services.social_media_insights_worker is not None
            and services.social_media_report_dispatcher is not None
        ):
            return

        from .social_media.analytics.insights_worker import SocialMediaInsightsWorker
        from .social_media.analytics.report_dispatcher import SocialMediaReportDispatcher
        from .social_media.approval_worker import SocialMediaApprovalWorker
        from .social_media.clip_fetcher import ClipFetcher
        from .social_media.clip_manager import ClipManager
        from .social_media.enrichment_worker import SocialMediaEnrichmentWorker
        from .social_media.retention_worker import SocialMediaRetentionWorker
        from .social_media.upload_worker import UploadWorker

        if services.clip_manager is None:
            services.clip_manager = ClipManager(twitch_api=services.api)
        if services.clip_fetcher is None:
            services.clip_fetcher = ClipFetcher(cog.bot, services.api, services.clip_manager)
        if services.upload_worker is None:
            services.upload_worker = UploadWorker(cog.bot, services.clip_manager)
        if services.social_media_approval_worker is None:
            services.social_media_approval_worker = SocialMediaApprovalWorker(
                cog.bot,
                services.clip_manager,
            )
        if services.social_media_retention_worker is None:
            services.social_media_retention_worker = SocialMediaRetentionWorker(cog.bot)
        if services.social_media_enrichment_worker is None:
            services.social_media_enrichment_worker = SocialMediaEnrichmentWorker(cog.bot)
        if services.social_media_insights_worker is None:
            services.social_media_insights_worker = SocialMediaInsightsWorker(cog.bot)
        if services.social_media_report_dispatcher is None:
            services.social_media_report_dispatcher = SocialMediaReportDispatcher(cog.bot)
        log.info(
            "Social Media Clip Management initialized "
            "(ClipManager + ClipFetcher + UploadWorker + ApprovalWorker + "
            "RetentionWorker + EnrichmentWorker + InsightsWorker + ReportDispatcher)"
        )

    def _stop_social_media_workers(self) -> None:
        cog = self._cog
        services = ensure_bot_runtime_container(cog).services

        if services.clip_fetcher:
            try:
                services.clip_fetcher.cog_unload()
                log.debug("ClipFetcher gecancelt")
            except Exception:
                log.exception("Konnte ClipFetcher nicht canceln")
            finally:
                services.clip_fetcher = None

        if services.upload_worker:
            try:
                services.upload_worker.cog_unload()
                log.debug("UploadWorker gecancelt")
            except Exception:
                log.exception("Konnte UploadWorker nicht canceln")
            finally:
                services.upload_worker = None

        if services.social_media_approval_worker:
            try:
                services.social_media_approval_worker.cog_unload()
                log.debug("SocialMediaApprovalWorker gecancelt")
            except Exception:
                log.exception("Konnte SocialMediaApprovalWorker nicht canceln")
            finally:
                services.social_media_approval_worker = None

        if services.social_media_retention_worker:
            try:
                services.social_media_retention_worker.cog_unload()
                log.debug("SocialMediaRetentionWorker gecancelt")
            except Exception:
                log.exception("Konnte SocialMediaRetentionWorker nicht canceln")
            finally:
                services.social_media_retention_worker = None

        if services.social_media_enrichment_worker:
            try:
                services.social_media_enrichment_worker.cog_unload()
                log.debug("SocialMediaEnrichmentWorker gecancelt")
            except Exception:
                log.exception("Konnte SocialMediaEnrichmentWorker nicht canceln")
            finally:
                services.social_media_enrichment_worker = None

        if services.social_media_insights_worker:
            try:
                services.social_media_insights_worker.cog_unload()
                log.debug("SocialMediaInsightsWorker gecancelt")
            except Exception:
                log.exception("Konnte SocialMediaInsightsWorker nicht canceln")
            finally:
                services.social_media_insights_worker = None

        if services.social_media_report_dispatcher:
            try:
                services.social_media_report_dispatcher.cog_unload()
                log.debug("SocialMediaReportDispatcher gecancelt")
            except Exception:
                log.exception("Konnte SocialMediaReportDispatcher nicht canceln")
            finally:
                services.social_media_report_dispatcher = None

        services.clip_manager = None

    def _register_reload_manager(self) -> None:
        cog = self._cog
        services = ensure_bot_runtime_container(cog).services
        services.reload_manager = TwitchReloadManager(cog)
        services.reload_manager.register(
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
        services.reload_manager.register(
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
        services.reload_manager.register(
            SubsystemDef(
                name="social",
                display_name="Social Media",
                modules=[
                    "bot.social_media.clip_fetcher",
                    "bot.social_media.clip_manager",
                    "bot.social_media.upload_worker",
                    "bot.social_media.retention_worker",
                    "bot.social_media.enrichment_worker",
                    "bot.social_media.analytics",
                    "bot.social_media.analytics.insights_worker",
                    "bot.social_media.analytics.report_writer",
                    "bot.social_media.analytics.report_dispatcher",
                    "bot.social_media.enrichment",
                    "bot.social_media.transcription",
                    "bot.social_media.transcription.vocab",
                    "bot.social_media.transcription.correction",
                    "bot.social_media.transcription.whisper",
                    "bot.social_media.transcription.seed_vocab",
                    "bot.social_media.llm",
                    "bot.social_media.llm.dispatcher",
                    "bot.social_media.llm.ollama",
                    "bot.social_media.llm.minimax",
                    "bot.social_media.llm.claude_haiku",
                    "bot.social_media.llm.prompts",
                    "bot.social_media.settings",
                ],
                loops=[],
                hot_reloadable=True,
                teardown_hook="_reload_social_teardown",
                startup_hook="_reload_social_startup",
            )
        )
        services.reload_manager.register(
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
        services.reload_manager.register(
            SubsystemDef(
                name="chat",
                display_name="Chat Bot",
                modules=["bot.chat.bot", "bot.chat.commands", "bot.chat.connection"],
                loops=[],
                hot_reloadable=False,
            )
        )
        services.reload_manager.register(
            SubsystemDef(
                name="dashboard",
                display_name="Dashboard",
                modules=["bot.dashboard.mixin", "bot.dashboard.server_v2", "bot.dashboard.routes_mixin"],
                loops=[],
                hot_reloadable=False,
            )
        )
        services.reload_manager.register(
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
            len(services.reload_manager.get_all_names()),
        )

    def _runtime_lifecycle_lock(self) -> asyncio.Lock:
        cog = self._cog
        state = ensure_bot_runtime_container(cog).state
        lock = state.runtime_start_lock
        if lock is None:
            lock = state.runtime_stop_lock
        if lock is None:
            lock = asyncio.Lock()
        state.runtime_start_lock = lock
        state.runtime_stop_lock = lock
        return lock

    async def _can_bind_port_async(self, host: str, port: int) -> tuple[bool, str | None]:
        port_probe = getattr(self._cog, "_can_bind_port_async", None)
        if callable(port_probe):
            return await port_probe(host, port)

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

        return False, last_error

    async def _wait_for_port_release(self, *, host: str, port: int, component: str) -> None:
        await asyncio.sleep(2.0)
        for retry in range(10):
            can_bind, _ = await self._can_bind_port_async(host, port)
            if can_bind:
                log.info(
                    "%s Port %s:%s erfolgreich freigegeben",
                    component,
                    host,
                    port,
                )
                return
            if retry < 9:
                log.debug(
                    "Warte auf Port-Freigabe %s:%s... (%d/10)",
                    host,
                    port,
                    retry + 1,
                )
                await asyncio.sleep(1.0)
            else:
                log.warning(
                    "Port %s:%s nach 10s noch belegt – fahre trotzdem fort",
                    host,
                    port,
                )

    async def _cleanup_runtime_components(
        self,
        *,
        wait_for_port_release: bool,
        full_shutdown: bool,
    ) -> None:
        cog = self._cog
        cog._runtime_started = False

        stop_highlight_clipper = getattr(cog, "_hc_stop", None)
        if callable(stop_highlight_clipper):
            try:
                await stop_highlight_clipper()
            except Exception:
                log.exception("Konnte HighlightClipper nicht stoppen")

        try:
            await cog._cancel_managed_bg_tasks()
        except Exception:
            log.exception("Konnte verwaltete Background-Tasks nicht stoppen")

        loops = (cog.poll_streams, cog.invites_refresh)
        for lp in loops:
            try:
                if lp.is_running():
                    lp.cancel()
                    log.debug("Loop gecancelt: %r", lp)
            except Exception:
                log.exception("Konnte Loop nicht canceln: %r", lp)

        self._stop_social_media_workers()

        try:
            await cog._cancel_periodic_channel_join_task()
        except Exception:
            log.exception("Konnte periodic channel maintenance task nicht stoppen")

        if cog._irc_lurker_tracker:
            try:
                await cog._irc_lurker_tracker.stop()
                log.debug("Experimental IRC Lurker Tracker gestoppt")
            except Exception:
                log.exception("Konnte Experimental IRC Lurker Tracker nicht stoppen")
            finally:
                cog._irc_lurker_tracker = None

        log.debug("EventSub Webhook: kein expliziter Teardown nötig")

        chat_bot = getattr(cog, "_twitch_chat_bot", None)
        if chat_bot:
            log.info("Beende Twitch Chat Bot...")
            try:
                if hasattr(chat_bot, "close"):
                    await chat_bot.close()
                    log.debug("Chat Bot close() abgeschlossen")

                if wait_for_port_release:
                    adapter = getattr(chat_bot, "adapter", None)
                    if adapter:
                        adapter_host = getattr(adapter, "_host", "127.0.0.1")
                        adapter_port = int(getattr(adapter, "_port", 4343))
                        await self._wait_for_port_release(
                            host=adapter_host,
                            port=adapter_port,
                            component="Chat Bot Adapter",
                        )

                log.info("Twitch Chat Bot beendet")
            except Exception:
                log.exception("Twitch Chat Bot shutdown fehlgeschlagen")
            finally:
                cog._twitch_chat_bot = None

        if full_shutdown and cog._bot_token_manager:
            try:
                await cog._bot_token_manager.cleanup()
                log.debug("Bot Token Manager cleanup abgeschlossen")
            except Exception:
                log.exception("Twitch Bot Token Manager shutdown fehlgeschlagen")
            finally:
                cog._bot_token_manager = None

        if getattr(cog, "_web", None):
            log.info("Stoppe Twitch Dashboard...")
            try:
                await cog._stop_dashboard()
                if wait_for_port_release:
                    dashboard_host = getattr(
                        cog,
                        "_dashboard_host",
                        TWITCH_DASHBOARD_HOST or "127.0.0.1",
                    )
                    dashboard_port_raw = getattr(cog, "_dashboard_port", TWITCH_DASHBOARD_PORT)
                    try:
                        dashboard_port = int(dashboard_port_raw)
                    except Exception:
                        dashboard_port = int(TWITCH_DASHBOARD_PORT)
                    await self._wait_for_port_release(
                        host=dashboard_host,
                        port=dashboard_port,
                        component="Dashboard",
                    )
                log.info("Twitch Dashboard gestoppt")
            except Exception:
                log.exception("Dashboard shutdown fehlgeschlagen")

        runner = cog._internal_api_runner
        if runner and runner.is_running:
            log.info("Stoppe interne Twitch API...")
            try:
                await cog._stop_internal_api()
            except Exception:
                log.exception("Internal API shutdown fehlgeschlagen")
        if full_shutdown and runner is not None:
            cog._internal_api_runner = None

        if full_shutdown and cog._raid_bot:
            try:
                await cog._raid_bot.cleanup()
                log.debug("RaidBot cleanup abgeschlossen")
            except Exception:
                log.exception("RaidBot cleanup fehlgeschlagen")
            finally:
                cog._raid_bot = None

        if full_shutdown and cog.api is not None:
            log.info("Schließe Twitch API Session...")
            try:
                if wait_for_port_release:
                    await asyncio.sleep(1.0)
                await cog.api.aclose()
                log.info("Twitch API Session geschlossen")
            except asyncio.CancelledError as exc:
                log.debug("Schließen der TwitchAPI-Session abgebrochen: %s", exc)
                raise
            except Exception:
                log.exception("TwitchAPI-Session konnte nicht geschlossen werden")
            finally:
                cog.api = None

        if full_shutdown:
            try:
                if cog._twl_command is not None:
                    existing = cog.bot.get_command(cog._twl_command.name)
                    if existing is cog._twl_command:
                        cog.bot.remove_command(cog._twl_command.name)
                        log.debug("!twl Command deregistriert")
            except Exception:
                log.exception("Konnte !twl-Command nicht deregistrieren")
            finally:
                cog._twl_command = None

    async def start_runtime(self) -> None:
        cog = self._cog
        async with self._runtime_lifecycle_lock():
            if getattr(cog, "_runtime_started", False):
                return

            try:
                if cog.api:
                    await asyncio.to_thread(storage_pg.prepare_runtime_storage)
                    self._ensure_social_media_workers()
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

                cog._spawn_bg_task(cog._ensure_category_id(), "twitch.ensure_category_id")
                cog._spawn_bg_task(cog._load_invite_codes_from_db(), "twitch.load_invites")
                cog._spawn_bg_task(cog._start_internal_api(), "twitch.start_internal_api")
                cog._spawn_bg_task(cog._refresh_all_invites(), "twitch.refresh_all_invites")
                eventsub_runner = getattr(cog, "_run_eventsub_listener_supervisor", None)
                if callable(eventsub_runner):
                    eventsub_task = cog._spawn_bg_task(eventsub_runner(), "twitch.eventsub")
                    if eventsub_task is not None:
                        cog._eventsub_supervisor_task = eventsub_task
                else:
                    cog._spawn_bg_task(cog._start_eventsub_listener(), "twitch.eventsub")
                if cog.api:
                    cog._spawn_bg_task(cog._sync_missing_user_ids(), "twitch.sync_user_ids")
                    cog._spawn_bg_task(cog._scout_deadlock_channels(), "twitch.scout_deadlock")
                    from bot.title_generator.knowledge_job import schedule_nightly_knowledge_job
                    from bot.title_generator.insight_job import schedule_weekly_insight_job
                    from bot.analytics.api_post_stream import backfill_post_stream_reports, schedule_report_retry_job
                    cog._spawn_bg_task(schedule_nightly_knowledge_job(start_delay_s=300), "title.knowledge_job")
                    cog._spawn_bg_task(schedule_weekly_insight_job(start_delay_s=600), "title.insight_job")
                    cog._spawn_bg_task(backfill_post_stream_reports(sessions_per_streamer=3), "post_stream.backfill")
                    cog._spawn_bg_task(schedule_report_retry_job(start_delay_s=1800), "post_stream.retry_job")
                start_highlight_clipper = getattr(cog, "_hc_start", None)
                if callable(start_highlight_clipper):
                    await start_highlight_clipper()
                cog._spawn_bg_task(cog._register_views_after_ready(), "twitch.views_warmup")
            except Exception:
                with contextlib.suppress(Exception):
                    await self._cleanup_runtime_components(
                        wait_for_port_release=False,
                        full_shutdown=False,
                    )
                raise
            else:
                cog._runtime_started = True

    async def stop_runtime(self) -> None:
        async with self._runtime_lifecycle_lock():
            log.info("Twitch Cog Unload gestartet – fahre alle Ressourcen herunter...")
            await self._cleanup_runtime_components(
                wait_for_port_release=True,
                full_shutdown=True,
            )
            await asyncio.sleep(0.5)
            log.info("Twitch Cog Unload abgeschlossen")


class DashboardRuntimeBootstrap:
    """Own the dashboard runtime setup stages explicitly."""

    def __init__(self, dashboard: Any) -> None:
        self._dashboard = dashboard

    def configure_runtime(self) -> None:
        dashboard = self._dashboard
        runtime = getattr(dashboard, "_runtime_state", None)
        if runtime is None:
            runtime = DashboardRuntimeContainer()
            if isinstance(dashboard, dict):
                dashboard["_runtime_state"] = runtime
            else:
                setattr(dashboard, "_runtime_state", runtime)
            return
        if not isinstance(runtime, DashboardRuntimeContainer):
            raise TypeError("owner.runtime_state must be a DashboardRuntimeContainer")


TwitchRuntimeBootstrap = BotRuntimeBootstrap


__all__ = [
    "BotRuntimeBootstrap",
    "DashboardRuntimeBootstrap",
    "TwitchRuntimeBootstrap",
]

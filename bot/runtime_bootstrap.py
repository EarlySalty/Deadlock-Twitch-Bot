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
from .runtime_security import require_noauth_loopback_guard
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
        cog._runtime_stop_lock = cog._runtime_start_lock

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
        require_noauth_loopback_guard(
            enabled=cog._dashboard_noauth,
            host=cog._dashboard_host,
        )
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
            ),
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
                cleanup_task = cog._raid_bot.start()
                if cleanup_task is None:
                    raise RuntimeError("RaidBot lifecycle start failed")
                log.debug("Raid-Bot initialisiert (redirect_uri: %s)", cog._raid_redirect_uri)
            except Exception:
                log.exception("Fehler beim Initialisieren des Raid-Bots")
                cog._raid_bot = None
        else:
            log.warning("Raid-Bot und Chat-Bot deaktiviert, da TWITCH_CLIENT_ID/SECRET fehlen.")

        self._ensure_social_media_workers()

        self._register_reload_manager()

    def _ensure_social_media_workers(self) -> None:
        cog = self._cog
        if not cog.api:
            return
        if (
            getattr(cog, "clip_manager", None) is not None
            and getattr(cog, "clip_fetcher", None) is not None
            and getattr(cog, "upload_worker", None) is not None
        ):
            return

        from .social_media.clip_fetcher import ClipFetcher
        from .social_media.clip_manager import ClipManager
        from .social_media.upload_worker import UploadWorker

        cog.clip_manager = ClipManager(twitch_api=cog.api)
        cog.clip_fetcher = ClipFetcher(cog.bot, cog.api, cog.clip_manager)
        cog.upload_worker = UploadWorker(cog.bot, cog.clip_manager)
        log.info(
            "Social Media Clip Management initialized (ClipManager + ClipFetcher + UploadWorker)"
        )

    def _stop_social_media_workers(self) -> None:
        cog = self._cog

        if cog.clip_fetcher:
            try:
                cog.clip_fetcher.cog_unload()
                log.debug("ClipFetcher gecancelt")
            except Exception:
                log.exception("Konnte ClipFetcher nicht canceln")
            finally:
                cog.clip_fetcher = None

        if cog.upload_worker:
            try:
                cog.upload_worker.cog_unload()
                log.debug("UploadWorker gecancelt")
            except Exception:
                log.exception("Konnte UploadWorker nicht canceln")
            finally:
                cog.upload_worker = None

        cog.clip_manager = None

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

    def _runtime_lifecycle_lock(self) -> asyncio.Lock:
        cog = self._cog
        lock = getattr(cog, "_runtime_start_lock", None)
        if lock is None:
            lock = getattr(cog, "_runtime_stop_lock", None)
        if lock is None:
            lock = asyncio.Lock()
        cog._runtime_start_lock = lock
        cog._runtime_stop_lock = lock
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

        if cog._web:
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

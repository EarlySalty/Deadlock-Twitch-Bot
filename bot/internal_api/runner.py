"""Lifecycle wrapper for the internal bot API web server."""

from __future__ import annotations

import asyncio
import errno

from aiohttp import web

from ..core.constants import log
from ..runtime_mode import enforce_internal_api_runtime
from .app import (
    INTERNAL_API_BASE_PATH,
    ArchiveStreamerCallback,
    ChattersDebugCallback,
    ComparisonCallback,
    DiscordFlagCallback,
    DiscordProfileCallback,
    EventsubDispatchCallback,
    EventsubProcessingDebugCallback,
    EventsubProcessingRequeueCallback,
    InternalApiCallbacks,
    LiveActiveAnnouncementsCallback,
    LiveLinkClickCallback,
    RaidAuthStateCallback,
    RaidAuthUrlCallback,
    RaidBlockStateCallback,
    RaidGoUrlCallback,
    RaidOauthCallback,
    RaidRequirementsCallback,
    RemoveStreamerCallback,
    SessionCallback,
    StatsCallback,
    StreamerAnalyticsCallback,
    StreamersCallback,
    VerifyStreamerCallback,
    AddStreamerCallback,
    ObservabilitySnapshotCallback,
    build_internal_api_app,
)


class InternalApiRunner:
    """Run the internal API with retry-aware start/stop lifecycle hooks."""

    def __init__(
        self,
        *,
        host: str,
        port: int,
        token: str | None,
        base_path: str = INTERNAL_API_BASE_PATH,
        callbacks: InternalApiCallbacks | None = None,
        add_cb: AddStreamerCallback | None = None,
        remove_cb: RemoveStreamerCallback | None = None,
        list_cb: StreamersCallback | None = None,
        stats_cb: StatsCallback | None = None,
        verify_cb: VerifyStreamerCallback | None = None,
        archive_cb: ArchiveStreamerCallback | None = None,
        discord_flag_cb: DiscordFlagCallback | None = None,
        discord_profile_cb: DiscordProfileCallback | None = None,
        streamer_analytics_cb: StreamerAnalyticsCallback | None = None,
        comparison_cb: ComparisonCallback | None = None,
        session_cb: SessionCallback | None = None,
        raid_auth_url_cb: RaidAuthUrlCallback | None = None,
        raid_auth_state_cb: RaidAuthStateCallback | None = None,
        raid_block_state_cb: RaidBlockStateCallback | None = None,
        raid_go_url_cb: RaidGoUrlCallback | None = None,
        raid_requirements_cb: RaidRequirementsCallback | None = None,
        raid_oauth_callback_cb: RaidOauthCallback | None = None,
        live_active_announcements_cb: LiveActiveAnnouncementsCallback | None = None,
        live_link_click_cb: LiveLinkClickCallback | None = None,
        observability_snapshot_cb: ObservabilitySnapshotCallback | None = None,
        chatters_debug_cb: ChattersDebugCallback | None = None,
        eventsub_dispatch_cb: EventsubDispatchCallback | None = None,
        eventsub_processing_debug_cb: EventsubProcessingDebugCallback | None = None,
        eventsub_processing_requeue_cb: EventsubProcessingRequeueCallback | None = None,
    ) -> None:
        self.host = host
        self.port = int(port)
        self.token = (token or "").strip()
        self.base_path = base_path
        self._callbacks = InternalApiCallbacks.coalesce(
            callbacks,
            add_cb=add_cb,
            remove_cb=remove_cb,
            list_cb=list_cb,
            stats_cb=stats_cb,
            verify_cb=verify_cb,
            archive_cb=archive_cb,
            discord_flag_cb=discord_flag_cb,
            discord_profile_cb=discord_profile_cb,
            streamer_analytics_cb=streamer_analytics_cb,
            comparison_cb=comparison_cb,
            session_cb=session_cb,
            raid_auth_url_cb=raid_auth_url_cb,
            raid_auth_state_cb=raid_auth_state_cb,
            raid_block_state_cb=raid_block_state_cb,
            raid_go_url_cb=raid_go_url_cb,
            raid_requirements_cb=raid_requirements_cb,
            raid_oauth_callback_cb=raid_oauth_callback_cb,
            live_active_announcements_cb=live_active_announcements_cb,
            live_link_click_cb=live_link_click_cb,
            observability_snapshot_cb=observability_snapshot_cb,
            chatters_debug_cb=chatters_debug_cb,
            eventsub_dispatch_cb=eventsub_dispatch_cb,
            eventsub_processing_debug_cb=eventsub_processing_debug_cb,
            eventsub_processing_requeue_cb=eventsub_processing_requeue_cb,
        )

        self._runner: web.AppRunner | None = None
        self._app: web.Application | None = None
        self._missing_token_warning_emitted = False
        self._last_start_error: str | None = None

    @property
    def is_running(self) -> bool:
        return self._runner is not None

    @property
    def last_start_error(self) -> str | None:
        return self._last_start_error

    async def start(self) -> None:
        if self._runner is not None:
            self._last_start_error = None
            return

        try:
            enforce_internal_api_runtime(port=self.port)
        except RuntimeError as exc:
            self._last_start_error = str(exc)
            log.error("%s", exc)
            return

        if not self.token:
            if not self._missing_token_warning_emitted:
                self._missing_token_warning_emitted = True
                log.warning(
                    "TWITCH_INTERNAL_API_TOKEN is empty. "
                    "Internal API is running in fail-closed mode."
                )

        max_retries = 5
        retry_delay = 0.5
        for attempt in range(max_retries):
            runner: web.AppRunner | None = None
            try:
                app = build_internal_api_app(
                    token=self.token,
                    base_path=self.base_path,
                    callbacks=self._callbacks,
                )
                runner = web.AppRunner(app)
                await runner.setup()
                site = web.TCPSite(runner, host=self.host, port=self.port)
                await site.start()

                self._app = app
                self._runner = runner
                self._last_start_error = None
                log.info(
                    "Internal API running on http://%s:%s%s",
                    self.host,
                    self.port,
                    self.base_path.rstrip("/"),
                )
                return
            except asyncio.CancelledError:
                if runner is not None:
                    await runner.cleanup()
                self._last_start_error = "Internal API startup cancelled"
                log.info("Internal API startup cancelled")
                return
            except OSError as exc:
                if runner is not None:
                    await runner.cleanup()
                is_addr_in_use = exc.errno in (10048, getattr(errno, "EADDRINUSE", 98))
                if is_addr_in_use and attempt < max_retries - 1:
                    log.warning(
                        "Internal API port %s busy on %s, retrying in %.1fs (%s/%s)",
                        self.port,
                        self.host,
                        retry_delay,
                        attempt + 1,
                        max_retries,
                    )
                    await asyncio.sleep(retry_delay)
                    retry_delay *= 2
                    continue
                self._last_start_error = f"{type(exc).__name__}: {exc}"
                log.exception("Failed to start internal API")
                return
            except Exception:
                if runner is not None:
                    await runner.cleanup()
                self._last_start_error = "Unexpected internal API startup failure"
                log.exception("Failed to start internal API")
                return

    async def stop(self) -> None:
        if self._runner is None:
            return
        try:
            await self._runner.cleanup()
        except asyncio.CancelledError:
            log.info("Internal API shutdown cancelled")
        finally:
            self._runner = None
            self._app = None
            log.info("Internal API stopped")


__all__ = ["InternalApiRunner"]

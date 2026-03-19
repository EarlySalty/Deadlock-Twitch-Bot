"""Lifecycle wrapper for the internal bot API web server."""

from __future__ import annotations

import asyncio
import errno
from collections.abc import Awaitable, Callable
from typing import Any

from aiohttp import web

from ..core.constants import log
from ..runtime_mode import enforce_internal_api_runtime
from .app import INTERNAL_API_BASE_PATH, build_internal_api_app


class InternalApiRunner:
    """Run the internal API with retry-aware start/stop lifecycle hooks."""

    def __init__(
        self,
        *,
        host: str,
        port: int,
        token: str | None,
        base_path: str = INTERNAL_API_BASE_PATH,
        add_cb: Callable[[str, bool], Awaitable[str]] | None = None,
        remove_cb: Callable[[str], Awaitable[str]] | None = None,
        list_cb: Callable[[], Awaitable[list[dict[str, Any]]]] | None = None,
        stats_cb: Callable[..., Awaitable[dict[str, Any]]] | None = None,
        verify_cb: Callable[[str, str], Awaitable[str]] | None = None,
        archive_cb: Callable[[str, str], Awaitable[str]] | None = None,
        discord_flag_cb: Callable[[str, bool], Awaitable[str]] | None = None,
        discord_profile_cb: Callable[..., Awaitable[str]] | None = None,
        streamer_analytics_cb: Callable[[str, int], Awaitable[dict[str, Any]]] | None = None,
        comparison_cb: Callable[[int], Awaitable[dict[str, Any]]] | None = None,
        session_cb: Callable[[int], Awaitable[dict[str, Any]]] | None = None,
        raid_auth_url_cb: Callable[[str], Awaitable[str]] | None = None,
        raid_auth_state_cb: Callable[[str], Awaitable[dict[str, Any]]] | None = None,
        raid_block_state_cb: Callable[..., Awaitable[dict[str, Any]]] | None = None,
        raid_go_url_cb: Callable[[str], Awaitable[str | None]] | None = None,
        raid_requirements_cb: Callable[[str], Awaitable[str]] | None = None,
        raid_oauth_callback_cb: Callable[..., Awaitable[dict[str, Any]]] | None = None,
        live_active_announcements_cb: Callable[[], Awaitable[list[dict[str, Any]]]] | None = None,
        live_link_click_cb: Callable[..., Awaitable[dict[str, Any] | None]] | None = None,
        observability_snapshot_cb: Callable[[], Awaitable[dict[str, Any]]] | None = None,
        chatters_debug_cb: Callable[[str], Awaitable[dict[str, Any]]] | None = None,
    ) -> None:
        self.host = host
        self.port = int(port)
        self.token = (token or "").strip()
        self.base_path = base_path

        self._add_cb = add_cb
        self._remove_cb = remove_cb
        self._list_cb = list_cb
        self._stats_cb = stats_cb
        self._verify_cb = verify_cb
        self._archive_cb = archive_cb
        self._discord_flag_cb = discord_flag_cb
        self._discord_profile_cb = discord_profile_cb
        self._streamer_analytics_cb = streamer_analytics_cb
        self._comparison_cb = comparison_cb
        self._session_cb = session_cb
        self._raid_auth_url_cb = raid_auth_url_cb
        self._raid_auth_state_cb = raid_auth_state_cb
        self._raid_block_state_cb = raid_block_state_cb
        self._raid_go_url_cb = raid_go_url_cb
        self._raid_requirements_cb = raid_requirements_cb
        self._raid_oauth_callback_cb = raid_oauth_callback_cb
        self._live_active_announcements_cb = live_active_announcements_cb
        self._live_link_click_cb = live_link_click_cb
        self._observability_snapshot_cb = observability_snapshot_cb
        self._chatters_debug_cb = chatters_debug_cb

        self._runner: web.AppRunner | None = None
        self._app: web.Application | None = None
        self._missing_token_warning_emitted = False

    @property
    def is_running(self) -> bool:
        return self._runner is not None

    async def start(self) -> None:
        if self._runner is not None:
            return

        try:
            enforce_internal_api_runtime(port=self.port)
        except RuntimeError as exc:
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
                    add_cb=self._add_cb,
                    remove_cb=self._remove_cb,
                    list_cb=self._list_cb,
                    stats_cb=self._stats_cb,
                    verify_cb=self._verify_cb,
                    archive_cb=self._archive_cb,
                    discord_flag_cb=self._discord_flag_cb,
                    discord_profile_cb=self._discord_profile_cb,
                    streamer_analytics_cb=self._streamer_analytics_cb,
                    comparison_cb=self._comparison_cb,
                    session_cb=self._session_cb,
                    raid_auth_url_cb=self._raid_auth_url_cb,
                    raid_auth_state_cb=self._raid_auth_state_cb,
                    raid_block_state_cb=self._raid_block_state_cb,
                    raid_go_url_cb=self._raid_go_url_cb,
                    raid_requirements_cb=self._raid_requirements_cb,
                    raid_oauth_callback_cb=self._raid_oauth_callback_cb,
                    live_active_announcements_cb=self._live_active_announcements_cb,
                    live_link_click_cb=self._live_link_click_cb,
                    observability_snapshot_cb=self._observability_snapshot_cb,
                    chatters_debug_cb=self._chatters_debug_cb,
                )
                runner = web.AppRunner(app)
                await runner.setup()
                site = web.TCPSite(runner, host=self.host, port=self.port)
                await site.start()

                self._app = app
                self._runner = runner
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
                log.exception("Failed to start internal API")
                return
            except Exception:
                if runner is not None:
                    await runner.cleanup()
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

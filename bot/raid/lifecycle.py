from __future__ import annotations

import asyncio
import logging
from collections.abc import Awaitable, Callable
from typing import Any


CleanupCoroutineFactory = Callable[[], Awaitable[Any]]


class RaidBotLifecycle:
    """Manage RaidBot background tasks with explicit start/stop semantics."""

    def __init__(
        self,
        cleanup_coro_factory: CleanupCoroutineFactory,
        *,
        logger: logging.Logger | None = None,
        cleanup_task_name: str = "raid.bot.periodic_cleanup",
    ) -> None:
        self._cleanup_coro_factory = cleanup_coro_factory
        self._logger = logger or logging.getLogger("TwitchStreams.RaidBotLifecycle")
        self._cleanup_task_name = cleanup_task_name
        self._managed_tasks: set[asyncio.Task[Any]] = set()
        self._cleanup_task: asyncio.Task[Any] | None = None

    @property
    def cleanup_task(self) -> asyncio.Task[Any] | None:
        return self._cleanup_task

    @property
    def managed_tasks(self) -> tuple[asyncio.Task[Any], ...]:
        return tuple(self._managed_tasks)

    @property
    def started(self) -> bool:
        return self._cleanup_task is not None and not self._cleanup_task.done()

    def start(self) -> asyncio.Task[Any] | None:
        """Start the cleanup task once; repeated calls are idempotent."""
        if self.started:
            return self._cleanup_task

        cleanup_coro = self._cleanup_coro_factory()
        task = self.spawn_background_task(cleanup_coro, name=self._cleanup_task_name)
        self._cleanup_task = task
        return task

    async def stop(self) -> None:
        """Cancel all managed tasks and wait for shutdown to settle."""
        tasks = list(self._managed_tasks)
        self._managed_tasks.clear()
        self._cleanup_task = None
        if not tasks:
            return

        for task in tasks:
            if not task.done():
                task.cancel()

        for task in tasks:
            if task.done():
                continue
            try:
                await task
            except asyncio.CancelledError:
                self._logger.debug("RaidBot background task cancelled: %s", task.get_name())
            except Exception:
                self._logger.debug(
                    "RaidBot background task failed during shutdown: %s",
                    task.get_name(),
                    exc_info=True,
                )

    def spawn_background_task(
        self,
        awaitable: Awaitable[Any],
        *,
        name: str,
    ) -> asyncio.Task[Any] | None:
        """Spawn and track a task, returning ``None`` when no loop is running."""
        try:
            task = asyncio.create_task(awaitable, name=name)
        except RuntimeError as exc:
            self._logger.error(
                "Cannot start RaidBot background task %s (no running loop yet): %s",
                name,
                exc,
            )
            self._close_awaitable(awaitable)
            return None
        except Exception:
            self._logger.exception("Failed to start RaidBot background task %s", name)
            self._close_awaitable(awaitable)
            return None

        self._managed_tasks.add(task)

        def _discard(completed: asyncio.Task[Any]) -> None:
            self._managed_tasks.discard(completed)

        task.add_done_callback(_discard)
        return task

    @staticmethod
    def _close_awaitable(awaitable: Awaitable[Any]) -> None:
        close = getattr(awaitable, "close", None)
        if callable(close):
            close()


__all__ = ["CleanupCoroutineFactory", "RaidBotLifecycle"]

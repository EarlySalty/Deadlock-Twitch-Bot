"""Hot-reload manager for individual Twitch subsystems without full cog restart."""

from __future__ import annotations

import asyncio
import importlib
import inspect
import logging
import sys
from dataclasses import dataclass, field
from datetime import UTC, datetime
from typing import Any

from discord.ext import tasks

log = logging.getLogger("TwitchStreams.ReloadManager")


@dataclass
class LoopSpec:
    """Specification for a tasks.Loop that can be hot-reloaded.

    The interval (hours/minutes/seconds) and before_loop hook are auto-read
    from the freshly imported class-level Loop descriptor at reload time.
    """

    attr_name: str  # Name of the loop method on the cog (e.g. "collect_analytics_data")


@dataclass
class SubsystemDef:
    """Definition of a hot-reloadable subsystem."""

    name: str                          # Internal key (e.g. "analytics")
    display_name: str                  # Human-readable (e.g. "Analytics")
    modules: list[str]                 # Module names to purge from sys.modules
    loops: list[LoopSpec] = field(default_factory=list)
    hot_reloadable: bool = True
    teardown_hook: str | None = None   # async method name on cog to call before purge
    startup_hook: str | None = None    # async method name on cog to call after rebind


@dataclass
class SubsystemState:
    running_loops: int = 0
    last_reload: datetime | None = None
    error: str | None = None


class TwitchReloadManager:
    """Manages hot-reload of individual Twitch subsystems.

    Usage:
        manager = TwitchReloadManager(cog)
        manager.register(SubsystemDef(name="analytics", ...))
        ok, msg = await manager.reload("analytics")
    """

    def __init__(self, cog: Any) -> None:
        self._cog = cog
        self._subsystems: dict[str, SubsystemDef] = {}
        self._last_reloads: dict[str, datetime] = {}
        self._last_errors: dict[str, str | None] = {}

    def register(self, subsystem: SubsystemDef) -> None:
        self._subsystems[subsystem.name] = subsystem

    def get_subsystem(self, name: str) -> SubsystemDef | None:
        return self._subsystems.get(name)

    def get_all_names(self) -> list[str]:
        return list(self._subsystems.keys())

    def get_all_states(self) -> dict[str, SubsystemState]:
        states: dict[str, SubsystemState] = {}
        for name, sub in self._subsystems.items():
            running = sum(
                1
                for ls in sub.loops
                if (lp := getattr(self._cog, ls.attr_name, None)) and lp.is_running()
            )
            states[name] = SubsystemState(
                running_loops=running,
                last_reload=self._last_reloads.get(name),
                error=self._last_errors.get(name),
            )
        return states

    # ------------------------------------------------------------------
    # Public reload entry point
    # ------------------------------------------------------------------

    async def reload(self, name: str) -> tuple[bool, str]:
        """Hot-reload a single subsystem. Returns (success, message)."""
        sub = self._subsystems.get(name)
        if not sub:
            return False, f"Unknown subsystem: `{name}`"

        if not sub.hot_reloadable:
            return (
                False,
                f"**{sub.display_name}** kann nicht einzeln neu geladen werden – "
                f"nutze `!master reload cogs.twitch` für einen Full-Reload.",
            )

        cog = self._cog
        log.info("Hot-reload: starting subsystem '%s'", name)

        # 1. Call teardown hook if registered
        if sub.teardown_hook:
            try:
                hook = getattr(cog, sub.teardown_hook, None)
                if callable(hook):
                    await hook()
            except Exception:
                log.exception("Hot-reload: teardown_hook failed for '%s'", name)

        # 2. Cancel all subsystem loops and wait for them to finish
        await self._cancel_loops(sub)

        # 3. Purge modules from sys.modules
        purged = self._purge_modules(sub)
        log.debug("Hot-reload: purged %d modules for '%s': %s", len(purged), name, purged)

        # 4. Re-import the primary modules
        fresh_modules = self._reimport_modules(sub)

        # 5. Rebind loops from fresh code
        errors: list[str] = []
        for loop_spec in sub.loops:
            try:
                await self._rebind_loop(loop_spec, fresh_modules)
            except Exception as exc:
                log.exception("Hot-reload: failed to rebind loop '%s'", loop_spec.attr_name)
                errors.append(f"`{loop_spec.attr_name}`: {exc}")

        # 6. Call startup hook if registered
        if sub.startup_hook:
            try:
                hook = getattr(cog, sub.startup_hook, None)
                if callable(hook):
                    await hook()
            except Exception as exc:
                log.exception("Hot-reload: startup_hook failed for '%s'", name)
                errors.append(f"startup_hook: {exc}")

        now = datetime.now(UTC)
        if errors:
            msg = f"Partial reload with errors: {'; '.join(errors)}"
            self._last_errors[name] = msg
            self._last_reloads[name] = now
            return False, msg

        self._last_reloads[name] = now
        self._last_errors[name] = None
        n_loops = len(sub.loops)
        log.info("Hot-reload: subsystem '%s' done — %d loop(s) restarted", name, n_loops)
        return True, (
            f"Subsystem **{sub.display_name}** neu geladen. "
            f"{n_loops} Loop(s) neu gestartet."
        )

    # ------------------------------------------------------------------
    # Internal helpers
    # ------------------------------------------------------------------

    async def _cancel_loops(self, sub: SubsystemDef) -> None:
        """Cancel all loops of a subsystem and wait for their tasks to finish."""
        tasks_to_wait: list[asyncio.Task] = []

        for loop_spec in sub.loops:
            old_loop: tasks.Loop | None = getattr(self._cog, loop_spec.attr_name, None)
            if old_loop is None:
                continue
            if old_loop.is_running():
                old_loop.cancel()
            task = getattr(old_loop, "_task", None)
            if task is not None and not task.done():
                tasks_to_wait.append(task)

        if tasks_to_wait:
            try:
                done, _ = await asyncio.wait(tasks_to_wait, timeout=5.0)
                if len(done) < len(tasks_to_wait):
                    log.warning(
                        "Hot-reload: %d loop task(s) did not finish within 5s timeout",
                        len(tasks_to_wait) - len(done),
                    )
            except Exception:
                log.exception("Hot-reload: error while waiting for loops to finish")

    def _purge_modules(self, sub: SubsystemDef) -> list[str]:
        """Remove all sub-modules matching the subsystem's module list from sys.modules."""
        purged: list[str] = []
        for mod_name in sub.modules:
            for key in list(sys.modules.keys()):
                if key == mod_name or key.startswith(mod_name + "."):
                    del sys.modules[key]
                    purged.append(key)
        return purged

    def _reimport_modules(self, sub: SubsystemDef) -> list[Any]:
        """Re-import the subsystem's modules after purging, return fresh module objects."""
        fresh: list[Any] = []
        for mod_name in sub.modules:
            try:
                mod = importlib.import_module(mod_name)  # nosemgrep: python.lang.security.audit.non-literal-import.non-literal-import
                fresh.append(mod)
            except Exception:
                log.exception("Hot-reload: failed to import '%s'", mod_name)
        return fresh

    def _find_fresh_loop_descriptor(
        self, attr_name: str, fresh_modules: list[Any]
    ) -> tasks.Loop | None:
        """Search freshly imported modules for a class-level tasks.Loop with attr_name."""
        for mod in fresh_modules:
            for _, cls in inspect.getmembers(mod, inspect.isclass):
                raw = cls.__dict__.get(attr_name)
                if isinstance(raw, tasks.Loop):
                    return raw
        return None

    async def _rebind_loop(
        self, loop_spec: LoopSpec, fresh_modules: list[Any]
    ) -> None:
        """Create a fresh Loop from reloaded code and bind it to the live cog instance."""
        cog = self._cog
        attr = loop_spec.attr_name

        # Locate the fresh class-level Loop descriptor (holds coro + before_loop + interval)
        fresh_descriptor = self._find_fresh_loop_descriptor(attr, fresh_modules)
        if fresh_descriptor is None:
            raise RuntimeError(
                f"Could not find tasks.Loop '{attr}' in any freshly imported module"
            )

        # Build new Loop with same interval as the freshly imported descriptor
        new_loop = tasks.loop(
            hours=fresh_descriptor.hours,
            minutes=fresh_descriptor.minutes,
            seconds=fresh_descriptor.seconds,
        )(fresh_descriptor.coro)

        # Copy the before_loop hook if the fresh descriptor has one
        if fresh_descriptor._before_loop is not None:
            new_loop._before_loop = fresh_descriptor._before_loop
        elif fresh_descriptor._after_loop is not None:
            new_loop._after_loop = fresh_descriptor._after_loop

        # Bind to the live cog and replace the instance attribute
        new_loop._injected = cog
        setattr(cog, attr, new_loop)
        new_loop.start()

        log.debug(
            "Hot-reload: loop '%s' rebound (%.0fh %.0fm %.0fs)",
            attr,
            fresh_descriptor.hours,
            fresh_descriptor.minutes,
            fresh_descriptor.seconds,
        )

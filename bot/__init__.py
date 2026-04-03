# =========================================
# cogs/twitch/__init__.py
# =========================================
"""Package entry point for the Twitch stream monitor cog."""

import asyncio
import hashlib
import inspect
import json
import logging
import os
from typing import Optional

import discord
from discord.ext import commands

from .logging_setup import ensure_twitch_logger_file_handler

# Last-synced fingerprint per guild — persists for the process lifetime so
# reloads don't re-sync when commands haven't changed.
_last_sync_hash: dict[int, str] = {}


def _command_payload_hash(bot: commands.Bot, guild: discord.Object) -> str:
    """Return a stable hash of the current app-command payload for *guild*."""
    payload = [cmd.to_dict(bot.tree) for cmd in bot.tree.get_commands(guild=guild)]
    # Sort by name so ordering differences don't cause false cache misses.
    payload.sort(key=lambda c: c.get("name", ""))
    return hashlib.sha256(json.dumps(payload, sort_keys=True).encode()).hexdigest()

log = logging.getLogger("TwitchStreams")


def _env_bool(name: str, default: bool = False) -> bool:
    raw = os.getenv(name)
    if raw is None:
        return default
    return raw.strip().lower() in {"1", "true", "yes", "y", "on"}


def _parse_sync_guild_ids(raw: str) -> list[int]:
    """Parse a comma/space separated guild-id list from env."""
    ids: list[int] = []
    for part in raw.replace(",", " ").split():
        value = part.strip()
        if not value or not value.isdigit():
            continue
        guild_id = int(value)
        if guild_id > 0 and guild_id not in ids:
            ids.append(guild_id)
    return ids


async def _invoke_twitch_leaderboard_callback(leaderboard_cb, ctx, *, filters: str) -> None:
    try:
        params = inspect.signature(leaderboard_cb).parameters
    except (TypeError, ValueError):
        params = None

    supports_filters = True
    if params is not None:
        supports_filters = "filters" in params or any(
            param.kind == inspect.Parameter.VAR_KEYWORD
            for param in params.values()
        )

    if supports_filters:
        await leaderboard_cb(ctx, filters=filters)
        return
    await leaderboard_cb(ctx)


async def _sync_app_commands_after_ready(bot: commands.Bot) -> None:
    """Ensure hybrid/app commands are synced after extension (re)load."""
    try:
        await bot.wait_until_ready()
    except Exception:
        log.debug("wait_until_ready failed while syncing app commands", exc_info=True)
        return

    guild_ids = _parse_sync_guild_ids(os.getenv("TWITCH_COMMAND_SYNC_GUILD_IDS", ""))
    if not guild_ids:
        main_guild_id = (os.getenv("MAIN_GUILD_ID") or "").strip()
        if main_guild_id.isdigit():
            guild_ids = [int(main_guild_id)]
    if not guild_ids:
        guild_ids = [int(g.id) for g in getattr(bot, "guilds", []) if getattr(g, "id", 0)]

    copy_global_to_guild = _env_bool("TWITCH_COMMAND_COPY_GLOBAL_TO_GUILD", default=False)
    for guild_id in guild_ids:
        guild = discord.Object(id=guild_id)
        try:
            if copy_global_to_guild:
                bot.tree.copy_global_to(guild=guild)
            current_hash = _command_payload_hash(bot, guild)
            if _last_sync_hash.get(guild_id) == current_hash:
                log.debug("App commands unchanged for guild %s — skipping sync", guild_id)
                continue
            synced = await bot.tree.sync(guild=guild)
            _last_sync_hash[guild_id] = current_hash
            log.info("Synced %d app command(s) to guild %s", len(synced), guild_id)
        except discord.HTTPException as exc:
            if exc.status == 429 and exc.code == 30034:
                log.warning(
                    "Guild %s: daily command-create quota exhausted (200/day). "
                    "Sync skipped — will retry on next bot restart.",
                    guild_id,
                )
            else:
                log.exception("Guild app-command sync failed for guild %s", guild_id)
        except Exception:
            log.exception("Guild app-command sync failed for guild %s", guild_id)

    try:
        synced_global = await bot.tree.sync()
        log.info("Synced %d global app command(s)", len(synced_global))
    except Exception:
        log.exception("Global app-command sync failed")


async def setup(bot: commands.Bot):
    """Add the Twitch stream cog to the master bot, and register the !twl proxy command exactly once."""
    ensure_twitch_logger_file_handler()

    from .cog import (
        TwitchStreamCog,
    )  # Local import to avoid self-import warnings during extension discovery

    # 1) Stale/alte Command-Objekte vorab entfernen
    existing = bot.get_command("twl")
    if existing is not None:
        bot.remove_command(existing.name)
        log.info("Removed pre-existing !twl command before adding Twitch cog")

    # 2) Cog hinzufügen – vorher prüfen ob bereits geladen (z.B. nach failedReload)
    existing_cog = bot.get_cog("TwitchStreamCog")
    if existing_cog is not None:
        log.warning("TwitchStreamCog ist bereits geladen – wird zuerst entfernt")
        await bot.remove_cog("TwitchStreamCog")

    cog = TwitchStreamCog(bot)
    await bot.add_cog(cog)

    # 3) Dünner Prefix-Proxy (!twl) → ruft IMMER die Cog-Methode auf (keine Doppel-Registrierung)
    async def _twl_proxy(ctx: commands.Context, *, filters: str = ""):
        active_cog: TwitchStreamCog | None = bot.get_cog(cog.__cog_name__)  # type: ignore[assignment]
        if not isinstance(active_cog, TwitchStreamCog):
            await ctx.reply("Twitch-Statistiken sind derzeit nicht verfügbar.")
            return

        leaderboard_cb = getattr(active_cog, "twitch_leaderboard", None)
        if not callable(leaderboard_cb):
            await ctx.reply("Twitch-Statistiken sind derzeit nicht verfügbar.")
            log.error("twitch_leaderboard callable missing on active cog")
            return

        try:
            await _invoke_twitch_leaderboard_callback(
                leaderboard_cb,
                ctx,
                filters=filters,
            )
        except Exception:
            log.exception("Failed to call twitch_leaderboard")
            await ctx.reply("Twitch-Statistiken konnten nicht geladen werden.")

    prefix_command = commands.Command(
        _twl_proxy,
        name="twl",
        help="Zeigt Twitch-Statistiken (Leaderboard) im Partner-Kanal an. Nutzung: !twl [samples=N] [avg=N] [partner=only|exclude|any] [limit=N]",
    )

    # Command bewusst NUR HIER registrieren (keine Decorators im Cog)
    bot.add_command(prefix_command)
    cog.set_prefix_command(prefix_command)
    log.debug("Registered !twl prefix command via setup hook")

    # Integriert in den MasterBot: dort übernimmt der zentrale Sync-Orchestrator.
    # Standalone-Betrieb: Fallback bleibt aktiv.
    managed_sync = callable(getattr(bot, "sync_app_commands", None))
    default_sync_on_load = not managed_sync
    sync_on_load = _env_bool("TWITCH_EXTENSION_SYNC_ON_LOAD", default=default_sync_on_load)
    if sync_on_load:
        asyncio.create_task(_sync_app_commands_after_ready(bot), name="twitch.sync_app_commands")
    else:
        log.info("Skipping twitch extension self-sync (managed centrally or disabled by env).")


async def teardown(bot: commands.Bot):
    """Purge twitch_cog modules from sys.modules so hot-reload works correctly."""
    import sys

    ensure_twitch_logger_file_handler()

    to_remove = [
        name for name in sys.modules
        if name == "bot" or name.startswith("bot.")
        or name == "twitch_cog"
    ]
    for name in to_remove:
        sys.modules.pop(name, None)
    log.debug("bot: purged %d modules from sys.modules", len(to_remove))

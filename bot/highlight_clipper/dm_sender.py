from __future__ import annotations

import logging
from pathlib import Path

import discord

from .config import MAX_DISCORD_FILE_MB

log = logging.getLogger("TwitchStreams.HighlightClipper")


async def send_highlight_dm(bot, discord_user_id: int, match_id: int, events: list, clip_paths: list[str]) -> None:
    user = await bot.fetch_user(int(discord_user_id))
    embed = discord.Embed(
        title=f"\N{VIDEO GAME} Neue Highlights aus Match #{match_id}",
        description=f"{len(clip_paths)} Clips",
        color=discord.Color.blue(),
    )
    await user.send(embed=embed)

    max_bytes = MAX_DISCORD_FILE_MB * 1024 * 1024
    for event, clip_path in zip(events, clip_paths, strict=False):
        path = Path(clip_path)
        if not path.exists():
            continue
        if path.stat().st_size > max_bytes:
            await user.send(f"{event.label}: Datei ist groesser als {MAX_DISCORD_FILE_MB} MB und wurde uebersprungen.")
            continue
        await user.send(
            content=event.label,
            file=discord.File(path, filename=path.name),
        )
    log.info("HighlightClipper DM sent for match_id=%s clips=%s", match_id, len(clip_paths))

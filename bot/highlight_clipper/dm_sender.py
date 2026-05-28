from __future__ import annotations

import logging
from pathlib import Path

import discord

from .config import HIGHLIGHT_DISCORD_CHANNEL_ID
from .config import MAX_DISCORD_FILE_MB

log = logging.getLogger("TwitchStreams.HighlightClipper")


async def send_highlight_to_channel(
    bot,
    streamer_login: str,
    match_id: int,
    events: list,
    clip_paths: list[str],
) -> None:
    channel = bot.get_channel(HIGHLIGHT_DISCORD_CHANNEL_ID)
    if channel is None:
        try:
            channel = await bot.fetch_channel(HIGHLIGHT_DISCORD_CHANNEL_ID)
        except Exception:
            log.error("HighlightClipper: Channel %s nicht gefunden", HIGHLIGHT_DISCORD_CHANNEL_ID)
            return

    embed = discord.Embed(
        title=f"\N{VIDEO GAME} Highlights — {streamer_login} (Match #{match_id})",
        description=f"{len(clip_paths)} Clip(s)",
        color=discord.Color.orange(),
    )
    await channel.send(embed=embed)

    max_bytes = MAX_DISCORD_FILE_MB * 1024 * 1024
    for event, clip_path in zip(events, clip_paths, strict=False):
        path = Path(clip_path)
        if not path.exists():
            continue
        if path.stat().st_size > max_bytes:
            await channel.send(f"{event.label}: Datei > {MAX_DISCORD_FILE_MB} MB, übersprungen.")
            continue
        await channel.send(
            content=f"**{streamer_login}** — {event.label}",
            file=discord.File(path, filename=path.name),
        )
    log.info(
        "HighlightClipper: %s Clips für %s match=%s gepostet",
        len(clip_paths),
        streamer_login,
        match_id,
    )

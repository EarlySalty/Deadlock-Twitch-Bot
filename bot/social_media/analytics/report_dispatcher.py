from __future__ import annotations

import asyncio
import logging
import os
from datetime import UTC, datetime, timedelta

import discord
from discord.ext import commands

from ..settings import get_setting, set_setting
from .report_writer import SocialMediaReportWriter

log = logging.getLogger("TwitchStreams.SocialMedia.ReportDispatcher")

KEY_ADMIN_WEEKLY_REPORT_SENT = "admin_weekly_report_last_sent_period_end"
DEFAULT_ADMIN_DISCORD_USER_ID = "662995601738170389"


class SocialMediaReportDispatcher(commands.Cog):
    """Versendet den woechentlichen Admin-Report per Discord-DM."""

    def __init__(self, bot, *, writer: SocialMediaReportWriter | None = None) -> None:
        self.bot = bot
        self.writer = writer or SocialMediaReportWriter()
        self.enabled = True
        self.interval_seconds = 6 * 60 * 60
        self._task = bot.loop.create_task(self._dispatch_loop())
        log.info(
            "Social media report dispatcher started (interval=%ss)",
            self.interval_seconds,
        )

    def cog_unload(self):
        if self._task:
            self._task.cancel()
        log.info("Social media report dispatcher stopped")

    async def _dispatch_loop(self):
        await self.bot.wait_until_ready()
        await asyncio.sleep(120)
        while not self.bot.is_closed() and self.enabled:
            try:
                await self.dispatch_weekly_admin_report()
            except Exception:
                log.exception("Social media report dispatch failed")
            await asyncio.sleep(self.interval_seconds)

    async def dispatch_weekly_admin_report(self) -> bool:
        period_end = _weekly_anchor()
        period_start = period_end - timedelta(days=7)
        if get_setting(KEY_ADMIN_WEEKLY_REPORT_SENT) == period_end.isoformat():
            return False

        admin_user = await self._resolve_admin_user()
        if admin_user is None:
            return False

        report = await self.writer.write_admin_weekly_report(
            period_start=period_start,
            period_end=period_end,
        )
        header = (
            f"**Social Media Wochenreport**\n"
            f"Zeitraum: {_format_period(period_start, period_end)}\n"
            f"Modell: {report.model or 'fallback'}"
        )
        try:
            for index, chunk in enumerate(_split_message(report.content_md), start=1):
                prefix = header if index == 1 else "**Fortsetzung Wochenreport**"
                await admin_user.send(f"{prefix}\n\n{chunk}")
        except discord.Forbidden:
            log.info("Cannot DM configured admin user for social-media weekly report")
            return False
        except Exception:
            log.warning("Failed to send social-media weekly report DM", exc_info=True)
            return False

        set_setting(
            KEY_ADMIN_WEEKLY_REPORT_SENT,
            period_end.isoformat(),
            updated_by="social_media_report_dispatcher",
        )
        log.info("Sent weekly social-media admin report for period_end=%s", period_end.isoformat())
        return True

    @staticmethod
    def _admin_discord_user_id() -> str:
        for env_name in (
            "SOCIAL_MEDIA_REPORT_ADMIN_DISCORD_USER_ID",
            "TWITCH_ADMIN_DISCORD_USER_ID",
        ):
            value = str(os.getenv(env_name) or "").strip()
            if value:
                return value
        return DEFAULT_ADMIN_DISCORD_USER_ID

    async def _resolve_admin_user(self) -> discord.abc.User | None:
        discord_user_id = self._admin_discord_user_id()
        try:
            user_id_int = int(discord_user_id)
        except (TypeError, ValueError):
            log.warning("Invalid admin Discord user id configured for social-media report DM")
            return None

        user = None
        getter = getattr(self.bot, "get_user", None)
        if callable(getter):
            user = getter(user_id_int)
        if user is None:
            try:
                user = await self.bot.fetch_user(user_id_int)
            except discord.NotFound:
                return None
            except discord.Forbidden:
                log.info("Cannot fetch admin Discord user %s for social-media report DM", discord_user_id)
                return None
            except discord.HTTPException:
                log.warning(
                    "Failed to fetch admin Discord user %s for social-media report DM",
                    discord_user_id,
                    exc_info=True,
                )
                return None
        return user


def _weekly_anchor(now: datetime | None = None) -> datetime:
    current = now.astimezone(UTC) if now else datetime.now(UTC)
    return (current - timedelta(days=current.weekday())).replace(
        hour=0,
        minute=0,
        second=0,
        microsecond=0,
    )


def _format_period(period_start: datetime, period_end: datetime) -> str:
    return (
        f"{period_start.astimezone(UTC):%d.%m.%Y} bis "
        f"{(period_end - timedelta(seconds=1)).astimezone(UTC):%d.%m.%Y}"
    )


def _split_message(content: str, *, limit: int = 1800) -> list[str]:
    text = str(content or "").strip()
    if len(text) <= limit:
        return [text]
    chunks: list[str] = []
    remaining = text
    while remaining:
        split_at = remaining.rfind("\n", 0, limit)
        if split_at <= 0:
            split_at = limit
        chunks.append(remaining[:split_at].strip())
        remaining = remaining[split_at:].lstrip()
    return chunks


async def setup(bot):
    await bot.add_cog(SocialMediaReportDispatcher(bot))

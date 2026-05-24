"""Chat-Commands für den AI-Engagement-Layer.

Fünf Commands:
- !engagement_on / !engagement_off — Toggle pro Channel (Owner/Mod/Super-Mod)
- !engagement_status — zeigt enabled/disabled + letzten Decision
- !engagement_ignore_me / !engagement_remember_me — Self-Opt-Out für Chatter
"""

from __future__ import annotations

import asyncio
import logging
from datetime import datetime, timezone

from ..storage.pg import query_one, transaction
from .constants import TWITCHIO_AVAILABLE, twitchio_commands

log = logging.getLogger("TwitchStreams.EngagementCommands")


def _sync_set_enabled(channel_login: str, enabled: bool, actor_id: str | None) -> None:
    with transaction() as conn:
        conn.execute(
            """
            INSERT INTO twitch_engagement_settings
                (channel_login, enabled, enabled_at, enabled_by, updated_at)
            VALUES (%s, %s, NOW(), %s, NOW())
            ON CONFLICT (channel_login) DO UPDATE SET
                enabled = EXCLUDED.enabled,
                enabled_at = CASE
                    WHEN EXCLUDED.enabled THEN NOW()
                    ELSE twitch_engagement_settings.enabled_at
                END,
                enabled_by = COALESCE(EXCLUDED.enabled_by, twitch_engagement_settings.enabled_by),
                updated_at = NOW();
            """,
            [channel_login, enabled, actor_id],
        )


def _sync_load_status(channel_login: str):
    settings_row = query_one(
        "SELECT enabled FROM twitch_engagement_settings WHERE channel_login = %s",
        [channel_login],
    )
    if settings_row is None:
        return None
    enabled = bool(settings_row[0])
    log_row = query_one(
        """
        SELECT decision, response_text, ts
        FROM twitch_engagement_log
        WHERE channel_login = %s
        ORDER BY ts DESC
        LIMIT 1
        """,
        [channel_login],
    )
    if log_row:
        return (enabled, log_row[0], log_row[1], log_row[2])
    return (enabled, None, None, None)


def _sync_set_optout(user_id: str, opt_out: bool) -> None:
    with transaction() as conn:
        if opt_out:
            conn.execute(
                """
                INSERT INTO twitch_user_engagement_optout (twitch_user_id)
                VALUES (%s)
                ON CONFLICT (twitch_user_id) DO NOTHING
                """,
                [user_id],
            )
        else:
            conn.execute(
                "DELETE FROM twitch_user_engagement_optout WHERE twitch_user_id = %s",
                [user_id],
            )


if TWITCHIO_AVAILABLE:

    class EngagementCommandsMixin:
        """5 Chat-Commands für Engagement-Steuerung. Eingebunden via Mixin in TwitchChatBot."""

        async def _engagement_can_toggle(self, ctx) -> bool:
            from bot.engagement.admin import is_super_mod

            is_broadcaster = getattr(
                ctx.author, "is_broadcaster", getattr(ctx.author, "broadcaster", False)
            )
            is_mod = getattr(
                ctx.author, "is_moderator", getattr(ctx.author, "moderator", False)
            )
            if is_broadcaster or is_mod:
                return True
            actor_id = getattr(ctx.author, "id", None)
            return await is_super_mod(str(actor_id) if actor_id else None)

        @twitchio_commands.command(name="engagement_on")
        async def cmd_engagement_on(self, ctx):
            """!engagement_on - AI-Engagement-Layer für diesen Channel aktivieren."""
            if not await self._engagement_can_toggle(ctx):
                await ctx.send(
                    f"@{ctx.author.name} Nur Broadcaster, Mods oder Super-Mod dürfen das."
                )
                return
            channel_login = ctx.channel.name.lower()
            actor_id = getattr(ctx.author, "id", None)
            try:
                await asyncio.to_thread(
                    _sync_set_enabled,
                    channel_login,
                    True,
                    str(actor_id) if actor_id else None,
                )
            except Exception:
                log.exception("engagement_on fehlgeschlagen für %s", channel_login)
                await ctx.send(f"@{ctx.author.name} Fehler beim Aktivieren, schau in die Logs.")
                return
            await ctx.send(
                f"@{ctx.author.name} AI-Engagement aktiviert. "
                "Deaktiviert sich automatisch bei Stream-Ende."
            )

        @twitchio_commands.command(name="engagement_off")
        async def cmd_engagement_off(self, ctx):
            """!engagement_off - AI-Engagement-Layer für diesen Channel deaktivieren."""
            if not await self._engagement_can_toggle(ctx):
                await ctx.send(
                    f"@{ctx.author.name} Nur Broadcaster, Mods oder Super-Mod dürfen das."
                )
                return
            channel_login = ctx.channel.name.lower()
            actor_id = getattr(ctx.author, "id", None)
            try:
                await asyncio.to_thread(
                    _sync_set_enabled,
                    channel_login,
                    False,
                    str(actor_id) if actor_id else None,
                )
            except Exception:
                log.exception("engagement_off fehlgeschlagen für %s", channel_login)
                await ctx.send(f"@{ctx.author.name} Fehler beim Deaktivieren, schau in die Logs.")
                return
            await ctx.send(f"@{ctx.author.name} AI-Engagement deaktiviert.")

        @twitchio_commands.command(name="engagement_status")
        async def cmd_engagement_status(self, ctx):
            """!engagement_status - Zeigt enabled/disabled + letzte AI-Aktion."""
            channel_login = ctx.channel.name.lower()
            try:
                status = await asyncio.to_thread(_sync_load_status, channel_login)
            except Exception:
                log.exception("engagement_status fehlgeschlagen für %s", channel_login)
                await ctx.send("Fehler beim Status-Abruf, schau in die Logs.")
                return
            if status is None:
                await ctx.send(f"AI-Engagement für {channel_login}: nie konfiguriert.")
                return
            enabled, last_decision, last_text, last_ts = status
            state = "AN" if enabled else "AUS"
            if last_decision and last_ts:
                ago_sec = int((datetime.now(timezone.utc) - last_ts).total_seconds())
                snippet = (last_text or "").strip()
                if len(snippet) > 80:
                    snippet = snippet[:77] + "…"
                tail = f" — “{snippet}”" if snippet else ""
                await ctx.send(
                    f"AI-Engagement: {state}. Letzte Aktion: {last_decision} vor {ago_sec}s{tail}."
                )
            else:
                await ctx.send(f"AI-Engagement: {state}. Noch keine Aktionen geloggt.")

        @twitchio_commands.command(name="engagement_ignore_me")
        async def cmd_engagement_ignore_me(self, ctx):
            """!engagement_ignore_me - AI ignoriert deine Nachrichten ab sofort."""
            user_id = getattr(ctx.author, "id", None)
            if not user_id:
                await ctx.send(f"@{ctx.author.name} Konnte deine User-ID nicht ermitteln.")
                return
            try:
                await asyncio.to_thread(_sync_set_optout, str(user_id), True)
            except Exception:
                log.exception("engagement_ignore_me fehlgeschlagen für %s", user_id)
                await ctx.send(f"@{ctx.author.name} Fehler beim Opt-Out, schau in die Logs.")
                return
            await ctx.send(
                f"@{ctx.author.name} OK, AI ignoriert dich ab sofort. "
                "Mit !engagement_remember_me wieder einschalten."
            )

        @twitchio_commands.command(name="engagement_remember_me")
        async def cmd_engagement_remember_me(self, ctx):
            """!engagement_remember_me - Opt-Out zurücknehmen."""
            user_id = getattr(ctx.author, "id", None)
            if not user_id:
                await ctx.send(f"@{ctx.author.name} Konnte deine User-ID nicht ermitteln.")
                return
            try:
                await asyncio.to_thread(_sync_set_optout, str(user_id), False)
            except Exception:
                log.exception("engagement_remember_me fehlgeschlagen für %s", user_id)
                await ctx.send(f"@{ctx.author.name} Fehler beim Opt-In, schau in die Logs.")
                return
            await ctx.send(f"@{ctx.author.name} OK, AI berücksichtigt dich wieder.")

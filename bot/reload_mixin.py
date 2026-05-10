"""Discord slash commands for hot-reloading individual Twitch subsystems."""

from __future__ import annotations

from datetime import UTC, datetime

import discord
from discord import app_commands


_SUBSYSTEM_CHOICES = [
    app_commands.Choice(name="Analytics",  value="analytics"),
    app_commands.Choice(name="Community",  value="community"),
    app_commands.Choice(name="Social",     value="social"),
    app_commands.Choice(name="Monitoring", value="monitoring"),
    app_commands.Choice(name="Chat",       value="chat"),
    app_commands.Choice(name="Dashboard",  value="dashboard"),
    app_commands.Choice(name="Raid",       value="raid"),
]

_STATE_EMOJI = {
    "ok":      "🟢",
    "partial": "🟡",
    "error":   "🔴",
    "idle":    "⚪",
}


class TwitchReloadMixin:
    """Slash commands: /twitch-reload, /twitch-status."""

    # ------------------------------------------------------------------
    # /twitch-reload
    # ------------------------------------------------------------------

    @app_commands.command(
        name="twitch-reload",
        description="Lädt ein einzelnes Twitch-Subsystem neu (ohne Bot-Neustart).",
    )
    @app_commands.choices(subsystem=_SUBSYSTEM_CHOICES)
    @app_commands.default_permissions(administrator=True)
    async def cmd_twitch_reload(
        self,
        interaction: discord.Interaction,
        subsystem: app_commands.Choice[str],
    ) -> None:
        """Hot-reload a single Twitch subsystem."""
        await interaction.response.defer(ephemeral=True)

        manager = getattr(self, "_reload_manager", None)
        if manager is None:
            await interaction.followup.send(
                "❌ Reload-Manager nicht initialisiert.", ephemeral=True
            )
            return

        ok, msg = await manager.reload(subsystem.value)

        colour = discord.Colour.green() if ok else discord.Colour.red()
        icon = "✅" if ok else "❌"
        embed = discord.Embed(
            title=f"{icon} Subsystem Reload: {subsystem.name}",
            description=msg,
            colour=colour,
            timestamp=datetime.now(UTC),
        )
        embed.set_footer(text=f"Subsystem: {subsystem.value}")
        await interaction.followup.send(embed=embed, ephemeral=True)

    # ------------------------------------------------------------------
    # /twitch-status
    # ------------------------------------------------------------------

    @app_commands.command(
        name="twitch-status",
        description="Zeigt den Status aller Twitch-Subsysteme und ihrer Loops.",
    )
    @app_commands.default_permissions(administrator=True)
    async def cmd_twitch_status(
        self,
        interaction: discord.Interaction,
    ) -> None:
        """Display running status of all subsystems."""
        await interaction.response.defer(ephemeral=True)

        manager = getattr(self, "_reload_manager", None)
        if manager is None:
            await interaction.followup.send(
                "❌ Reload-Manager nicht initialisiert.", ephemeral=True
            )
            return

        states = manager.get_all_states()
        lines: list[str] = []

        for name, state in states.items():
            sub = manager.get_subsystem(name)
            display = sub.display_name if sub else name
            total_loops = len(sub.loops) if sub else 0
            hot = "♻️" if (sub and sub.hot_reloadable) else "🔒"

            if state.error:
                emoji = _STATE_EMOJI["error"]
            elif total_loops > 0 and state.running_loops == total_loops:
                emoji = _STATE_EMOJI["ok"]
            elif total_loops > 0 and state.running_loops > 0:
                emoji = _STATE_EMOJI["partial"]
            else:
                emoji = _STATE_EMOJI["idle"]

            loop_info = (
                f"{state.running_loops}/{total_loops} loops"
                if total_loops > 0
                else "no loops"
            )

            if state.last_reload:
                ts = int(state.last_reload.replace(tzinfo=UTC).timestamp())
                reload_str = f" | last reload: <t:{ts}:R>"
            else:
                reload_str = ""

            line = f"{emoji} {hot} **{display}** — {loop_info}{reload_str}"
            if state.error:
                line += f"\n  ⚠️ {state.error[:120]}"
            lines.append(line)

        embed = discord.Embed(
            title="🔄 Twitch Subsystem Status",
            description="\n".join(lines) or "Keine Subsysteme registriert.",
            colour=discord.Colour.blurple(),
            timestamp=datetime.now(UTC),
        )
        embed.set_footer(text="♻️ = hot-reloadable  |  🔒 = full-reload only")
        await interaction.followup.send(embed=embed, ephemeral=True)

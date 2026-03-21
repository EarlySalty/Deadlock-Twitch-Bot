"""Shared Discord views and embeds for Twitch raid authorization."""

from __future__ import annotations

import logging
import os
from urllib.parse import urlencode, urlsplit, urlunsplit

import aiohttp
import discord

from ..internal_api import INTERNAL_API_BASE_PATH, INTERNAL_TOKEN_HEADER

log = logging.getLogger("TwitchStreams.RaidViews")

AUTH_BUTTON_LABEL = "Link für deinen Kanal erzeugen"
AUTH_LINK_LABEL = "Bot für deinen Kanal aktivieren"


def _parse_env_bool(var_name: str, default: bool = False) -> bool:
    raw = (os.getenv(var_name) or "").strip().lower()
    if not raw:
        return default
    if raw in {"1", "true", "yes", "on"}:
        return True
    if raw in {"0", "false", "no", "off"}:
        return False
    return default


def _split_runtime_enforced_role() -> str:
    role = (os.getenv("TWITCH_SPLIT_RUNTIME_ROLE") or "").strip().lower()
    if not role:
        return ""
    if role not in {"bot", "dashboard"}:
        return ""
    if not _parse_env_bool("TWITCH_SPLIT_RUNTIME_ENFORCE", False):
        return ""
    return role


def _split_internal_api_auth_url(
    twitch_login: str,
    discord_user_id: int,
) -> tuple[str, dict[str, str]] | None:
    base_url = (os.getenv("TWITCH_INTERNAL_API_BASE_URL") or "").strip()
    from ..secret_store import load_secret_value

    token = load_secret_value(
        "TWITCH_INTERNAL_API_TOKEN",
        prefer_env=True,
        allow_empty_env_override=True,
    )
    if not base_url or not token:
        return None

    raw = base_url if "://" in base_url else f"http://{base_url}"
    try:
        parsed = urlsplit(raw)
    except Exception:
        return None

    if not parsed.scheme or not parsed.netloc:
        return None

    base_path = (parsed.path or "").rstrip("/")
    internal_base = INTERNAL_API_BASE_PATH.rstrip("/")
    if base_path == internal_base:
        base_path = ""
    elif base_path.endswith(internal_base):
        base_path = base_path[: -len(internal_base)]

    normalized_base = urlunsplit(
        (parsed.scheme, parsed.netloc, base_path.rstrip("/"), "", "")
    )
    endpoint = f"{normalized_base.rstrip('/')}{internal_base}/raid/auth-url"
    query = urlencode(
        {
            "login": twitch_login,
            "discord_user_id": str(discord_user_id),
        }
    )
    headers = {INTERNAL_TOKEN_HEADER: token}
    return f"{endpoint}?{query}", headers


def _prefer_split_internal_raid_auth_api() -> bool:
    if _split_internal_api_auth_url("placeholder", 0) is None:
        return False
    return _split_runtime_enforced_role() != "bot"


async def _fetch_split_raid_auth_url(twitch_login: str, discord_user_id: int) -> str | None:
    request_data = _split_internal_api_auth_url(twitch_login, discord_user_id)
    if request_data is None:
        return None

    url, headers = request_data
    timeout = aiohttp.ClientTimeout(total=8.0)

    try:
        async with aiohttp.ClientSession(timeout=timeout) as session:
            async with session.get(url, headers=headers) as response:
                body = await response.text()
                payload: dict[str, object] = {}
                if body:
                    try:
                        decoded = await response.json(content_type=None)
                    except Exception:
                        decoded = {}
                    if isinstance(decoded, dict):
                        payload = decoded

                if response.status != 200:
                    log.warning(
                        "Split-API raid auth url failed (%s): %s",
                        response.status,
                        str(payload.get("message") or body or "").strip()[:200],
                    )
                    return None

                auth_url = str(payload.get("auth_url") or "").strip()
                return auth_url or None
    except Exception as exc:
        log.warning("Split-API raid auth request failed: %r", exc)
        return None


def build_raid_requirements_embed(twitch_login: str) -> discord.Embed:
    """Build the shared requirements embed for raid authorization."""
    login = (twitch_login or "").strip() or "dein Kanal"
    description = (
        f"Hey **{login}**!\n\n"
        "Dein Kanal ist noch nicht vollständig für das **Deadlock-Partnernetzwerk** eingerichtet.\n\n"
        "Was noch fehlt:\n"
        "Du musst den Bot einmal für deinen Kanal aktivieren.\n\n"
        "**Was dir das bringt**\n"
        "1) **Auto-Raid im Deadlock-Partnernetzwerk**\n"
        "- Wenn du Deadlock streamst und offline gehst, kann der Bot deine Viewer automatisch an passende Partner weiterleiten.\n"
        "2) **Chat Guard**\n"
        '- Der Bot filtert typische Spam- und Viewer-Bot-Muster wie "Best viewers" oder "streamboo.com".\n'
        "3) **Dashboard & weitere Funktionen**\n"
        "- Nach der Aktivierung stehen dir Dashboard, Netzwerk-Funktionen und weitere Tools zur Verfügung.\n\n"
        "Wichtig: Auto-Raids greifen nur bei Deadlock. Dein normales manuelles Raiden bleibt unverändert.\n"
    )
    return discord.Embed(
        title="🔗 Bot für deinen Kanal aktivieren",
        description=description,
        color=0x9146FF,
    )


async def _send_interaction_message(
    interaction: discord.Interaction,
    content: str,
    *,
    view: discord.ui.View | None = None,
) -> None:
    """Send a response or follow-up, using ephemeral only in guilds."""
    ephemeral = interaction.guild_id is not None
    kwargs = {"view": view} if view else {}
    if ephemeral:
        kwargs["ephemeral"] = True

    try:
        if interaction.response.is_done():
            await interaction.followup.send(content, **kwargs)
        else:
            await interaction.response.send_message(content, **kwargs)
    except Exception:
        log.exception("Failed to respond with raid auth link")


class _RaidAuthGenerateButton(discord.ui.Button):
    def __init__(self, twitch_login: str, *, label: str) -> None:
        login = (twitch_login or "").strip().lower()
        super().__init__(
            label=label,
            style=discord.ButtonStyle.primary,
            custom_id=f"raid_auth_generate:{login}",
        )
        self._twitch_login = login

    async def callback(self, interaction: discord.Interaction) -> None:  # type: ignore[override]
        try:
            login = self._twitch_login
            if not login:
                await interaction.response.send_message(
                    "⚠️ Bot nicht bereit – bitte kurz warten und nochmal versuchen.\n"
                    "Alternativ kannst du `/traid` in Discord nutzen.",
                    ephemeral=True,
                )
                return

            button_url = ""
            if _prefer_split_internal_raid_auth_api():
                button_url = str(
                    await _fetch_split_raid_auth_url(login, int(interaction.user.id)) or ""
                ).strip()

            if not button_url:
                # Auth-Manager dynamisch aus dem Cog holen (restart-sicher)
                auth_manager = None
                for cog in interaction.client.cogs.values():
                    if hasattr(cog, "_raid_bot") and getattr(cog, "_raid_bot", None):
                        auth_manager = cog._raid_bot.auth_manager  # type: ignore[union-attr]
                        break
                if auth_manager:
                    button_url = str(
                        auth_manager.generate_discord_button_url(
                            login,
                            discord_user_id=str(interaction.user.id),
                        )
                        or ""
                    ).strip()
                elif not _prefer_split_internal_raid_auth_api():
                    # Legacy fallback: if split mode is not preferred, try internal API if configured.
                    button_url = str(
                        await _fetch_split_raid_auth_url(login, int(interaction.user.id))
                        or ""
                    ).strip()

            if not button_url:
                await interaction.response.send_message(
                    "⚠️ Bot nicht bereit – bitte kurz warten und nochmal versuchen.\n"
                    "Alternativ kannst du `/traid` in Discord nutzen.",
                    ephemeral=True,
                )
                return

            # generate_discord_button_url liefert einen kurzen Redirect-URL (<512 Zeichen)
            # statt des vollen Twitch-OAuth-URL der das Discord-Limit überschreiten würde.
            link_view = discord.ui.View(timeout=300)
            link_view.add_item(
                discord.ui.Button(
                    label=AUTH_LINK_LABEL,
                    url=button_url,
                    style=discord.ButtonStyle.link,
                )
            )
            content = (
                f"Hier ist dein Link, um den Bot für **{login}** zu aktivieren.\n"
                "Bitte innerhalb von 10 Minuten öffnen, danach läuft der Link ab."
            )
            await _send_interaction_message(interaction, content, view=link_view)

        except Exception:
            log.exception("RaidAuthButton callback failed for %s", self._twitch_login)
            try:
                if not interaction.response.is_done():
                    await interaction.response.send_message(
                        "❌ Fehler beim Erzeugen des Links. Bitte nutze `/traid` in Discord.",
                        ephemeral=True,
                    )
            except Exception:
                log.debug(
                    "RaidAuthButton fallback error message failed for %s",
                    self._twitch_login,
                    exc_info=True,
                )


class RaidAuthGenerateView(discord.ui.View):
    """View that generates a fresh OAuth link on click. Persistent across bot restarts."""

    def __init__(
        self,
        *,
        auth_manager=None,  # Nur noch für Kompatibilität, wird im Button dynamisch geholt
        twitch_login: str,
        button_label: str = AUTH_BUTTON_LABEL,
    ) -> None:
        super().__init__(timeout=None)  # persistent – kein Timeout
        self.auth_manager = auth_manager  # optional, Button holt es selbst
        self.twitch_login = (twitch_login or "").strip().lower()
        self.add_item(_RaidAuthGenerateButton(self.twitch_login, label=button_label))

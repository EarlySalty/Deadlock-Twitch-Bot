# cogs/twitch/raid_manager.py
"""
Raid Bot Manager - RaidExecutor

Verwaltet:
- Raid-Ausführung und Metadaten-Speicherung
"""

import logging
import os
import time

import aiohttp

from ..storage import transaction
from .scope_profiles import BASE_STREAMER_SCOPES

TWITCH_TOKEN_URL = "https://id.twitch.tv/oauth2/token"  # noqa: S105
TWITCH_AUTHORIZE_URL = "https://id.twitch.tv/oauth2/authorize"
TWITCH_API_BASE = "https://api.twitch.tv/helix"

# Erforderliche Scopes für Raid-Funktionalität + Zusatz-Metriken (Follower/Chat)
# Hinweis: Re-Auth notwendig, falls bisher nur channel:manage:raids erteilt war.
RAID_SCOPES = list(BASE_STREAMER_SCOPES)

RAID_TARGET_COOLDOWN_DAYS = 7  # Avoid repeating the same raid target if alternatives exist
RECRUIT_DISCORD_INVITE = (
    os.getenv("RECRUIT_DISCORD_INVITE") or ""
).strip() or "Discord: Server hinzufügen & Code eingeben: z5TfVHuQq2"
RECRUIT_DISCORD_INVITE_DIRECT = (
    os.getenv("RECRUIT_DISCORD_INVITE_DIRECT") or ""
).strip() or "https://discord.gg/z5TfVHuQq2"

_recruit_direct_invite_threshold_raw = (
    os.getenv("RECRUIT_DIRECT_INVITE_MAX_FOLLOWERS") or "120"
).strip()
try:
    RECRUIT_DIRECT_INVITE_MAX_FOLLOWERS = max(0, int(_recruit_direct_invite_threshold_raw))
except ValueError:
    RECRUIT_DIRECT_INVITE_MAX_FOLLOWERS = 120


def _parse_env_int(name: str, default: int = 0) -> int:
    raw = (os.getenv(name) or "").strip()
    if not raw:
        return default
    try:
        return int(raw)
    except ValueError:
        return default


STREAMER_ROLE_ID = _parse_env_int("STREAMER_ROLE_ID", 1313624729466441769)
STREAMER_GUILD_ID = _parse_env_int("STREAMER_GUILD_ID", 0)
FALLBACK_MAIN_GUILD_ID = _parse_env_int("MAIN_GUILD_ID", 0)

log = logging.getLogger("TwitchStreams.RaidManager")


def _mask_log_identifier(value: object, *, visible_prefix: int = 3, visible_suffix: int = 2) -> str:
    text = str(value or "").strip()
    if not text:
        return "<empty>"
    if len(text) <= visible_prefix + visible_suffix:
        return "***"
    return f"{text[:visible_prefix]}...{text[-visible_suffix:]}"


from .auth import RaidAuthManager  # noqa: E402


class RaidExecutor:
    """Führt Raids aus und speichert Metadaten."""

    def __init__(self, client_id: str, auth_manager: RaidAuthManager):
        self.client_id = client_id
        self.auth_manager = auth_manager

    async def start_raid(
        self,
        from_broadcaster_id: str,
        from_broadcaster_login: str,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        viewer_count: int,
        stream_duration_sec: int,
        target_stream_started_at: str,
        candidates_count: int,
        session: aiohttp.ClientSession,
        reason: str = "auto_raid_on_offline",
    ) -> tuple[bool, str | None]:
        """
        Startet einen Raid von from_broadcaster zu to_broadcaster.

        Returns:
            (success, error_message)
        """
        # Access Token holen
        access_token = await self.auth_manager.get_valid_token(from_broadcaster_id, session)
        if not access_token:
            error_msg = f"No valid token for {from_broadcaster_login}"
            log.warning(error_msg)
            self._save_raid_history(
                from_broadcaster_id,
                from_broadcaster_login,
                to_broadcaster_id,
                to_broadcaster_login,
                viewer_count,
                stream_duration_sec,
                target_stream_started_at,
                candidates_count,
                reason,
                success=False,
                error_message=error_msg,
            )
            return False, error_msg

        # Raid über Twitch API starten
        url = f"{TWITCH_API_BASE}/raids"
        params = {
            "from_broadcaster_id": from_broadcaster_id,
            "to_broadcaster_id": to_broadcaster_id,
        }
        headers = {
            "Client-ID": self.client_id,
            "Authorization": f"Bearer {access_token}",
        }

        try:
            api_started = time.monotonic()
            async with session.post(url, headers=headers, params=params) as r:
                api_elapsed_ms = (time.monotonic() - api_started) * 1000.0
                if r.status != 200:
                    txt = await r.text()
                    error_msg = (
                        f"Raid API failed in {api_elapsed_ms:.0f}ms: HTTP {r.status}: {txt[:200]}"
                    )
                    log.error(error_msg)
                    self._save_raid_history(
                        from_broadcaster_id,
                        from_broadcaster_login,
                        to_broadcaster_id,
                        to_broadcaster_login,
                        viewer_count,
                        stream_duration_sec,
                        target_stream_started_at,
                        candidates_count,
                        reason,
                        success=False,
                        error_message=error_msg,
                    )
                    return False, error_msg

                # Erfolg!
                log.info(
                    "Raid successful: %s -> %s (%d viewers, %d candidates, api=%.0fms)",
                    from_broadcaster_login,
                    to_broadcaster_login,
                    viewer_count,
                    candidates_count,
                    api_elapsed_ms,
                )
                self._save_raid_history(
                    from_broadcaster_id,
                    from_broadcaster_login,
                    to_broadcaster_id,
                    to_broadcaster_login,
                    viewer_count,
                    stream_duration_sec,
                    target_stream_started_at,
                    candidates_count,
                    reason,
                    success=True,
                    error_message=None,
                )
                return True, None

        except Exception as e:
            error_msg = f"Exception during raid: {e}"
            log.exception("Raid exception: %s -> %s", from_broadcaster_login, to_broadcaster_login)
            self._save_raid_history(
                from_broadcaster_id,
                from_broadcaster_login,
                to_broadcaster_id,
                to_broadcaster_login,
                viewer_count,
                stream_duration_sec,
                target_stream_started_at,
                candidates_count,
                reason,
                success=False,
                error_message=error_msg,
            )
            return False, error_msg

    async def cancel_raid(
        self,
        from_broadcaster_id: str,
        from_broadcaster_login: str,
        session: aiohttp.ClientSession,
    ) -> tuple[bool, str | None]:
        """Bricht einen ausstehenden Raid des Broadcasters ab (DELETE /helix/raids).

        Funktioniert nur, solange der 90-Sekunden-Countdown läuft. Klickt der
        Streamer vorher "Raid Now" oder ist der Countdown abgelaufen, existiert
        kein ausstehender Raid mehr und Twitch liefert einen Fehler.
        """
        access_token = await self.auth_manager.get_valid_token(from_broadcaster_id, session)
        if not access_token:
            return False, f"No valid token for {from_broadcaster_login}"

        url = f"{TWITCH_API_BASE}/raids"
        params = {"broadcaster_id": from_broadcaster_id}
        headers = {
            "Client-ID": self.client_id,
            "Authorization": f"Bearer {access_token}",
        }
        try:
            async with session.delete(url, headers=headers, params=params) as r:
                if r.status not in (200, 204):
                    txt = await r.text()
                    error_msg = f"Cancel-Raid API failed: HTTP {r.status}: {txt[:200]}"
                    log.warning(
                        "Cancel raid failed for %s: HTTP %s",
                        from_broadcaster_login,
                        r.status,
                    )
                    return False, error_msg
                log.info("Raid cancelled for %s", from_broadcaster_login)
                return True, None
        except Exception as e:  # noqa: BLE001
            log.exception("Cancel raid exception for %s", from_broadcaster_login)
            return False, f"Exception during cancel raid: {e}"

    def _save_raid_history(
        self,
        from_broadcaster_id: str,
        from_broadcaster_login: str,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        viewer_count: int,
        stream_duration_sec: int,
        target_stream_started_at: str,
        candidates_count: int,
        reason: str,
        success: bool,
        error_message: str | None,
    ) -> None:
        """Speichert Raid-Metadaten in der Datenbank."""
        history_reason = (reason or "").strip() or "auto_raid_on_offline"
        with transaction() as conn:
            conn.execute(
                """
                INSERT INTO twitch_raid_history
                (from_broadcaster_id, from_broadcaster_login, to_broadcaster_id,
                 to_broadcaster_login, viewer_count, stream_duration_sec, reason,
                 success, error_message, target_stream_started_at, candidates_count)
                VALUES (%s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s)
                """,
                (
                    from_broadcaster_id,
                    from_broadcaster_login,
                    to_broadcaster_id,
                    to_broadcaster_login,
                    viewer_count,
                    stream_duration_sec,
                    history_reason,
                    bool(success),
                    error_message,
                    target_stream_started_at,
                    candidates_count,
                ),
            )
            # autocommit – no explicit commit needed

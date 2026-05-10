"""
Social Media Token Refresh Worker - Auto-refresh tokens before expiry.

Background worker that:
1. Runs every 5 minutes
2. Checks for tokens expiring within 1 hour
3. Refreshes them automatically
4. Updates encrypted storage
5. Logs failures for manual intervention

Pattern: Similar to TwitchBotTokenManager
"""

import asyncio
import logging
import os
from datetime import UTC, datetime, timedelta

import discord
from discord.ext import commands

try:
    from service.field_crypto import get_crypto
except ModuleNotFoundError:  # pragma: no cover - split runtime fallback
    from ..compat.field_crypto import get_crypto

from ..storage import readonly_connection, transaction
from .oauth_manager import OAuthTokenRefreshError, SocialMediaOAuthManager

log = logging.getLogger("TwitchStreams.TokenRefreshWorker")

_DEFAULT_ADMIN_DISCORD_USER_ID = "662995601738170389"  # nosemgrep: discord-client-id
_GLOBAL_STREAMER_SCOPE = "__global__"


def _sanitize_log_value(value):
    """Prevent CRLF log-forging via untrusted values."""
    if value is None:
        return "<none>"
    return str(value).replace("\r", "\\r").replace("\n", "\\n")


def _utcnow() -> datetime:
    return datetime.now(UTC)


class SocialMediaTokenRefreshWorker(commands.Cog):
    """Background worker for automatic token refresh."""

    def __init__(self, bot):
        """Initialize worker."""
        self.bot = bot
        self.enabled = True
        self.interval_seconds = 5 * 60  # 5 minutes
        self.refresh_threshold_hours = 1  # Refresh if expires within 1 hour

        self.crypto = get_crypto()
        self.oauth_manager = SocialMediaOAuthManager()

        # Start background task
        self._task = bot.loop.create_task(self._refresh_loop())
        log.info(
            "Auth refresh worker started (interval=%ss, threshold=%sh)",
            self.interval_seconds,
            self.refresh_threshold_hours,
        )

    def cog_unload(self):
        """Cleanup on cog unload."""
        if self._task:
            self._task.cancel()
        log.info("Auth refresh worker stopped")

    async def _refresh_loop(self):
        """Main refresh loop."""
        await self.bot.wait_until_ready()
        await asyncio.sleep(60)  # Initial delay

        while not self.bot.is_closed() and self.enabled:
            try:
                await self._refresh_expiring_tokens()
            except Exception:
                log.exception("Token refresh run failed")

            await asyncio.sleep(self.interval_seconds)

    async def _refresh_expiring_tokens(self):
        """Refresh tokens expiring within threshold."""
        threshold = _utcnow() + timedelta(hours=self.refresh_threshold_hours)

        # Find tokens expiring soon
        with readonly_connection() as conn:
            expiring = conn.execute(
                """
                SELECT id, platform, streamer_login,
                       refresh_token_enc, client_id, client_secret_enc,
                       token_expires_at, enc_version
                FROM social_media_platform_auth
                WHERE enabled = 1
                  AND refresh_token_enc IS NOT NULL
                  AND token_expires_at IS NOT NULL
                  AND token_expires_at < %s
                ORDER BY token_expires_at ASC
                """,
                (threshold.isoformat(),),
            ).fetchall()

        if not expiring:
            log.debug("No auth entries expiring within %sh", self.refresh_threshold_hours)
            return

        log.info(
            "Found %s auth entries expiring within %sh",
            len(expiring),
            self.refresh_threshold_hours,
        )

        # Refresh each token
        for row in expiring:
            try:
                await self._refresh_platform_token(row)
            except Exception:
                safe_platform = _sanitize_log_value(row["platform"])
                safe_streamer = _sanitize_log_value(row["streamer_login"])
                log.exception(
                    "Failed to refresh OAuth auth data for platform=%s, streamer=%s",
                    safe_platform,
                    safe_streamer,
                )

    async def _refresh_platform_token(self, row: dict):
        """
        Refresh a single platform token.

        Args:
            row: Database row with encrypted tokens
        """
        platform = row["platform"]
        streamer_login = row["streamer_login"]
        safe_platform = _sanitize_log_value(platform)
        safe_streamer = _sanitize_log_value(streamer_login)
        row_id = f"{platform}|{streamer_login or 'global'}"

        log.info(
            "Refreshing OAuth auth data for platform=%s, streamer=%s",
            safe_platform,
            safe_streamer,
        )

        # Decrypt refresh token
        aad_refresh = f"social_media_platform_auth|refresh_token|{row_id}|{row['enc_version']}"
        refresh_token = self.crypto.decrypt_field(row["refresh_token_enc"], aad_refresh)

        # Decrypt client secret (if exists)
        client_secret = None
        if row["client_secret_enc"]:
            aad_secret = f"social_media_platform_auth|client_secret|{row_id}|{row['enc_version']}"
            client_secret = self.crypto.decrypt_field(row["client_secret_enc"], aad_secret)

        # Refresh token via OAuth manager
        try:
            new_tokens = await self.oauth_manager.refresh_token(
                platform=platform,
                refresh_token=refresh_token,
                client_id=row["client_id"],
                client_secret=client_secret or "",
            )
        except OAuthTokenRefreshError as exc:
            log.error(
                "OAuth auth refresh failed for platform=%s, streamer=%s, error_kind=%s",
                safe_platform,
                safe_streamer,
                _sanitize_log_value(exc.error_kind),
            )
            if not exc.transient:
                await self._notify_admin_reauth_required(
                    platform=platform,
                    streamer_login=streamer_login,
                    error_kind=exc.error_kind,
                    details=str(exc),
                )
            return
        except Exception:
            log.error(
                "OAuth auth refresh failed for platform=%s, streamer=%s",
                safe_platform,
                safe_streamer,
            )
            return

        # Save new tokens (encrypted)
        await self._save_refreshed_tokens(
            platform=platform,
            streamer_login=streamer_login,
            row_id=row_id,
            new_tokens=new_tokens,
        )

        log.info(
            "OAuth auth data refreshed successfully for platform=%s, streamer=%s",
            safe_platform,
            safe_streamer,
        )

    async def _save_refreshed_tokens(
        self, platform: str, streamer_login: str, row_id: str, new_tokens: dict
    ):
        """Save refreshed tokens to database."""
        # Encrypt new access token
        aad_access = f"social_media_platform_auth|access_token|{row_id}|1"
        access_enc = self.crypto.encrypt_field(new_tokens["access_token"], aad_access, kid="v1")

        # Encrypt new refresh token (if provided)
        refresh_enc = None
        if new_tokens.get("refresh_token"):
            aad_refresh = f"social_media_platform_auth|refresh_token|{row_id}|1"
            refresh_enc = self.crypto.encrypt_field(
                new_tokens["refresh_token"], aad_refresh, kid="v1"
            )

        # Update database
        with transaction() as conn:
            if refresh_enc:
                # Update both access and refresh tokens
                conn.execute(
                    """
                    UPDATE social_media_platform_auth
                    SET access_token_enc = %s,
                        refresh_token_enc = %s,
                        token_expires_at = %s,
                        last_refreshed_at = CURRENT_TIMESTAMP
                    WHERE platform = %s AND (streamer_login = %s OR (streamer_login IS NULL AND %s IS NULL))
                    """,
                    (
                        access_enc,
                        refresh_enc,
                        new_tokens["expires_at"].isoformat()
                        if isinstance(new_tokens["expires_at"], datetime)
                        else new_tokens["expires_at"],
                        platform,
                        streamer_login,
                        streamer_login,
                    ),
                )
            else:
                # Update only access token (keep existing refresh token)
                conn.execute(
                    """
                    UPDATE social_media_platform_auth
                    SET access_token_enc = %s,
                        token_expires_at = %s,
                        last_refreshed_at = CURRENT_TIMESTAMP
                    WHERE platform = %s AND (streamer_login = %s OR (streamer_login IS NULL AND %s IS NULL))
                    """,
                    (
                        access_enc,
                        new_tokens["expires_at"].isoformat()
                        if isinstance(new_tokens["expires_at"], datetime)
                        else new_tokens["expires_at"],
                        platform,
                        streamer_login,
                        streamer_login,
                    ),
                )

    @staticmethod
    def _notification_scope(streamer_login: str | None) -> str:
        normalized = str(streamer_login or "").strip().lower()
        return normalized or _GLOBAL_STREAMER_SCOPE

    @staticmethod
    def _admin_discord_user_id() -> str:
        for env_name in (
            "SOCIAL_MEDIA_REAUTH_ADMIN_DISCORD_USER_ID",
            "TWITCH_ADMIN_DISCORD_USER_ID",
        ):
            value = str(os.getenv(env_name) or "").strip()
            if value:
                return value
        return _DEFAULT_ADMIN_DISCORD_USER_ID

    def _notification_due(
        self,
        *,
        streamer_login: str | None,
        platform: str,
        error_kind: str,
        now: datetime,
    ) -> bool:
        scope = self._notification_scope(streamer_login)
        with readonly_connection() as conn:
            row = conn.execute(
                """
                SELECT last_sent_at
                FROM social_media_reauth_notifications
                WHERE streamer_login = %s
                  AND platform = %s
                  AND error_kind = %s
                """,
                (
                    scope,
                    platform,
                    error_kind,
                ),
            ).fetchone()
        if not row:
            return True

        last_sent_at = row["last_sent_at"] if hasattr(row, "keys") else row[0]
        if last_sent_at is None:
            return True
        return last_sent_at <= now - timedelta(hours=24)

    def _record_notification_sent(
        self,
        *,
        streamer_login: str | None,
        platform: str,
        error_kind: str,
        now: datetime,
    ) -> None:
        scope = self._notification_scope(streamer_login)
        with transaction() as conn:
            conn.execute(
                """
                INSERT INTO social_media_reauth_notifications (
                    streamer_login,
                    platform,
                    error_kind,
                    last_sent_at
                )
                VALUES (%s, %s, %s, %s)
                ON CONFLICT (streamer_login, platform, error_kind) DO UPDATE
                SET last_sent_at = EXCLUDED.last_sent_at
                """,
                (
                    scope,
                    platform,
                    error_kind,
                    now,
                ),
            )

    async def _resolve_admin_user(self) -> discord.abc.User | None:
        discord_user_id = self._admin_discord_user_id()
        try:
            user_id_int = int(discord_user_id)
        except (TypeError, ValueError):
            log.warning("Invalid admin Discord user id configured for social-media reauth DM")
            return None

        user = None
        getter = getattr(self.bot, "get_user", None)
        if callable(getter):
            user = getter(user_id_int)
        if user is None:
            try:
                user = await self.bot.fetch_user(user_id_int)
            except discord.NotFound:
                user = None
            except discord.Forbidden:
                log.info("Cannot fetch admin Discord user %s for social-media reauth DM", discord_user_id)
                return None
            except discord.HTTPException:
                log.warning(
                    "Failed to fetch admin Discord user %s for social-media reauth DM",
                    discord_user_id,
                    exc_info=True,
                )
                return None
        return user

    async def _notify_admin_reauth_required(
        self,
        *,
        platform: str,
        streamer_login: str | None,
        error_kind: str,
        details: str,
    ) -> None:
        now = _utcnow()
        if not self._notification_due(
            streamer_login=streamer_login,
            platform=platform,
            error_kind=error_kind,
            now=now,
        ):
            return

        admin_user = await self._resolve_admin_user()
        if admin_user is None:
            return

        safe_streamer = str(streamer_login or "global").strip() or "global"
        truncated_details = str(details or "").replace("\n", " ").strip()
        if len(truncated_details) > 240:
            truncated_details = truncated_details[:240] + "..."

        embed = discord.Embed(
            title="Social Media Re-Auth erforderlich",
            description=(
                "Ein Social-Media-Refresh ist dauerhaft fehlgeschlagen. "
                "Bitte die Verbindung im Dashboard neu autorisieren."
            ),
            color=discord.Color.orange(),
            timestamp=now,
        )
        embed.add_field(name="Streamer", value=safe_streamer, inline=True)
        embed.add_field(name="Plattform", value=platform, inline=True)
        embed.add_field(name="Fehler", value=error_kind, inline=True)
        embed.add_field(
            name="Aktion",
            value=(
                "Im Social-Media-Dashboard die betroffene Plattform fuer diesen Streamer "
                "neu verbinden."
            ),
            inline=False,
        )
        if truncated_details:
            embed.add_field(name="Details", value=f"```{truncated_details}```", inline=False)
        embed.set_footer(text="Twitch Streams • Social Media Token Refresh")

        try:
            await admin_user.send(embed=embed)
            self._record_notification_sent(
                streamer_login=streamer_login,
                platform=platform,
                error_kind=error_kind,
                now=now,
            )
            log.info(
                "Sent social-media reauth DM for platform=%s, streamer=%s, error_kind=%s",
                _sanitize_log_value(platform),
                _sanitize_log_value(streamer_login),
                _sanitize_log_value(error_kind),
            )
        except discord.Forbidden:
            log.info("Cannot DM configured admin user for social-media reauth notification")
        except Exception:
            log.warning("Failed to send social-media reauth DM", exc_info=True)


async def setup(bot):
    """Setup function for Discord.py cog."""
    await bot.add_cog(SocialMediaTokenRefreshWorker(bot))

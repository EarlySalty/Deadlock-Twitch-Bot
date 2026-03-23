"""Token Error Handler für Twitch OAuth Refresh-Fehler.

Verwaltet:
- Blacklist für ungültige Refresh-Tokens
- Discord-Benachrichtigungen bei Token-Problemen
- Verhindert endlose Refresh-Versuche
"""

import logging
from datetime import UTC, datetime, timedelta

import discord

from ..discord_role_sync import (
    normalize_discord_user_id,
    schedule_streamer_role_sync as schedule_discord_role_sync,
)
from ..storage import pg as storage_pg
from ..storage import (
    load_streamer_identity,
    readonly_connection,
    set_partner_raid_bot_enabled,
    transaction,
)

log = logging.getLogger("TwitchStreams.TokenErrorHandler")

# Kanal-ID für Token-Fehler-Benachrichtigungen (Admin)
TOKEN_ERROR_CHANNEL_ID = 1374364800817303632

# Grace-Period: Wie viele Tage der User Zeit hat bevor die Rolle entfernt wird
GRACE_PERIOD_DAYS = 7
def _mask_log_identifier(value: object, *, visible_prefix: int = 3, visible_suffix: int = 2) -> str:
    text = str(value or "").strip()
    if not text:
        return "<empty>"
    if len(text) <= visible_prefix + visible_suffix:
        return "***"
    return f"{text[:visible_prefix]}...{text[-visible_suffix:]}"


class TokenErrorHandler:
    """Verwaltet Token-Fehler und verhindert endlose Refresh-Versuche."""

    def __init__(self, discord_bot: discord.Client | None = None):
        """
        Args:
            discord_bot: Discord Bot-Instanz für Benachrichtigungen
        """
        self.discord_bot = discord_bot
        self._migrate_db()

    @staticmethod
    def _migrate_db() -> None:
        """Fügt neue Spalten zur twitch_token_blacklist hinzu (idempotent)."""
        column_add_statements = {
            "grace_expires_at": "ALTER TABLE twitch_token_blacklist ADD COLUMN grace_expires_at TEXT",
            "user_dm_sent": "ALTER TABLE twitch_token_blacklist ADD COLUMN user_dm_sent INTEGER DEFAULT 0",
            "reminder_sent": "ALTER TABLE twitch_token_blacklist ADD COLUMN reminder_sent INTEGER DEFAULT 0",
            "role_removed": "ALTER TABLE twitch_token_blacklist ADD COLUMN role_removed INTEGER DEFAULT 0",
        }

        def _apply_migration() -> None:
            with transaction() as conn:
                existing = {
                    row[0]
                    for row in conn.execute(
                        """
                        SELECT column_name
                        FROM information_schema.columns
                        WHERE table_name = 'twitch_token_blacklist'
                        """
                    )
                }
                for col_name, statement in column_add_statements.items():
                    if col_name not in existing:
                        conn.execute(statement)
        try:
            _apply_migration()
        except RuntimeError as exc:
            if "prepare_runtime_storage" not in str(exc):
                log.warning(
                    "DB migration for twitch_token_blacklist failed (non-critical)",
                    exc_info=True,
                )
                return
            try:
                storage_pg.prepare_runtime_storage()
                _apply_migration()
            except Exception:
                log.warning(
                    "DB migration for twitch_token_blacklist failed (non-critical)",
                    exc_info=True,
                )
        except Exception:
            log.warning(
                "DB migration for twitch_token_blacklist failed (non-critical)",
                exc_info=True,
            )

    @staticmethod
    def _normalize_discord_user_id(raw: str | None) -> str | None:
        return normalize_discord_user_id(raw)

    def schedule_streamer_role_sync(
        self,
        discord_user_id: str | None,
        *,
        should_have_role: bool,
        reason: str,
    ) -> None:
        normalized_id = self._normalize_discord_user_id(discord_user_id)
        if not normalized_id:
            return

        schedule_discord_role_sync(
            self.discord_bot,
            normalized_id,
            should_have_role=should_have_role,
            reason=reason,
            task_name="twitch.token_error.role_sync",
            logger=log,
        )

    # Anzahl aufeinanderfolgender Fehler, bevor der Raid-Bot wirklich deaktiviert wird
    BLACKLIST_DISABLE_THRESHOLD = 3
    CONSECUTIVE_FAILURE_WINDOW_HOURS = 12

    def _mark_reauth_required(
        self,
        twitch_user_id: str,
        twitch_login: str,
        *,
        mark_notified: bool = False,
    ) -> None:
        """Disables Twitch auth usage until the streamer re-authorizes in the dashboard."""
        login_hint = str(twitch_login or "").strip().lower()
        try:
            with transaction() as conn:
                if mark_notified:
                    conn.execute(
                        """
                        UPDATE twitch_raid_auth
                        SET raid_enabled = FALSE,
                            needs_reauth = TRUE,
                            twitch_login = COALESCE(NULLIF(%s, ''), twitch_login),
                            reauth_notified_at = COALESCE(reauth_notified_at, %s)
                        WHERE twitch_user_id = %s
                        """,
                        (
                            login_hint,
                            datetime.now(UTC).isoformat(),
                            twitch_user_id,
                        ),
                    )
                else:
                    conn.execute(
                        """
                        UPDATE twitch_raid_auth
                        SET raid_enabled = FALSE,
                            needs_reauth = TRUE,
                            twitch_login = COALESCE(NULLIF(%s, ''), twitch_login)
                        WHERE twitch_user_id = %s
                        """,
                        (
                            login_hint,
                            twitch_user_id,
                        ),
                    )
                try:
                    set_partner_raid_bot_enabled(conn, twitch_user_id=twitch_user_id, enabled=False)
                except Exception:
                    log.debug(
                        "Could not mirror raid_bot_enabled into partner registry for user_id=%s",
                        _mask_log_identifier(twitch_user_id),
                        exc_info=True,
                    )
        except Exception:
            log.warning(
                "Could not flag dashboard reauth for user_id=%s",
                _mask_log_identifier(twitch_user_id),
                exc_info=True,
            )

    def is_token_blacklisted(self, twitch_user_id: str) -> bool:
        """
        Prüft, ob ein Token endgültig gesperrt ist (>= BLACKLIST_DISABLE_THRESHOLD Fehler).

        Args:
            twitch_user_id: Twitch User ID

        Returns:
            True wenn Token dauerhaft blacklisted ist
        """
        try:
            with readonly_connection() as conn:
                row = conn.execute(
                    "SELECT error_count FROM twitch_token_blacklist WHERE twitch_user_id = %s",
                    (twitch_user_id,),
                ).fetchone()
                if not row:
                    return False
                return int(row[0]) >= self.BLACKLIST_DISABLE_THRESHOLD
        except Exception:
            log.error("Error checking token blacklist", exc_info=True)
            return False

    def add_to_blacklist(
        self,
        twitch_user_id: str,
        twitch_login: str,
        error_message: str,
    ):
        """
        Fügt einen Token zur Blacklist hinzu oder erhöht den Error-Counter.

        Args:
            twitch_user_id: Twitch User ID
            twitch_login: Twitch Login Name
            error_message: Fehlermeldung vom Token-Refresh
        """
        now = datetime.now(UTC).isoformat()

        try:
            with transaction() as conn:
                # Prüfe ob bereits vorhanden
                existing = conn.execute(
                    "SELECT error_count, last_error_at FROM twitch_token_blacklist WHERE twitch_user_id = %s",
                    (twitch_user_id,),
                ).fetchone()

                if existing:
                    prior_count = int(existing[0] or 0)
                    last_error_raw = existing[1]
                    reset_counter = False

                    if last_error_raw:
                        try:
                            last_error_dt = datetime.fromisoformat(
                                str(last_error_raw).replace("Z", "+00:00")
                            )
                            if last_error_dt.tzinfo is None:
                                last_error_dt = last_error_dt.replace(tzinfo=UTC)
                            reset_counter = (datetime.now(UTC) - last_error_dt) > timedelta(
                                hours=self.CONSECUTIVE_FAILURE_WINDOW_HOURS
                            )
                        except Exception:
                            reset_counter = False

                    if reset_counter:
                        new_count = 1
                        conn.execute(
                            """
                            UPDATE twitch_token_blacklist
                            SET error_count = %s, first_error_at = %s, last_error_at = %s,
                                error_message = %s, notified = 0
                            WHERE twitch_user_id = %s
                            """,
                            (new_count, now, now, error_message, twitch_user_id),
                        )
                        log.info(
                            "Reset OAuth refresh failure counter for broadcaster=%s after %dh without errors",
                            _mask_log_identifier(twitch_login),
                            self.CONSECUTIVE_FAILURE_WINDOW_HOURS,
                        )
                    else:
                        # Erhöhe Counter innerhalb des Consecutive-Fensters
                        new_count = max(1, prior_count + 1)
                        conn.execute(
                            """
                            UPDATE twitch_token_blacklist
                            SET error_count = %s, last_error_at = %s, error_message = %s
                            WHERE twitch_user_id = %s
                            """,
                            (new_count, now, error_message, twitch_user_id),
                        )
                else:
                    # Neuer Eintrag – Grace-Period ab jetzt
                    grace_expires = (
                        datetime.now(UTC) + timedelta(days=GRACE_PERIOD_DAYS)
                    ).isoformat()
                    conn.execute(
                        """
                        INSERT INTO twitch_token_blacklist
                        (twitch_user_id, twitch_login, error_message, first_error_at, last_error_at,
                         grace_expires_at)
                        VALUES (%s, %s, %s, %s, %s, %s)
                        """,
                        (
                            twitch_user_id,
                            twitch_login,
                            error_message,
                            now,
                            now,
                            grace_expires,
                        ),
                    )

            # error_count nach dem Commit neu lesen (könnte direkt aus dem UPSERT stammen)
            try:
                with readonly_connection() as conn:
                    cnt_row = conn.execute(
                        "SELECT error_count FROM twitch_token_blacklist WHERE twitch_user_id = %s",
                        (twitch_user_id,),
                    ).fetchone()
                current_count = int(cnt_row[0]) if cnt_row else 1
            except Exception:
                current_count = 1

            self._mark_reauth_required(twitch_user_id, twitch_login)
            log.warning(
                "Blocked auto-refresh for %s (ID: %s) after auth failure. "
                "Consecutive failures: %d/%d",
                twitch_login,
                twitch_user_id,
                current_count,
                self.BLACKLIST_DISABLE_THRESHOLD,
            )

            # Raid-Bot erst nach BLACKLIST_DISABLE_THRESHOLD aufeinanderfolgenden Fehlern deaktivieren
            if current_count >= self.BLACKLIST_DISABLE_THRESHOLD:
                self._disable_raid_bot(twitch_user_id)
            else:
                log.info(
                    "OAuth refresh error for broadcaster=%s (count %d/%d) - dashboard reauth required until the streamer reconnects",
                    _mask_log_identifier(twitch_login),
                    current_count,
                    self.BLACKLIST_DISABLE_THRESHOLD,
                )

        except Exception:
            log.error("Error adding to token blacklist", exc_info=True)

    def _disable_raid_bot(self, twitch_user_id: str):
        """Keeps raid/auth disabled for a streamer with repeated token failures."""
        login_hint = ""
        try:
            with transaction() as conn:
                auth_row = conn.execute(
                    "SELECT twitch_login FROM twitch_raid_auth WHERE twitch_user_id = %s",
                    (twitch_user_id,),
                ).fetchone()
                if auth_row:
                    login_hint = str(
                        auth_row[0]
                        if not hasattr(auth_row, "keys")
                        else auth_row["twitch_login"] or ""
                    ).strip()

            self._mark_reauth_required(twitch_user_id, login_hint)
            log.info(
                "Disabled raid bot and kept partner active for user_id=%s due to OAuth refresh error",
                _mask_log_identifier(twitch_user_id),
            )
            # Rolle wird NICHT sofort entfernt – User hat %d Tage Grace-Period
            # Stelle sicher dass grace_expires_at gesetzt ist (wurde beim ersten Blacklist-Eintrag gesetzt)
            try:
                with transaction() as conn:
                    row = conn.execute(
                        "SELECT grace_expires_at FROM twitch_token_blacklist WHERE twitch_user_id = %s",
                        (twitch_user_id,),
                    ).fetchone()
                    if row and not row[0]:
                        grace_expires = (
                            datetime.now(UTC) + timedelta(days=GRACE_PERIOD_DAYS)
                        ).isoformat()
                        conn.execute(
                            "UPDATE twitch_token_blacklist SET grace_expires_at = %s WHERE twitch_user_id = %s",
                            (grace_expires, twitch_user_id),
                        )
            except Exception:
                log.warning(
                    "Could not ensure grace_expires_at for %s",
                    twitch_user_id,
                    exc_info=True,
                )
        except Exception:
            log.error("Error disabling raid bot", exc_info=True)

    # Minimale Wartezeit zwischen zwei Refresh-Versuchen nach einem Fehler (in Stunden)
    RETRY_COOLDOWN_HOURS = 2

    def has_recent_failure(self, twitch_user_id: str) -> bool:
        """
        Gibt True zurück, wenn in den letzten RETRY_COOLDOWN_HOURS ein Fehler aufgetreten ist.
        Verhindert, dass der Maintenance-Loop oder on-demand Refreshes zu schnell wiederholt werden.
        Nur relevant wenn error_count < BLACKLIST_DISABLE_THRESHOLD (vollständig blacklisted
        wird bereits via is_token_blacklisted() abgefangen).
        """
        try:
            with readonly_connection() as conn:
                row = conn.execute(
                    "SELECT error_count, last_error_at FROM twitch_token_blacklist WHERE twitch_user_id = %s",
                    (twitch_user_id,),
                ).fetchone()
                if not row or not row[1]:
                    return False
                error_count = int(row[0] or 0)
                # Vollständig blacklisted – wird separat via is_token_blacklisted() behandelt
                if error_count >= self.BLACKLIST_DISABLE_THRESHOLD:
                    return False
                last_error_raw = str(row[1])
                last_error_dt = datetime.fromisoformat(last_error_raw.replace("Z", "+00:00"))
                if last_error_dt.tzinfo is None:
                    last_error_dt = last_error_dt.replace(tzinfo=UTC)
                return (datetime.now(UTC) - last_error_dt) < timedelta(
                    hours=self.RETRY_COOLDOWN_HOURS
                )
        except Exception:
            log.error("Error in has_recent_failure for %s", twitch_user_id, exc_info=True)
            return False

    def clear_failure_count(self, twitch_user_id: str) -> None:
        """
        Setzt den Fehler-Counter zurück (z.B. nach erfolgreichem Refresh).
        Löscht den Blacklist-Eintrag komplett, falls vorhanden.
        """
        try:
            with transaction() as conn:
                conn.execute(
                    "DELETE FROM twitch_token_blacklist WHERE twitch_user_id = %s",
                    (twitch_user_id,),
                )
        except Exception:
            log.error("Error clearing failure count for %s", twitch_user_id, exc_info=True)

    def remove_from_blacklist(self, twitch_user_id: str):
        """
        Entfernt einen Token von der Blacklist (z.B. nach erfolgreicher Re-Autorisierung).

        Args:
            twitch_user_id: Twitch User ID
        """
        try:
            with transaction() as conn:
                conn.execute(
                    "DELETE FROM twitch_token_blacklist WHERE twitch_user_id = %s",
                    (twitch_user_id,),
                )
            log.info(
                "Removed user_id=%s from OAuth refresh blacklist",
                _mask_log_identifier(twitch_user_id),
            )
        except Exception:
            log.error("Error removing from token blacklist", exc_info=True)

    async def notify_token_error(
        self,
        twitch_user_id: str,
        twitch_login: str,
        error_message: str,
    ):
        """
        Sendet eine Discord-Benachrichtigung über einen Token-Fehler.
        Wird nur einmal pro Streamer gesendet, um Spam zu vermeiden.

        Args:
            twitch_user_id: Twitch User ID
            twitch_login: Twitch Login Name
            error_message: Fehlermeldung vom Token-Refresh
        """
        if not self.discord_bot:
            log.warning("Discord bot not available, skipping notification")
            return

        # Prüfe ob bereits benachrichtigt
        try:
            with readonly_connection() as conn:
                row = conn.execute(
                    "SELECT notified FROM twitch_token_blacklist WHERE twitch_user_id = %s",
                    (twitch_user_id,),
                ).fetchone()

                if row and row[0] == 1:
                    log.debug("Already notified about auth error for %s", twitch_login)
                    return
        except Exception:
            log.error("Error checking notification status", exc_info=True)
            return

        try:
            channel = self.discord_bot.get_channel(TOKEN_ERROR_CHANNEL_ID)
            if not channel:
                log.warning(
                    "Auth error notification channel %s not found",
                    TOKEN_ERROR_CHANNEL_ID,
                )
                return

            # Erstelle Discord Embed
            embed = discord.Embed(
                title="⚠️ Twitch Token Error",
                description=f"Der Refresh-Token für **{twitch_login}** ist ungültig.",
                color=discord.Color.red(),
                timestamp=datetime.now(UTC),
            )

            embed.add_field(
                name="Streamer",
                value=f"[{twitch_login}](https://twitch.tv/{twitch_login})",
                inline=True,
            )

            embed.add_field(
                name="User ID",
                value=f"`{twitch_user_id}`",
                inline=True,
            )

            embed.add_field(
                name="Fehler",
                value=f"```{error_message[:200]}```",
                inline=False,
            )

            embed.add_field(
                name="Aktion erforderlich",
                value=(
                    "Der Streamer muss den Bot **neu für seinen Kanal aktivieren**, damit Auto-Raid und Twitch-Integrationen wieder funktionieren.\n"
                    "➡️ Bitte im Dashboard neu verbinden oder alternativ `/traid` verwenden."
                ),
                inline=False,
            )

            embed.add_field(
                name="Status",
                value="❌ Auto-Raid **deaktiviert** bis zur Re-Autorisierung",
                inline=False,
            )

            embed.set_footer(text="Twitch Raid Bot • Token Error Handler")

            await channel.send(embed=embed)

            # User-DM senden
            await self._send_user_dm_token_error(twitch_user_id, twitch_login, error_message)
            self._mark_reauth_required(twitch_user_id, twitch_login, mark_notified=True)

            # Markiere als benachrichtigt
            with transaction() as conn:
                conn.execute(
                    """
                    UPDATE twitch_token_blacklist
                    SET notified = 1
                    WHERE twitch_user_id = %s
                    """,
                    (twitch_user_id,),
                )

            log.info(
                "Sent auth error notification for %s to channel %s",
                twitch_login,
                TOKEN_ERROR_CHANNEL_ID,
            )

        except Exception:
            log.error("Error sending token error notification", exc_info=True)

    async def _send_user_dm_token_error(
        self,
        twitch_user_id: str,
        twitch_login: str,
        error_message: str,
        *,
        is_reminder: bool = False,
    ) -> bool:
        """Sendet dem Streamer eine DM über den Token-Fehler. Gibt True zurück wenn erfolgreich."""
        if not self.discord_bot:
            return False

        discord_user_id = self._get_discord_user_id(twitch_user_id, twitch_login)
        if not discord_user_id:
            log.debug("No discord_user_id found for %s, cannot send DM", twitch_login)
            return False

        try:
            user = await self.discord_bot.fetch_user(int(discord_user_id))
        except Exception:
            log.debug("Could not fetch Discord user %s for DM", discord_user_id)
            return False

        grace_dt = datetime.now(UTC) + timedelta(days=GRACE_PERIOD_DAYS)
        deadline_ts = int(grace_dt.timestamp())

        if is_reminder:
            embed = discord.Embed(
                title="⚠️ Twitch Bot – Aktivierung weiterhin ausstehend",
                description=f"Die Verbindung für **{twitch_login}** wurde seit {GRACE_PERIOD_DAYS} Tagen noch nicht erneuert.",
                color=discord.Color.dark_red(),
                timestamp=datetime.now(UTC),
            )
            embed.add_field(
                name="Streamer",
                value=f"[{twitch_login}](https://twitch.tv/{twitch_login})",
                inline=True,
            )
            embed.add_field(
                name="Status",
                value="⚠️ Bot-Funktionen bleiben deaktiviert, bis dein Kanal wieder verbunden ist.",
                inline=True,
            )
            embed.add_field(
                name="Lösung",
                value=(
                    "Klicke auf den Button unten, um einen neuen Link für deinen Kanal zu erhalten.\n"
                    "Melde dich dazu im Dashboard an und verbinde Twitch erneut.\n\n"
                    "Alternativ kannst du `/traid` auf dem Discord-Server nutzen."
                ),
                inline=False,
            )
            embed.add_field(
                name="Hinweis",
                value=(
                    "Bei Problemen oder Fragen bitte auf dem Server melden.\n"
                    "Die Verbindung ist erforderlich, damit Auto-Raid, Chat-Schutz und Analytics wieder laufen."
                ),
                inline=False,
            )
        else:
            embed = discord.Embed(
                title="⚠️ Twitch Bot – Verbindung fehlgeschlagen",
                description=f"Die Verbindung für den Twitch Bot ist für **{twitch_login}** fehlgeschlagen und muss erneuert werden.",
                color=discord.Color.orange(),
                timestamp=datetime.now(UTC),
            )
            embed.add_field(
                name="Streamer",
                value=f"[{twitch_login}](https://twitch.tv/{twitch_login})",
                inline=True,
            )
            embed.add_field(
                name="Mögliche Ursachen",
                value=(
                    "· Du hast das Passwort geändert\n"
                    "· Du hast 2FA aktiviert oder geändert\n"
                    "· Du bist Twitch Affiliate oder Partner geworden\n"
                    "· Bot in den Twitch-Einstellungen deautorisiert\n"
                    "· Sicherheitsänderung im Twitch-Profil geändert\n"
                    "· Du hast deine Email Adresse geändert."
                ),
                inline=False,
            )
            embed.add_field(
                name="Lösung",
                value=(
                    "Klicke auf den Button unten, um einen neuen Link für deinen Kanal zu erhalten.\n"
                    "Melde dich dazu im Dashboard an und verbinde Twitch erneut.\n"
                    "Nach erfolgreicher Verbindung wird der Bot automatisch wieder aktiviert.\n\n"
                ),
                inline=False,
            )
            embed.add_field(
                name="Status",
                value=(
                    f"Bis zur erneuten Verbindung bleiben die Twitch-Bot-Funktionen für **{twitch_login}** deaktiviert.\n"
                    f"Die aktuelle Frist im System läuft bis <t:{deadline_ts}:F>."
                ),
                inline=False,
            )
            embed.add_field(
                name="Hinweis",
                value=(
                    "Bei Problemen oder Fragen bitte dich sofort bei @EarlySalty melden :).\n"
                    "Die Verbindung ist erforderlich, damit alle Features wieder laufen "
                    "(Auto-Raid, Chat-Schutz, Analytics)."
                ),
                inline=False,
            )

        embed.set_footer(text="Twitch Raid Bot • Token Error Handler")

        # Auth-Button anhängen (persistent, funktioniert auch in DMs)
        try:
            from ..raid.views import RaidAuthGenerateView

            view = RaidAuthGenerateView(
                twitch_login=twitch_login, button_label="🔗 Link für deinen Kanal erzeugen"
            )
        except Exception:
            view = None

        try:
            await user.send(embed=embed, view=view)
            log.info(
                "Sent OAuth refresh error DM to Discord user=%s (broadcaster=%s)",
                _mask_log_identifier(discord_user_id),
                _mask_log_identifier(twitch_login),
            )
            # user_dm_sent in DB markieren
            try:
                with transaction() as conn:
                    conn.execute(
                        "UPDATE twitch_token_blacklist SET user_dm_sent = 1 WHERE twitch_user_id = %s",
                        (twitch_user_id,),
                    )
            except Exception:
                log.debug(
                    "Failed to mark user_dm_sent for broadcaster=%s",
                    _mask_log_identifier(twitch_user_id),
                    exc_info=True,
                )
            return True
        except discord.Forbidden:
            log.info("Cannot DM Discord user %s (DMs closed), skipping", discord_user_id)
            return False
        except Exception:
            log.warning(
                "Failed to send OAuth refresh error DM to Discord user=%s",
                _mask_log_identifier(discord_user_id),
                exc_info=True,
            )
            return False

    def _get_discord_user_id(self, twitch_user_id: str, twitch_login: str) -> str | None:
        """Holt die Discord User ID eines Streamers aus der DB."""
        try:
            with readonly_connection() as conn:
                row = load_streamer_identity(
                    conn,
                    twitch_user_id=twitch_user_id,
                    twitch_login=twitch_login,
                )
                if row:
                    val = str(
                        row[2] if not hasattr(row, "keys") else row["discord_user_id"] or ""
                    ).strip()
                    return val if val.isdigit() else None
        except Exception:
            log.warning("Could not fetch discord_user_id for %s", twitch_login, exc_info=True)
        return None

    async def check_grace_periods(self) -> None:
        """
        Prüft abgelaufene Grace-Periods (stündlich aufrufen).
        - Sendet Erinnerungs-DM an User + Admin-Channel-Notification
        - Entfernt die Streamer-Rolle nach Ablauf der Frist
        """
        now_iso = datetime.now(UTC).isoformat()
        try:
            with readonly_connection() as conn:
                expired = conn.execute(
                    """
                    SELECT twitch_user_id, twitch_login, error_message,
                           reminder_sent, role_removed, grace_expires_at
                    FROM twitch_token_blacklist
                    WHERE error_count >= %s
                      AND grace_expires_at IS NOT NULL
                      AND grace_expires_at <= %s
                      AND role_removed = 0
                    """,
                    (self.BLACKLIST_DISABLE_THRESHOLD, now_iso),
                ).fetchall()
        except Exception:
            log.error("check_grace_periods: DB query failed", exc_info=True)
            return

        for row in expired:
            uid, login, err_msg, reminder_sent, role_removed, grace_expires_at = row
            discord_user_id = self._get_discord_user_id(uid, login)

            # 1. Erinnerungs-DM an User
            if not reminder_sent:
                await self._send_user_dm_token_error(uid, login, err_msg or "", is_reminder=True)
                # Admin-Channel benachrichtigen damit Admin selbst auch schreiben kann
                await self._notify_admin_grace_expired(uid, login, discord_user_id)
                try:
                    with transaction() as conn:
                        conn.execute(
                            "UPDATE twitch_token_blacklist SET reminder_sent = 1 WHERE twitch_user_id = %s",
                            (uid,),
                        )
                except Exception:
                    log.warning("Could not set reminder_sent for %s", login, exc_info=True)

            # 2. Streamer-Rolle entfernen
            if discord_user_id:
                self.schedule_streamer_role_sync(
                    discord_user_id,
                    should_have_role=False,
                    reason=f"Twitch-Token seit {GRACE_PERIOD_DAYS} Tagen ungültig – Grace-Period abgelaufen",
                )
            try:
                with transaction() as conn:
                    conn.execute(
                        "UPDATE twitch_token_blacklist SET role_removed = 1 WHERE twitch_user_id = %s",
                        (uid,),
                    )
            except Exception:
                log.warning("Could not set role_removed for %s", login, exc_info=True)

            log.info(
                "Grace period expired for %s (id=%s) – role removed, reminder sent",
                login,
                uid,
            )

    async def _notify_admin_grace_expired(
        self,
        twitch_user_id: str,
        twitch_login: str,
        discord_user_id: str | None,
    ) -> None:
        """Benachrichtigt den Admin-Channel wenn eine Grace-Period abgelaufen ist."""
        if not self.discord_bot:
            return
        try:
            channel = self.discord_bot.get_channel(TOKEN_ERROR_CHANNEL_ID)
            if not channel:
                return

            mention = f"<@{discord_user_id}>" if discord_user_id else f"`{twitch_login}`"
            embed = discord.Embed(
                title="🚨 Grace-Period abgelaufen – Streamer-Rolle entzogen",
                description=(
                    f"Der Streamer **{twitch_login}** hat seinen Token innerhalb von "
                    f"**{GRACE_PERIOD_DAYS} Tagen** nicht erneuert.\n"
                    f"Die Streamer-Rolle wurde automatisch entzogen."
                ),
                color=discord.Color.dark_red(),
                timestamp=datetime.now(UTC),
            )
            embed.add_field(
                name="Streamer",
                value=f"[{twitch_login}](https://twitch.tv/{twitch_login})",
                inline=True,
            )
            embed.add_field(name="Discord", value=mention, inline=True)
            embed.add_field(name="User ID", value=f"`{twitch_user_id}`", inline=True)
            embed.add_field(
                name="Nächste Schritte",
                value=(
                    f"Bitte kontaktiere {mention} direkt.\n"
                    f"Der User kann sich über `/traid` neu autorisieren, um die Rolle zurückzubekommen."
                ),
                inline=False,
            )
            embed.set_footer(text="Twitch Raid Bot • Grace-Period Handler")
            await channel.send(embed=embed)
        except Exception:
            log.warning(
                "Failed to send grace-expired admin notification for %s",
                twitch_login,
                exc_info=True,
            )

    def cleanup_old_entries(self, days: int = 30):
        """
        Entfernt alte Blacklist-Einträge.

        Args:
            days: Einträge älter als diese Anzahl Tage werden gelöscht
        """
        try:
            cutoff = datetime.now(UTC).timestamp() - (days * 86400)
            cutoff_iso = datetime.fromtimestamp(cutoff, UTC).isoformat()

            with transaction() as conn:
                result = conn.execute(
                    """
                    DELETE FROM twitch_token_blacklist
                    WHERE last_error_at < %s
                    """,
                    (cutoff_iso,),
                )
                deleted = result.rowcount

            if deleted > 0:
                log.info(
                    "Cleaned up %d old token blacklist entries (>%d days)",
                    deleted,
                    days,
                )

        except Exception:
            log.error("Error cleaning up token blacklist", exc_info=True)

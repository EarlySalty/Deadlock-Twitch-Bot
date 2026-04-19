import logging
import secrets

from ..storage import (
    load_active_partner,
    readonly_connection,
    set_partner_raid_bot_enabled,
    set_partner_silent_flags,
    transaction,
)
from .constants import TWITCHIO_AVAILABLE, twitchio_commands

log = logging.getLogger("TwitchStreams.ChatBot")


if TWITCHIO_AVAILABLE:

    class RaidCommandsMixin:
        def _load_last_autoban_from_log(self, channel_key: str):
            """Best-effort Fallback: letzten Auto-Ban aus Logdatei laden (überlebt Bot-Restarts)."""
            autoban_log = getattr(self, "_autoban_log", None)
            if not autoban_log:
                return None
            try:
                with autoban_log.open("r", encoding="utf-8") as handle:
                    lines = handle.read().splitlines()
            except Exception:
                log.debug("Konnte Auto-Ban-Logdatei nicht lesen", exc_info=True)
                return None

            for line in reversed(lines):
                parts = line.split("\t")
                if len(parts) < 5:
                    continue
                status = (parts[1] or "").strip().upper()
                logged_channel = self._normalize_channel_login(parts[2] if len(parts) > 2 else "")
                chatter_login = (parts[3] if len(parts) > 3 else "").strip()
                chatter_id = (parts[4] if len(parts) > 4 else "").strip()
                if status != "[BANNED]" or logged_channel != channel_key:
                    continue
                if not chatter_id:
                    continue
                return {
                    "user_id": chatter_id,
                    "login": chatter_login,
                }
            return None

        @twitchio_commands.command(name="raid_enable", aliases=["raidbot"])
        async def cmd_raid_enable(self, ctx: twitchio_commands.Context):
            """!raid_enable - Aktiviert den Auto-Raid-Bot."""
            # Nur Broadcaster oder Mods dürfen den Bot steuern
            is_mod = getattr(ctx.author, "is_moderator", getattr(ctx.author, "moderator", False))
            is_broadcaster = getattr(
                ctx.author, "is_broadcaster", getattr(ctx.author, "broadcaster", False)
            )
            if not (is_broadcaster or is_mod):
                await ctx.send(
                    f"@{ctx.author.name} Nur der Broadcaster oder Mods können den Twitch-Bot steuern."
                )
                return

            channel_name = ctx.channel.name
            streamer_data = self._get_streamer_by_channel(channel_name)

            if not streamer_data:
                await ctx.send(
                    f"@{ctx.author.name} Dieser Kanal ist nicht als Partner registriert. "
                    "Kontaktiere einen Admin für Details."
                )
                return

            twitch_login, twitch_user_id, raid_bot_enabled = streamer_data

            # Prüfen, ob bereits autorisiert
            with readonly_connection() as conn:
                auth_row = conn.execute(
                    "SELECT raid_enabled FROM twitch_raid_auth WHERE twitch_user_id = %s",
                    (twitch_user_id,),
                ).fetchone()

            if not auth_row:
                # Noch nicht autorisiert -> OAuth-Link senden
                if not self._raid_bot:
                    await ctx.send(
                        f"@{ctx.author.name} Der Twitch-Bot ist derzeit nicht verfügbar. "
                        "Kontaktiere einen Admin."
                    )
                    return

                auth_url = self._raid_bot.auth_manager.generate_auth_url(twitch_login)
                await ctx.send(
                    f"@{ctx.author.name} OAuth fehlt – Anforderung: Twitch-Bot autorisieren (Pflicht für Streamer-Partner). "
                    f"Link: {auth_url} | Danach aktiv: Auto-Raid, Chat Guard und Discord Auto-Post "
                    "(bei Frage, Chat-Aktivitaet oder Viewer-Spike; mit Cooldowns)."
                )
                log.info("Sent raid auth link to %s via chat", twitch_login)
                return

            # Bereits autorisiert -> aktivieren
            raid_enabled = auth_row[0]
            if raid_enabled:
                await ctx.send(
                    f"@{ctx.author.name} ✅ Auto-Raid ist bereits aktiviert! "
                    "Der Twitch-Bot raidet automatisch andere Partner, wenn du offline gehst."
                )
                return

            # Aktivieren
            with transaction() as conn:
                conn.execute(
                    "UPDATE twitch_raid_auth SET raid_enabled = %s WHERE twitch_user_id = %s",
                    (True, twitch_user_id),
                )
                set_partner_raid_bot_enabled(conn, twitch_user_id=twitch_user_id, enabled=True)

            await ctx.send(
                f"@{ctx.author.name} ✅ Auto-Raid aktiviert! "
                "Wenn du offline gehst, raidet der Twitch-Bot automatisch den Partner mit der kürzesten Stream-Zeit."
            )
            log.info("Enabled auto-raid for %s via chat", twitch_login)

        @twitchio_commands.command(name="raid_disable", aliases=["raidbot_off"])
        async def cmd_raid_disable(self, ctx: twitchio_commands.Context):
            """!raid_disable - Deaktiviert den Auto-Raid-Bot."""
            is_mod = getattr(ctx.author, "is_moderator", getattr(ctx.author, "moderator", False))
            is_broadcaster = getattr(
                ctx.author, "is_broadcaster", getattr(ctx.author, "broadcaster", False)
            )
            if not (is_broadcaster or is_mod):
                await ctx.send(
                    f"@{ctx.author.name} Nur der Broadcaster oder Mods können den Twitch-Bot steuern."
                )
                return

            channel_name = ctx.channel.name
            streamer_data = self._get_streamer_by_channel(channel_name)

            if not streamer_data:
                await ctx.send(
                    f"@{ctx.author.name} Dieser Kanal ist nicht als Partner registriert."
                )
                return

            twitch_login, twitch_user_id, _ = streamer_data

            with transaction() as conn:
                conn.execute(
                    "UPDATE twitch_raid_auth SET raid_enabled = %s WHERE twitch_user_id = %s",
                    (False, twitch_user_id),
                )
                set_partner_raid_bot_enabled(conn, twitch_user_id=twitch_user_id, enabled=False)

            await ctx.send(
                f"@{ctx.author.name} 🛑 Auto-Raid deaktiviert. "
                "Du kannst es jederzeit mit !raid_enable wieder aktivieren."
            )
            log.info("Disabled auto-raid for %s via chat", twitch_login)

        @twitchio_commands.command(name="raid_status", aliases=["raidbot_status"])
        async def cmd_raid_status(self, ctx: twitchio_commands.Context):
            """!raid_status - Zeigt den Twitch-Bot-Status an."""
            channel_name = ctx.channel.name
            streamer_data = self._get_streamer_by_channel(channel_name)

            if not streamer_data:
                await ctx.send(
                    f"@{ctx.author.name} Dieser Kanal ist nicht als Partner registriert."
                )
                return

            twitch_login, twitch_user_id, raid_bot_enabled = streamer_data

            with readonly_connection() as conn:
                auth_row = conn.execute(
                    """
                    SELECT raid_enabled, authorized_at
                    FROM twitch_raid_auth
                    WHERE twitch_user_id = %s
                    """,
                    (twitch_user_id,),
                ).fetchone()

                # Statistiken
                stats = conn.execute(
                    """
                    SELECT COUNT(*) as total,
                           SUM(CASE WHEN COALESCE(success, FALSE) IS TRUE THEN 1 ELSE 0 END) as successful
                    FROM twitch_raid_history
                    WHERE from_broadcaster_id = %s
                    """,
                    (twitch_user_id,),
                ).fetchone()
                total_raids, successful_raids = stats if stats else (0, 0)

                # Letzter Raid
                last_raid = conn.execute(
                    """
                    SELECT to_broadcaster_login, viewer_count, executed_at, success
                    FROM twitch_raid_history
                    WHERE from_broadcaster_id = %s
                    ORDER BY executed_at DESC
                    LIMIT 1
                    """,
                    (twitch_user_id,),
                ).fetchone()

            # Status bestimmen
            if not auth_row:
                status = "❌ Nicht autorisiert (OAuth fehlt)"
                action = "Anforderung: Twitch-Bot autorisieren mit !raid_enable."
            elif auth_row[0]:  # raid_enabled
                status = "✅ Aktiv"
                action = "Auto-Raids sind aktiviert."
            else:
                status = "🛑 Deaktiviert"
                action = "Aktiviere mit !raid_enable."

            # Nachricht zusammenstellen
            message = f"@{ctx.author.name} Twitch-Bot Status: {status}. {action}"

            if total_raids:
                message += (
                    f" | Statistik: {total_raids} Raids ({successful_raids or 0} erfolgreich)"
                )

            if last_raid:
                to_login, viewers, executed_at, success = last_raid
                icon = "✅" if success else "❌"
                time_str = executed_at[:16] if executed_at else "?"
                message += f" | Letzter Raid {icon}: {to_login} ({viewers} Viewer) am {time_str}"

            await ctx.send(message)

        @twitchio_commands.command(name="uban", aliases=["unban"])
        async def cmd_uban(self, ctx: twitchio_commands.Context):
            """!uban / !unban - hebt den letzten Auto-Ban im aktuellen Channel auf."""
            is_mod = getattr(ctx.author, "is_moderator", getattr(ctx.author, "moderator", False))
            is_broadcaster = getattr(
                ctx.author, "is_broadcaster", getattr(ctx.author, "broadcaster", False)
            )
            if not (is_broadcaster or is_mod):
                await ctx.send(f"@{ctx.author.name} Nur der Broadcaster oder Mods.")
                return

            channel_name = ctx.channel.name
            streamer_data = self._get_streamer_by_channel(channel_name)
            if not streamer_data:
                await ctx.send(
                    f"@{ctx.author.name} Dieser Kanal ist nicht als Partner registriert."
                )
                return

            twitch_login, twitch_user_id, _ = streamer_data
            channel_key = self._normalize_channel_login(channel_name)
            last = self._last_autoban.get(channel_key)
            if not last:
                last = self._load_last_autoban_from_log(channel_key)
                if last:
                    self._last_autoban[channel_key] = last
            if not last:
                await ctx.send(f"@{ctx.author.name} Kein Auto-Ban-Eintrag zum Aufheben gefunden.")
                return

            target_user_id = last.get("user_id", "")
            target_login = last.get("login") or target_user_id
            if not target_user_id:
                await ctx.send(f"@{ctx.author.name} Kein Nutzer gespeichert für Unban.")
                return

            success = await self._unban_user(
                broadcaster_id=str(twitch_user_id),
                target_user_id=str(target_user_id),
                channel_name=channel_name,
                login_hint=target_login,
            )
            if success:
                await ctx.send(f"@{ctx.author.name} Unban ausgeführt für {target_login}.")
            else:
                await ctx.send(f"@{ctx.author.name} Unban fehlgeschlagen für {target_login}.")

        @twitchio_commands.command(name="raid_history", aliases=["raidbot_history"])
        async def cmd_raid_history(self, ctx: twitchio_commands.Context):
            """!raid_history - Zeigt die letzten 3 Raids an."""
            channel_name = ctx.channel.name
            streamer_data = self._get_streamer_by_channel(channel_name)

            if not streamer_data:
                return

            twitch_login, twitch_user_id, _ = streamer_data

            with readonly_connection() as conn:
                raids = conn.execute(
                    """
                    SELECT to_broadcaster_login, viewer_count, executed_at, success
                    FROM twitch_raid_history
                    WHERE from_broadcaster_id = %s
                    ORDER BY executed_at DESC
                    LIMIT 3
                    """,
                    (twitch_user_id,),
                ).fetchall()

            if not raids:
                await ctx.send(f"@{ctx.author.name} Noch keine Raids durchgeführt.")
                return

            raids_text = " | ".join(
                [
                    f"{'✅' if success else '❌'} {to_login} ({viewers}V, {executed_at[:10] if executed_at else '?'})"
                    for to_login, viewers, executed_at, success in raids
                ]
            )

            await ctx.send(f"@{ctx.author.name} Letzte Raids: {raids_text}")

        @twitchio_commands.command(name="clip", aliases=["createclip"])
        async def cmd_clip(self, ctx: twitchio_commands.Context, *, description: str = ""):
            """!clip [titel] - Erstellt einen ca. 60s Clip aus dem aktuellen Stream-Buffer und nutzt den angegebenen Text als Titel."""
            channel_name = ctx.channel.name
            streamer_data = self._get_streamer_by_channel(channel_name)
            if not streamer_data:
                await ctx.send(
                    f"@{ctx.author.name} Dieser Kanal ist nicht als Partner registriert."
                )
                return

            twitch_login, twitch_user_id, _ = streamer_data

            if not self._raid_bot or not hasattr(self._raid_bot, "auth_manager"):
                await ctx.send(f"@{ctx.author.name} Twitch-Bot nicht verfügbar.")
                return

            auth_manager = self._raid_bot.auth_manager
            api_session = getattr(self._raid_bot, "session", None)
            if not api_session:
                await ctx.send(
                    f"@{ctx.author.name} Twitch-Bot nicht verfügbar (keine API-Session)."
                )
                return

            # Broadcaster-Token bevorzugen (Clip wird dem Broadcaster zugeschrieben)
            access_token = None
            try:
                tokens = await auth_manager.get_tokens_for_user(str(twitch_user_id), api_session)
                if tokens:
                    access_token = tokens[0]
            except Exception:
                log.debug(
                    "Clip command: broadcaster credential unavailable for %s",
                    twitch_login,
                    exc_info=True,
                )

            # Fallback: Bot-eigenen Token verwenden
            if not access_token:
                token_mgr = getattr(self, "_token_manager", None)
                if token_mgr:
                    try:
                        bot_token, _ = await token_mgr.get_valid_token()
                        access_token = bot_token
                        log.debug(
                            "Clip command: using bot credential as fallback for %s",
                            twitch_login,
                        )
                    except Exception:
                        log.debug(
                            "Clip command: Bot-Token-Fetch fehlgeschlagen",
                            exc_info=True,
                        )

            if not access_token:
                auth_url = auth_manager.generate_auth_url(twitch_login)
                await ctx.send(
                    f"@{ctx.author.name} OAuth fehlt. Bitte einmal per !raid_enable autorisieren: {auth_url}"
                )
                return

            clip_title = description.strip()
            if not clip_title:
                clip_title = secrets.choice(
                    [
                        "Clip des Streams",
                        "Highlight des Tages",
                        "Das müssen wir teilen",
                        "Unfassbarer Moment",
                        "Clip it!",
                    ]
                )
            max_title_len = 60
            if len(clip_title) > max_title_len:
                clip_title = clip_title[: max_title_len - 3].rstrip() + "..."
            requested_duration = 60.0

            try:
                from ..api.twitch_api import (
                    TwitchAPI,
                )  # lokal importieren, um Zyklus zu vermeiden

                api = TwitchAPI(
                    auth_manager.client_id,
                    auth_manager.client_secret,
                    session=api_session,
                )
                clip = await api.create_clip(
                    str(twitch_user_id),
                    user_token=str(access_token),
                    title=clip_title,
                    duration=requested_duration,
                    has_delay=False,
                )
            except Exception:
                log.exception("Clip command failed for %s", twitch_login)
                clip = None

            if not clip:
                await ctx.send(
                    f"@{ctx.author.name} Clip konnte nicht erstellt werden. "
                    "Bitte in 10 Sekunden nochmal versuchen."
                )
                return

            clip_id = str(clip.get("id") or "").strip()
            edit_url = str(clip.get("edit_url") or "").strip()
            clip_url = f"https://clips.twitch.tv/{clip_id}" if clip_id else edit_url
            if not clip_url:
                await ctx.send(
                    f"@{ctx.author.name} Clip wurde angefordert, aber es kam kein Link zurück."
                )
                return

            desc_part = f' – "{clip_title}"' if clip_title else ""
            await ctx.send(
                f"@{ctx.author.name} 🎬 Clip erstellt{desc_part} (ca. letzte {int(requested_duration)}s): {clip_url}"
            )
            log.info(
                "Clip command successful: %s in #%s (clip_id=%s)",
                twitch_login,
                channel_name,
                clip_id or "-",
            )

        @twitchio_commands.command(name="ping", aliases=["health", "status", "bot"])
        async def cmd_ping(self, ctx: twitchio_commands.Context):
            """!ping - Zeigt ob der Bot online ist."""
            responses = [
                f"@{ctx.author.name} Eure Majestät! 👑 Der Bot steht zu Euren Diensten. Was kann ich für Euch tun?",
                f"@{ctx.author.name} Bin da! Ausgeschlafen, aufgewärmt und bereit für Chaos. 🤖✅",
                f"@{ctx.author.name} Ja ich lebe noch, keine Sorge. Puls: 🟢 Signal: 📶 Kaffee: ☕ alles gut.",
                f"@{ctx.author.name} Bot online! Bereit für Euren Befehl, oh weiser Chatter. 🫡",
                f"@{ctx.author.name} Ich atme noch! Und ich hab sogar alle meine Kabel dran.",
                f"@{ctx.author.name} Natürlich bin ich online – wer soll sonst die Clips machen? 😏🎬",
            ]
            await ctx.send(secrets.choice(responses))

        @twitchio_commands.command(name="silentban")
        async def cmd_silentban(self, ctx: twitchio_commands.Context):
            """!silentban - Schaltet die Auto-Ban Chat-Benachrichtigung für diesen Channel ein/aus."""
            is_mod = getattr(ctx.author, "is_moderator", getattr(ctx.author, "moderator", False))
            is_broadcaster = getattr(
                ctx.author, "is_broadcaster", getattr(ctx.author, "broadcaster", False)
            )
            if not (is_broadcaster or is_mod):
                await ctx.send(
                    f"@{ctx.author.name} Nur der Broadcaster oder Mods können den Bot steuern."
                )
                return

            channel_name = ctx.channel.name
            streamer_data = self._get_streamer_by_channel(channel_name)
            if not streamer_data:
                await ctx.send(
                    f"@{ctx.author.name} Dieser Kanal ist nicht als Partner registriert."
                )
                return

            twitch_login, twitch_user_id_sb, _ = streamer_data
            if hasattr(self, "_is_fully_authed"):
                try:
                    if not await self._is_fully_authed(twitch_user_id_sb):
                        await ctx.send(
                            f"@{ctx.author.name} Neu-Autorisierung erforderlich. "
                            "Bitte prüfe deine Discord-DMs oder nutze /traid."
                        )
                        return
                except Exception:
                    log.debug(
                        "Silentban auth precheck failed for %s",
                        twitch_login,
                        exc_info=True,
                    )

            with transaction() as conn:
                partner_row = load_active_partner(conn, twitch_login=twitch_login)
                current = int((partner_row["silent_ban"] if partner_row and hasattr(partner_row, "keys") else (partner_row[14] if partner_row else 0)) or 0)
                new_value = 0 if current else 1
                set_partner_silent_flags(conn, twitch_login=twitch_login, silent_ban=new_value)

            if new_value:
                await ctx.send(
                    f"@{ctx.author.name} 🔇 Auto-Ban Benachrichtigungen deaktiviert. Bans werden weiterhin ausgeführt, aber keine Nachricht mehr im Chat."
                )
            else:
                await ctx.send(f"@{ctx.author.name} 🔊 Auto-Ban Benachrichtigungen aktiviert.")
            log.info(
                "silentban toggled to %d for %s by %s",
                new_value,
                twitch_login,
                ctx.author.name,
            )

        @twitchio_commands.command(name="silentraid")
        async def cmd_silentraid(self, ctx: twitchio_commands.Context):
            """!silentraid - Schaltet die Raid-Benachrichtigung für diesen Channel ein/aus."""
            is_mod = getattr(ctx.author, "is_moderator", getattr(ctx.author, "moderator", False))
            is_broadcaster = getattr(
                ctx.author, "is_broadcaster", getattr(ctx.author, "broadcaster", False)
            )
            if not (is_broadcaster or is_mod):
                await ctx.send(
                    f"@{ctx.author.name} Nur der Broadcaster oder Mods können den Bot steuern."
                )
                return

            channel_name = ctx.channel.name
            streamer_data = self._get_streamer_by_channel(channel_name)
            if not streamer_data:
                await ctx.send(
                    f"@{ctx.author.name} Dieser Kanal ist nicht als Partner registriert."
                )
                return

            twitch_login, twitch_user_id_sr, _ = streamer_data
            if hasattr(self, "_is_fully_authed"):
                try:
                    if not await self._is_fully_authed(twitch_user_id_sr):
                        await ctx.send(
                            f"@{ctx.author.name} Neu-Autorisierung erforderlich. "
                            "Bitte prüfe deine Discord-DMs oder nutze /traid."
                        )
                        return
                except Exception:
                    log.debug(
                        "Silentraid auth precheck failed for %s",
                        twitch_login,
                        exc_info=True,
                    )

            with transaction() as conn:
                partner_row = load_active_partner(conn, twitch_login=twitch_login)
                current = int((partner_row["silent_raid"] if partner_row and hasattr(partner_row, "keys") else (partner_row[15] if partner_row else 0)) or 0)
                new_value = 0 if current else 1
                set_partner_silent_flags(conn, twitch_login=twitch_login, silent_raid=new_value)

            if new_value:
                await ctx.send(
                    f"@{ctx.author.name} 🔇 Raid-Benachrichtigungen deaktiviert. Raids werden weiterhin ausgeführt, aber keine Nachricht mehr im Chat."
                )
            else:
                await ctx.send(f"@{ctx.author.name} 🔊 Raid-Benachrichtigungen aktiviert.")
            log.info(
                "silentraid toggled to %d for %s by %s",
                new_value,
                twitch_login,
                ctx.author.name,
            )

        @twitchio_commands.command(
            name="lurkersteuer_off",
            aliases=["lurkersteuer_aus", "lurker_tax_off"],
        )
        async def cmd_lurkersteuer_off(self, ctx: twitchio_commands.Context):
            """!lurkersteuer_off - Deaktiviert die Lurker Steuer für den aktuellen Channel."""
            is_broadcaster = getattr(
                ctx.author, "is_broadcaster", getattr(ctx.author, "broadcaster", False)
            )
            if not is_broadcaster:
                await ctx.send(
                    f"@{ctx.author.name} Nur der Broadcaster kann die Lurker Steuer dauerhaft deaktivieren."
                )
                return

            channel_name = ctx.channel.name
            streamer_data = self._get_streamer_by_channel(channel_name)
            if not streamer_data:
                await ctx.send(
                    f"@{ctx.author.name} Dieser Kanal ist nicht als Partner registriert."
                )
                return

            twitch_login, twitch_user_id, _ = streamer_data
            loader = getattr(self, "_load_lurker_tax_settings", None)
            saver = getattr(self, "_set_lurker_tax_enabled", None)
            settings = (
                loader(twitch_login)
                if callable(loader)
                else {"plan_id": "raid_free", "is_paid_plan": False, "enabled": False}
            )
            if not bool(settings.get("is_paid_plan")):
                await ctx.send(
                    f"@{ctx.author.name} Die Lurker Steuer ist nur in bezahlten Plänen verfügbar."
                )
                return

            saved = False
            if callable(saver):
                saved = bool(
                    saver(
                        twitch_login=twitch_login,
                        twitch_user_id=str(twitch_user_id),
                        plan_id=str(settings.get("plan_id") or ""),
                        enabled=False,
                    )
                )
            else:
                try:
                    with transaction() as conn:
                        conn.execute(
                            """
                            UPDATE streamer_plans
                               SET lurker_tax_enabled = 0
                             WHERE LOWER(COALESCE(twitch_login, '')) = LOWER(%s)
                            """,
                            (twitch_login,),
                        )
                    saved = True
                except Exception:
                    log.debug(
                        "Lurker-Steuer disable fallback failed for %s",
                        twitch_login,
                        exc_info=True,
                    )

            if not saved:
                await ctx.send(
                    f"@{ctx.author.name} Lurker Steuer konnte gerade nicht deaktiviert werden."
                )
                return

            if bool(settings.get("enabled")):
                await ctx.send(
                    f"@{ctx.author.name} Lurker Steuer deaktiviert. Im Abo-Bereich kannst du sie später wieder aktivieren."
                )
            else:
                await ctx.send(
                    f"@{ctx.author.name} Lurker Steuer ist bereits deaktiviert."
                )
            log.info("Disabled lurker tax for %s via chat", twitch_login)

        @twitchio_commands.command(name="raid", aliases=["traid"])
        async def cmd_raid(self, ctx: twitchio_commands.Context):
            """!raid / !traid - Startet sofort einen Raid auf den bestmöglichen Partner (wie Auto-Raid)."""
            is_mod = getattr(ctx.author, "is_moderator", getattr(ctx.author, "moderator", False))
            is_broadcaster = getattr(
                ctx.author, "is_broadcaster", getattr(ctx.author, "broadcaster", False)
            )
            if not (is_broadcaster or is_mod):
                await ctx.send(
                    f"@{ctx.author.name} Nur Broadcaster oder Mods können !raid benutzen."
                )
                return

            channel_name = ctx.channel.name
            streamer_data = self._get_streamer_by_channel(channel_name)
            if not streamer_data:
                await ctx.send(
                    f"@{ctx.author.name} Dieser Kanal ist nicht als Partner registriert. Bitte erst mit !raid_enable verifizieren."
                )
                return

            twitch_login, twitch_user_id, _ = streamer_data

            if not self._raid_bot or not self._raid_bot.auth_manager.has_enabled_auth(
                twitch_user_id
            ):
                await ctx.send(
                    f"@{ctx.author.name} OAuth fehlt – Anforderung: Twitch-Bot autorisieren mit !raid_enable."
                )
                return

            # needs_reauth=1 → Streamer muss erst re-authen, kein Raid
            if hasattr(self, "_is_fully_authed"):
                try:
                    if not await self._is_fully_authed(twitch_user_id):
                        await ctx.send(
                            f"@{ctx.author.name} Neu-Autorisierung erforderlich. "
                            "Bitte prüfe deine Discord-DMs oder nutze /traid für den neuen Auth-Link."
                        )
                        return
                except Exception:
                    log.debug(
                        "Manual raid auth precheck failed for %s",
                        twitch_login,
                        exc_info=True,
                    )

            if getattr(self._raid_bot, "session", None) is None:
                await ctx.send(f"@{ctx.author.name} Twitch-Bot nicht verfügbar.")
                return

            try:
                result = await self._raid_bot.start_manual_raid(
                    broadcaster_id=str(twitch_user_id),
                    broadcaster_login=str(twitch_login).lower(),
                )
            except Exception as exc:
                log.exception("Manual raid failed for %s", twitch_login)
                await ctx.send(f"@{ctx.author.name} Raid fehlgeschlagen: {exc}")
                return

            status = str(result.get("status") or "")
            if status == "started":
                target_login = str(result.get("target_login") or "").strip()
                await ctx.send(
                    f"@{ctx.author.name} Raid auf {target_login} gestartet! (Twitch-Countdown ~90s)"
                )
                return
            if status == "source_not_live":
                await ctx.send(
                    f"@{ctx.author.name} Du musst live sein, um !raid zu benutzen."
                )
                return
            if status == "source_not_eligible":
                await ctx.send(
                    f"@{ctx.author.name} !raid ist nur verfügbar, wenn du gerade Deadlock streamst oder gerade erst von Deadlock auf Just Chatting gewechselt bist."
                )
                return
            if status == "no_target":
                await ctx.send(
                    f"@{ctx.author.name} Weder Deadlock-Partner noch andere deutsche Deadlock-Streamer live."
                )
                return
            if status == "unavailable":
                await ctx.send(f"@{ctx.author.name} Twitch-Bot nicht verfügbar.")
                return

            await ctx.send(
                f"@{ctx.author.name} Raid fehlgeschlagen: {result.get('error') or 'unbekannter Fehler'}"
            )

        @twitchio_commands.command(name="title", aliases=["titel"])
        async def cmd_title(self, ctx: twitchio_commands.Context) -> None:
            """`!title <keywords>` — Generiert einen Stream-Titel mit MiniMax."""
            if not (ctx.author.is_broadcaster or ctx.author.is_mod):
                return

            raw_args = ctx.message.content.partition(" ")[2].strip()
            if not raw_args:
                await ctx.send("Verwendung: !title <keywords>  — z.B.: !title ranked solo grind")
                return

            include_live = "--live" in raw_args
            keywords = raw_args.replace("--live", "").strip()
            if not keywords:
                await ctx.send("Bitte Keywords angeben, z.B.: !title ranked solo grind")
                return

            await ctx.send("Generiere deinen Titel, einen Moment...")

            try:
                from bot.title_generator.title_db import (
                    get_streamer_avg_viewers,
                    get_streamer_title_history,
                    get_top_knowledge_titles,
                )
                from bot.title_generator.title_ai import RateLimitExceeded, generate_title
                from bot.title_generator.steam_lookup import (
                    get_live_state_for_discord_user,
                    get_rank_for_discord_user,
                )

                streamer_twitch_login = ctx.channel.name.lower()

                with readonly_connection() as conn:
                    row = conn.execute(
                        "SELECT twitch_user_id, discord_user_id FROM twitch_streamers WHERE LOWER(twitch_login) = %s LIMIT 1",
                        (streamer_twitch_login,),
                    ).fetchone()

                if not row:
                    await ctx.send("Streamer nicht gefunden – bitte Onboarding prüfen.")
                    return

                streamer_id = str(row[0])
                discord_user_id_raw = row[1]

                history = get_streamer_title_history(streamer_id, limit=30)
                own_avg = get_streamer_avg_viewers(streamer_id)

                for item in history:
                    avg = item.get("avg_viewers") or 0
                    followers = item.get("followers_start") or 1
                    item["relative_perf"] = avg / own_avg if own_avg > 0 else 0.0
                    item["engagement_rate"] = avg / followers

                knowledge = get_top_knowledge_titles(limit=30)

                rank_display = None
                live_state = None
                if discord_user_id_raw:
                    try:
                        discord_user_id = int(discord_user_id_raw)
                        rank_info = await get_rank_for_discord_user(discord_user_id)
                        if rank_info:
                            rank_display = rank_info["rank_display"]
                        if include_live:
                            live_state = await get_live_state_for_discord_user(discord_user_id)
                    except (ValueError, TypeError):
                        pass

                result = await generate_title(
                    streamer_id=streamer_id,
                    keywords=keywords,
                    title_history=history,
                    knowledge_titles=knowledge,
                    rank_display=rank_display,
                    live_state=live_state,
                    source="chat",
                )

                primary = result.get("primary") or "Kein Titel generiert"
                alts = result.get("alternatives", [])
                alt_str = f" | Alternativen: {' | '.join(alts)}" if alts else ""
                await ctx.send(f"Titel: {primary}{alt_str}")

            except RateLimitExceeded as exc:
                await ctx.send(f"Bitte warte noch {exc.retry_after} Sekunden vor der nächsten Anfrage.")
            except Exception:
                log.exception("cmd_title failed")
                await ctx.send("Fehler beim Generieren. Bitte später erneut versuchen.")
else:

    class RaidCommandsMixin:
        pass

import asyncio
import inspect
import logging
from datetime import UTC, datetime, timedelta

from ..core.partner_utils import is_partner_channel_for_chat_tracking
from ..storage import get_conn
from .constants import CHAT_JOIN_OFFLINE, eventsub

log = logging.getLogger("TwitchStreams.ChatBot")


class ConnectionMixin:
    @staticmethod
    def _looks_like_transport_session_gone_error(text: str) -> bool:
        lowered = str(text or "").lower()
        return (
            "websocket transport session does not exist" in lowered
            or "session does not exist" in lowered
            or "has already disconnected" in lowered
            or "session has disconnected" in lowered
        )

    @staticmethod
    def _looks_like_bot_banned_error(status: int | None, text: str) -> bool:
        if not text:
            return False
        lowered = text.lower()
        if "user is banned" in lowered:
            return True
        if "banned" in lowered:
            return True
        if status in {400, 403} and "ban" in lowered:
            return True
        return False

    @staticmethod
    def _is_partner_channel_for_chat_tracking(login: str) -> bool:
        """Check if channel is a partner (wrapper for partner_utils)."""
        return is_partner_channel_for_chat_tracking(login)

    def _blacklist_streamer_for_bot_ban(
        self,
        broadcaster_id: str | None,
        broadcaster_login: str,
        status: int | None,
        text: str,
    ) -> None:
        login = str(broadcaster_login or "").strip().lower().lstrip("#")
        if not login:
            return
        try:
            if self._is_partner_channel_for_chat_tracking(login):
                log.info(
                    "Blacklist übersprungen für Partner-Channel %s (_ensure_bot_is_mod)",
                    login,
                )
                return
        except Exception:
            log.debug(
                "Partner-Check in _ensure_bot_is_mod fehlgeschlagen fuer %s",
                login,
                exc_info=True,
            )

        target_id = str(broadcaster_id or "").strip() or None
        snippet = (text or "").replace("\n", " ").strip()[:180]
        reason = "chat_bot_banned_in_channel"
        if status is not None:
            reason += f" (HTTP {status})"
        if snippet:
            reason += f": {snippet}"

        raid_bot = getattr(self, "_raid_bot", None)
        if raid_bot and hasattr(raid_bot, "_add_to_blacklist"):
            raid_bot._add_to_blacklist(target_id, login, reason)
        else:
            try:
                with get_conn() as conn:
                    conn.execute(
                        """
                        INSERT INTO twitch_raid_blacklist (target_id, target_login, reason)
                        VALUES (?, ?, ?)
                        ON CONFLICT (target_login) DO UPDATE SET
                            target_id = EXCLUDED.target_id,
                            reason = EXCLUDED.reason
                        """,
                        (target_id, login, reason),
                    )
                    conn.commit()
            except Exception:
                log.debug(
                    "Konnte Bot-Ban Blacklist nicht schreiben fuer %s",
                    login,
                    exc_info=True,
                )

        log.warning("Bot-Ban erkannt: %s auf Raid-Blacklist gesetzt.", login)

    async def _ensure_bot_is_mod(self, broadcaster_id: str, broadcaster_login: str) -> bool:
        """
        Setzt den Bot als Moderator im Ziel-Channel über den Streamer-Token.
        Wird aufgerufen wenn ein join() mit 403 fehlschlägt.
        Gibt True zurück wenn der Bot erfolgreich als Mod gesetzt wurde.
        """
        raid_bot = getattr(self, "_raid_bot", None)
        if not raid_bot or not hasattr(raid_bot, "auth_manager"):
            log.debug(
                "_ensure_bot_is_mod: Kein RaidManager verfügbar für %s",
                broadcaster_login,
            )
            return False

        safe_bot_id = self.bot_id_safe or self.bot_id or ""
        if not safe_bot_id:
            log.debug("_ensure_bot_is_mod: Keine Bot-ID verfügbar")
            return False

        # Streamer-Token holen (wird bei Bedarf automatisch refreshed)
        session = raid_bot.session if hasattr(raid_bot, "session") else None
        if not session:
            log.debug("_ensure_bot_is_mod: Keine HTTP-Session im RaidManager")
            return False

        tokens = await raid_bot.auth_manager.get_tokens_for_user(broadcaster_id, session)
        if not tokens:
            log.warning(
                "_ensure_bot_is_mod: Keine gültige Autorisierung für %s verfügbar.",
                broadcaster_login,
            )
            return False

        access_token, _ = tokens

        try:
            import aiohttp

            url = "https://api.twitch.tv/helix/moderation/moderators"
            params = {
                "broadcaster_id": str(broadcaster_id),
                "user_id": str(safe_bot_id),
            }
            headers = {
                "Client-ID": self._client_id,
                "Authorization": f"Bearer {access_token}",
            }
            # Eigene Session öffnen – raid_bot.session kann jederzeit geschlossen
            # sein (Shutdown, Polling-Zyklus).  Konsistent mit _auto_ban_and_cleanup
            # und _unban_user.
            async with aiohttp.ClientSession() as mod_session:
                async with mod_session.post(url, headers=headers, params=params) as r:
                    if r.status in {200, 204}:
                        log.info(
                            "_ensure_bot_is_mod: Bot (ID: %s) ist jetzt Mod in %s (ID: %s)",
                            safe_bot_id,
                            broadcaster_login,
                            broadcaster_id,
                        )
                        return True
                    if r.status == 422:
                        # 422 = Bot ist bereits Mod → sollte nicht vorkommen wenn 403 vorher kam
                        log.info(
                            "_ensure_bot_is_mod: Bot ist bereits Mod in %s (422)",
                            broadcaster_login,
                        )
                        return True
                    txt = await r.text()
                    if self._looks_like_bot_banned_error(r.status, txt):
                        self._blacklist_streamer_for_bot_ban(
                            broadcaster_id=str(broadcaster_id),
                            broadcaster_login=broadcaster_login,
                            status=r.status,
                            text=txt,
                        )
                    # 400 "user is banned" → Bot wurde im Channel gebannt,
                    # Mod-Status kann nicht gesetzt werden bis der Ban aufgehoben wurde
                    log.warning(
                        "_ensure_bot_is_mod: Bot konnte nicht Mod werden in %s: HTTP %s %s",
                        broadcaster_login,
                        r.status,
                        txt[:180].replace("\n", " "),
                    )
                    return False
        except Exception:
            log.exception("_ensure_bot_is_mod: Exception für %s", broadcaster_login)
            return False

    async def _ensure_bot_token_registered(self) -> None:
        """
        TwitchIO nutzt intern den Token, der über add_token()
        registriert wurde.  Falls setup_hook() noch nicht fertig war oder der
        Token zwischenzeitlich refreshed wurde, kann dieser fehlen.  Wir
        registrieren ihn hier nochmal, um die Fehlerquelle zu eliminieren.
        """
        api_token = (self._bot_token or "").replace("oauth:", "").strip()
        if not api_token:
            return
        try:
            await self.add_token(api_token, self._bot_refresh_token)
        except Exception:
            log.debug("_ensure_bot_token_registered: add_token fehlgeschlagen", exc_info=True)

    async def join(self, channel_login: str, channel_id: str | None = None):
        """Joint einen Channel via EventSub (TwitchIO 3.x)."""
        try:
            normalized_login = channel_login.lower().lstrip("#")

            # Prüfe ZUERST, ob wir bereits subscribed sind
            if normalized_login in self._monitored_streamers:
                log.debug("Channel %s already monitored, skipping subscribe", channel_login)
                return True

            if not channel_id:
                user = await self.fetch_user(login=channel_login.lstrip("#"))
                if not user:
                    log.error("Could not find user ID for channel %s", channel_login)
                    return False
                channel_id = str(user.id)

            # Wir nutzen IMMER den Bot-Token für alle Channels.
            # Das hält die Anzahl der WebSocket-Verbindungen auf 1 (Limit bei Twitch ist 3 pro Client ID).
            # Voraussetzung: Der Bot muss Moderator im Ziel-Kanal sein.
            safe_bot_id = self.bot_id_safe or self.bot_id or ""

            # Token vor dem Subscribe sicherstellen – verhindert
            # "invalid transport and auth combination" wenn setup_hook()
            # noch nicht vollständig abgeschlossen war.
            await self._ensure_bot_token_registered()

            payload = eventsub.ChatMessageSubscription(
                broadcaster_user_id=str(channel_id), user_id=str(safe_bot_id)
            )

            # Wir abonnieren über den Standard-WebSocket des Bots
            await self.subscribe_websocket(payload=payload)

            self._monitored_streamers.add(normalized_login)
            self._channel_ids[normalized_login] = str(channel_id)
            return True
        except Exception as e:
            msg = str(e)
            if self._looks_like_transport_session_gone_error(msg):
                log.warning(
                    "join(): transport session invalid for %s - scheduling chat bot restart.",
                    channel_login,
                )
                restart = getattr(self, "request_transport_restart", None)
                if callable(restart):
                    try:
                        await restart(
                            reason="eventsub websocket transport session does not exist",
                            failed_channel=normalized_login,
                        )
                    except Exception:
                        log.exception(
                            "join(): failed to schedule chat bot restart for %s",
                            channel_login,
                        )
                log.error("Failed to join channel %s: %s", channel_login, e)
                return False
            if "invalid transport and auth combination" in msg:
                # Token war zum Zeitpunkt des ersten Versuchs noch nicht
                # gebunden.  Kurz warten, Token nochmal registrieren und
                # einmal erneut versuchen.
                log.warning(
                    "join(): 'invalid transport and auth combination' für %s – "
                    "Token wird neu registriert und ein Retry folgt.",
                    channel_login,
                )
                await asyncio.sleep(1)
                await self._ensure_bot_token_registered()
                try:
                    payload = eventsub.ChatMessageSubscription(
                        broadcaster_user_id=str(channel_id), user_id=str(safe_bot_id)
                    )
                    await self.subscribe_websocket(payload=payload)
                    self._monitored_streamers.add(normalized_login)
                    self._channel_ids[normalized_login] = str(channel_id)
                    log.info(
                        "join(): Retry erfolgreich für %s nach Token-Registrierung",
                        channel_login,
                    )
                    return True
                except Exception as retry_err:
                    log.error(
                        "join(): Retry für %s fehlgeschlagen: %s",
                        channel_login,
                        retry_err,
                    )
                return False
            if "403" in msg and "subscription missing proper authorization" in msg:
                # Monitored-Only Channels: kein Mod-Versuch, einfach überspringen.
                # Diese Channels haben keinen Streamer-Token, daher ist _ensure_bot_is_mod
                # sinnlos und würde nur Warnungen produzieren.
                if self._is_monitored_only(normalized_login):
                    log.info(
                        "join(): 403 für Monitored-Only Channel %s – kein Mod-Versuch, "
                        "Channel wird übersprungen (kein Streamer-Token verfügbar).",
                        channel_login,
                    )
                    return False

                # Cooldown-Prüfung: Bei gebannen Bots nicht wiederholt versuchen
                cd_key = normalized_login
                cd_until = self._mod_retry_cooldown.get(cd_key)
                if cd_until and datetime.now(UTC) < cd_until:
                    log.debug(
                        "join(): Mod-Retry für %s auf Cooldown bis %s – überspringe",
                        channel_login,
                        cd_until.isoformat(),
                    )
                    return False

                # Automatischer Retry: Bot als Mod setzen und nochmal versuchen
                log.info(
                    "join(): 403 für %s – versuche Bot automatisch als Mod zu setzen...",
                    channel_login,
                )
                mod_set = await self._ensure_bot_is_mod(str(channel_id), channel_login)
                if mod_set:
                    # Kurze Pause damit Twitch den Mod-Status propagiert
                    await asyncio.sleep(1)
                    try:
                        payload = eventsub.ChatMessageSubscription(
                            broadcaster_user_id=str(channel_id),
                            user_id=str(safe_bot_id),
                        )
                        await self.subscribe_websocket(payload=payload)
                        self._monitored_streamers.add(normalized_login)
                        self._channel_ids[normalized_login] = str(channel_id)
                        log.info(
                            "join(): Retry erfolgreich für %s nach Mod-Autorisierung",
                            channel_login,
                        )
                        return True
                    except Exception as retry_err:
                        log.warning(
                            "join(): Retry für %s fehlgeschlagen nach Mod-Autorisierung: %s",
                            channel_login,
                            retry_err,
                        )
                else:
                    # Cooldown setzen: Nächster Retry erst nach 10 Minuten
                    self._mod_retry_cooldown[cd_key] = datetime.now(UTC) + timedelta(minutes=10)
                    log.warning(
                        "join(): Konnte Bot nicht als Mod in %s setzen. "
                        "Falls der Bot im Channel gebannt ist, muss er dort zuerst "
                        "entbannt werden (/unban deutschedeadlockcommunity), "
                        "danach /mod deutschedeadlockcommunity ausführen. "
                        "Nächster Retry in 10 min.",
                        channel_login,
                    )
            elif "429" in msg or "transport limit exceeded" in msg.lower():
                log.error(
                    "Cannot join chat for %s: WebSocket Transport Limit (429) reached. "
                    "Ensure the bot uses only one WebSocket connection.",
                    channel_login,
                )
            else:
                log.error("Failed to join channel %s: %s", channel_login, e)
            return False

    async def join_channels(self, channels: list[str], rate_limit_delay: float = 0.2) -> int:
        """Kompatibilitäts-Helper für Bulk-Joins (z.B. Scout-Task)."""
        if not channels:
            return 0

        normalized = [str(ch or "").strip().lower().lstrip("#") for ch in channels]
        normalized = [ch for ch in normalized if ch]
        if not normalized:
            return 0

        try:
            set_monitored = getattr(self, "set_monitored_channels", None)
            if callable(set_monitored):
                set_monitored(normalized)
        except Exception:
            log.debug(
                "join_channels: konnte monitored-only Liste nicht aktualisieren",
                exc_info=True,
            )

        joined = 0
        for login in normalized:
            try:
                success = await self.join(login)
                if success:
                    joined += 1
                    if rate_limit_delay > 0:
                        await asyncio.sleep(rate_limit_delay)
            except Exception:
                log.exception("join_channels: unerwarteter Fehler bei %s", login)

        return joined

    async def part_channels(self, channels: list[str]) -> int:
        """Best-effort Unsubscribe + lokale Cache-Bereinigung für Channels."""
        if not channels:
            return 0

        normalized = [str(ch or "").strip().lower().lstrip("#") for ch in channels]
        normalized = [ch for ch in normalized if ch]
        if not normalized:
            return 0

        # Kandidaten-IDs vor dem Cache-Cleanup sichern
        channel_ids: dict[str, str] = {}
        channel_id_map = getattr(self, "_channel_ids", None)
        if isinstance(channel_id_map, dict):
            for login in normalized:
                raw_id = channel_id_map.get(login)
                if raw_id:
                    channel_ids[login] = str(raw_id)

        # Fehlende IDs nachladen (best effort)
        missing = [login for login in normalized if login not in channel_ids]
        fetch_users = getattr(self, "fetch_users", None)
        if missing and callable(fetch_users):
            try:
                users = await fetch_users(logins=missing)
                for user in users or []:
                    login = str(getattr(user, "login", "") or "").lower()
                    uid = str(getattr(user, "id", "") or "").strip()
                    if login and uid:
                        channel_ids[login] = uid
            except Exception:
                log.debug(
                    "part_channels: konnte User-IDs nicht nachladen (%s)",
                    ", ".join(missing),
                    exc_info=True,
                )
        elif missing:
            fetch_user = getattr(self, "fetch_user", None)
            if callable(fetch_user):
                for login in missing:
                    try:
                        user = await fetch_user(login=login)
                        uid = str(getattr(user, "id", "") or "").strip() if user else ""
                        if uid:
                            channel_ids[login] = uid
                    except Exception:
                        log.debug(
                            "part_channels: fetch_user fehlgeschlagen für %s",
                            login,
                            exc_info=True,
                        )

        target_ids = {uid for uid in channel_ids.values() if uid}
        unsubscribed = 0

        # 1) Primärpfad: Helix EventSub listing (TwitchIO fetch_eventsub_subscriptions)
        fetch_subs = getattr(self, "fetch_eventsub_subscriptions", None)
        delete_sub = getattr(self, "delete_eventsub_subscription", None)
        if target_ids and callable(fetch_subs) and callable(delete_sub):
            try:
                subs_result = fetch_subs()
                if inspect.isawaitable(subs_result):
                    subs_result = await subs_result

                subs_list = []

                async def _consume_async_iter(source) -> bool:
                    if source is None:
                        return False
                    if hasattr(source, "__aiter__"):
                        async for sub in source:
                            subs_list.append(sub)
                        return True
                    if hasattr(source, "__anext__"):
                        while True:
                            try:
                                sub = await source.__anext__()
                            except StopAsyncIteration:
                                break
                            subs_list.append(sub)
                        return True
                    return False

                handled = await _consume_async_iter(subs_result)
                if not handled and subs_result is not None:
                    inner = getattr(subs_result, "subscriptions", None)
                    if inner is not None:
                        handled = await _consume_async_iter(inner)
                        if not handled:
                            try:
                                subs_list.extend(list(inner))
                            except TypeError:
                                log.debug(
                                    "part_channels: unerwarteter subscriptions-Typ: %s",
                                    type(inner),
                                )
                    else:
                        try:
                            subs_list.extend(list(subs_result))
                        except TypeError:
                            log.debug(
                                "part_channels: unerwarteter fetch_eventsub_subscriptions-Typ: %s",
                                type(subs_result),
                            )

                for sub in subs_list:
                    try:
                        sub_type = getattr(sub, "type", "") or getattr(sub, "subscription_type", "")
                        if sub_type != "channel.chat.message":
                            continue
                        condition = getattr(sub, "condition", None)
                        if isinstance(condition, dict):
                            broadcaster_id = str(
                                condition.get("broadcaster_user_id")
                                or condition.get("broadcaster_id")
                                or ""
                            ).strip()
                        else:
                            broadcaster_id = str(
                                getattr(condition, "broadcaster_user_id", "")
                                or getattr(condition, "broadcaster_id", "")
                                or ""
                            ).strip()
                        if not broadcaster_id or broadcaster_id not in target_ids:
                            continue

                        sub_id = (
                            getattr(sub, "id", None)
                            or getattr(sub, "subscription_id", None)
                            or getattr(sub, "uuid", None)
                        )
                        if sub_id:
                            await delete_sub(sub_id)
                            unsubscribed += 1
                    except Exception:
                        log.debug(
                            "part_channels: Fehler beim Loeschen einer EventSub-Subscription",
                            exc_info=True,
                        )
            except Exception:
                log.debug("part_channels: fetch_eventsub_subscriptions fehlgeschlagen", exc_info=True)

        # 2) Fallback: TwitchIO 3.x WebSocket-Subscriptions (falls verfügbar)
        if target_ids and unsubscribed == 0:
            ws_subs = getattr(self, "websocket_subscriptions", None)
            ws_delete = getattr(self, "delete_websocket_subscription", None)
            if callable(ws_subs) and callable(ws_delete):
                try:
                    subs_map = await ws_subs()
                    if isinstance(subs_map, dict):
                        for sub_id, sub in subs_map.items():
                            try:
                                condition = getattr(sub, "condition", None)
                                if isinstance(condition, dict):
                                    broadcaster_id = str(
                                        condition.get("broadcaster_user_id")
                                        or condition.get("broadcaster_id")
                                        or ""
                                    ).strip()
                                else:
                                    broadcaster_id = str(
                                        getattr(condition, "broadcaster_user_id", "")
                                        or getattr(condition, "broadcaster_id", "")
                                        or ""
                                    ).strip()
                                if not broadcaster_id or broadcaster_id not in target_ids:
                                    continue

                                sub_type = getattr(sub, "type", "")
                                sub_type_value = getattr(sub_type, "value", None) or str(sub_type or "")
                                if not sub_type_value:
                                    continue
                                if (
                                    "channel.chat.message" not in sub_type_value
                                    and "ChannelChatMessage" not in sub_type_value
                                ):
                                    continue

                                await ws_delete(sub_id)
                                unsubscribed += 1
                            except Exception:
                                log.debug(
                                    "part_channels: Fehler beim Loeschen einer WebSocket-Subscription",
                                    exc_info=True,
                                )
                except Exception:
                    log.debug("part_channels: websocket_subscriptions fehlgeschlagen", exc_info=True)

        # Lokale Caches bereinigen
        monitored = getattr(self, "_monitored_streamers", None)
        if isinstance(monitored, set):
            for login in normalized:
                monitored.discard(login)
        if isinstance(channel_id_map, dict):
            for login in normalized:
                channel_id_map.pop(login, None)
        monitored_only = getattr(self, "_monitored_only_channels", None)
        if isinstance(monitored_only, set):
            for login in normalized:
                monitored_only.discard(login)

        if unsubscribed:
            log.info(
                "part_channels: %d EventSub-Subscription(s) geloescht (%d Channels).",
                unsubscribed,
                len(normalized),
            )
        else:
            log.debug(
                "part_channels: keine EventSub-Subscription geloescht (Channels=%d, targets=%d)",
                len(normalized),
                len(target_ids),
            )

        return unsubscribed

    async def follow_channel(self, broadcaster_id: str) -> bool:
        """
        Prüft, ob der Bot dem Channel bereits folgt.

        Hinweis: Twitch bietet seit dem 28.07.2021 keine öffentliche Helix-API
        mehr zum Erstellen von Follows an.
        """
        safe_bot_id = self.bot_id_safe or self.bot_id
        if not safe_bot_id or not self._token_manager:
            log.debug("follow_channel: Kein Bot-ID oder Token-Manager verfügbar")
            return False

        import aiohttp

        for attempt in range(2):
            try:
                tokens = await self._token_manager.get_valid_token()
                if not tokens:
                    return False
                access_token, _ = tokens

                async with aiohttp.ClientSession() as session:
                    headers = {
                        "Client-ID": self._client_id,
                        "Authorization": f"Bearer {access_token}",
                    }
                    params = {
                        "user_id": str(safe_bot_id),
                        "broadcaster_id": str(broadcaster_id),
                    }
                    async with session.get(
                        "https://api.twitch.tv/helix/channels/followed",
                        headers=headers,
                        params=params,
                    ) as r:
                        if r.status == 200:
                            data = await r.json(content_type=None)
                            follows = data.get("data", []) if isinstance(data, dict) else []
                            if follows:
                                log.info(
                                    "follow_channel: Bot folgt bereits %s",
                                    broadcaster_id,
                                )
                                return True

                            if not getattr(self, "_follow_api_create_removed_logged", False):
                                log.info(
                                    "follow_channel: Twitch-API kann keine Follows mehr erstellen "
                                    "(abgeschaltet am 28.07.2021). Manual Follow erforderlich."
                                )
                                self._follow_api_create_removed_logged = True
                            log.debug(
                                "follow_channel: Bot folgt %s derzeit nicht",
                                broadcaster_id,
                            )
                            return False
                        txt = await r.text()
                        if r.status == 401:
                            txt_l = txt.lower()
                            if "user:read:follows" in txt_l or "missing required scope" in txt_l:
                                if not getattr(self, "_follow_scope_missing_logged", False):
                                    log.warning(
                                        "follow_channel: Bot-Token ohne Scope user:read:follows; "
                                        "Follow-Status kann nicht geprüft werden."
                                    )
                                    self._follow_scope_missing_logged = True
                                return False
                            if attempt == 0:
                                log.debug(
                                    "follow_channel: 401 für %s, triggere Token-Refresh",
                                    broadcaster_id,
                                )
                                await self._token_manager.get_valid_token(force_refresh=True)
                                continue
                        log.debug(
                            "follow_channel: Follow-Check HTTP %s – %s",
                            r.status,
                            txt[:200],
                        )
                        return False
            except Exception:
                log.debug("follow_channel: Exception", exc_info=True)
                return False
        return False

    async def join_partner_channels(self):
        """
        Joint ALLE Channels (Partner + Monitored + Category).

        Datensammlung: ALLE
        Bot-Funktionen: Nur Partner (wird in event_message geprüft)
        """
        with get_conn() as conn:
            # Hole ALLE Streamer mit OAuth (Partner + wer OAuth hat)
            # Datensammlung läuft für alle, Bot-Funktionen nur für Partner
            partners = conn.execute(
                """
                SELECT DISTINCT s.twitch_login,
                                s.twitch_user_id,
                                a.scopes,
                                l.is_live,
                                COALESCE(l.last_game, '')
                FROM twitch_streamers_partner_state s
                JOIN twitch_raid_auth a ON s.twitch_user_id = a.twitch_user_id
                LEFT JOIN twitch_live_state l ON s.twitch_user_id = l.twitch_user_id
                WHERE a.raid_enabled IS TRUE
                   OR s.is_partner_active = 1
                """
            ).fetchall()

        channels_to_join = []
        for login, uid, scopes_raw, is_live, last_game in partners:
            login_norm = (login or "").strip()
            if not login_norm:
                continue
            scopes = [s.strip().lower() for s in (scopes_raw or "").split() if s.strip()]
            has_channel_bot_grant = "channel:bot" in scopes
            if not has_channel_bot_grant:
                continue
            if (is_live is None or not bool(is_live)) and not CHAT_JOIN_OFFLINE:
                continue
            # Normalisieren und prüfen
            normalized_login = login_norm.lower().lstrip("#")

            if normalized_login in self._monitored_streamers:
                continue
            channels_to_join.append((login_norm, uid))

        if channels_to_join:
            label = "LIVE partner" if not CHAT_JOIN_OFFLINE else "partner"
            log.info(
                "Joining %d new %s channels: %s",
                len(channels_to_join),
                label,
                ", ".join([c[0] for c in channels_to_join[:10]]),
            )
            for login, uid in channels_to_join:
                try:
                    # Wir übergeben ID falls vorhanden, sonst wird sie in join() gefetched
                    success = await self.join(login, channel_id=uid)
                    if success:
                        await asyncio.sleep(0.2)  # Rate limiting
                except Exception as e:
                    log.exception("Unexpected error joining channel %s: %s", login, e)

"""Analytics background tasks for Twitch."""

from __future__ import annotations

import asyncio
import logging
from datetime import UTC, datetime

from discord.ext import tasks

from ..core.chat_bots import build_known_chat_bot_not_in_clause, is_known_chat_bot
from .. import storage as storage_oauth  # SQLite — OAuth tokens & raid auth state
from ..storage import pg as storage  # PostgreSQL — analytics data

# NOTE: Twitch game-owner analytics are intentionally unused.
# The data is global and tied to owned/managed games, not to individual
# streamer channels, so we do not request or query it in this product.

log = logging.getLogger("TwitchStreams.Analytics")


class TwitchAnalyticsMixin:
    """
    Mixin for periodic analytics collection (Subs, Ads, etc.).
    Requires authorized OAuth tokens and matching scopes.
    """

    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        # Warn about missing chatters scope only once per live session to avoid log spam
        self._chatters_scope_warned: set[tuple[str, int]] = set()
        self._chatters_user_fallback_warned: set[tuple[str, int]] = set()
        self._analytics_task = self.collect_analytics_data.start()
        self._chatters_task = self.collect_chatters_data.start()
        self._retention_task = self.compute_raid_retention.start()

    async def cog_unload(self):
        await super().cog_unload()
        self.collect_analytics_data.cancel()
        self.collect_chatters_data.cancel()
        self.compute_raid_retention.cancel()

    @tasks.loop(hours=6)
    async def collect_analytics_data(self):
        """
        Periodically collect analytics data for authorized streamers.
        Runs every 6 hours to avoid API spam, as these numbers don't change extremely fast.
        """
        if not self.api:
            return

        try:
            await self.bot.wait_until_ready()
        except Exception:
            return

        log.info("Starting analytics collection (Subs + Ads)...")

        # Get authorized users with raid_enabled=1 (assuming they granted scopes)
        # Note: We should actually check if they have the specific scope,
        # but for now we assume the new scope set is used if they re-authed.
        try:
            with storage_oauth.get_conn() as conn:
                rows = conn.execute(
                    """
                    SELECT twitch_user_id, twitch_login
                    FROM twitch_raid_auth
                    WHERE raid_enabled IS TRUE
                    """
                ).fetchall()
        except Exception:
            log.exception("Failed to load authorized users for analytics")
            return

        users_processed = 0
        subs_snapshots = 0
        ads_snapshots = 0
        for row in rows:
            user_id = row[0] if not hasattr(row, "keys") else row["twitch_user_id"]
            login = row[1] if not hasattr(row, "keys") else row["twitch_login"]

            # Use RaidBot's auth manager to get a fresh token if possible
            if not getattr(self, "_raid_bot", None):
                continue

            session = self.api.get_http_session()
            token = await self._raid_bot.auth_manager.get_valid_token(user_id, session)

            if not token:
                log.debug("Skipping analytics collection: no valid authorization available.")
                continue

            scopes = {s.lower() for s in self._raid_bot.auth_manager.get_scopes(user_id)}
            did_collect_for_user = False

            try:
                if "channel:read:subscriptions" in scopes:
                    if await self._collect_subs_for_user(user_id, login, token):
                        subs_snapshots += 1
                        did_collect_for_user = True

                if "channel:read:ads" in scopes:
                    if await self._collect_ads_schedule_for_user(user_id, login, token):
                        ads_snapshots += 1
                        did_collect_for_user = True
            except Exception:
                log.exception("Failed to collect analytics for %s", login)

            if did_collect_for_user:
                users_processed += 1
                # Sleep to be nice to the API
                await asyncio.sleep(2)
            else:
                log.debug(
                    "Skipping analytics metrics for %s: missing scopes (need channel:read:subscriptions and/or channel:read:ads).",
                    login,
                )

        log.info(
            "Analytics collection finished. users=%d, subs_snapshots=%d, ads_snapshots=%d",
            users_processed,
            subs_snapshots,
            ads_snapshots,
        )

    async def _collect_subs_for_user(self, user_id: str, login: str, token: str) -> bool:
        """Fetch and store subscription data."""
        data = await self.api.get_broadcaster_subscriptions(user_id, token)
        if not data:
            return False

        total = int(data.get("total", 0))
        points = int(data.get("points", 0))

        # Determine breakdown from 'data' list if available (depends on API response pagination,
        # usually getting 'total' is enough for the headline number.
        # Detailed breakdown per tier might require iterating all pages which is expensive.
        # For now, we store total and points.

        # Twitch API /subscriptions returns a list of sub objects.
        # "total" field in the response represents the total number of subscriptions.
        # "points" is also returned in the response root.

        # We can try to approximate tiers if we only fetch the first page, but 'total' is exact.

        now_iso = datetime.now(UTC).isoformat()

        with storage.get_conn() as conn:
            conn.execute(
                """
                INSERT INTO twitch_subscriptions_snapshot
                (twitch_user_id, twitch_login, total, points, snapshot_at)
                VALUES (%s, %s, %s, %s, %s)
                """,
                (user_id, login, total, points, now_iso),
            )
        return True

    async def _collect_ads_schedule_for_user(self, user_id: str, login: str, token: str) -> bool:
        """Fetch and store ad schedule data."""
        data = await self.api.get_ad_schedule(user_id, token)
        if not data:
            return False

        def _safe_int(value):
            if value is None:
                return None
            try:
                return int(value)
            except (TypeError, ValueError):
                return None

        def _safe_time_text(value):
            if value is None:
                return None
            if isinstance(value, str):
                return value.strip() or None
            if isinstance(value, (int, float)):
                ts = float(value)
                if ts <= 0:
                    return None
                # Some APIs occasionally return milliseconds; normalize to seconds.
                if ts > 10_000_000_000:
                    ts = ts / 1000.0
                try:
                    return datetime.fromtimestamp(ts, tz=UTC).isoformat()
                except (OverflowError, OSError, ValueError):
                    return str(int(ts))
            text = str(value).strip()
            return text or None

        now_iso = datetime.now(UTC).isoformat()
        next_ad_at = _safe_time_text(data.get("next_ad_at"))
        last_ad_at = _safe_time_text(data.get("last_ad_at"))
        duration = _safe_int(data.get("duration"))
        preroll_free_time = _safe_int(data.get("preroll_free_time"))
        snooze_count = _safe_int(data.get("snooze_count"))
        snooze_refresh_at = _safe_time_text(data.get("snooze_refresh_at"))

        with storage.get_conn() as conn:
            conn.execute(
                """
                INSERT INTO twitch_ads_schedule_snapshot
                (
                    twitch_user_id, twitch_login, next_ad_at, last_ad_at,
                    duration, preroll_free_time, snooze_count, snooze_refresh_at, snapshot_at
                )
                VALUES (%s, %s, %s, %s, %s, %s, %s, %s, %s)
                """,
                (
                    user_id,
                    login,
                    next_ad_at,
                    last_ad_at,
                    duration,
                    preroll_free_time,
                    snooze_count,
                    snooze_refresh_at,
                    now_iso,
                ),
            )
        return True

    @collect_analytics_data.before_loop
    async def _before_analytics(self):
        await self.bot.wait_until_ready()

    # ------------------------------------------------------------------
    # Chatters Poller (alle 5 Min, nur für live Streams)
    # Tracked Lurker via GET /helix/chat/chatters (moderator:read:chatters)
    # ------------------------------------------------------------------

    def _get_chatters_user_fallback_warned_cache(self) -> set[tuple[str, int]]:
        warned = getattr(self, "_chatters_user_fallback_warned", None)
        if warned is None:
            warned = set()
            self._chatters_user_fallback_warned = warned
        return warned

    def _warn_chatters_user_fallback_once(
        self,
        *,
        user_id: str,
        session_id: int,
        login: str,
    ) -> None:
        key = (str(user_id), int(session_id))
        warned = self._get_chatters_user_fallback_warned_cache()
        if key in warned:
            return
        warned.add(key)
        log.warning(
            "Chatters-Poller: nutze Legacy-Broadcaster-Token fuer %s (Session %s). "
            "Der botzentrierte Pfad sollte 'moderator:read:chatters' ueber den Bot abdecken.",
            login,
            session_id,
        )

    def _clear_chatters_user_fallback_warning(
        self,
        *,
        user_id: str,
        session_id: int,
    ) -> None:
        self._get_chatters_user_fallback_warned_cache().discard((str(user_id), int(session_id)))

    async def _poll_chatters_single(
        self,
        user_id: str,
        login: str,
        session_id: int,
        now_iso: str,
        token: str | None = None,
    ) -> tuple[int, str, list[dict]] | None:
        """Pollt Chatters für einen Streamer via Helix API (nur wenn Token + moderator:read:chatters Scope vorhanden)."""
        chatters = []
        streamer_scopes = (
            {s.lower() for s in self._raid_bot.auth_manager.get_scopes(user_id)}
            if token and getattr(self, "_raid_bot", None)
            else set()
        )
        has_streamer_chatters_scope = "moderator:read:chatters" in streamer_scopes
        missing_streamer_scope = bool(token) and not has_streamer_chatters_scope

        # 1. Versuch: Bot-Token verwenden, wenn der Bot den Channel bereits überwacht.
        # Bot-first ist das gewollte Zielbild; Broadcaster-Scopes bleiben nur Fallback.
        bot_fallback_used = False
        bot_token, bot_id, bot_scopes = await self._resolve_bot_chatters_fallback(login)
        if bot_token and bot_id and (not bot_scopes or "moderator:read:chatters" in bot_scopes):
            try:
                chatters = await self.api.get_chatters(
                    broadcaster_id=user_id,
                    moderator_id=bot_id,
                    user_token=bot_token,
                )
                bot_fallback_used = bool(chatters)
                if chatters:
                    log.debug(
                        "Chatters-Poller: %d Chatters via Bot-Fallback für %s",
                        len(chatters),
                        login,
                    )
            except Exception:
                log.warning(
                    "Chatters-Poller: Bot-Fallback via Helix API fehlgeschlagen für %s",
                    login,
                    exc_info=True,
                )

        # 2. Fallback: Broadcaster-Token nur noch als Legacy-Rettungsnetz verwenden.
        if not chatters and token:
            if has_streamer_chatters_scope:
                self._warn_chatters_user_fallback_once(
                    user_id=user_id,
                    session_id=session_id,
                    login=login,
                )
                try:
                    chatters = await self.api.get_chatters(
                        broadcaster_id=user_id,
                        moderator_id=user_id,
                        user_token=token,
                    )
                    log.debug(
                        "Chatters-Poller: %d Chatters via Helix API für %s",
                        len(chatters),
                        login,
                    )
                except Exception:
                    log.warning(
                        "Chatters-Poller: Helix API fehlgeschlagen für %s",
                        login,
                        exc_info=True,
                    )

        if not chatters:
            if missing_streamer_scope:
                key = (user_id, session_id)
                if key not in self._chatters_scope_warned:
                    self._chatters_scope_warned.add(key)
                    log.warning(
                        "Chatters-Poller: %s hat keinen Zugriff auf 'moderator:read:chatters'. "
                        "Bot-Token (Scope + Mod-Status im Channel) ist erforderlich; Streamer-Token dient nur als Legacy-Fallback.",
                        login,
                    )
            return None

        if bot_fallback_used and missing_streamer_scope:
            key = (user_id, session_id)
            self._chatters_scope_warned.discard(key)
        if bot_fallback_used:
            self._clear_chatters_user_fallback_warning(user_id=user_id, session_id=session_id)

        log.debug(
            "Chatters-Poller: %d Chatters für %s (session %s)",
            len(chatters),
            login,
            session_id,
        )
        return (session_id, login, chatters)

    @staticmethod
    def _normalize_scope_values(scopes_raw) -> set[str]:
        if isinstance(scopes_raw, str):
            return {scope.strip().lower() for scope in scopes_raw.split() if scope.strip()}
        if isinstance(scopes_raw, (list, tuple, set)):
            return {
                str(scope).strip().lower()
                for scope in scopes_raw
                if str(scope).strip()
            }
        return set()

    async def _resolve_bot_chatters_fallback(
        self,
        login: str,
    ) -> tuple[str | None, str | None, set[str]]:
        login_norm = str(login or "").strip().lower()
        if not login_norm:
            return None, None, set()

        chat_bot = getattr(self, "_twitch_chat_bot", None)
        if not chat_bot:
            return None, None, set()

        monitored = {
            str(channel or "").strip().lower()
            for channel in (getattr(chat_bot, "_monitored_streamers", set()) or set())
            if str(channel or "").strip()
        }
        if login_norm not in monitored:
            return None, None, set()

        token_mgr = getattr(self, "_bot_token_manager", None)
        if not token_mgr:
            return None, None, set()

        try:
            token, manager_bot_id = await token_mgr.get_valid_token()
        except Exception:
            log.debug("Chatters-Poller: konnte Bot-Token nicht laden", exc_info=True)
            return None, None, set()

        token = str(token or "").strip()
        if token.lower().startswith("oauth:"):
            token = token[6:]
        bot_id = (
            str(
                manager_bot_id
                or getattr(chat_bot, "bot_id_safe", None)
                or getattr(chat_bot, "bot_id", None)
                or ""
            ).strip()
            or None
        )
        bot_scopes = self._normalize_scope_values(getattr(token_mgr, "scopes", ()))
        if not token or not bot_id:
            return None, bot_id, bot_scopes
        return token, bot_id, bot_scopes

    @tasks.loop(seconds=30)
    async def collect_chatters_data(self):
        """
        Pollt Chatters-Liste für ALLE live Streamer (Partner + Monitored + Category).

        WICHTIG: Datensammlung für Analyse läuft für ALLE.
        Bot-Funktionen (Raids, Commands, etc.) nur für Partner!
        """
        if not self.api:
            return

        try:
            # Live-Sessions kommen aus Postgres (Analytics-DB)
            with storage.get_conn() as conn:
                rows = conn.execute(
                    """
                    SELECT twitch_user_id, streamer_login, active_session_id
                    FROM twitch_live_state
                    WHERE is_live = 1
                      AND active_session_id IS NOT NULL
                    """
                ).fetchall()

            # OAuth/Permissions bleiben in SQLite (canonical raid_auth)
            auth_ids: set[str] = set()
            with storage_oauth.get_conn() as conn_sqlite:
                auth_rows = conn_sqlite.execute(
                    "SELECT twitch_user_id FROM twitch_raid_auth WHERE raid_enabled IS TRUE"
                ).fetchall()
                auth_ids = {
                    (r["twitch_user_id"] if hasattr(r, "keys") else r[0]) for r in auth_rows
                }

            # Track active sessions to reset per-session warning cache
            active_sessions = {
                (r[2] if not hasattr(r, "keys") else r["active_session_id"]) for r in rows
            }
            if self._chatters_scope_warned:
                self._chatters_scope_warned = {
                    key for key in self._chatters_scope_warned if key[1] in active_sessions
                }
            if self._get_chatters_user_fallback_warned_cache():
                self._chatters_user_fallback_warned = {
                    key
                    for key in self._get_chatters_user_fallback_warned_cache()
                    if key[1] in active_sessions
                }

            if rows:
                log.debug(
                    "Chatters-Poller: Tracking %d live Streamer (alle für Analyse)",
                    len(rows),
                )
        except Exception:
            log.exception("Chatters-Poller: Fehler beim Laden der live Streamer")
            return

        if not rows:
            return

        now_iso = datetime.now(UTC).isoformat(timespec="seconds")
        tasks_list = []

        # Token-Resolution vorbereiten (nur für Partner)
        session = self.api.get_http_session()
        auth_mgr = getattr(self, "_raid_bot", None) and getattr(
            self._raid_bot, "auth_manager", None
        )

        for row in rows:
            user_id = row[0] if not hasattr(row, "keys") else row["twitch_user_id"]
            login = row[1] if not hasattr(row, "keys") else row["streamer_login"]
            sess_id = row[2] if not hasattr(row, "keys") else row["active_session_id"]
            has_auth = user_id in auth_ids

            async def _wrap_poll(u_id, lgn, s_id, has_auth_flag):
                tok = None
                if has_auth_flag and auth_mgr:
                    tok = await auth_mgr.get_valid_token(u_id, session)
                return await self._poll_chatters_single(u_id, lgn, s_id, now_iso, tok)

            tasks_list.append(_wrap_poll(user_id, login, sess_id, has_auth))

        # Alle API-Calls parallel feuern
        results = await asyncio.gather(*tasks_list, return_exceptions=True)

        # Batch-Write
        payloads = [r for r in results if isinstance(r, tuple)]
        if not payloads:
            return

        try:
            with storage.get_conn() as conn:
                for session_id, login, chatters in payloads:
                    # Build normalized chatter list from API response
                    chatter_entries: list[tuple[str, str | None]] = []
                    excluded_bots = 0
                    for chatter in chatters:
                        c_login = (chatter.get("user_login") or "").lower().strip()
                        c_id = (chatter.get("user_id") or "").strip() or None
                        if c_login and not is_known_chat_bot(c_login):
                            chatter_entries.append((c_login, c_id))
                        elif c_login:
                            excluded_bots += 1

                    if not chatter_entries:
                        continue

                    logins = [e[0] for e in chatter_entries]

                    with conn.cursor() as cur:
                        # Check rollup to determine is_first_time_streamer per chatter
                        cur.execute(
                            "SELECT chatter_login FROM twitch_chatter_rollup"
                            " WHERE streamer_login = %s AND chatter_login = ANY(%s)",
                            (login, logins),
                        )
                        known_globally: set[str] = {row[0] for row in cur.fetchall()}

                        # Upsert all chatters into session table.
                        # ON CONFLICT: only refresh last_seen_at — don't overwrite messages
                        # or seen_via_chatters_api if the IRC path already set them.
                        cur.executemany(
                            """
                            INSERT INTO twitch_session_chatters (
                                session_id, streamer_login, chatter_login, chatter_id,
                                first_message_at, messages, is_first_time_streamer,
                                seen_via_chatters_api, last_seen_at
                            ) VALUES (%s, %s, %s, %s, %s, 0, %s, TRUE, %s)
                            ON CONFLICT (session_id, chatter_login) DO UPDATE
                                SET last_seen_at = EXCLUDED.last_seen_at
                            """,
                            [
                                (session_id, login, c_login, c_id, now_iso,
                                 c_login not in known_globally, now_iso)
                                for c_login, c_id in chatter_entries
                            ],
                        )

                        # Upsert rollup so lurkers become part of the global chatter history.
                        # ON CONFLICT: only refresh last_seen_at, preserve chatter_id if known.
                        cur.executemany(
                            """
                            INSERT INTO twitch_chatter_rollup (
                                streamer_login, chatter_login, chatter_id,
                                first_seen_at, last_seen_at, total_messages, total_sessions
                            ) VALUES (%s, %s, %s, %s, %s, 0, 1)
                            ON CONFLICT (streamer_login, chatter_login) DO UPDATE
                                SET last_seen_at = EXCLUDED.last_seen_at,
                                    chatter_id = COALESCE(
                                        twitch_chatter_rollup.chatter_id, EXCLUDED.chatter_id
                                    )
                            """,
                            [
                                (login, c_login, c_id, now_iso, now_iso)
                                for c_login, c_id in chatter_entries
                            ],
                        )

                    log.debug(
                        "Chatters-Poller: %d Chatter für %s (session %s) gespeichert (%d erstmalig, %d Bots gefiltert)",
                        len(chatter_entries), login, session_id,
                        sum(1 for c_login, _ in chatter_entries if c_login not in known_globally),
                        excluded_bots,
                    )
        except Exception:
            log.exception("Chatters-Poller: Batch-DB-Fehler")

    @collect_chatters_data.before_loop
    async def _before_chatters(self):
        await self.bot.wait_until_ready()

    # ------------------------------------------------------------------
    async def _handle_stream_online(
        self, broadcaster_user_id: str, broadcaster_login: str, event: dict
    ) -> None:
        """Wird von stream.online EventSub aufgerufen – triggert sofort den Go-Live-Handler."""
        started_at = (event.get("started_at") or "").strip() or None
        stream_id = str(event.get("id") or event.get("stream_id") or "").strip() or None
        login_value = (broadcaster_login or event.get("broadcaster_user_login") or "").strip().lower()
        now_iso = datetime.now(UTC).isoformat(timespec="seconds")

        try:
            with storage.get_conn() as c:
                if login_value:
                    c.execute(
                        """
                        DELETE FROM twitch_live_state
                         WHERE LOWER(streamer_login) = LOWER(%s)
                           AND LOWER(COALESCE(twitch_user_id, '')) <> LOWER(%s)
                        """,
                        (login_value, broadcaster_user_id),
                    )
                c.execute(
                    """
                    INSERT INTO twitch_live_state (
                        twitch_user_id, streamer_login, is_live, last_seen_at, last_stream_id, last_started_at
                    )
                    VALUES (%s, %s, 1, %s, %s, %s)
                    ON CONFLICT (twitch_user_id) DO UPDATE
                        SET streamer_login = COALESCE(NULLIF(EXCLUDED.streamer_login, ''), twitch_live_state.streamer_login),
                            is_live = 1,
                            last_seen_at = EXCLUDED.last_seen_at,
                            last_stream_id = COALESCE(EXCLUDED.last_stream_id, twitch_live_state.last_stream_id),
                            last_started_at = COALESCE(EXCLUDED.last_started_at, twitch_live_state.last_started_at)
                    """,
                    (
                        broadcaster_user_id,
                        login_value or broadcaster_user_id,
                        now_iso,
                        stream_id,
                        started_at,
                    ),
                )
        except Exception:
            log.debug(
                "_handle_stream_online: Konnte minimalen Live-State nicht speichern fuer %s",
                broadcaster_user_id,
                exc_info=True,
            )

        handler = getattr(self, "_handle_stream_went_live", None)
        if callable(handler):
            log.info(
                "EventSub stream.online: %s (%s) ist live – triggere Go-Live-Handler",
                broadcaster_login or broadcaster_user_id,
                broadcaster_user_id,
            )
            await handler(broadcaster_user_id, broadcaster_login)
        refresh = getattr(self, "_request_partner_raid_score_refresh", None)
        if callable(refresh):
            try:
                await refresh(
                    twitch_user_id=broadcaster_user_id,
                    login=login_value or broadcaster_login,
                    trigger="eventsub_stream_online",
                )
            except Exception:
                log.debug(
                    "_handle_stream_online: Partner raid score refresh failed for %s",
                    broadcaster_user_id,
                    exc_info=True,
                )

    async def _handle_channel_update(self, broadcaster_user_id: str, event: dict) -> None:
        """Speichert eine channel.update Notification (Titel/Game-Änderung) in der DB."""
        title = (event.get("title") or "").strip() or None
        game_name = (event.get("category_name") or event.get("game_name") or "").strip() or None
        language = (event.get("broadcaster_language") or "").strip() or None
        try:
            with storage.get_conn() as c:
                c.execute(
                    """
                    INSERT INTO twitch_channel_updates (twitch_user_id, title, game_name, language, recorded_at)
                    VALUES (%s, %s, %s, %s, %s)
                    """,
                    (
                        broadcaster_user_id,
                        title,
                        game_name,
                        language,
                        datetime.now(UTC).isoformat(timespec="seconds"),
                    ),
                )
                # Auch twitch_live_state aktualisieren, falls Stream gerade läuft
                c.execute(
                    """
                    UPDATE twitch_live_state
                       SET last_title = COALESCE(%s, last_title),
                           last_game  = COALESCE(%s, last_game)
                     WHERE twitch_user_id = %s AND is_live = 1
                    """,
                    (title, game_name, broadcaster_user_id),
                )
            refresh = getattr(self, "_request_partner_raid_score_refresh", None)
            if callable(refresh):
                await refresh(
                    twitch_user_id=broadcaster_user_id,
                    trigger="eventsub_channel_update",
                )
        except Exception:
            log.exception("_handle_channel_update: Fehler für %s", broadcaster_user_id)

    async def _store_subscription_event(
        self, broadcaster_user_id: str, event: dict, event_type: str
    ) -> None:
        """Speichert channel.subscribe / channel.subscription.gift / channel.subscription.message."""
        user_login = (
            event.get("user_login") or event.get("user_name") or ""
        ).strip().lower() or None
        tier = (event.get("tier") or "1000").strip()
        is_gift = bool(event.get("is_gift"))
        gifter_login = (
            event.get("gifter_login") or event.get("gifter_user_login") or ""
        ).strip().lower() or None
        cumulative_months = int(event.get("cumulative_months") or event.get("months") or 0) or None
        streak_months = int(event.get("streak_months") or 0) or None
        message_data = event.get("message") or {}
        if isinstance(message_data, dict):
            message = (message_data.get("text") or "").strip() or None
        else:
            message = str(message_data).strip() or None
        total_gifted = int(event.get("total") or 0) or None

        session_id = self._get_active_session_id_by_user_id(broadcaster_user_id)

        try:
            with storage.get_conn() as c:
                c.execute(
                    """
                    INSERT INTO twitch_subscription_events
                        (session_id, twitch_user_id, event_type, user_login, tier,
                         is_gift, gifter_login, cumulative_months, streak_months,
                         message, total_gifted, received_at)
                    VALUES (%s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s)
                    """,
                    (
                        session_id,
                        broadcaster_user_id,
                        event_type,
                        user_login,
                        tier,
                        is_gift,
                        gifter_login,
                        cumulative_months,
                        streak_months,
                        message,
                        total_gifted,
                        datetime.now(UTC).isoformat(timespec="seconds"),
                    ),
                )
        except Exception:
            log.exception(
                "_store_subscription_event: Fehler für %s (%s)",
                broadcaster_user_id,
                event_type,
            )

    def _get_active_session_id_by_user_id(self, broadcaster_user_id: str) -> int | None:
        """Gibt die aktive session_id für einen Broadcaster zurück (über twitch_live_state).

        twitch_stream_sessions hat keine twitch_user_id-Spalte – deshalb über
        twitch_live_state.active_session_id lookupaben.
        """
        try:
            with storage.get_conn() as c:
                row = c.execute(
                    "SELECT active_session_id FROM twitch_live_state WHERE twitch_user_id = %s",
                    (broadcaster_user_id,),
                ).fetchone()
            if row and row[0] is not None:
                return int(row[0] if not hasattr(row, "keys") else row["active_session_id"])
        except Exception:
            log.debug(
                "_get_active_session_id_by_user_id: Fehler für %s",
                broadcaster_user_id,
                exc_info=True,
            )
        return None

    async def _store_ad_break_event(self, broadcaster_user_id: str, event: dict) -> None:
        """Speichert ein channel.ad_break.begin Event."""
        duration_seconds = int(event.get("duration_seconds") or 0) or None
        is_automatic_raw = event.get("is_automatic")
        # Use a real boolean so Postgres boolean columns accept the value.
        is_automatic = bool(is_automatic_raw) if is_automatic_raw is not None else False

        session_id = self._get_active_session_id_by_user_id(broadcaster_user_id)

        try:
            with storage.get_conn() as c:
                c.execute(
                    """
                    INSERT INTO twitch_ad_break_events
                        (session_id, twitch_user_id, duration_seconds, is_automatic, started_at)
                    VALUES (%s, %s, %s, %s, %s)
                    """,
                    (
                        session_id,
                        broadcaster_user_id,
                        duration_seconds,
                        is_automatic,
                        datetime.now(UTC).isoformat(timespec="seconds"),
                    ),
                )
        except Exception:
            log.exception("_store_ad_break_event: Fehler für %s", broadcaster_user_id)

    async def _store_bits_event(self, broadcaster_user_id: str, event: dict) -> None:
        """Speichert ein channel.cheer (Bits) Event in der Datenbank."""
        donor_login = (
            event.get("user_login") or event.get("user_name") or ""
        ).strip().lower() or None
        amount = int(event.get("bits") or event.get("amount") or 0)
        # Message kann ein String oder ein Dict {"text": "...", "emotes": ...} sein
        message_data = event.get("message")
        if isinstance(message_data, dict):
            message = (message_data.get("text") or "").strip() or None
        elif isinstance(message_data, str):
            message = message_data.strip() or None
        else:
            message = None
        if not amount:
            return
        # Session ID für den aktuellen Stream bestimmen (optional)
        session_id = self._get_active_session_id_by_user_id(broadcaster_user_id)

        try:
            with storage.get_conn() as c:
                c.execute(
                    """
                    INSERT INTO twitch_bits_events
                        (session_id, twitch_user_id, donor_login, amount, message, received_at)
                    VALUES (%s, %s, %s, %s, %s, %s)
                    """,
                    (
                        session_id,
                        broadcaster_user_id,
                        donor_login,
                        amount,
                        message,
                        datetime.now(UTC).isoformat(timespec="seconds"),
                    ),
                )
        except Exception:
            log.exception("_store_bits_event: Fehler beim Speichern für %s", broadcaster_user_id)

    async def _store_channel_points_event(self, broadcaster_user_id: str, event: dict) -> None:
        """Speichert ein channel.channel_points_*_reward_redemption.add Event."""
        user_login = (
            event.get("user_login") or event.get("user_name") or ""
        ).strip().lower() or None
        reward = event.get("reward") or {}
        reward_id = (reward.get("id") or event.get("reward_id") or "").strip() or None
        reward_title = (reward.get("title") or event.get("reward_title") or "").strip() or None
        reward_cost = int(reward.get("cost") or event.get("reward_cost") or 0) or None
        user_input = (event.get("user_input") or "").strip() or None
        redeemed_at = (event.get("redeemed_at") or "").strip() or datetime.now(UTC).isoformat(
            timespec="seconds"
        )

        session_id = self._get_active_session_id_by_user_id(broadcaster_user_id)

        try:
            with storage.get_conn() as c:
                c.execute(
                    """
                    INSERT INTO twitch_channel_points_events
                        (session_id, twitch_user_id, user_login, reward_id, reward_title, reward_cost, user_input, redeemed_at)
                    VALUES (%s, %s, %s, %s, %s, %s, %s, %s)
                    """,
                    (
                        session_id,
                        broadcaster_user_id,
                        user_login,
                        reward_id,
                        reward_title,
                        reward_cost,
                        user_input,
                        redeemed_at,
                    ),
                )
        except Exception:
            log.exception(
                "_store_channel_points_event: Fehler beim Speichern für %s",
                broadcaster_user_id,
            )

    async def _store_hype_train_event(
        self,
        broadcaster_user_id: str,
        event: dict,
        *,
        ended: bool,
        progress: bool = False,
    ) -> None:
        """Speichert ein channel.hype_train.begin/progress/end Event in der Datenbank."""
        started_at = (event.get("started_at") or "").strip() or None
        ended_at = (event.get("ended_at") or "").strip() or None if ended else None
        level = int(event.get("level") or 0) or None
        total_progress = int(event.get("total") or event.get("total_progress") or 0) or None
        duration_seconds: int | None = None
        if started_at and ended_at:
            try:
                from datetime import datetime as _dt

                dt_start = _dt.fromisoformat(started_at.replace("Z", "+00:00"))
                dt_end = _dt.fromisoformat(ended_at.replace("Z", "+00:00"))
                duration_seconds = max(0, int((dt_end - dt_start).total_seconds()))
            except (TypeError, ValueError):
                log.debug(
                    "_store_hype_train_event: Konnte Dauer nicht berechnen für %s",
                    broadcaster_user_id,
                    exc_info=True,
                )

        session_id = self._get_active_session_id_by_user_id(broadcaster_user_id)

        try:
            with storage.get_conn() as c:
                if ended:
                    # Versuche, ein bereits vorhandenes begin-Event zu aktualisieren
                    updated = c.execute(
                        """
                        UPDATE twitch_hype_train_events
                           SET ended_at = %s,
                               duration_seconds = %s,
                               level = COALESCE(%s, level),
                               total_progress = COALESCE(%s, total_progress)
                         WHERE twitch_user_id = %s
                           AND started_at = %s
                           AND ended_at IS NULL
                        """,
                        (
                            ended_at,
                            duration_seconds,
                            level,
                            total_progress,
                            broadcaster_user_id,
                            started_at,
                        ),
                    ).rowcount
                    if updated:
                        return
                phase = "progress" if progress else ("end" if ended else "begin")
                c.execute(
                    """
                    INSERT INTO twitch_hype_train_events
                        (session_id, twitch_user_id, started_at, ended_at,
                         duration_seconds, level, total_progress, event_phase)
                    VALUES (%s, %s, %s, %s, %s, %s, %s, %s)
                    """,
                    (
                        session_id,
                        broadcaster_user_id,
                        started_at,
                        ended_at,
                        duration_seconds,
                        level,
                        total_progress,
                        phase,
                    ),
                )
        except Exception:
            log.exception(
                "_store_hype_train_event: Fehler beim Speichern für %s",
                broadcaster_user_id,
            )

    async def _store_ban_event(
        self, broadcaster_user_id: str, event: dict, *, unbanned: bool = False
    ) -> None:
        """Speichert ein channel.ban / channel.unban Event."""
        event_type = "unban" if unbanned else "ban"
        target_login = (
            event.get("user_login") or event.get("user_name") or ""
        ).strip().lower() or None
        target_id = str(event.get("user_id") or "").strip() or None
        moderator_login = (event.get("moderator_user_login") or "").strip().lower() or None
        reason = (event.get("reason") or "").strip() or None
        ends_at = (event.get("ends_at") or "").strip() or None  # None = permanent

        session_id = self._get_active_session_id_by_user_id(broadcaster_user_id)

        try:
            with storage.get_conn() as c:
                c.execute(
                    """
                    INSERT INTO twitch_ban_events
                        (session_id, twitch_user_id, event_type, target_login, target_id,
                         moderator_login, reason, ends_at, received_at)
                    VALUES (%s, %s, %s, %s, %s, %s, %s, %s, %s)
                    """,
                    (
                        session_id,
                        broadcaster_user_id,
                        event_type,
                        target_login,
                        target_id,
                        moderator_login,
                        reason,
                        ends_at,
                        datetime.now(UTC).isoformat(timespec="seconds"),
                    ),
                )
        except Exception:
            log.exception("_store_ban_event: Fehler für %s (%s)", broadcaster_user_id, event_type)

    async def _store_shoutout_event(
        self, broadcaster_user_id: str, event: dict, *, direction: str
    ) -> None:
        """Speichert ein channel.shoutout.create / channel.shoutout.receive Event.
        direction: 'sent' | 'received'
        """
        if direction == "sent":
            other_id = str(event.get("to_broadcaster_user_id") or "").strip() or None
            other_login = (event.get("to_broadcaster_user_login") or "").strip().lower() or None
            moderator_login = (event.get("moderator_user_login") or "").strip().lower() or None
            viewer_count = int(event.get("viewer_count") or 0)
        else:
            other_id = str(event.get("from_broadcaster_user_id") or "").strip() or None
            other_login = (event.get("from_broadcaster_user_login") or "").strip().lower() or None
            moderator_login = None
            viewer_count = int(event.get("viewer_count") or 0)

        try:
            with storage.get_conn() as c:
                c.execute(
                    """
                    INSERT INTO twitch_shoutout_events
                        (twitch_user_id, direction, other_broadcaster_id, other_broadcaster_login,
                         moderator_login, viewer_count, received_at)
                    VALUES (%s, %s, %s, %s, %s, %s, %s)
                    """,
                    (
                        broadcaster_user_id,
                        direction,
                        other_id,
                        other_login,
                        moderator_login,
                        viewer_count,
                        datetime.now(UTC).isoformat(timespec="seconds"),
                    ),
                )
        except Exception:
            log.exception(
                "_store_shoutout_event: Fehler für %s (%s)",
                broadcaster_user_id,
                direction,
            )

    @tasks.loop(hours=1)
    async def compute_raid_retention(self):
        """Hourly: compute retention metrics for recent outgoing raids into twitch_raid_retention."""
        try:
            with storage.get_conn() as analytics_conn:
                raids = analytics_conn.execute(
                    """
                    SELECT id, from_broadcaster_login, to_broadcaster_login,
                           viewer_count, executed_at
                    FROM twitch_raid_history
                    WHERE executed_at >= datetime('now', '-7 days')
                    ORDER BY executed_at DESC
                    """
                ).fetchall()
        except Exception:
            log.exception("compute_raid_retention: failed to load raids from analytics storage")
            return

        if not raids:
            return

        session_bot_clause, session_bot_params = build_known_chat_bot_not_in_clause(
            column_expr="sc.chatter_login",
            placeholder="%s",
        )
        rollup_bot_clause, rollup_bot_params = build_known_chat_bot_not_in_clause(
            column_expr="chatter_login",
            placeholder="%s",
        )

        processed = 0
        for raid in raids:
            raid_id = raid[0]
            from_login = raid[1].lower()
            to_login = raid[2].lower()
            viewer_count = raid[3]
            executed_at_raw = raid[4]

            try:
                from datetime import UTC, datetime as _dt
                if isinstance(executed_at_raw, str):
                    executed_at = _dt.fromisoformat(executed_at_raw.replace("Z", "+00:00"))
                elif isinstance(executed_at_raw, _dt):
                    executed_at = executed_at_raw
                    if executed_at.tzinfo is None:
                        executed_at = executed_at.replace(tzinfo=UTC)
                else:
                    continue

                with storage.get_conn() as pg:
                    existing = pg.execute(
                        """
                        SELECT raid_id
                        FROM twitch_raid_retention
                        WHERE raid_id = %s
                          AND executed_at = %s
                        """,
                        (raid_id, executed_at),
                    ).fetchone()
                    if existing:
                        continue

                    target_session = pg.execute(
                        """
                        SELECT id FROM twitch_stream_sessions
                        WHERE LOWER(streamer_login) = %s
                          AND started_at <= %s
                          AND (ended_at IS NULL OR ended_at >= %s)
                        ORDER BY started_at DESC LIMIT 1
                        """,
                        (to_login, executed_at, executed_at),
                    ).fetchone()
                    if not target_session:
                        continue

                    target_session_id = target_session["id"]

                    def _count_chatters(offset_min: int) -> int:
                        row = pg.execute(
                            f"""
                            SELECT COUNT(
                                DISTINCT COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id)
                            ) AS cnt
                            FROM twitch_session_chatters sc
                            WHERE sc.session_id = %s
                              AND sc.last_seen_at >= %s
                              AND sc.last_seen_at <= (%s + (%s || ' minutes')::INTERVAL)
                              AND {session_bot_clause}
                            """,
                            (
                                target_session_id,
                                executed_at,
                                executed_at,
                                str(offset_min),
                                *session_bot_params,
                            ),
                        ).fetchone()
                        return row["cnt"] if row else 0

                    c5 = _count_chatters(5)
                    c15 = _count_chatters(15)
                    c30 = _count_chatters(30)

                    known_row = pg.execute(
                        f"""
                        SELECT COUNT(DISTINCT sc.chatter_login) AS known
                        FROM twitch_session_chatters sc
                        WHERE sc.session_id = %s
                          AND sc.last_seen_at >= %s
                          AND {session_bot_clause}
                          AND sc.chatter_login IN (
                              SELECT chatter_login FROM twitch_chatter_rollup
                              WHERE LOWER(streamer_login) = %s
                                AND {rollup_bot_clause}
                          )
                        """,
                        (
                            target_session_id,
                            executed_at,
                            *session_bot_params,
                            from_login,
                            *rollup_bot_params,
                        ),
                    ).fetchone()
                    known_from_raider = known_row["known"] if known_row else 0

                    new_row = pg.execute(
                        f"""
                        SELECT COUNT(
                            DISTINCT COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id)
                        ) AS new_viewers
                        FROM twitch_session_chatters sc
                        WHERE sc.session_id = %s
                          AND sc.last_seen_at >= %s
                          AND {session_bot_clause}
                          AND (
                              sc.chatter_login IS NULL
                              OR sc.chatter_login = ''
                              OR sc.chatter_login NOT IN (
                                  SELECT chatter_login FROM twitch_chatter_rollup
                                  WHERE LOWER(streamer_login) = %s
                                    AND first_seen_at < %s
                                    AND {rollup_bot_clause}
                              )
                          )
                        """,
                        (
                            target_session_id,
                            executed_at,
                            *session_bot_params,
                            to_login,
                            executed_at,
                            *rollup_bot_params,
                        ),
                    ).fetchone()
                    new_to_target = new_row["new_viewers"] if new_row else 0

                    new_chat_row = pg.execute(
                        f"""
                        SELECT COUNT(
                            DISTINCT COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id)
                        ) AS new_chatters
                        FROM twitch_session_chatters sc
                        WHERE sc.session_id = %s
                          AND sc.first_message_at >= %s
                          AND sc.messages > 0
                          AND {session_bot_clause}
                          AND (
                              sc.chatter_login IS NULL
                              OR sc.chatter_login = ''
                              OR sc.chatter_login NOT IN (
                                  SELECT chatter_login FROM twitch_chatter_rollup
                                  WHERE LOWER(streamer_login) = %s
                                    AND first_seen_at < %s
                                    AND {rollup_bot_clause}
                              )
                          )
                        """,
                        (
                            target_session_id,
                            executed_at,
                            *session_bot_params,
                            to_login,
                            executed_at,
                            *rollup_bot_params,
                        ),
                    ).fetchone()
                    new_chatters = new_chat_row["new_chatters"] if new_chat_row else 0

                    pg.execute(
                        """
                        INSERT INTO twitch_raid_retention
                            (raid_id, from_broadcaster_login, to_broadcaster_login,
                             viewer_count_sent, executed_at, target_session_id,
                             chatters_at_plus5m, chatters_at_plus15m, chatters_at_plus30m,
                             known_from_raider, new_to_target, new_chatters)
                        VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)
                        ON CONFLICT (raid_id, executed_at) DO NOTHING
                        """,
                        (
                            raid_id,
                            from_login,
                            to_login,
                            viewer_count,
                            executed_at,
                            target_session_id,
                            c5,
                            c15,
                            c30,
                            known_from_raider,
                            new_to_target,
                            new_chatters,
                        ),
                    )
                    processed += 1

            except Exception:
                log.exception("compute_raid_retention: error for raid_id=%s", raid_id)
                continue

        if processed:
            log.info("compute_raid_retention: inserted %d new rows", processed)

# cogs/twitch/raid_mixin.py
"""Mixin für Auto-Raid-Integration in TwitchStreamCog."""

import asyncio
import logging
import time
from datetime import UTC, datetime

from ..storage import load_active_partner, readonly_connection

log = logging.getLogger("TwitchStreams.RaidMixin")


class TwitchRaidMixin:
    """Integration der Raid-Bot-Logik in die Stream-Überwachung."""

    @staticmethod
    def _dashboard_raid_history_sync(limit: int = 50, from_broadcaster: str = "") -> list[dict]:
        with readonly_connection() as conn:
            if from_broadcaster:
                rows = conn.execute(
                    """
                    SELECT from_broadcaster_id, from_broadcaster_login,
                           to_broadcaster_id, to_broadcaster_login,
                           viewer_count, stream_duration_sec, executed_at,
                           success, error_message, target_stream_started_at,
                           candidates_count
                    FROM twitch_raid_history
                    WHERE from_broadcaster_login = %s
                    ORDER BY executed_at DESC
                    LIMIT %s
                    """,
                    (from_broadcaster.lower(), limit),
                ).fetchall()
            else:
                rows = conn.execute(
                    """
                    SELECT from_broadcaster_id, from_broadcaster_login,
                           to_broadcaster_id, to_broadcaster_login,
                           viewer_count, stream_duration_sec, executed_at,
                           success, error_message, target_stream_started_at,
                           candidates_count
                    FROM twitch_raid_history
                    ORDER BY executed_at DESC
                    LIMIT %s
                    """,
                    (limit,),
                ).fetchall()

        return [dict(row) for row in rows]

    async def _handle_auto_raid_on_offline(
        self,
        login: str,
        twitch_user_id: str | None,
        previous_state: dict,
        streams_by_login: dict[str, dict],
        offline_trigger_ts: float | None = None,
    ):
        """
        Wird aufgerufen, wenn ein Streamer offline geht.
        Versucht automatisch zu raiden, falls aktiviert.
        """
        if offline_trigger_ts is None:
            offline_trigger_ts = time.monotonic()

        recency_cap_seconds = 360  # Maximaler Abstand (10min), damit Deadlock noch relevant ist

        if not twitch_user_id:
            log.debug("Kein twitch_user_id für %s, überspringe Auto-Raid", login)
            return

        # Raid-Bot verfügbar?
        if not hasattr(self, "_raid_bot") or not self._raid_bot:
            log.debug("Raid-Bot nicht initialisiert, überspringe Auto-Raid für %s", login)
            return

        # Nur wenn Streamer Auto-Raid explizit aktiviert und autorisiert hat
        try:
            with readonly_connection() as conn:
                row = load_active_partner(conn, twitch_user_id=twitch_user_id)

            # Falls er nicht in twitch_streamers steht (noch kein Partner),
            # gehen wir davon aus, dass er den Bot via OAuth aktiviert hat.
            raid_bot_enabled = row["raid_bot_enabled"] if row and hasattr(row, "keys") else (row[13] if row else None)
            if row and not raid_bot_enabled:
                log.debug(
                    "Auto-Raid übersprungen für %s: deaktiviert im Partner-Registry",
                    login,
                )
                return
        except Exception:
            log.debug(
                "Auto-Raid DB-Check für %s fehlgeschlagen, fahre mit Auth-Check fort",
                login,
            )

        auth_mgr = getattr(self._raid_bot, "auth_manager", None)
        if not auth_mgr or not auth_mgr.has_enabled_auth(twitch_user_id):
            log.debug(
                "Auto-Raid übersprungen für %s: kein aktiver OAuth-Grant (OAuth via /traid oder !raid_enable erforderlich)",
                login,
            )
            return

        # needs_reauth=1 → Streamer muss erst re-authen, kein Auto-Raid
        if hasattr(self, "_is_fully_authed"):
            try:
                fully_authed = await self._is_fully_authed(twitch_user_id)
            except Exception:
                fully_authed = True  # im Zweifel erlauben
            if not fully_authed:
                log.info(
                    "Auto-Raid übersprungen für %s: needs_reauth=1 (Re-Auth erforderlich)",
                    login,
                )
                return

        if hasattr(
            self._raid_bot, "is_offline_auto_raid_suppressed"
        ) and self._raid_bot.is_offline_auto_raid_suppressed(twitch_user_id):
            log.info(
                "Auto-Raid übersprungen für %s: kürzlich manueller/externer Raid erkannt (in-memory)",
                login,
            )
            return

        target_game_lower = self._raid_bot._get_target_game_lower()

        last_game = (previous_state.get("last_game") or "").strip()
        had_deadlock_session = bool(int(previous_state.get("had_deadlock_in_session", 0) or 0))
        last_deadlock_seen_at_str = (
            previous_state.get("last_deadlock_seen_at") or ""
        ).strip() or None
        source_evaluation = self._raid_bot._evaluate_deadlock_raid_source(
            current_game=last_game,
            had_deadlock_session=had_deadlock_session,
            last_deadlock_seen_at=last_deadlock_seen_at_str,
        )
        allow_auto_raid = bool(source_evaluation.get("eligible"))

        if allow_auto_raid and target_game_lower:
            if str(source_evaluation.get("reason") or "") == "active_deadlock":
                log.debug(
                    "Auto-Raid erlaubt für %s: Aktiv %s gestreamt (last_game=%s)",
                    login,
                    target_game_lower.title(),
                    last_game,
                )
            elif str(source_evaluation.get("reason") or "") == "recent_deadlock_session":
                log.debug(
                    "Auto-Raid erlaubt für %s: Just Chatting mit frischer Deadlock-Session (last_seen=%s)",
                    login,
                    last_deadlock_seen_at_str or "unknown",
                )

        if not allow_auto_raid:
            evaluation_reason = str(source_evaluation.get("reason") or "")
            if evaluation_reason == "stale_deadlock_session":
                log.info(
                    "Auto-Raid ausgelassen für %s: letzter Deadlock > %ds her (last_seen=%s, last_game=%s)",
                    login,
                    recency_cap_seconds,
                    last_deadlock_seen_at_str or "unknown",
                    last_game or "unbekannt",
                )
                return
            if evaluation_reason == "just_chatting_without_deadlock_session":
                log.info(
                    "Auto-Raid ausgelassen für %s: Just Chatting ohne Deadlock-Session-Historie",
                    login,
                )
                return
            log.info(
                "Auto-Raid ausgelassen für %s: letzte Kategorie '%s' (had_deadlock_session=%s, reason=%s)",
                login,
                last_game or "unbekannt",
                had_deadlock_session,
                evaluation_reason or "unknown",
            )
            return

        # Stream-Dauer berechnen
        started_at_str = previous_state.get("last_started_at")
        stream_duration_sec = 0
        if started_at_str:
            try:
                started_at = datetime.fromisoformat(started_at_str.replace("Z", "+00:00"))
                now_calc = datetime.now(UTC)
                stream_duration_sec = int((now_calc - started_at).total_seconds())
            except Exception:
                log.debug("Konnte Stream-Dauer für %s nicht berechnen", login, exc_info=True)

        # Viewer-Count
        viewer_count = int(previous_state.get("last_viewer_count", 0))

        log.info(
            "Auto-Raid Trigger gestartet: %s (id=%s) offline, viewers=%d, duration=%ds",
            login,
            twitch_user_id,
            viewer_count,
            stream_duration_sec,
        )

        partner_rows = self._raid_bot._load_partner_roster_for_raid(twitch_user_id)
        online_partners = self._raid_bot._build_online_partner_candidates(
            partner_rows,
            streams_by_login,
        )
        eligible_partners, filtered_out = self._raid_bot._filter_deadlock_eligible_partner_candidates(
            online_partners
        )

        log.info(
            "Auto-Raid triggered für %s (offline): %d Online-Partner gefunden (%d eligible), "
            "Stream-Dauer: %d Sek, Viewer: %d",
            login,
            len(online_partners),
            len(eligible_partners),
            stream_duration_sec,
            viewer_count,
        )
        if filtered_out:
            log.debug(
                "Auto-Raid: Partner ausgeschlossen (Kategorie/Session): %s",
                "; ".join(filtered_out),
            )

        # Raid ausführen (mit Fallback auf DE-Deadlock-Streamer)
        try:
            target_login = await self._raid_bot.handle_streamer_offline(
                broadcaster_id=twitch_user_id,
                broadcaster_login=login,
                viewer_count=viewer_count,
                stream_duration_sec=stream_duration_sec,
                online_partners=eligible_partners,
                api=self.api if hasattr(self, "api") else None,
                category_id=self._category_id if hasattr(self, "_category_id") else None,
                offline_trigger_ts=offline_trigger_ts,
            )
            if target_login:
                log.info("✅ Auto-Raid erfolgreich: %s -> %s", login, target_login)
            else:
                log.debug(
                    "Auto-Raid für %s nicht durchgeführt (Bedingungen nicht erfüllt)",
                    login,
                )
        except Exception:
            log.exception("Fehler beim Auto-Raid für %s", login)

    async def _dashboard_raid_history(
        self, limit: int = 50, from_broadcaster: str = ""
    ) -> list[dict]:
        """Callback für Dashboard: Raid-History abrufen."""
        return await asyncio.to_thread(
            self._dashboard_raid_history_sync,
            limit,
            from_broadcaster,
        )

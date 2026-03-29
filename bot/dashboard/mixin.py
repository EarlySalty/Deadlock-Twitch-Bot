"""Dashboard helpers for the Twitch cog."""

from __future__ import annotations

import asyncio
import json
import os
from datetime import UTC, datetime, timedelta
from urllib.parse import parse_qsl, urlencode, urlparse, urlunparse

import discord
from aiohttp import web

from ..analytics.backend_extended import AnalyticsBackendExtended
from ..core.constants import (
    TWITCH_BUTTON_LABEL,
    TWITCH_DISCORD_REF_CODE,
    TWITCH_TARGET_GAME_NAME,
    log,
)
from ..discord_role_sync import normalize_discord_user_id, sync_streamer_role
from ..raid.integration_state import RaidIntegrationStateResolver
from ..storage import pg as storage
from ..raid.views import RaidAuthGenerateView, build_raid_requirements_embed
from .raids.oauth_callback import build_raid_oauth_callback_payload
from .runtime import DashboardBotService, DashboardRuntimeServices
from .server_v2 import build_v2_app
RAID_OAUTH_SUCCESS_REDIRECT_URL = "https://twitch.earlysalty.com/twitch/dashboard"
PUBLIC_WEBSITE_ONBOARDING_LOGIN = "public:website_onboarding"


VERIFICATION_SUCCESS_DM_MESSAGE = (
    "🎉 Glückwunsch! Du wurdest erfolgreich als **Streamer-Partner** verifiziert und bist jetzt offiziell Teil des "
    "Streamer-Teams. Wir melden uns, falls wir noch Fragen haben – ansonsten schauen wir uns deine Angaben kurz an. "
    "Bei Fragen kannst du dich gerne hier melden: https://discord.com/channels/1289721245281292288/1428062025145385111"
)


def _row_to_dict(row) -> dict:
    if row is None:
        return {}
    if hasattr(row, "keys"):
        return {k: row[k] for k in row.keys()}
    try:
        return dict(row)
    except Exception:
        return {}


class TwitchDashboardMixin:
    """Expose the aiohttp dashboard endpoints."""

    def _dashboard_bot_service(self) -> DashboardBotService:
        return DashboardBotService.from_cog(self)

    @staticmethod
    def _dashboard_build_referral_url(login: str) -> str:
        normalized_login = str(login or "").strip()
        base_url = (
            f"https://www.twitch.tv/{normalized_login}"
            if normalized_login
            else "https://www.twitch.tv/"
        )
        ref_code = (TWITCH_DISCORD_REF_CODE or "").strip()
        if not ref_code:
            return base_url
        parsed = urlparse(base_url)
        query = dict(parse_qsl(parsed.query, keep_blank_values=True))
        query["ref"] = ref_code
        return urlunparse(parsed._replace(query=urlencode(query)))

    @staticmethod
    def _dashboard_live_button_label_from_config(raw_json: object) -> str:
        text = str(raw_json or "").strip()
        if not text:
            return TWITCH_BUTTON_LABEL
        try:
            parsed = json.loads(text)
        except Exception:
            return TWITCH_BUTTON_LABEL
        if not isinstance(parsed, dict):
            return TWITCH_BUTTON_LABEL

        button_cfg = parsed.get("button") if isinstance(parsed.get("button"), dict) else {}
        label = str(button_cfg.get("label") or button_cfg.get("label_template") or "").strip()
        return label[:80] if label else TWITCH_BUTTON_LABEL

    def _dashboard_live_button_label(self, login: str) -> str:
        normalized_login = self._normalize_login(login)
        if not normalized_login:
            return TWITCH_BUTTON_LABEL
        try:
            with storage.readonly_connection() as conn:
                row = conn.execute(
                    """
                    SELECT config_json
                    FROM twitch_live_announcement_configs
                    WHERE LOWER(streamer_login) = LOWER(%s)
                    LIMIT 1
                    """,
                    (normalized_login,),
                ).fetchone()
        except Exception:
            log.debug(
                "Could not load live announcement label config for %s",
                normalized_login,
                exc_info=True,
            )
            return TWITCH_BUTTON_LABEL

        if not row:
            return TWITCH_BUTTON_LABEL

        raw_json = row[0] if not hasattr(row, "keys") else row["config_json"]
        return self._dashboard_live_button_label_from_config(raw_json)

    async def _dashboard_add(self, login: str, require_link: bool) -> str:
        return await self._cmd_add(login, require_link)

    async def _dashboard_remove(self, login: str) -> str:
        return await self._cmd_remove(login)

    async def _dashboard_live_active_announcements(self) -> list[dict[str, object]]:
        channel_id = int(getattr(self, "_notify_channel_id", 0) or 0)
        if channel_id <= 0:
            return []

        def _load_rows():
            with storage.readonly_connection() as conn:
                return conn.execute(
                    """
                    SELECT ls.streamer_login,
                           ls.last_discord_message_id,
                           ls.last_tracking_token,
                           cfg.config_json
                    FROM twitch_live_state ls
                    LEFT JOIN twitch_live_announcement_configs cfg
                      ON LOWER(cfg.streamer_login) = LOWER(ls.streamer_login)
                    WHERE ls.last_discord_message_id IS NOT NULL
                      AND ls.last_tracking_token IS NOT NULL
                    ORDER BY LOWER(ls.streamer_login)
                    """
                ).fetchall()

        rows = await asyncio.to_thread(_load_rows)

        announcements: list[dict[str, object]] = []
        for row in rows:
            streamer_login = self._normalize_login(
                row["streamer_login"] if hasattr(row, "keys") else row[0]
            )
            message_id_raw = row["last_discord_message_id"] if hasattr(row, "keys") else row[1]
            tracking_token_raw = row["last_tracking_token"] if hasattr(row, "keys") else row[2]
            config_json_raw = row["config_json"] if hasattr(row, "keys") else row[3]
            if not streamer_login:
                continue
            tracking_token = str(tracking_token_raw or "").strip()
            if not tracking_token:
                continue
            try:
                message_id = int(str(message_id_raw or "").strip())
            except (TypeError, ValueError):
                continue
            if message_id <= 0:
                continue
            announcements.append(
                {
                    "streamer_login": streamer_login,
                    "message_id": message_id,
                    "tracking_token": tracking_token,
                    "referral_url": self._dashboard_build_referral_url(streamer_login),
                    "button_label": self._dashboard_live_button_label_from_config(config_json_raw),
                    "channel_id": channel_id,
                }
            )
        return announcements

    async def _dashboard_live_link_click(
        self,
        *,
        streamer_login: str,
        tracking_token: str,
        discord_user_id: str,
        discord_username: str,
        guild_id: str | None,
        channel_id: str,
        message_id: str,
        source_hint: str,
    ) -> dict[str, object]:
        normalized_login = self._normalize_login(streamer_login)
        if not normalized_login:
            raise ValueError("invalid streamer_login")

        tracking_token_value = str(tracking_token or "").strip()
        if not tracking_token_value:
            raise ValueError("invalid tracking_token")

        discord_user_id_value = str(discord_user_id or "").strip()
        if not discord_user_id_value.isdigit():
            raise ValueError("invalid discord_user_id")

        channel_id_value = str(channel_id or "").strip()
        message_id_value = str(message_id or "").strip()
        guild_id_value = str(guild_id or "").strip() or None
        if not channel_id_value.isdigit():
            raise ValueError("invalid channel_id")
        if not message_id_value.isdigit():
            raise ValueError("invalid message_id")
        if guild_id_value is not None and not guild_id_value.isdigit():
            raise ValueError("invalid guild_id")

        clicked_at = datetime.now(tz=UTC).isoformat(timespec="seconds")
        ref_code = (TWITCH_DISCORD_REF_CODE or "").strip() or None

        def _persist_click() -> None:
            with storage.transaction() as conn:
                conn.execute(
                    """
                    INSERT INTO twitch_link_clicks (
                        clicked_at,
                        streamer_login,
                        tracking_token,
                        discord_user_id,
                        discord_username,
                        guild_id,
                        channel_id,
                        message_id,
                        ref_code,
                        source_hint
                    ) VALUES (%s, %s, %s, %s, %s, %s, %s, %s, %s, %s)
                    """,
                    (
                        clicked_at,
                        normalized_login,
                        tracking_token_value,
                        discord_user_id_value,
                        str(discord_username or "").strip(),
                        guild_id_value,
                        channel_id_value,
                        message_id_value,
                        ref_code,
                        str(source_hint or "").strip(),
                    ),
                )

        await asyncio.to_thread(_persist_click)

        return {"ok": True}

    async def _dashboard_list(self):
        return await asyncio.to_thread(self._dashboard_list_sync)

    def _dashboard_list_sync(self):
        target_game = (os.getenv("TWITCH_TARGET_GAME_NAME") or TWITCH_TARGET_GAME_NAME or "").strip()
        with storage.readonly_connection() as c:
            rows = c.execute(
                """
                SELECT s.twitch_login,
                       COALESCE(NULLIF(s.twitch_user_id, ''), NULLIF(a.twitch_user_id, '')) AS twitch_user_id,
                       s.manual_verified_permanent,
                       s.manual_verified_until,
                       s.manual_verified_at,
                       s.manual_partner_opt_out,
                       s.archived_at,
                       s.is_on_discord,
                       s.discord_user_id,
                       s.discord_display_name,
                       s.raid_bot_enabled,
                       a.raid_enabled AS raid_auth_enabled,
                       a.needs_reauth AS raid_needs_reauth,
                       a.authorized_at AS raid_authorized_at,
                       a.token_expires_at AS raid_token_expires_at,
                       sess.last_deadlock_stream_at
                  FROM twitch_partners_all_state s
                  LEFT JOIN twitch_raid_auth a
                    ON (
                         s.twitch_user_id IS NOT NULL
                         AND s.twitch_user_id = a.twitch_user_id
                       )
                    OR (
                         s.twitch_user_id IS NULL
                         AND LOWER(s.twitch_login) = LOWER(a.twitch_login)
                       )
                  LEFT JOIN (
                       SELECT LOWER(streamer_login) AS streamer_login,
                              MAX(CASE
                                    WHEN had_deadlock_in_session
                                         OR LOWER(COALESCE(game_name,'')) = LOWER(%s)
                                    THEN COALESCE(ended_at, started_at)
                              END) AS last_deadlock_stream_at
                         FROM twitch_stream_sessions
                        GROUP BY LOWER(streamer_login)
                  ) AS sess
                    ON sess.streamer_login = LOWER(s.twitch_login)
                 WHERE s.status = 'active'
                  ORDER BY s.twitch_login
                """,
                (target_game,),
            ).fetchall()
        return [_row_to_dict(row) for row in rows]

    async def _dashboard_raid_auth_url(
        self,
        login: str,
        discord_user_id: str | None = None,
        scope_profile: str | None = None,
    ) -> str:
        raw = str(login or "").strip()
        if not raw:
            raise ValueError("invalid or missing login")

        normalized: str
        use_discord_button_url = False
        normalized_discord_user_id = (
            str(discord_user_id or "").strip() if str(discord_user_id or "").strip().isdigit() else None
        )
        if raw.lower().startswith("discord:"):
            discord_id = raw.split(":", 1)[1].strip()
            if not discord_id.isdigit():
                raise ValueError("invalid discord user id")
            if normalized_discord_user_id is not None and normalized_discord_user_id != discord_id:
                raise ValueError("discord_user_id does not match login target")
            normalized = f"discord:{discord_id}"
            use_discord_button_url = True
        elif raw.lower() == PUBLIC_WEBSITE_ONBOARDING_LOGIN:
            normalized = PUBLIC_WEBSITE_ONBOARDING_LOGIN
        else:
            normalized = self._normalize_login(raw)
            if not normalized:
                raise ValueError("invalid or missing login")

        auth_manager = self._dashboard_bot_service().auth_manager()
        if not auth_manager:
            raise RuntimeError("Raid bot not initialized")

        if use_discord_button_url:
            return str(
                auth_manager.generate_discord_button_url(
                    normalized,
                    scope_profile=scope_profile or None,
                    discord_user_id=normalized_discord_user_id or normalized.split(":", 1)[1],
                )
            )
        if normalized_discord_user_id:
            return str(
                auth_manager.generate_discord_button_url(
                    normalized,
                    scope_profile=scope_profile or None,
                    expected_twitch_login=normalized,
                    discord_user_id=normalized_discord_user_id,
                )
            )
        if normalized == PUBLIC_WEBSITE_ONBOARDING_LOGIN:
            return str(auth_manager.generate_auth_url(normalized, scope_profile=scope_profile or None))
        return str(
            auth_manager.generate_auth_url(
                normalized,
                scope_profile=scope_profile or None,
                expected_twitch_login=normalized,
            )
        )

    async def _dashboard_raid_go_url(self, state: str) -> str | None:
        state_clean = str(state or "").strip()
        if not state_clean:
            raise ValueError("missing state parameter")

        auth_manager = self._dashboard_bot_service().auth_manager()
        if not auth_manager:
            raise RuntimeError("Raid bot not initialized")

        full_url = auth_manager.get_pending_auth_url(state_clean)
        return str(full_url).strip() if full_url else None

    @staticmethod
    def _dashboard_load_streamer_identity_sync(normalized_login: str):
        with storage.readonly_connection() as conn:
            return storage.load_streamer_identity(conn, twitch_login=normalized_login)

    async def _dashboard_raid_requirements(self, login: str) -> str:
        normalized = self._normalize_login(login)
        if not normalized:
            raise ValueError("Missing login parameter")

        auth_manager = self._dashboard_bot_service().auth_manager()
        if not auth_manager:
            raise RuntimeError("Raid bot not initialized")

        try:
            row = await asyncio.to_thread(self._dashboard_load_streamer_identity_sync, normalized)
        except Exception as exc:
            raise RuntimeError("Failed to load Discord link") from exc

        if not row:
            raise LookupError("Streamer not found")

        discord_user_id = str(
            row["discord_user_id"] if hasattr(row, "keys") else row[2] or ""
        ).strip()
        if not discord_user_id:
            raise LookupError("No Discord user linked for this streamer")

        try:
            user_id_int = int(discord_user_id)
        except (TypeError, ValueError) as exc:
            raise ValueError("Invalid Discord user id") from exc

        discord_bot = self._dashboard_bot_service().discord_bot()
        if not discord_bot:
            raise RuntimeError("Discord bot not available")

        user = discord_bot.get_user(user_id_int)
        if user is None:
            try:
                user = await discord_bot.fetch_user(user_id_int)
            except discord.NotFound:
                user = None
            except discord.HTTPException as exc:
                raise RuntimeError("Failed to fetch Discord user") from exc

        if user is None:
            raise LookupError("Discord user not found")

        embed = build_raid_requirements_embed(normalized)
        view = RaidAuthGenerateView(auth_manager=auth_manager, twitch_login=normalized)

        try:
            await user.send(embed=embed, view=view)
        except discord.Forbidden as exc:
            raise PermissionError("Discord DM blocked") from exc
        except discord.HTTPException as exc:
            raise RuntimeError("Failed to send Discord DM") from exc

        return f"Anforderungen per Discord an @{normalized} gesendet"

    async def _dashboard_raid_oauth_callback(
        self,
        *,
        code: str,
        state: str,
        error: str,
    ) -> dict:
        redirect_url = (
            (os.getenv("TWITCH_RAID_SUCCESS_REDIRECT_URL") or "").strip()
            or RAID_OAUTH_SUCCESS_REDIRECT_URL
        )
        return await build_raid_oauth_callback_payload(
            code=code,
            state=state,
            error=error,
            raid_bot=self._dashboard_bot_service().raid_bot,
            auth_manager=self._dashboard_bot_service().auth_manager(),
            success_redirect_url=redirect_url,
            failure_title="Autorisierung fehlgeschlagen",
            failure_body_html=(
                "<p>Autorisierung fehlgeschlagen.</p>"
                "<p>Bitte erneut versuchen oder Admin kontaktieren.</p>"
            ),
            schedule_background=getattr(self, "_spawn_bg_task", None),
        )

    def _raid_integration_state_resolver(self) -> RaidIntegrationStateResolver:
        raid_bot = self._dashboard_bot_service().raid_bot
        auth_manager = getattr(raid_bot, "auth_manager", None) if raid_bot else None
        token_error_handler = (
            getattr(auth_manager, "token_error_handler", None) if auth_manager else None
        )
        return RaidIntegrationStateResolver(
            auth_manager=auth_manager,
            token_error_handler=token_error_handler,
        )

    async def _integration_raid_auth_state(self, discord_user_id: str) -> dict[str, object]:
        state = self._raid_integration_state_resolver().resolve_auth_state(discord_user_id)
        return state.to_payload()

    async def _integration_raid_block_state(
        self,
        *,
        discord_user_id: str | None = None,
        twitch_login: str | None = None,
    ) -> dict[str, object]:
        state = self._raid_integration_state_resolver().resolve_block_state(
            discord_user_id=discord_user_id,
            twitch_login=twitch_login,
        )
        return state.to_payload()

    @staticmethod
    def _dashboard_load_analytics_suggestions_sync(
        cutoff: str,
        limit: int,
        partner_logins: set[str],
    ) -> list[dict[str, object]]:
        extras: list[dict[str, object]] = []
        with storage.readonly_connection() as c:
            rows = c.execute(
                """
                SELECT streamer,
                       COUNT(*) AS samples,
                       MAX(ts_utc) AS last_seen,
                       AVG(viewer_count) AS avg_viewers
                  FROM twitch_stats_category
                 WHERE ts_utc >= %s
                 GROUP BY streamer
                 ORDER BY samples DESC, last_seen DESC
                 LIMIT %s
                """,
                (cutoff, limit * 2),
            ).fetchall()
        for row in rows:
            login = str(row["streamer"] if hasattr(row, "keys") else row[0] or "").strip()
            if not login:
                continue
            lower = login.lower()
            if lower in partner_logins:
                continue
            extras.append(
                {
                    "twitch_login": login,
                    "avg_viewers": float(
                        row["avg_viewers"] if hasattr(row, "keys") else row[3] or 0.0
                    ),
                    "samples": int(row["samples"] if hasattr(row, "keys") else row[1] or 0),
                    "last_seen": str(row["last_seen"] if hasattr(row, "keys") else row[2] or ""),
                }
            )
            if len(extras) >= limit:
                break
        return extras

    async def _dashboard_analytics_suggestions(
        self,
        include_non_partners: bool = True,
        *,
        days: int = 90,
        limit: int = 120,
    ) -> dict:
        """Partner- und optionale Non-Partner-Vorschläge für das Analytics-Dashboard."""
        partners = await self._dashboard_list()
        extras: list[dict] = []

        if include_non_partners:
            partner_logins = {
                (row.get("twitch_login") or row.get("streamer") or "").strip().lower()
                for row in partners
                if isinstance(row, dict)
            }
            cutoff = (datetime.now(UTC) - timedelta(days=days)).isoformat()
            try:
                extras = await asyncio.to_thread(
                    self._dashboard_load_analytics_suggestions_sync,
                    cutoff,
                    limit,
                    partner_logins,
                )
            except Exception:
                log.debug(
                    "Konnte Non-Partner-Suggestions für Analytics nicht laden",
                    exc_info=True,
                )

        return {"partners": partners, "extras": extras}

    @staticmethod
    def _dashboard_set_discord_flag_sync(normalized: str, is_on_discord: bool) -> None:
        with storage.transaction() as conn:
            row = storage.set_streamer_discord_member(
                conn,
                twitch_login=normalized,
                is_on_discord=is_on_discord,
            )
            if not row:
                raise ValueError(f"{normalized} ist nicht gespeichert")

    async def _dashboard_set_discord_flag(self, login: str, is_on_discord: bool) -> str:
        normalized = self._normalize_login(login)
        if not normalized:
            raise ValueError("Ungültiger Login")

        await asyncio.to_thread(
            self._dashboard_set_discord_flag_sync,
            normalized,
            is_on_discord,
        )

        if is_on_discord:
            return f"{normalized} als Discord-Mitglied markiert"
        return f"Discord-Markierung für {normalized} entfernt"

    @staticmethod
    def _dashboard_archive_sync(normalized: str, desired: str) -> str:
        with storage.transaction() as conn:
            active_row = storage.load_active_partner(conn, twitch_login=normalized)
            history_row = storage.load_latest_partner_history(conn, twitch_login=normalized)
            if not active_row and not history_row:
                raise ValueError(f"{normalized} ist nicht gespeichert")
            if not active_row and history_row:
                current_status = str(
                    (
                        history_row.get("status")
                        if hasattr(history_row, "keys")
                        else history_row[20]
                    )
                    or ""
                ).strip().lower()
                if current_status and current_status != "active":
                    raise ValueError(f"{normalized} ist departnered und nicht nur archiviert")
                raise ValueError(f"{normalized} ist kein aktiver Partner")

            current = None
            if active_row:
                current = (
                    active_row.get("admin_archived_at")
                    if hasattr(active_row, "keys")
                    else None
                )

            if desired == "archive":
                if current:
                    return f"{normalized} ist bereits archiviert (seit {current})"
                storage.set_streamer_archive_state(conn, twitch_login=normalized, archived=True)
                return f"{normalized} archiviert"

            if desired == "unarchive":
                if not current:
                    return f"{normalized} ist nicht archiviert"
                storage.set_streamer_archive_state(conn, twitch_login=normalized, archived=False)
                return f"{normalized} ent-archiviert"

            if current:
                storage.set_streamer_archive_state(conn, twitch_login=normalized, archived=False)
                return f"{normalized} reaktiviert"
            storage.set_streamer_archive_state(conn, twitch_login=normalized, archived=True)
            return f"{normalized} archiviert"

    async def _dashboard_archive(self, login: str, mode: str) -> str:
        """
        Setzt oder entfernt das Admin-Archiv-Flag eines Streamers.

        mode: 'archive'/'on' -> setzt archived_at=now, 'unarchive'/'off' -> NULL, 'toggle' -> flip.
        """
        normalized = self._normalize_login(login)
        if not normalized:
            raise ValueError("Ungültiger Login")

        mode_clean = (mode or "").strip().lower()
        if mode_clean in {"archive", "on", "set"}:
            desired = "archive"
        elif mode_clean in {"unarchive", "off", "unset", "restore"}:
            desired = "unarchive"
        else:
            desired = "toggle"

        return await asyncio.to_thread(self._dashboard_archive_sync, normalized, desired)

    async def _dashboard_save_discord_profile(
        self,
        login: str,
        *,
        discord_user_id: str | None,
        discord_display_name: str | None,
        mark_member: bool,
    ) -> str:
        normalized = self._normalize_login(login)
        if not normalized:
            raise ValueError("Ungültiger Login")

        discord_id_clean = (discord_user_id or "").strip()
        if discord_id_clean and not discord_id_clean.isdigit():
            raise ValueError("Discord-ID muss eine Zahl sein")

        display_name_clean = (discord_display_name or "").strip()
        if len(display_name_clean) > 120:
            display_name_clean = display_name_clean[:120]

        # Versuche twitch_user_id zu ermitteln
        twitch_user_id: str | None = None

        # 1. Versuche aus raid_auth zu laden
        try:
            with storage.readonly_connection() as conn:
                raid_row = conn.execute(
                    "SELECT twitch_user_id FROM twitch_raid_auth WHERE LOWER(twitch_login)=LOWER(%s)",
                    (normalized,),
                ).fetchone()
                if raid_row:
                    twitch_user_id = raid_row[0]
        except Exception:
            log.debug(
                "Konnte user_id nicht aus raid_auth laden für %s",
                normalized,
                exc_info=True,
            )

        # 2. Falls nicht in raid_auth: API-Call
        twitch_api = self._dashboard_bot_service().twitch_api
        if not twitch_user_id and twitch_api:
            try:
                users = await twitch_api.get_users([normalized])
                user = users.get(normalized)
                if user:
                    twitch_user_id = user.get("id")
                    log.info(
                        "Fetched twitch_user_id %s for %s from API",
                        twitch_user_id,
                        normalized,
                    )
            except Exception:
                log.warning(
                    "Konnte user_id nicht von API holen für %s",
                    normalized,
                    exc_info=True,
                )

        try:
            with storage.transaction() as conn:
                storage.save_streamer_discord_profile(
                    conn,
                    twitch_login=normalized,
                    twitch_user_id=twitch_user_id,
                    discord_user_id=discord_id_clean or None,
                    discord_display_name=display_name_clean or None,
                    mark_member=mark_member,
                )
        except Exception:
            raise ValueError("Discord-ID wird bereits verwendet")

        return f"Discord-Daten für {normalized} aktualisiert"

    async def _get_monetization_stats(self) -> dict:
        """Aggregate monetization & hype train data for the last 30 days."""
        cutoff_30d = (datetime.now(UTC) - timedelta(days=30)).isoformat()

        ads: dict = {
            "total": 0,
            "auto": 0,
            "manual": 0,
            "sessions_with_ads": 0,
            "avg_duration_s": 0.0,
            "avg_viewer_drop_pct": None,
            "worst_ads": [],
        }
        hype_train: dict = {
            "total": 0,
            "avg_level": 0.0,
            "max_level": 0,
            "avg_duration_s": 0.0,
        }
        bits: dict = {"total": 0, "cheer_events": 0}
        subs: dict = {"total_events": 0, "gifted": 0}

        with storage.readonly_connection() as c:
            # 1a. Ad Break overview
            ad_agg = c.execute(
                """
                SELECT COUNT(*) AS total_ads,
                       SUM(CASE WHEN is_automatic IS TRUE THEN 1 ELSE 0 END) AS auto_ads,
                       AVG(duration_seconds) AS avg_duration,
                       COUNT(DISTINCT session_id) AS sessions_with_ads
                  FROM twitch_ad_break_events
                 WHERE started_at >= %s
                """,
                (cutoff_30d,),
            ).fetchone()
            if ad_agg:
                total = int(ad_agg["total_ads"] or 0)
                auto = int(ad_agg["auto_ads"] or 0)
                ads["total"] = total
                ads["auto"] = auto
                ads["manual"] = total - auto
                ads["sessions_with_ads"] = int(ad_agg["sessions_with_ads"] or 0)
                ads["avg_duration_s"] = float(ad_agg["avg_duration"] or 0.0)

            # 1b. Viewer-impact analysis
            ad_rows = c.execute(
                """
                SELECT a.id, a.session_id, a.started_at, a.duration_seconds, a.is_automatic,
                       s.started_at AS session_start
                  FROM twitch_ad_break_events a
                  JOIN twitch_stream_sessions s ON s.id = a.session_id
                 WHERE a.started_at >= %s
                   AND a.session_id IS NOT NULL
                 ORDER BY a.started_at DESC
                 LIMIT 200
                """,
                (cutoff_30d,),
            ).fetchall()

            timeline_map: dict = {}
            if ad_rows:
                session_ids = list({int(r["session_id"]) for r in ad_rows if r["session_id"]})
                if session_ids:
                    session_ids_json = json.dumps(session_ids)
                    viewer_rows = c.execute(
                        """
                        SELECT session_id, minutes_from_start, viewer_count
                          FROM twitch_session_viewers
                         WHERE session_id IN (
                            SELECT CAST(value AS INTEGER) FROM json_each(%s)
                         )
                         ORDER BY session_id, minutes_from_start
                        """,
                        (session_ids_json,),
                    ).fetchall()
                    for vr in viewer_rows:
                        sid = int(vr["session_id"])
                        timeline_map.setdefault(sid, []).append(
                            (
                                float(vr["minutes_from_start"] or 0),
                                int(vr["viewer_count"] or 0),
                            )
                        )

            drop_pcts: list[float] = []
            worst_ads: list[dict] = []
            for ad in ad_rows:
                session_id = int(ad["session_id"] or 0)
                ad_started = ad["started_at"]
                session_start = ad["session_start"]
                duration_s = float(ad["duration_seconds"] or 30)
                try:
                    ad_dt = datetime.fromisoformat(str(ad_started).replace("Z", "+00:00"))
                    sess_dt = datetime.fromisoformat(str(session_start).replace("Z", "+00:00"))
                    minutes_into = (ad_dt - sess_dt).total_seconds() / 60.0
                except Exception:
                    continue
                timeline = timeline_map.get(session_id, [])
                if not timeline:
                    continue
                duration_min = duration_s / 60.0
                pre_vals = [v for m, v in timeline if (minutes_into - 5) <= m < minutes_into]
                post_start = minutes_into + duration_min
                post_vals = [v for m, v in timeline if post_start <= m < (post_start + 5)]
                if not pre_vals or not post_vals:
                    continue
                pre_avg = sum(pre_vals) / len(pre_vals)
                if pre_avg <= 0:
                    continue
                post_avg = sum(post_vals) / len(post_vals)
                drop_pct = (post_avg - pre_avg) / pre_avg * 100.0
                drop_pcts.append(drop_pct)
                worst_ads.append(
                    {
                        "started_at": str(ad_started or "")[:16],
                        "duration_s": int(duration_s),
                        "drop_pct": round(drop_pct, 1),
                        "is_automatic": bool(ad["is_automatic"]),
                    }
                )

            if drop_pcts:
                ads["avg_viewer_drop_pct"] = round(sum(drop_pcts) / len(drop_pcts), 1)
            worst_ads.sort(key=lambda x: x["drop_pct"])
            ads["worst_ads"] = worst_ads[:5]

            # 1c. Hype Train overview
            try:
                ht_row = c.execute(
                    """
                    SELECT COUNT(*) AS total_trains,
                           AVG(level) AS avg_level,
                           MAX(level) AS max_level,
                           AVG(duration_seconds) AS avg_duration
                      FROM twitch_hype_train_events
                     WHERE started_at >= %s
                       AND ended_at IS NOT NULL
                    """,
                    (cutoff_30d,),
                ).fetchone()
                if ht_row:
                    hype_train["total"] = int(ht_row["total_trains"] or 0)
                    hype_train["avg_level"] = round(float(ht_row["avg_level"] or 0.0), 1)
                    hype_train["max_level"] = int(ht_row["max_level"] or 0)
                    hype_train["avg_duration_s"] = round(float(ht_row["avg_duration"] or 0.0), 0)
            except Exception:
                log.debug("Hype Train query fehlgeschlagen", exc_info=True)

            # 1d. Bits
            try:
                bits_row = c.execute(
                    "SELECT SUM(amount) AS total_bits, COUNT(*) AS cheer_events FROM twitch_bits_events WHERE received_at >= %s",
                    (cutoff_30d,),
                ).fetchone()
                if bits_row:
                    bits["total"] = int(bits_row["total_bits"] or 0)
                    bits["cheer_events"] = int(bits_row["cheer_events"] or 0)
            except Exception:
                log.debug("Bits query fehlgeschlagen", exc_info=True)

            # 1d. Subs
            try:
                subs_row = c.execute(
                    """
                    SELECT COUNT(*) AS total_events,
                           SUM(CASE WHEN is_gift=1 THEN 1 ELSE 0 END) AS gifted
                      FROM twitch_subscription_events
                     WHERE received_at >= %s
                    """,
                    (cutoff_30d,),
                ).fetchone()
                if subs_row:
                    subs["total_events"] = int(subs_row["total_events"] or 0)
                    subs["gifted"] = int(subs_row["gifted"] or 0)
            except Exception:
                log.debug("Subs query fehlgeschlagen", exc_info=True)

        return {
            "ads": ads,
            "hype_train": hype_train,
            "bits": bits,
            "subs": subs,
            "window_days": 30,
        }

    async def _dashboard_stats(
        self,
        *,
        hour_from: int | None = None,
        hour_to: int | None = None,
        streamer: str | None = None,
    ) -> dict:
        stats = await self._compute_stats(
            hour_from=hour_from,
            hour_to=hour_to,
            streamer=streamer,
        )
        tracked_top = stats.get("tracked", {}).get("top", []) or []
        category_top = stats.get("category", {}).get("top", []) or []

        def _agg(items: list[dict]):
            samples = sum(int(d.get("samples") or 0) for d in items)
            uniq = len(items)
            avg_over_streamers = (
                (sum(float(d.get("avg_viewers") or 0.0) for d in items) / float(uniq))
                if uniq
                else 0.0
            )
            return samples, uniq, avg_over_streamers

        cat_samples, cat_uniq, cat_avg = _agg(category_top)
        tr_samples, tr_uniq, tr_avg = _agg(tracked_top)

        stats.setdefault("tracked", {})["samples"] = tr_samples
        stats["tracked"]["unique_streamers"] = tr_uniq
        stats.setdefault("category", {})["samples"] = cat_samples
        stats["category"]["unique_streamers"] = cat_uniq
        stats["avg_viewers_all"] = cat_avg
        stats["avg_viewers_tracked"] = tr_avg

        try:
            eventsub_fetcher = getattr(self, "_get_eventsub_capacity_overview", None)
            if callable(eventsub_fetcher):
                stats["eventsub"] = await eventsub_fetcher(hours=24)
        except Exception:
            log.debug("Konnte EventSub-Capacity-Overview nicht laden", exc_info=True)

        try:
            stats["monetization"] = await self._get_monetization_stats()
        except Exception:
            log.debug("Konnte Monetization-Stats nicht laden", exc_info=True)

        return stats

    async def _dashboard_streamer_analytics_data(self, streamer_login: str, days: int = 30) -> dict:
        """
        New comprehensive analytics using AnalyticsBackendExtended.
        Returns data structure compatible with the new React dashboard.
        """
        return await AnalyticsBackendExtended.get_comprehensive_analytics(
            streamer_login=streamer_login, days=days
        )

    async def _dashboard_streamer_overview(self, login: str) -> dict:
        """Fetch comprehensive stats for a single streamer."""
        login = self._normalize_login(login)
        if not login:
            return {}
        return await asyncio.to_thread(self._dashboard_streamer_overview_sync, login)

    def _dashboard_streamer_overview_sync(self, login: str) -> dict:
        data = {"login": login}
        with storage.readonly_connection() as c:
            # 1. Stammdaten
            row = c.execute(
                "SELECT * FROM twitch_partners_all_state WHERE LOWER(twitch_login)=LOWER(%s) AND status='active'",
                (login,),
            ).fetchone()
            if not row:
                return {}
            data["meta"] = _row_to_dict(row)

            # 2. Aggregated Session Stats (Last 30 days)
            since_30d = (datetime.now(UTC) - timedelta(days=30)).isoformat()
            agg = c.execute(
                """
                SELECT COUNT(*) as total_streams,
                       SUM(duration_seconds) as total_duration,
                       AVG(avg_viewers) as avg_avg_viewers,
                       MAX(peak_viewers) as max_peak,
                       SUM(follower_delta) as total_follower_delta,
                       SUM(unique_chatters) as total_unique_chatters
                  FROM twitch_stream_sessions
                 WHERE streamer_login=%s
                   AND started_at > %s
                """,
                (login, since_30d),
            ).fetchone()
            data["stats_30d"] = _row_to_dict(agg) if agg else {}

            # 3. Recent Sessions
            sessions = c.execute(
                """
                SELECT id, stream_id, started_at, duration_seconds, 
                       avg_viewers, peak_viewers, follower_delta, stream_title
                  FROM twitch_stream_sessions
                 WHERE streamer_login=%s
                 ORDER BY started_at DESC
                 LIMIT 20
                """,
                (login,),
            ).fetchall()
            data["recent_sessions"] = [_row_to_dict(s) for s in sessions]

        return data

    async def _dashboard_session_detail(self, session_id: int) -> dict:
        """Fetch deep-dive data for a single session."""
        return await asyncio.to_thread(self._dashboard_session_detail_sync, session_id)

    def _dashboard_session_detail_sync(self, session_id: int) -> dict:
        data = {}
        with storage.readonly_connection() as c:
            # 1. Session Meta
            row = c.execute(
                "SELECT * FROM twitch_stream_sessions WHERE id=%s", (session_id,)
            ).fetchone()
            if not row:
                return {}
            data["session"] = _row_to_dict(row)

            # 2. Viewer Timeline (Chart data)
            timeline = c.execute(
                """
                SELECT minutes_from_start, viewer_count 
                  FROM twitch_session_viewers 
                 WHERE session_id=%s 
                 ORDER BY minutes_from_start ASC
                """,
                (session_id,),
            ).fetchall()
            data["timeline"] = [_row_to_dict(t) for t in timeline]

            # 3. Chat Stats (if needed separately, though rolled up in session)
            # potentially fetch top chatters here
            top_chatters = c.execute(
                """
                SELECT chatter_login, messages 
                  FROM twitch_session_chatters
                 WHERE session_id=%s
                 ORDER BY messages DESC
                 LIMIT 10
                """,
                (session_id,),
            ).fetchall()
            data["top_chatters"] = [_row_to_dict(tc) for tc in top_chatters]

        return data

    async def _dashboard_comparison_stats(self, days: int = 30) -> dict:
        """Fetch comparative stats: Me vs Category vs Top."""
        return await asyncio.to_thread(self._dashboard_comparison_stats_sync, days)

    def _dashboard_comparison_stats_sync(self, days: int = 30) -> dict:
        data = {}
        since_dt = (datetime.now(UTC) - timedelta(days=days)).isoformat()
        with storage.readonly_connection() as c:
            # Global Category Stats (Deadlock)
            cat_stats = c.execute(
                """
                SELECT AVG(viewer_count) as avg_viewers, MAX(viewer_count) as peak_viewers
                  FROM twitch_stats_category
                 WHERE ts_utc > %s
                """,
                (since_dt,),
            ).fetchone()
            data["category"] = _row_to_dict(cat_stats) if cat_stats else {}

            # Tracked Partner Stats
            track_stats = c.execute(
                """
                SELECT AVG(viewer_count) as avg_viewers, MAX(viewer_count) as peak_viewers
                  FROM twitch_stats_tracked
                 WHERE ts_utc > %s
                """,
                (since_dt,),
            ).fetchone()
            data["tracked_avg"] = _row_to_dict(track_stats) if track_stats else {}

            # Top 5 Streamers by Avg Viewers (Local Data)
            top_streamers = c.execute(
                """
                SELECT streamer_login, AVG(avg_viewers) as val
                  FROM twitch_stream_sessions
                 WHERE started_at > %s
                 GROUP BY streamer_login
                 ORDER BY val DESC
                 LIMIT 5
                """,
                (since_dt,),
            ).fetchall()
            data["top_streamers"] = [_row_to_dict(r) for r in top_streamers]

        return data

    def _dashboard_verify_storage_step(self, login: str, mode: str) -> dict[str, object]:
        if mode in {"permanent", "temp"}:
            row_data = None
            should_notify = False
            copied = 0
            with storage.transaction() as c:
                source_row = c.execute(
                    """
                    SELECT twitch_user_id, discord_user_id, discord_display_name, manual_verified_at
                    FROM twitch_streamers
                    WHERE twitch_login=%s
                    """,
                    (login,),
                ).fetchone()
                partner_row = storage.load_active_partner(c, twitch_login=login)
                twitch_user_id = ""
                if source_row:
                    row_data = _row_to_dict(source_row)
                    twitch_user_id = str(row_data.get("twitch_user_id") or "").strip()
                    should_notify = row_data.get("manual_verified_at") is None
                elif partner_row:
                    row_data = {
                        "twitch_user_id": partner_row["twitch_user_id"] if hasattr(partner_row, "keys") else partner_row[1],
                        "discord_user_id": partner_row["discord_user_id"] if hasattr(partner_row, "keys") else partner_row[21],
                        "discord_display_name": partner_row["discord_display_name"] if hasattr(partner_row, "keys") else partner_row[22],
                        "manual_verified_at": partner_row["manual_verified_at"] if hasattr(partner_row, "keys") else partner_row[11],
                    }
                    twitch_user_id = str(row_data.get("twitch_user_id") or "").strip()
                    should_notify = row_data.get("manual_verified_at") is None

                if not twitch_user_id:
                    return {"kind": "message", "message": f"{login} ist nicht gespeichert"}

                verification = storage.verification_payload(mode)
                storage.promote_streamer_to_partner(
                    c,
                    twitch_login=login,
                    twitch_user_id=twitch_user_id,
                    discord_user_id=row_data.get("discord_user_id") if row_data else None,
                    discord_display_name=row_data.get("discord_display_name") if row_data else None,
                    is_on_discord=1 if row_data and row_data.get("discord_user_id") else 0,
                    **verification,
                )
                copied = storage.backfill_tracked_stats_from_category(c, login)

            base_msg = (
                f"{login} dauerhaft verifiziert"
                if mode == "permanent"
                else f"{login} für 30 Tage verifiziert"
            )
            return {
                "kind": "verified",
                "base_msg": base_msg,
                "copied": copied,
                "should_notify": should_notify,
                "row_data": row_data,
            }

        if mode == "clear":
            with storage.transaction() as c:
                result = storage.departner_active_partner(
                    c,
                    twitch_login=login,
                    clear_verification=True,
                )
                if not result:
                    return {"kind": "message", "message": f"{login} ist nicht gespeichert"}
            return {
                "kind": "cleared",
                "message": f"Verifizierung für {login} zurückgesetzt (keine DM versendet)",
                "row_data": result,
            }

        if mode == "failed":
            row_data = None
            with storage.transaction() as c:
                identity_row = storage.load_streamer_identity(c, twitch_login=login)
                if identity_row:
                    row_data = _row_to_dict(identity_row)
                archived = storage.departner_active_partner(
                    c,
                    twitch_login=login,
                    clear_verification=True,
                )
                if archived and row_data is None:
                    row_data = archived
            if not row_data:
                return {"kind": "message", "message": f"{login} ist nicht gespeichert"}
            return {"kind": "failed", "row_data": row_data}

        return {"kind": "message", "message": "Unbekannter Modus"}

    async def _ensure_streamer_role(self, row_data: dict | None) -> str:
        """Assign the streamer role when available; return a short status hint."""
        if not row_data:
            return ""

        user_id_raw = row_data.get("discord_user_id")
        if not user_id_raw:
            log.info(
                "Streamer verification: no Discord ID stored for %s",
                row_data.get("discord_display_name"),
            )
            return ""

        normalized_id = normalize_discord_user_id(str(user_id_raw))
        if not normalized_id:
            log.warning("Streamer verification: invalid Discord ID %r", user_id_raw)
            return "(Streamer-Rolle konnte nicht vergeben werden – ungültige Discord-ID)"

        changed = await sync_streamer_role(
            self.bot,
            normalized_id,
            should_have_role=True,
            reason="Streamer-Verifizierung über Dashboard bestätigt",
            logger=log,
        )
        return "(Streamer-Rolle vergeben)" if changed else ""

    async def _remove_streamer_role(self, row_data: dict | None, *, reason: str) -> str:
        """Remove the streamer role when partner access is revoked."""
        if not row_data:
            return ""

        user_id_raw = row_data.get("discord_user_id")
        if not user_id_raw:
            log.info(
                "Streamer role removal skipped for %s because no Discord ID is stored",
                row_data.get("discord_display_name"),
            )
            return ""

        normalized_id = normalize_discord_user_id(str(user_id_raw))
        if not normalized_id:
            log.warning("Streamer role removal skipped due to invalid Discord ID %r", user_id_raw)
            return "(Streamer-Rolle konnte nicht entfernt werden – ungültige Discord-ID)"

        changed = await sync_streamer_role(
            self.bot,
            normalized_id,
            should_have_role=False,
            reason=reason,
            logger=log,
        )
        return "(Streamer-Rolle entfernt)" if changed else ""

    async def _notify_verification_success(self, login: str, row_data: dict | None) -> str:
        if not row_data:
            log.info(
                "Keine Discord-Daten für %s zum Versenden der Erfolgsnachricht gefunden",
                login,
            )
            return ""

        user_id_raw = row_data.get("discord_user_id")
        if not user_id_raw:
            log.info(
                "Keine Discord-ID für %s hinterlegt – überspringe Erfolgsnachricht",
                login,
            )
            return ""

        try:
            user_id_int = int(str(user_id_raw))
        except (TypeError, ValueError):
            log.warning(
                "Ungültige Discord-ID %r für %s – keine Erfolgsnachricht",
                user_id_raw,
                login,
            )
            return "(Discord-DM konnte nicht zugestellt werden)"

        user = self.bot.get_user(user_id_int)
        if user is None:
            try:
                user = await self.bot.fetch_user(user_id_int)
            except discord.NotFound:
                user = None
            except discord.HTTPException:
                log.exception("Konnte Discord-User %s nicht abrufen", user_id_int)
                user = None

        if user is None:
            log.warning("Discord-User %s (%s) konnte nicht gefunden werden", user_id_int, login)
            return "(Discord-DM konnte nicht zugestellt werden)"

        try:
            await user.send(VERIFICATION_SUCCESS_DM_MESSAGE)
        except discord.Forbidden:
            log.warning(
                "DM an %s (%s) wegen erfolgreicher Verifizierung blockiert",
                user_id_int,
                login,
            )
            return "(Discord-DM konnte nicht zugestellt werden)"
        except discord.HTTPException:
            log.exception(
                "Konnte Erfolgsnachricht nach Verifizierung nicht an %s senden",
                user_id_int,
            )
            return "(Discord-DM konnte nicht zugestellt werden)"

        log.info("Verifizierungs-Erfolgsnachricht an %s (%s) gesendet", user_id_int, login)
        return ""

    async def _dashboard_verify(self, login: str, mode: str) -> str:
        login = self._normalize_login(login)
        if not login:
            return "Ungültiger Login"

        storage_result = await asyncio.to_thread(self._dashboard_verify_storage_step, login, mode)
        result_kind = str(storage_result.get("kind") or "")
        if result_kind == "message":
            return str(storage_result.get("message") or "Unbekannter Modus")
        if result_kind == "verified":
            row_data = storage_result.get("row_data")
            should_notify = bool(storage_result.get("should_notify"))
            copied = int(storage_result.get("copied") or 0)
            base_msg = str(storage_result.get("base_msg") or "").strip()

            notes: list[str] = []
            if copied:
                notes.append(f"({copied} historische Datenpunkte übernommen)")
            if should_notify:
                dm_note = await self._notify_verification_success(login, row_data)
                if dm_note:
                    notes.append(dm_note)
            role_note = await self._ensure_streamer_role(row_data)
            if role_note:
                notes.append(role_note)
            merged = " ".join(notes).strip()
            return f"{base_msg} {merged}".strip()
        if result_kind == "cleared":
            message = str(
                storage_result.get("message")
                or f"Verifizierung für {login} zurückgesetzt"
            )
            role_note = await self._remove_streamer_role(
                storage_result.get("row_data"),
                reason="Streamer-Verifizierung über Dashboard entfernt",
            )
            return f"{message} {role_note}".strip()
        if result_kind != "failed":
            return "Unbekannter Modus"

        row_data = storage_result.get("row_data")
        if not isinstance(row_data, dict):
            return f"{login} ist nicht gespeichert"
        role_note = await self._remove_streamer_role(
            row_data,
            reason="Streamer-Verifizierung über Dashboard fehlgeschlagen",
        )
        user_id_raw = row_data.get("discord_user_id")
        if not user_id_raw:
            return f"Keine Discord-ID für {login} hinterlegt {role_note}".strip()

        try:
            user_id_int = int(str(user_id_raw))
        except (TypeError, ValueError):
            return f"Ungültige Discord-ID für {login} {role_note}".strip()

        user = self.bot.get_user(user_id_int)
        if user is None:
            try:
                user = await self.bot.fetch_user(user_id_int)
            except discord.NotFound:
                user = None
            except discord.HTTPException:
                log.exception("Konnte Discord-User %s nicht abrufen", user_id_int)
                user = None

        if user is None:
            return f"Discord-User {user_id_int} konnte nicht gefunden werden {role_note}".strip()

        message = (
            "Hey! Deine Deadlock-Streamer-Verifizierung konnte leider nicht abgeschlossen werden. "
            "Du erfüllst aktuell nicht alle Voraussetzungen. Bitte prüfe die Anforderungen erneut "
            "und starte die Verifizierung anschließend mit /streamer noch einmal."
        )

        try:
            await user.send(message)
        except discord.Forbidden:
            log.warning(
                "DM an %s (%s) wegen fehlgeschlagener Verifizierung blockiert",
                user_id_int,
                login,
            )
            return (
                f"Konnte {row_data.get('discord_display_name') or user.name} nicht per DM erreichen. "
                f"{role_note}"
            ).strip()
        except discord.HTTPException:
            log.exception(
                "Konnte Verifizierungsfehler-Nachricht nicht senden an %s",
                user_id_int,
            )
            return f"Nachricht konnte nicht gesendet werden {role_note}".strip()

        log.info(
            "Verifizierungsfehler-Benachrichtigung an %s (%s) gesendet",
            user_id_int,
            login,
        )
        return (
            f"{login}: Discord-User wurde über die fehlgeschlagene Verifizierung informiert "
            f"{role_note}"
        ).strip()

    async def _reload_twitch_cog(self) -> str:
        """Hot reload the entire Twitch cog.

        Verwendet explizites unload → load statt reload_extension(),
        damit bei einem fehlgeschlagenen vorherigen Reload der Cog
        nicht in einem inkonsistenten "already loaded" Zustand bleibt.

        Wartet nach dem Unload explizit auf Port-Freigabe, damit
        der neue Cog sauber starten kann.
        """
        try:
            # 1) Sicher unloaden (ignoriere Fehler wenn nicht geladen)
            try:
                await self.bot.unload_extension("cogs.twitch")
                log.info("Twitch cog unloaded for reload")

                # Warte explizit darauf, dass alle Ressourcen freigegeben wurden
                # (besonders wichtig: Ports 4343 und 8765)
                log.info("Warte 3 Sekunden auf vollständige Ressourcen-Freigabe...")
                await asyncio.sleep(3.0)

            except Exception as unload_err:
                log.warning("Twitch cog unload before reload: %s", unload_err)
                # Auch bei Fehler kurz warten, damit teilweise Cleanups Zeit haben
                await asyncio.sleep(2.0)

            # 2) Neu laden
            await self.bot.load_extension("cogs.twitch")
            log.info("Twitch cog hot reloaded via dashboard")
            return "Twitch-Modul erfolgreich neu geladen"
        except Exception as e:
            log.exception("Twitch cog hot reload failed")
            return f"Fehler beim Neuladen: {e}"

    async def _start_dashboard(self):
        if not getattr(self, "_dashboard_embedded", True):
            log.debug("Twitch dashboard embedded server disabled; skipping _start_dashboard")
            return

        # Retry logic for port availability during reloads
        max_retries = 5
        retry_delay = 0.5
        app = None
        runner = None
        dashboard_services = DashboardRuntimeServices(
            add_cb=self._dashboard_add,
            remove_cb=self._dashboard_remove,
            list_cb=self._dashboard_list,
            stats_cb=self._dashboard_stats,
            verify_cb=self._dashboard_verify,
            archive_cb=self._dashboard_archive,
            discord_flag_cb=self._dashboard_set_discord_flag,
            discord_profile_cb=self._dashboard_save_discord_profile,
            raid_history_cb=getattr(self, "_dashboard_raid_history", None),
            raid_auth_url_cb=self._dashboard_raid_auth_url,
            raid_go_url_cb=self._dashboard_raid_go_url,
            raid_requirements_cb=self._dashboard_raid_requirements,
            raid_oauth_callback_cb=self._dashboard_raid_oauth_callback,
            reload_cb=self._reload_twitch_cog,
            eventsub_webhook_handler=getattr(self, "_eventsub_webhook_handler", None),
            social_media_clip_manager=getattr(self, "clip_manager", None),
            social_media_twitch_api=getattr(self, "api", None),
            bot_service=DashboardBotService.from_cog(
                self,
                reload_cb=self._reload_twitch_cog,
            ),
        )

        for attempt in range(max_retries):
            try:
                app = build_v2_app(
                    noauth=self._dashboard_noauth,
                    token=self._dashboard_token,
                    partner_token=self._partner_dashboard_token,
                    oauth_client_id=self.client_id or None,
                    oauth_client_secret=self.client_secret or None,
                    oauth_redirect_uri=getattr(self, "_dashboard_auth_redirect_uri", None),
                    session_ttl_seconds=getattr(self, "_dashboard_session_ttl", 6 * 3600),
                    legacy_stats_url=getattr(self, "_legacy_stats_url", None),
                    dashboard_services=dashboard_services,
                    add_cb=self._dashboard_add,
                    remove_cb=self._dashboard_remove,
                    list_cb=self._dashboard_list,
                    stats_cb=self._dashboard_stats,
                    verify_cb=self._dashboard_verify,
                    archive_cb=self._dashboard_archive,
                    discord_flag_cb=self._dashboard_set_discord_flag,
                    discord_profile_cb=self._dashboard_save_discord_profile,
                    raid_history_cb=getattr(self, "_dashboard_raid_history", None),
                    raid_auth_url_cb=self._dashboard_raid_auth_url,
                    raid_go_url_cb=self._dashboard_raid_go_url,
                    raid_requirements_cb=self._dashboard_raid_requirements,
                    raid_oauth_callback_cb=self._dashboard_raid_oauth_callback,
                    reload_cb=self._reload_twitch_cog,
                    social_media_clip_manager=getattr(self, "clip_manager", None),
                    social_media_twitch_api=getattr(self, "api", None),
                    eventsub_webhook_handler=getattr(self, "_eventsub_webhook_handler", None),
                )
                runner = web.AppRunner(app)
                await runner.setup()
                site = web.TCPSite(runner, host=self._dashboard_host, port=self._dashboard_port)
                await site.start()
                self._web = runner
                self._web_app = app
                log.debug(
                    "Twitch dashboard running on http://%s:%s/twitch",
                    self._dashboard_host,
                    self._dashboard_port,
                )
                return
            except OSError as e:
                if runner:
                    await runner.cleanup()

                # Check for address in use (WinError 10048 on Windows, EADDRINUSE=98 on Linux)
                import errno as _errno

                is_addr_in_use = e.errno in (10048, getattr(_errno, "EADDRINUSE", 98))

                if is_addr_in_use and attempt < max_retries - 1:
                    log.debug(
                        "Twitch dashboard port %s belegt, versuche es erneut in %ss... (Versuch %s/%s)",
                        self._dashboard_port,
                        retry_delay,
                        attempt + 1,
                        max_retries,
                    )
                    await asyncio.sleep(retry_delay)
                    retry_delay *= 2
                    continue
                log.exception("Konnte Dashboard nicht starten (Port belegt oder anderer Fehler)")
                break
            except Exception:
                if runner:
                    await runner.cleanup()
                log.exception("Konnte Dashboard nicht starten")
                break

    async def _stop_dashboard(self):
        if self._web:
            await self._web.cleanup()
            self._web = None
            self._web_app = None

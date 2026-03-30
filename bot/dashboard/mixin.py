"""Dashboard helpers for the Twitch cog."""

from __future__ import annotations

import asyncio
import json
import os
from datetime import UTC, datetime, timedelta
from urllib.parse import parse_qsl, urlencode, urlparse, urlunparse

import discord
from aiohttp import web

from ..core.constants import (
    TWITCH_BUTTON_LABEL,
    TWITCH_DISCORD_REF_CODE,
    TWITCH_TARGET_GAME_NAME,
    log,
)
from ..runtime.contracts import DashboardBotService
from ..runtime.dashboard_runtime import DashboardRuntimeServices
from ..runtime_security import require_noauth_loopback_guard
from ..raid.integration_state import RaidIntegrationStateResolver
from ..storage import pg as storage
from ..raid.views import RaidAuthGenerateView, build_raid_requirements_embed
from .raids.oauth_callback import build_raid_oauth_callback_payload
from .server_v2 import build_v2_app
from .dashboard_metrics_mixin import (
    _dashboard_comparison_stats,
    _dashboard_comparison_stats_sync,
    _dashboard_session_detail,
    _dashboard_session_detail_sync,
    _dashboard_stats,
    _dashboard_streamer_analytics_data,
    _dashboard_streamer_overview,
    _dashboard_streamer_overview_sync,
    _get_monetization_stats,
)
from .streamer_admin_mixin import (
    _dashboard_archive,
    _dashboard_archive_sync,
    _dashboard_load_twitch_user_id_from_raid_auth_sync,
    _dashboard_save_discord_profile,
    _dashboard_save_discord_profile_sync,
    _dashboard_set_discord_flag,
    _dashboard_set_discord_flag_sync,
    _dashboard_verify,
    _dashboard_verify_storage_step,
    _ensure_streamer_role,
    _notify_verification_success,
    _remove_streamer_role,
)
RAID_OAUTH_SUCCESS_REDIRECT_URL = "https://twitch.earlysalty.com/twitch/dashboard"
PUBLIC_WEBSITE_ONBOARDING_LOGIN = "public:website_onboarding"


def _parse_env_bool(name: str, default: bool) -> bool:
    raw = (os.getenv(name) or "").strip().lower()
    if not raw:
        return default
    if raw in {"1", "true", "yes", "on"}:
        return True
    if raw in {"0", "false", "no", "off"}:
        return False
    return default


def _require_embedded_dashboard_noauth_opt_in(*, enabled: bool) -> None:
    if not enabled:
        return
    if _parse_env_bool("TWITCH_ALLOW_DASHBOARD_NOAUTH", False):
        return
    raise RuntimeError(
        "Refusing to start embedded dashboard with no-auth enabled. "
        "Set TWITCH_ALLOW_DASHBOARD_NOAUTH=1 only for controlled local debugging."
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
        services = getattr(self, "_dashboard_services", None)
        bot_service = getattr(services, "bot_service", None)
        if isinstance(bot_service, DashboardBotService):
            return bot_service

        raid_bot = getattr(self, "_raid_bot", None)
        if raid_bot is None:
            return DashboardBotService()

        return DashboardBotService(
            _auth_manager=getattr(raid_bot, "auth_manager", None),
            _discord_bot=getattr(self, "bot", None),
            _chat_bot=(
                getattr(self, "_twitch_chat_bot", None)
                or getattr(raid_bot, "chat_bot", None)
            ),
            _token_manager=getattr(self, "_bot_token_manager", None),
            _clip_manager=getattr(self, "clip_manager", None),
            _twitch_api=getattr(self, "api", None),
            _eventsub_webhook_handler=getattr(self, "_eventsub_webhook_handler", None),
            _raid_complete_setup_cb=getattr(raid_bot, "complete_setup_for_streamer", None),
            _raid_sync_partner_state_cb=getattr(raid_bot, "_sync_partner_state_after_auth", None),
            _reload_cb=getattr(self, "_reload_twitch_cog", None),
            _schedule_background=getattr(self, "_spawn_bg_task", None),
        )

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
        raid_bot = getattr(self, "_raid_bot", None)
        redirect_url = (
            (os.getenv("TWITCH_RAID_SUCCESS_REDIRECT_URL") or "").strip()
            or RAID_OAUTH_SUCCESS_REDIRECT_URL
        )
        return await build_raid_oauth_callback_payload(
            code=code,
            state=state,
            error=error,
            auth_manager=self._dashboard_bot_service().auth_manager(),
            session=getattr(raid_bot, "session", None),
            success_redirect_url=redirect_url,
            failure_title="Autorisierung fehlgeschlagen",
            failure_body_html=(
                "<p>Autorisierung fehlgeschlagen.</p>"
                "<p>Bitte erneut versuchen oder Admin kontaktieren.</p>"
            ),
            complete_setup_cb=self._dashboard_bot_service().raid_complete_setup_cb(),
            sync_partner_state_cb=self._dashboard_bot_service().raid_sync_partner_state_cb(),
            schedule_background=self._dashboard_bot_service().schedule_background(),
        )

    def _raid_integration_state_resolver(self) -> RaidIntegrationStateResolver:
        auth_manager = self._dashboard_bot_service().auth_manager()
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
    _dashboard_set_discord_flag_sync = staticmethod(_dashboard_set_discord_flag_sync)
    _dashboard_set_discord_flag = _dashboard_set_discord_flag
    _dashboard_archive_sync = staticmethod(_dashboard_archive_sync)
    _dashboard_archive = _dashboard_archive
    _dashboard_load_twitch_user_id_from_raid_auth_sync = staticmethod(
        _dashboard_load_twitch_user_id_from_raid_auth_sync
    )
    _dashboard_save_discord_profile_sync = staticmethod(_dashboard_save_discord_profile_sync)
    _dashboard_save_discord_profile = _dashboard_save_discord_profile
    _get_monetization_stats = _get_monetization_stats
    _dashboard_stats = _dashboard_stats
    _dashboard_streamer_analytics_data = _dashboard_streamer_analytics_data
    _dashboard_streamer_overview_sync = _dashboard_streamer_overview_sync
    _dashboard_streamer_overview = _dashboard_streamer_overview
    _dashboard_session_detail_sync = _dashboard_session_detail_sync
    _dashboard_session_detail = _dashboard_session_detail
    _dashboard_comparison_stats_sync = _dashboard_comparison_stats_sync
    _dashboard_comparison_stats = _dashboard_comparison_stats

    _dashboard_verify_storage_step = _dashboard_verify_storage_step
    _ensure_streamer_role = _ensure_streamer_role
    _remove_streamer_role = _remove_streamer_role
    _notify_verification_success = _notify_verification_success
    _dashboard_verify = _dashboard_verify

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
        _require_embedded_dashboard_noauth_opt_in(
            enabled=bool(getattr(self, "_dashboard_noauth", False))
        )
        require_noauth_loopback_guard(
            enabled=bool(getattr(self, "_dashboard_noauth", False)),
            host=str(getattr(self, "_dashboard_host", "127.0.0.1") or "127.0.0.1"),
        )

        # Retry logic for port availability during reloads
        max_retries = 5
        retry_delay = 0.5
        app = None
        runner = None
        bot_service = self._dashboard_bot_service()
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
            bot_service=bot_service,
        )
        self._dashboard_services = dashboard_services

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

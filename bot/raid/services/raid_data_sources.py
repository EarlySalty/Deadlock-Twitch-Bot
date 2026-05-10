from __future__ import annotations

import logging
from collections.abc import Awaitable, Callable
from contextlib import AbstractContextManager
from dataclasses import dataclass, field
from datetime import UTC, datetime
from typing import Any

from bot.storage import readonly_connection

from ...core.constants import TWITCH_TARGET_GAME_NAME


log = logging.getLogger("TwitchStreams.RaidManager")
_API_UNSET = object()

ReadonlyConnectionFactory = Callable[[], AbstractContextManager[Any]]
SessionGetter = Callable[[], Any | None]
TargetGameLowerGetter = Callable[[], str | None]
LanguageFilterGetter = Callable[[], list[str | None]]
SharedStreamFetch = Callable[[list[str]], Awaitable[dict[str, dict[str, Any]]]]
CategoryIdGetter = Callable[[], str | None]
TwitchApiFactory = Callable[[], Any | None]


def _row_value(row: Any, key: str, index: int, default: Any = None) -> Any:
    if row is None:
        return default
    if hasattr(row, "get"):
        return row.get(key, default)
    try:
        return row[index]
    except Exception:
        return default


def _safe_int(value: object, default: int = 0) -> int:
    try:
        if value is None:
            return int(default)
        return int(value)
    except (TypeError, ValueError):
        return int(default)


def _parse_datetime(value: object) -> datetime | None:
    raw = str(value or "").strip()
    if not raw:
        return None
    try:
        parsed = datetime.fromisoformat(raw.replace("Z", "+00:00"))
    except ValueError:
        return None
    if parsed.tzinfo is None:
        return parsed.replace(tzinfo=UTC)
    return parsed.astimezone(UTC)


@dataclass(slots=True)
class RaidDataSourceService:
    client_id: str | None = None
    client_secret: str | None = None
    session_getter: SessionGetter | None = None
    target_game_lower_getter: TargetGameLowerGetter | None = None
    language_filter_getter: LanguageFilterGetter | None = None
    shared_stream_fetch: SharedStreamFetch | None = None
    cached_category_id_getter: CategoryIdGetter | None = None
    readonly_connection_factory: ReadonlyConnectionFactory | None = None
    twitch_api_factory: TwitchApiFactory | None = None
    logger: logging.Logger = field(default_factory=lambda: log)
    target_game_name: str = TWITCH_TARGET_GAME_NAME
    utcnow: Callable[[], datetime] = lambda: datetime.now(UTC)

    def get_target_game_lower(self) -> str:
        resolved = ""
        if callable(self.target_game_lower_getter):
            try:
                resolved = str(self.target_game_lower_getter() or "").strip().lower()
            except Exception:
                resolved = ""
        if not resolved:
            resolved = str(self.target_game_name or "").strip().lower()
        return resolved

    def is_recent_deadlock(
        self,
        last_deadlock_seen_at: str | None,
        *,
        now_utc: datetime | None = None,
        recency_cap_seconds: int = 360,
    ) -> bool:
        last_deadlock_dt = _parse_datetime(last_deadlock_seen_at)
        if last_deadlock_dt is None:
            return False
        reference = now_utc if now_utc is not None else self.utcnow()
        return (reference - last_deadlock_dt).total_seconds() <= recency_cap_seconds

    def evaluate_deadlock_raid_source(
        self,
        *,
        current_game: str,
        had_deadlock_session: bool,
        last_deadlock_seen_at: str | None,
    ) -> dict[str, object]:
        target_game_lower = self.get_target_game_lower()
        current_game_text = str(current_game or "").strip()
        current_game_lower = current_game_text.lower()

        recent_deadlock = False
        if target_game_lower and current_game_lower == target_game_lower:
            recent_deadlock = True
        elif current_game_lower == "just chatting" and had_deadlock_session:
            recent_deadlock = self.is_recent_deadlock(last_deadlock_seen_at)

        if not target_game_lower:
            return {
                "eligible": False,
                "reason": "target_game_unconfigured",
                "current_game": current_game_text,
                "recent_deadlock": recent_deadlock,
                "had_deadlock_session": had_deadlock_session,
            }

        if current_game_lower == target_game_lower:
            return {
                "eligible": True,
                "reason": "active_deadlock",
                "current_game": current_game_text,
                "recent_deadlock": True,
                "had_deadlock_session": had_deadlock_session,
            }

        if current_game_lower == "just chatting":
            if not had_deadlock_session:
                return {
                    "eligible": False,
                    "reason": "just_chatting_without_deadlock_session",
                    "current_game": current_game_text,
                    "recent_deadlock": False,
                    "had_deadlock_session": had_deadlock_session,
                }
            if recent_deadlock:
                return {
                    "eligible": True,
                    "reason": "recent_deadlock_session",
                    "current_game": current_game_text,
                    "recent_deadlock": True,
                    "had_deadlock_session": had_deadlock_session,
                }
            return {
                "eligible": False,
                "reason": "stale_deadlock_session",
                "current_game": current_game_text,
                "recent_deadlock": False,
                "had_deadlock_session": had_deadlock_session,
            }

        if not current_game_lower:
            reason = "missing_current_game"
        else:
            reason = "source_category_mismatch"
        return {
            "eligible": False,
            "reason": reason,
            "current_game": current_game_text,
            "recent_deadlock": False,
            "had_deadlock_session": had_deadlock_session,
        }

    def is_deadlock_raid_source_eligible(
        self,
        *,
        last_game: str,
        had_deadlock_session: bool,
        last_deadlock_seen_at: str | None,
    ) -> bool:
        evaluation = self.evaluate_deadlock_raid_source(
            current_game=last_game,
            had_deadlock_session=had_deadlock_session,
            last_deadlock_seen_at=last_deadlock_seen_at,
        )
        return bool(evaluation.get("eligible"))

    def is_deadlock_partner_candidate_eligible(
        self,
        *,
        game_name: str,
        had_deadlock_session: bool,
        last_deadlock_seen_at: str | None,
    ) -> bool:
        target_game_lower = self.get_target_game_lower()
        if not target_game_lower:
            return True

        game_lower = str(game_name or "").strip().lower()
        if game_lower == target_game_lower:
            return True
        if game_lower == "just chatting" and had_deadlock_session:
            return self.is_recent_deadlock(last_deadlock_seen_at)
        return False

    def load_partner_roster_for_raid(self, source_user_id: str) -> list[dict[str, object]]:
        connection_factory = self.readonly_connection_factory or readonly_connection
        with connection_factory() as conn:
            rows = conn.execute(
                """
                SELECT DISTINCT s.twitch_login, s.twitch_user_id,
                       r.raid_enabled, r.authorized_at
                  FROM twitch_streamers_partner_state s
                  LEFT JOIN twitch_raid_auth r ON s.twitch_user_id = r.twitch_user_id
                 WHERE s.is_partner_active = 1
                   AND s.twitch_user_id IS NOT NULL
                   AND s.twitch_login IS NOT NULL
                   AND s.twitch_user_id != %s
                """,
                (source_user_id,),
            ).fetchall()

        partners: list[dict[str, object]] = []
        for row in rows:
            partner_login = str(_row_value(row, "twitch_login", 0, "") or "").strip().lower()
            partner_user_id = str(_row_value(row, "twitch_user_id", 1, "") or "").strip()
            raid_enabled = bool(_row_value(row, "raid_enabled", 2, False))
            raid_authorized_at = _row_value(row, "authorized_at", 3, None)
            if not partner_login or not partner_user_id:
                continue
            if not raid_enabled and not raid_authorized_at:
                continue
            partners.append(
                {
                    "twitch_login": partner_login,
                    "twitch_user_id": partner_user_id,
                    "raid_enabled": raid_enabled or bool(raid_authorized_at),
                }
            )
        return partners

    def build_online_partner_candidates(
        self,
        partner_rows: list[dict[str, object]],
        streams_by_login: dict[str, dict[str, Any]],
    ) -> list[dict[str, Any]]:
        online_partners: list[dict[str, Any]] = []
        for partner_row in partner_rows:
            partner_login = str(partner_row.get("twitch_login") or "").strip().lower()
            partner_user_id = str(partner_row.get("twitch_user_id") or "").strip()
            if not partner_login or not partner_user_id:
                continue
            stream_data = streams_by_login.get(partner_login)
            if not stream_data:
                continue
            candidate = dict(stream_data)
            candidate["user_id"] = partner_user_id
            candidate["raid_enabled"] = bool(partner_row.get("raid_enabled", True))
            online_partners.append(candidate)
        return online_partners

    def load_partner_live_state_map(
        self,
        partner_logins_lower: list[str],
    ) -> dict[str, dict[str, object]]:
        if not partner_logins_lower:
            return {}

        placeholders = ",".join("%s" for _ in partner_logins_lower)
        connection_factory = self.readonly_connection_factory or readonly_connection
        with connection_factory() as conn:
            rows = conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
                f"""
                SELECT streamer_login, had_deadlock_in_session, last_game, last_deadlock_seen_at
                  FROM twitch_live_state
                 WHERE streamer_login IN ({placeholders})
                """,
                partner_logins_lower,
            ).fetchall()

        live_state_by_login: dict[str, dict[str, object]] = {}
        for row in rows:
            login_lower = str(_row_value(row, "streamer_login", 0, "") or "").strip().lower()
            if not login_lower:
                continue
            live_state_by_login[login_lower] = {
                "had_deadlock_in_session": bool(
                    _safe_int(_row_value(row, "had_deadlock_in_session", 1, 0), 0)
                ),
                "last_game": str(_row_value(row, "last_game", 2, "") or "").strip(),
                "last_deadlock_seen_at": str(
                    _row_value(row, "last_deadlock_seen_at", 3, "") or ""
                ).strip(),
            }
        return live_state_by_login

    def filter_deadlock_eligible_partner_candidates(
        self,
        online_partners: list[dict[str, Any]],
    ) -> tuple[list[dict[str, Any]], list[str]]:
        target_game_lower = self.get_target_game_lower()
        if not target_game_lower or not online_partners:
            return list(online_partners), []

        partner_logins_lower = [
            str(stream_data.get("user_login") or "").strip().lower()
            for stream_data in online_partners
            if str(stream_data.get("user_login") or "").strip()
        ]
        live_state_by_login: dict[str, dict[str, object]] = {}
        try:
            live_state_by_login = self.load_partner_live_state_map(partner_logins_lower)
        except Exception:
            self.logger.debug("Konnte Live-State für Partner nicht laden", exc_info=True)

        filtered_active: list[dict[str, Any]] = []
        filtered_recent: list[dict[str, Any]] = []
        filtered_out: list[str] = []

        for stream_data in online_partners:
            partner_login_lower = str(stream_data.get("user_login") or "").strip().lower()
            game_name = str(stream_data.get("game_name") or "").strip()
            game_lower = game_name.lower()
            live_state = live_state_by_login.get(partner_login_lower, {})
            had_deadlock_partner = bool(live_state.get("had_deadlock_in_session", False))
            last_game_state = str(live_state.get("last_game") or "").strip()
            last_deadlock_seen_partner = (
                str(live_state.get("last_deadlock_seen_at") or "").strip() or None
            )

            allow_partner = self.is_deadlock_partner_candidate_eligible(
                game_name=game_name,
                had_deadlock_session=had_deadlock_partner,
                last_deadlock_seen_at=last_deadlock_seen_partner,
            )
            if allow_partner:
                if game_lower == target_game_lower:
                    filtered_active.append(stream_data)
                else:
                    filtered_recent.append(stream_data)
                continue

            filtered_out.append(
                f"{partner_login_lower} (game='{game_name or last_game_state}', "
                f"had_deadlock_session={had_deadlock_partner}, "
                f"last_deadlock_seen={last_deadlock_seen_partner or 'none'})"
            )

        eligible_partners = filtered_active if filtered_active else filtered_recent
        return eligible_partners, filtered_out

    def load_broadcaster_live_state(self, broadcaster_id: str) -> dict[str, object]:
        connection_factory = self.readonly_connection_factory or readonly_connection
        with connection_factory() as conn:
            row = conn.execute(
                """
                SELECT twitch_user_id, streamer_login, is_live, last_started_at,
                       last_game, last_viewer_count, had_deadlock_in_session, last_deadlock_seen_at
                  FROM twitch_live_state
                 WHERE twitch_user_id = %s
                """,
                (broadcaster_id,),
            ).fetchone()

        if not row:
            return {}

        return {
            "twitch_user_id": str(_row_value(row, "twitch_user_id", 0, "") or "").strip(),
            "streamer_login": str(_row_value(row, "streamer_login", 1, "") or "").strip().lower(),
            "is_live": bool(_safe_int(_row_value(row, "is_live", 2, 0), 0)),
            "last_started_at": str(_row_value(row, "last_started_at", 3, "") or "").strip(),
            "last_game": str(_row_value(row, "last_game", 4, "") or "").strip(),
            "last_viewer_count": _safe_int(_row_value(row, "last_viewer_count", 5, 0), 0),
            "had_deadlock_in_session": bool(
                _safe_int(_row_value(row, "had_deadlock_in_session", 6, 0), 0)
            ),
            "last_deadlock_seen_at": str(
                _row_value(row, "last_deadlock_seen_at", 7, "") or ""
            ).strip(),
        }

    def calculate_stream_duration_sec(self, started_at: str | None) -> int:
        started_dt = _parse_datetime(started_at)
        if started_dt is None:
            return 0
        return max(0, int((self.utcnow() - started_dt).total_seconds()))

    def raid_language_filters(self) -> list[str | None]:
        if callable(self.language_filter_getter):
            try:
                values = list(self.language_filter_getter())
            except Exception:
                values = []
            if values:
                return values
        return [None]

    def create_twitch_api(self) -> Any | None:
        if callable(self.twitch_api_factory):
            return self.twitch_api_factory()
        session = self.session_getter() if callable(self.session_getter) else None
        if session is None:
            return None
        try:
            from ...api.twitch_api import TwitchAPI
        except Exception:
            return None
        return TwitchAPI(
            self.client_id,
            self.client_secret,
            session=session,
        )

    async def fetch_streams_by_logins_for_raid(
        self,
        logins: list[str],
        *,
        api: Any = None,
    ) -> dict[str, dict[str, Any]]:
        normalized_logins = [
            login_lower
            for login_lower in dict.fromkeys(
                str(login or "").strip().lower()
                for login in logins
                if str(login or "").strip()
            )
            if login_lower
        ]
        if not normalized_logins:
            return {}

        if callable(self.shared_stream_fetch):
            try:
                streams_by_login = await self.shared_stream_fetch(normalized_logins)
                if streams_by_login:
                    return streams_by_login
            except Exception:
                self.logger.debug("RaidBot: shared stream fetch failed", exc_info=True)

        api_client = api or self.create_twitch_api()
        if api_client is None:
            return {}

        streams_by_login: dict[str, dict[str, Any]] = {}
        for language in self.raid_language_filters():
            try:
                streams = await api_client.get_streams_by_logins(
                    normalized_logins,
                    language=language,
                )
            except Exception:
                self.logger.debug(
                    "RaidBot: get_streams_by_logins failed (language=%s)",
                    language or "any",
                    exc_info=True,
                )
                continue
            for stream in streams:
                login_lower = str(stream.get("user_login") or "").strip().lower()
                if login_lower:
                    streams_by_login[login_lower] = stream
        return streams_by_login

    def overlay_broadcaster_live_state_from_stream(
        self,
        live_state: dict[str, object],
        stream_data: dict[str, object],
    ) -> dict[str, object]:
        merged_state = dict(live_state)
        twitch_user_id = str(
            stream_data.get("user_id") or merged_state.get("twitch_user_id") or ""
        ).strip()
        streamer_login = str(
            stream_data.get("user_login") or merged_state.get("streamer_login") or ""
        ).strip().lower()
        merged_state.update(
            {
                "twitch_user_id": twitch_user_id,
                "streamer_login": streamer_login,
                "is_live": True,
                "last_started_at": str(stream_data.get("started_at") or "").strip(),
                "last_game": str(stream_data.get("game_name") or "").strip(),
                "last_viewer_count": _safe_int(stream_data.get("viewer_count"), 0),
            }
        )
        return merged_state

    async def resolve_manual_raid_source_state(
        self,
        *,
        broadcaster_id: str,
        broadcaster_login: str,
        api: Any = _API_UNSET,
    ) -> dict[str, object]:
        db_live_state = self.load_broadcaster_live_state(broadcaster_id)
        resolved_live_state = dict(db_live_state)
        normalized_login = str(broadcaster_login or "").strip().lower()
        api_client = self.create_twitch_api() if api is _API_UNSET else api

        if api_client is not None and normalized_login:
            try:
                streams = await api_client.get_streams_by_logins([normalized_login])
            except Exception:
                self.logger.debug(
                    "Manual raid source refresh failed for %s; falling back to DB snapshot",
                    broadcaster_login,
                    exc_info=True,
                )
            else:
                matched_stream = next(
                    (
                        stream
                        for stream in streams
                        if str(stream.get("user_login") or "").strip().lower()
                        == normalized_login
                    ),
                    None,
                )
                if matched_stream is None and len(streams) == 1:
                    matched_stream = streams[0]
                if matched_stream is not None:
                    return {
                        "status": "ok",
                        "state_source": "api_live",
                        "live_state": self.overlay_broadcaster_live_state_from_stream(
                            resolved_live_state,
                            matched_stream,
                        ),
                    }
                return {
                    "status": "source_not_live",
                    "state_source": "api_offline",
                    "live_state": resolved_live_state,
                }

        if resolved_live_state and bool(resolved_live_state.get("is_live")):
            return {
                "status": "ok",
                "state_source": "db",
                "live_state": resolved_live_state,
            }
        return {
            "status": "source_not_live",
            "state_source": "db",
            "live_state": resolved_live_state,
        }

    async def resolve_target_category_id(self, api: Any = _API_UNSET) -> str | None:
        if callable(self.cached_category_id_getter):
            cached_category_id = self.cached_category_id_getter()
            if cached_category_id:
                return str(cached_category_id)

        api_client = self.create_twitch_api() if api is _API_UNSET else api
        if api_client is None:
            return None

        try:
            return await api_client.get_category_id(self.target_game_name)
        except Exception:
            self.logger.debug("RaidBot: could not resolve target category id", exc_info=True)
            return None


__all__ = ["RaidDataSourceService"]

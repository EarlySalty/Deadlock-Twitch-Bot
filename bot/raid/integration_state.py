from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from ..api.token_error_handler import TokenErrorHandler
from ..core.constants import log
from ..discord_role_sync import normalize_discord_user_id
from ..storage import readonly_connection


def _normalize_bool(value: object) -> bool:
    if isinstance(value, bool):
        return value
    if isinstance(value, (int, float)):
        return bool(value)
    text = str(value or "").strip().lower()
    return text in {"1", "true", "yes", "y", "on", "t"}


def _normalize_login(value: object) -> str | None:
    text = str(value or "").strip().lower()
    return text or None


def _row_value(row: Any, key: str, index: int) -> Any:
    if row is None:
        return None
    if hasattr(row, "keys"):
        return row[key]
    return row[index]


@dataclass(frozen=True, slots=True)
class RaidIntegrationState:
    discord_user_id: str | None
    twitch_login: str | None
    twitch_user_id: str | None
    authorized: bool
    partner_opt_out: bool
    token_blacklisted: bool
    raid_blacklisted: bool

    @property
    def blocked(self) -> bool:
        return self.partner_opt_out or self.token_blacklisted or self.raid_blacklisted

    def to_payload(self) -> dict[str, Any]:
        return {
            "discord_user_id": self.discord_user_id,
            "twitch_login": self.twitch_login,
            "twitch_user_id": self.twitch_user_id,
            "authorized": self.authorized,
            "partner_opt_out": self.partner_opt_out,
            "token_blacklisted": self.token_blacklisted,
            "raid_blacklisted": self.raid_blacklisted,
            "blocked": self.blocked,
        }


class RaidIntegrationStateResolver:
    def __init__(
        self,
        *,
        auth_manager: Any | None = None,
        token_error_handler: TokenErrorHandler | None = None,
    ) -> None:
        self._auth_manager = auth_manager
        self._token_error_handler = token_error_handler

    @staticmethod
    def _query_streamer_rows_by_discord(conn: Any, discord_user_id: str) -> list[Any]:
        return conn.execute(
            """
            SELECT twitch_login, twitch_user_id, discord_user_id, manual_partner_opt_out,
                   manual_verified_at, created_at
            FROM twitch_streamers_partner_state
            WHERE discord_user_id = %s
            ORDER BY
                CASE WHEN manual_verified_at IS NULL THEN 1 ELSE 0 END,
                manual_verified_at DESC,
                CASE WHEN created_at IS NULL THEN 1 ELSE 0 END,
                created_at DESC
            """,
            (discord_user_id,),
        ).fetchall()

    @staticmethod
    def _query_streamer_row_by_login(conn: Any, twitch_login: str) -> Any:
        return conn.execute(
            """
            SELECT twitch_login, twitch_user_id, discord_user_id, manual_partner_opt_out,
                   manual_verified_at, created_at
            FROM twitch_streamers_partner_state
            WHERE LOWER(twitch_login) = LOWER(%s)
            ORDER BY
                CASE WHEN manual_verified_at IS NULL THEN 1 ELSE 0 END,
                manual_verified_at DESC,
                CASE WHEN created_at IS NULL THEN 1 ELSE 0 END,
                created_at DESC
            LIMIT 1
            """,
            (twitch_login,),
        ).fetchone()

    @staticmethod
    def _query_auth_row_by_user_id(conn: Any, twitch_user_id: str) -> Any:
        return conn.execute(
            """
            SELECT twitch_login, twitch_user_id, raid_enabled, authorized_at
            FROM twitch_raid_auth
            WHERE twitch_user_id = %s
            LIMIT 1
            """,
            (twitch_user_id,),
        ).fetchone()

    @staticmethod
    def _query_auth_row_by_login(conn: Any, twitch_login: str) -> Any:
        return conn.execute(
            """
            SELECT twitch_login, twitch_user_id, raid_enabled, authorized_at
            FROM twitch_raid_auth
            WHERE LOWER(twitch_login) = LOWER(%s)
            LIMIT 1
            """,
            (twitch_login,),
        ).fetchone()

    @staticmethod
    def _query_token_blacklist(conn: Any, twitch_user_id: str) -> bool:
        row = conn.execute(
            """
            SELECT error_count
            FROM twitch_token_blacklist
            WHERE twitch_user_id = %s
            LIMIT 1
            """,
            (twitch_user_id,),
        ).fetchone()
        if row is None:
            return False
        try:
            return int(_row_value(row, "error_count", 0) or 0) >= TokenErrorHandler.BLACKLIST_DISABLE_THRESHOLD
        except Exception:
            return True

    @staticmethod
    def _query_raid_blacklist(conn: Any, twitch_login: str) -> bool:
        row = conn.execute(
            """
            SELECT 1
            FROM twitch_raid_blacklist
            WHERE LOWER(target_login) = LOWER(%s)
            LIMIT 1
            """,
            (twitch_login,),
        ).fetchone()
        return bool(row)

    def _is_token_blacklisted(self, conn: Any, twitch_user_id: str) -> bool:
        handler = self._token_error_handler
        if handler is not None and hasattr(handler, "is_token_blacklisted"):
            try:
                return bool(handler.is_token_blacklisted(twitch_user_id))
            except Exception:
                log.warning(
                    "Raid integration state: token blacklist check failed for %s",
                    twitch_user_id,
                    exc_info=True,
                )
        return self._query_token_blacklist(conn, twitch_user_id)

    def _is_authorized(self, conn: Any, twitch_user_id: str | None, twitch_login: str | None) -> tuple[bool, str | None]:
        resolved_user_id = str(twitch_user_id or "").strip() or None

        if resolved_user_id and self._auth_manager is not None and hasattr(self._auth_manager, "has_enabled_auth"):
            try:
                if bool(self._auth_manager.has_enabled_auth(resolved_user_id)):
                    return True, resolved_user_id
            except Exception:
                log.warning(
                    "Raid integration state: auth-manager check failed for %s",
                    resolved_user_id,
                    exc_info=True,
                )

        auth_row = None
        if resolved_user_id:
            auth_row = self._query_auth_row_by_user_id(conn, resolved_user_id)
        if auth_row is None and twitch_login:
            auth_row = self._query_auth_row_by_login(conn, twitch_login)

        if auth_row is None:
            return False, resolved_user_id

        auth_user_id = str(_row_value(auth_row, "twitch_user_id", 1) or "").strip() or None
        authorized = _normalize_bool(_row_value(auth_row, "raid_enabled", 2)) or bool(
            str(_row_value(auth_row, "authorized_at", 3) or "").strip()
        )
        return authorized, auth_user_id or resolved_user_id

    def resolve(
        self,
        *,
        discord_user_id: str | int | None = None,
        twitch_login: str | None = None,
    ) -> RaidIntegrationState:
        normalized_discord_id = normalize_discord_user_id(
            None if discord_user_id is None else str(discord_user_id)
        )
        normalized_login = _normalize_login(twitch_login)

        if normalized_discord_id is None and not normalized_login:
            raise ValueError("discord_user_id or twitch_login is required")

        result_discord_id = normalized_discord_id
        result_login: str | None = None
        result_user_id: str | None = None
        partner_opt_out = False
        candidate_logins: set[str] = set()
        candidate_user_ids: set[str] = set()

        with readonly_connection() as conn:
            if normalized_discord_id is not None:
                discord_rows = self._query_streamer_rows_by_discord(conn, normalized_discord_id)
                if discord_rows:
                    first_row = discord_rows[0]
                    result_login = _normalize_login(_row_value(first_row, "twitch_login", 0))
                    result_user_id = str(_row_value(first_row, "twitch_user_id", 1) or "").strip() or None
                    result_discord_id = (
                        normalize_discord_user_id(_row_value(first_row, "discord_user_id", 2))
                        or result_discord_id
                    )
                    for row in discord_rows:
                        login_value = _normalize_login(_row_value(row, "twitch_login", 0))
                        user_id_value = str(_row_value(row, "twitch_user_id", 1) or "").strip()
                        if login_value:
                            candidate_logins.add(login_value)
                        if user_id_value:
                            candidate_user_ids.add(user_id_value)
                        partner_opt_out = partner_opt_out or _normalize_bool(
                            _row_value(row, "manual_partner_opt_out", 3)
                        )

            if normalized_login:
                login_row = self._query_streamer_row_by_login(conn, normalized_login)
                if login_row is not None:
                    login_value = _normalize_login(_row_value(login_row, "twitch_login", 0))
                    user_id_value = str(_row_value(login_row, "twitch_user_id", 1) or "").strip() or None
                    discord_value = normalize_discord_user_id(_row_value(login_row, "discord_user_id", 2))
                    if login_value:
                        candidate_logins.add(login_value)
                    if user_id_value:
                        candidate_user_ids.add(user_id_value)
                    if result_login is None:
                        result_login = login_value
                    if result_user_id is None and user_id_value:
                        result_user_id = user_id_value
                    if result_discord_id is None and discord_value:
                        result_discord_id = discord_value
                    partner_opt_out = partner_opt_out or _normalize_bool(
                        _row_value(login_row, "manual_partner_opt_out", 3)
                    )

            authorized, authorized_user_id = self._is_authorized(conn, result_user_id, result_login or normalized_login)
            if authorized_user_id:
                result_user_id = authorized_user_id
                candidate_user_ids.add(authorized_user_id)

            if result_user_id is None and (result_login or normalized_login):
                auth_row = self._query_auth_row_by_login(conn, result_login or normalized_login or "")
                if auth_row is not None:
                    login_value = _normalize_login(_row_value(auth_row, "twitch_login", 0))
                    user_id_value = str(_row_value(auth_row, "twitch_user_id", 1) or "").strip() or None
                    if result_login is None:
                        result_login = login_value
                    if result_user_id is None and user_id_value:
                        result_user_id = user_id_value
                    if login_value:
                        candidate_logins.add(login_value)
                    if user_id_value:
                        candidate_user_ids.add(user_id_value)

            if normalized_login:
                candidate_logins.add(normalized_login)
            if result_login:
                candidate_logins.add(result_login)

            token_blacklisted = any(
                self._is_token_blacklisted(conn, user_id)
                for user_id in sorted(candidate_user_ids)
                if user_id
            )
            raid_blacklisted = any(
                self._query_raid_blacklist(conn, login_value)
                for login_value in sorted(candidate_logins)
                if login_value
            )

        return RaidIntegrationState(
            discord_user_id=result_discord_id,
            twitch_login=result_login or normalized_login,
            twitch_user_id=result_user_id,
            authorized=authorized,
            partner_opt_out=partner_opt_out,
            token_blacklisted=token_blacklisted,
            raid_blacklisted=raid_blacklisted,
        )

    def resolve_auth_state(self, discord_user_id: str | int) -> RaidIntegrationState:
        normalized_discord_id = normalize_discord_user_id(str(discord_user_id))
        if normalized_discord_id is None:
            raise ValueError("invalid discord_user_id")
        return self.resolve(discord_user_id=normalized_discord_id)

    def resolve_block_state(
        self,
        *,
        discord_user_id: str | int | None = None,
        twitch_login: str | None = None,
    ) -> RaidIntegrationState:
        normalized_discord_id = None
        if discord_user_id is not None:
            normalized_discord_id = normalize_discord_user_id(str(discord_user_id))
            if normalized_discord_id is None:
                raise ValueError("invalid discord_user_id")
        normalized_login = _normalize_login(twitch_login)
        if normalized_discord_id is None and not normalized_login:
            raise ValueError("discord_user_id or twitch_login is required")
        return self.resolve(discord_user_id=normalized_discord_id, twitch_login=normalized_login)


__all__ = ["RaidIntegrationState", "RaidIntegrationStateResolver"]

from __future__ import annotations

from datetime import UTC, datetime, timedelta
from typing import Any

PARTNER_STATUS_ACTIVE = "active"
PARTNER_STATUS_ARCHIVED = "archived"
_UNSET = object()


def _now_iso() -> str:
    return datetime.now(UTC).isoformat()


def _normalize_login(login: str | None) -> str:
    return str(login or "").strip().lower()


def _normalize_user_id(twitch_user_id: str | None) -> str:
    return str(twitch_user_id or "").strip()


def _row_value(row: Any, key: str, index: int = 0, default: Any = None) -> Any:
    if row is None:
        return default
    if hasattr(row, "get"):
        return row.get(key, default)
    try:
        return row[index]
    except Exception:
        return default


def _bool_int(value: Any, default: int = 0) -> int:
    if value is None:
        return int(default)
    if isinstance(value, bool):
        return 1 if value else 0
    try:
        return 1 if int(value) else 0
    except (TypeError, ValueError):
        return int(default)


def _load_streamer_row(
    conn: Any,
    *,
    twitch_login: str | None = None,
    twitch_user_id: str | None = None,
) -> Any:
    normalized_login = _normalize_login(twitch_login)
    normalized_user_id = _normalize_user_id(twitch_user_id)
    if not normalized_login and not normalized_user_id:
        return None
    return conn.execute(
        """
        SELECT
            twitch_login,
            twitch_user_id,
            require_discord_link,
            next_link_check_at,
            discord_user_id,
            discord_display_name,
            is_on_discord,
            created_at,
            archived_at,
            raid_bot_enabled,
            silent_ban,
            silent_raid,
            is_monitored_only,
            live_ping_role_id,
            COALESCE(live_ping_enabled, 1) AS live_ping_enabled
        FROM twitch_streamers
        WHERE (? <> '' AND twitch_user_id = ?)
           OR (? <> '' AND LOWER(twitch_login) = ?)
        ORDER BY
            CASE WHEN ? <> '' AND twitch_user_id = ? THEN 0 ELSE 1 END,
            LOWER(twitch_login)
        LIMIT 1
        """,
        (
            normalized_user_id,
            normalized_user_id,
            normalized_login,
            normalized_login,
            normalized_user_id,
            normalized_user_id,
        ),
    ).fetchone()


def _load_partner_row(
    conn: Any,
    *,
    twitch_login: str | None = None,
    twitch_user_id: str | None = None,
    status: str | None = None,
    latest: bool = False,
) -> Any:
    normalized_login = _normalize_login(twitch_login)
    normalized_user_id = _normalize_user_id(twitch_user_id)
    if not normalized_login and not normalized_user_id:
        return None
    where = [
        "((? <> '' AND p.twitch_user_id = ?) OR (? <> '' AND LOWER(p.twitch_login) = ?))"
    ]
    params: list[Any] = [
        normalized_user_id,
        normalized_user_id,
        normalized_login,
        normalized_login,
    ]
    if status:
        where.append("p.status = ?")
        params.append(status)
    order_clause = (
        "ORDER BY COALESCE(p.departnered_at, p.partnered_at, '') DESC, p.id DESC"
        if latest
        else "ORDER BY p.id DESC"
    )
    sql = f"""
        SELECT
            p.id,
            p.twitch_user_id,
            p.twitch_login,
            p.require_discord_link,
            p.last_description,
            p.last_link_ok,
            p.added_by,
            p.last_link_checked_at,
            p.next_link_check_at,
            p.manual_verified_permanent,
            p.manual_verified_until,
            p.manual_verified_at,
            p.manual_partner_opt_out,
            p.raid_bot_enabled,
            p.silent_ban,
            p.silent_raid,
            p.live_ping_role_id,
            COALESCE(p.live_ping_enabled, 1) AS live_ping_enabled,
            p.partnered_at,
            p.departnered_at,
            p.status,
            i.discord_user_id,
            i.discord_display_name,
            i.is_on_discord,
            i.created_at AS identity_created_at,
            i.updated_at AS identity_updated_at
        FROM twitch_partners p
        LEFT JOIN twitch_streamer_identities i
          ON i.twitch_user_id = p.twitch_user_id
        WHERE {" AND ".join(where)}
        {order_clause}
        LIMIT 1
    """
    return conn.execute(sql, tuple(params)).fetchone()


def load_active_partner(
    conn: Any,
    *,
    twitch_login: str | None = None,
    twitch_user_id: str | None = None,
) -> Any:
    return _load_partner_row(
        conn,
        twitch_login=twitch_login,
        twitch_user_id=twitch_user_id,
        status=PARTNER_STATUS_ACTIVE,
    )


def load_latest_partner_history(
    conn: Any,
    *,
    twitch_login: str | None = None,
    twitch_user_id: str | None = None,
) -> Any:
    return _load_partner_row(
        conn,
        twitch_login=twitch_login,
        twitch_user_id=twitch_user_id,
        latest=True,
    )


def load_partner_by_discord_user_id(conn: Any, discord_user_id: str | None) -> Any:
    normalized_discord_user_id = str(discord_user_id or "").strip()
    if not normalized_discord_user_id:
        return None
    try:
        return conn.execute(
            """
            SELECT
                p.id,
                p.twitch_user_id,
                p.twitch_login,
                p.require_discord_link,
                p.next_link_check_at,
                p.manual_verified_permanent,
                p.manual_verified_until,
                p.manual_verified_at,
                p.manual_partner_opt_out,
                p.raid_bot_enabled,
                p.silent_ban,
                p.silent_raid,
                p.live_ping_role_id,
                COALESCE(p.live_ping_enabled, 1) AS live_ping_enabled,
                p.partnered_at,
                p.departnered_at,
                p.status,
                i.discord_user_id,
                i.discord_display_name,
                i.is_on_discord
            FROM twitch_partners p
            JOIN twitch_streamer_identities i
              ON i.twitch_user_id = p.twitch_user_id
            WHERE p.status = ?
              AND i.discord_user_id = ?
            LIMIT 1
            """,
            (PARTNER_STATUS_ACTIVE, normalized_discord_user_id),
        ).fetchone()
    except Exception as exc:
        # Backwards-compat fallback for minimal schemas (e.g. unit tests) where the partner tables
        # are not present yet. Do not silently swallow unrelated DB errors.
        msg = str(exc).lower()
        if "twitch_partners" not in msg and "twitch_streamer_identities" not in msg:
            raise

    try:
        return conn.execute(
            """
            SELECT
                NULL AS id,
                s.twitch_user_id,
                s.twitch_login,
                0 AS require_discord_link,
                NULL AS next_link_check_at,
                0 AS manual_verified_permanent,
                NULL AS manual_verified_until,
                NULL AS manual_verified_at,
                0 AS manual_partner_opt_out,
                0 AS raid_bot_enabled,
                0 AS silent_ban,
                0 AS silent_raid,
                NULL AS live_ping_role_id,
                1 AS live_ping_enabled,
                NULL AS partnered_at,
                NULL AS departnered_at,
                ? AS status,
                s.discord_user_id,
                NULL AS discord_display_name,
                1 AS is_on_discord
            FROM twitch_streamers s
            WHERE s.discord_user_id = ?
            LIMIT 1
            """,
            (PARTNER_STATUS_ACTIVE, normalized_discord_user_id),
        ).fetchone()
    except Exception:
        return None


def load_streamer_identity(
    conn: Any,
    *,
    twitch_login: str | None = None,
    twitch_user_id: str | None = None,
    discord_user_id: str | None = None,
) -> Any:
    normalized_login = _normalize_login(twitch_login)
    normalized_user_id = _normalize_user_id(twitch_user_id)
    normalized_discord_user_id = str(discord_user_id or "").strip()
    if not normalized_login and not normalized_user_id and not normalized_discord_user_id:
        return None
    return conn.execute(
        """
        SELECT
            twitch_user_id,
            twitch_login,
            discord_user_id,
            discord_display_name,
            is_on_discord,
            created_at,
            updated_at
        FROM twitch_streamer_identities
        WHERE (? <> '' AND twitch_user_id = ?)
           OR (? <> '' AND LOWER(twitch_login) = ?)
           OR (? <> '' AND discord_user_id = ?)
        ORDER BY updated_at DESC
        LIMIT 1
        """,
        (
            normalized_user_id,
            normalized_user_id,
            normalized_login,
            normalized_login,
            normalized_discord_user_id,
            normalized_discord_user_id,
        ),
    ).fetchone()


def upsert_streamer_identity(
    conn: Any,
    *,
    twitch_user_id: str | None,
    twitch_login: str | None,
    discord_user_id: str | None = None,
    discord_display_name: str | None = None,
    is_on_discord: bool | int | None = None,
) -> None:
    normalized_user_id = _normalize_user_id(twitch_user_id)
    normalized_login = _normalize_login(twitch_login)
    if not normalized_user_id or not normalized_login:
        return
    normalized_discord_user_id = str(discord_user_id or "").strip() or None
    normalized_display_name = str(discord_display_name or "").strip() or None
    is_on_discord_value = (
        _bool_int(is_on_discord, default=1 if normalized_discord_user_id else 0)
        if is_on_discord is not None or normalized_discord_user_id
        else None
    )
    if normalized_discord_user_id:
        conn.execute(
            """
            UPDATE twitch_streamer_identities
            SET discord_user_id = NULL,
                discord_display_name = NULL,
                is_on_discord = 0,
                updated_at = CURRENT_TIMESTAMP
            WHERE discord_user_id = ?
              AND twitch_user_id <> ?
            """,
            (
                normalized_discord_user_id,
                normalized_user_id,
            ),
        )
    conn.execute(
        """
        INSERT INTO twitch_streamer_identities (
            twitch_user_id,
            twitch_login,
            discord_user_id,
            discord_display_name,
            is_on_discord,
            created_at,
            updated_at
        ) VALUES (?, ?, ?, ?, COALESCE(?, 0), CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
        ON CONFLICT (twitch_user_id) DO UPDATE SET
            twitch_login = EXCLUDED.twitch_login,
            discord_user_id = COALESCE(EXCLUDED.discord_user_id, twitch_streamer_identities.discord_user_id),
            discord_display_name = COALESCE(EXCLUDED.discord_display_name, twitch_streamer_identities.discord_display_name),
            is_on_discord = COALESCE(EXCLUDED.is_on_discord, twitch_streamer_identities.is_on_discord),
            updated_at = CURRENT_TIMESTAMP
        """,
        (
            normalized_user_id,
            normalized_login,
            normalized_discord_user_id,
            normalized_display_name,
            is_on_discord_value,
        ),
    )


def upsert_non_partner_streamer(
    conn: Any,
    *,
    twitch_login: str,
    twitch_user_id: str | None = None,
    require_discord_link: Any = _UNSET,
    next_link_check_at: Any = _UNSET,
    discord_user_id: Any = _UNSET,
    discord_display_name: Any = _UNSET,
    is_on_discord: Any = _UNSET,
    archived_at: Any = _UNSET,
    raid_bot_enabled: Any = _UNSET,
    silent_ban: Any = _UNSET,
    silent_raid: Any = _UNSET,
    is_monitored_only: Any = _UNSET,
    live_ping_role_id: Any = _UNSET,
    live_ping_enabled: Any = _UNSET,
    created_at: str | None = None,
) -> None:
    normalized_login = _normalize_login(twitch_login)
    normalized_user_id = _normalize_user_id(twitch_user_id)
    if not normalized_login:
        raise ValueError("twitch_login_required")

    existing = _load_streamer_row(
        conn,
        twitch_login=normalized_login,
        twitch_user_id=normalized_user_id,
    )
    existing_login = _normalize_login(_row_value(existing, "twitch_login", 0))
    if existing and existing_login and existing_login != normalized_login:
        conn.execute(
            """
            UPDATE twitch_streamers
            SET twitch_login = ?
            WHERE twitch_user_id = ?
               OR LOWER(twitch_login) = LOWER(?)
            """,
            (
                normalized_login,
                normalized_user_id or _row_value(existing, "twitch_user_id", 1),
                existing_login,
            ),
        )
        existing = _load_streamer_row(
            conn,
            twitch_login=normalized_login,
            twitch_user_id=normalized_user_id,
        )

    row_values = {
        "twitch_login": normalized_login,
        "twitch_user_id": normalized_user_id
        or str(_row_value(existing, "twitch_user_id", 1, "") or "").strip()
        or None,
        "require_discord_link": _bool_int(
            _row_value(existing, "require_discord_link", 2, 0)
            if require_discord_link is _UNSET
            else require_discord_link,
            default=0,
        ),
        "next_link_check_at": _row_value(existing, "next_link_check_at", 3, None)
        if next_link_check_at is _UNSET
        else next_link_check_at,
        "discord_user_id": _row_value(existing, "discord_user_id", 4, None)
        if discord_user_id is _UNSET
        else (str(discord_user_id or "").strip() or None),
        "discord_display_name": _row_value(existing, "discord_display_name", 5, None)
        if discord_display_name is _UNSET
        else (str(discord_display_name or "").strip() or None),
        "is_on_discord": _bool_int(
            _row_value(existing, "is_on_discord", 6, 0)
            if is_on_discord is _UNSET
            else is_on_discord,
            default=0,
        ),
        "created_at": _row_value(existing, "created_at", 7, None) or created_at or _now_iso(),
        "archived_at": _row_value(existing, "archived_at", 8, None)
        if archived_at is _UNSET
        else archived_at,
        "raid_bot_enabled": _bool_int(
            _row_value(existing, "raid_bot_enabled", 9, 0)
            if raid_bot_enabled is _UNSET
            else raid_bot_enabled,
            default=0,
        ),
        "silent_ban": _bool_int(
            _row_value(existing, "silent_ban", 10, 0) if silent_ban is _UNSET else silent_ban,
            default=0,
        ),
        "silent_raid": _bool_int(
            _row_value(existing, "silent_raid", 11, 0) if silent_raid is _UNSET else silent_raid,
            default=0,
        ),
        "is_monitored_only": _bool_int(
            _row_value(existing, "is_monitored_only", 12, 0)
            if is_monitored_only is _UNSET
            else is_monitored_only,
            default=0,
        ),
        "live_ping_role_id": _row_value(existing, "live_ping_role_id", 13, None)
        if live_ping_role_id is _UNSET
        else live_ping_role_id,
        "live_ping_enabled": _bool_int(
            _row_value(existing, "live_ping_enabled", 14, 1)
            if live_ping_enabled is _UNSET
            else live_ping_enabled,
            default=1,
        ),
    }

    conn.execute(
        """
        INSERT INTO twitch_streamers (
            twitch_login,
            twitch_user_id,
            require_discord_link,
            next_link_check_at,
            discord_user_id,
            discord_display_name,
            is_on_discord,
            created_at,
            archived_at,
            raid_bot_enabled,
            silent_ban,
            silent_raid,
            is_monitored_only,
            live_ping_role_id,
            live_ping_enabled
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT (twitch_login) DO UPDATE SET
            twitch_user_id = COALESCE(EXCLUDED.twitch_user_id, twitch_streamers.twitch_user_id),
            require_discord_link = EXCLUDED.require_discord_link,
            next_link_check_at = EXCLUDED.next_link_check_at,
            discord_user_id = EXCLUDED.discord_user_id,
            discord_display_name = EXCLUDED.discord_display_name,
            is_on_discord = EXCLUDED.is_on_discord,
            archived_at = EXCLUDED.archived_at,
            raid_bot_enabled = EXCLUDED.raid_bot_enabled,
            silent_ban = EXCLUDED.silent_ban,
            silent_raid = EXCLUDED.silent_raid,
            is_monitored_only = EXCLUDED.is_monitored_only,
            live_ping_role_id = EXCLUDED.live_ping_role_id,
            live_ping_enabled = EXCLUDED.live_ping_enabled
        """,
        (
            row_values["twitch_login"],
            row_values["twitch_user_id"],
            row_values["require_discord_link"],
            row_values["next_link_check_at"],
            row_values["discord_user_id"],
            row_values["discord_display_name"],
            row_values["is_on_discord"],
            row_values["created_at"],
            row_values["archived_at"],
            row_values["raid_bot_enabled"],
            row_values["silent_ban"],
            row_values["silent_raid"],
            row_values["is_monitored_only"],
            row_values["live_ping_role_id"],
            row_values["live_ping_enabled"],
        ),
    )

    if row_values["twitch_user_id"]:
        upsert_streamer_identity(
            conn,
            twitch_user_id=row_values["twitch_user_id"],
            twitch_login=row_values["twitch_login"],
            discord_user_id=row_values["discord_user_id"],
            discord_display_name=row_values["discord_display_name"],
            is_on_discord=row_values["is_on_discord"],
        )


def _normalize_related_tables(
    conn: Any,
    *,
    twitch_user_id: str,
    twitch_login: str,
) -> None:
    if not twitch_user_id or not twitch_login:
        return
    updates = [
        (
            """
            UPDATE twitch_raid_auth
            SET twitch_login = ?,
                twitch_user_id = COALESCE(NULLIF(twitch_user_id, ''), ?)
            WHERE twitch_user_id = ?
               OR LOWER(twitch_login) = LOWER(?)
            """,
            (twitch_login, twitch_user_id, twitch_user_id, twitch_login),
        ),
        (
            """
            UPDATE streamer_plans
            SET twitch_login = ?,
                twitch_user_id = COALESCE(NULLIF(twitch_user_id, ''), ?)
            WHERE twitch_user_id = ?
               OR LOWER(COALESCE(twitch_login, '')) = LOWER(?)
            """,
            (twitch_login, twitch_user_id, twitch_user_id, twitch_login),
        ),
        (
            """
            UPDATE twitch_partner_raid_scores
            SET twitch_login = ?,
                twitch_user_id = COALESCE(NULLIF(twitch_user_id, ''), ?)
            WHERE twitch_user_id = ?
               OR LOWER(COALESCE(twitch_login, '')) = LOWER(?)
            """,
            (twitch_login, twitch_user_id, twitch_user_id, twitch_login),
        ),
        (
            """
            UPDATE twitch_live_state
            SET streamer_login = ?
            WHERE twitch_user_id = ?
               OR LOWER(streamer_login) = LOWER(?)
            """,
            (twitch_login, twitch_user_id, twitch_login),
        ),
    ]
    for sql, params in updates:
        try:
            conn.execute(sql, params)
        except Exception:
            continue


def promote_streamer_to_partner(
    conn: Any,
    *,
    twitch_login: str,
    twitch_user_id: str,
    require_discord_link: Any = _UNSET,
    last_description: Any = _UNSET,
    last_link_ok: Any = _UNSET,
    added_by: Any = _UNSET,
    last_link_checked_at: Any = _UNSET,
    next_link_check_at: Any = _UNSET,
    manual_verified_permanent: Any = _UNSET,
    manual_verified_until: Any = _UNSET,
    manual_verified_at: Any = _UNSET,
    manual_partner_opt_out: Any = _UNSET,
    raid_bot_enabled: Any = _UNSET,
    silent_ban: Any = _UNSET,
    silent_raid: Any = _UNSET,
    live_ping_role_id: Any = _UNSET,
    live_ping_enabled: Any = _UNSET,
    discord_user_id: Any = _UNSET,
    discord_display_name: Any = _UNSET,
    is_on_discord: Any = _UNSET,
    partnered_at: str | None = None,
    clear_source: bool = True,
) -> dict[str, Any]:
    normalized_login = _normalize_login(twitch_login)
    normalized_user_id = _normalize_user_id(twitch_user_id)
    if not normalized_login or not normalized_user_id:
        raise ValueError("twitch_login_and_user_id_required")

    source_row = _load_streamer_row(
        conn,
        twitch_login=normalized_login,
        twitch_user_id=normalized_user_id,
    )
    active_row = load_active_partner(
        conn,
        twitch_login=normalized_login,
        twitch_user_id=normalized_user_id,
    )

    partner_values = {
        "require_discord_link": _bool_int(
            _row_value(source_row, "require_discord_link", 2, 0)
            if require_discord_link is _UNSET
            else require_discord_link,
            default=_bool_int(_row_value(active_row, "require_discord_link", 3, 0), default=0),
        ),
        "last_description": _row_value(active_row, "last_description", 4, None)
        if last_description is _UNSET
        else last_description,
        "last_link_ok": _row_value(active_row, "last_link_ok", 5, None)
        if last_link_ok is _UNSET
        else _bool_int(last_link_ok, default=0),
        "added_by": _row_value(active_row, "added_by", 6, None) if added_by is _UNSET else added_by,
        "last_link_checked_at": _row_value(active_row, "last_link_checked_at", 7, None)
        if last_link_checked_at is _UNSET
        else last_link_checked_at,
        "next_link_check_at": _row_value(source_row, "next_link_check_at", 3, None)
        if next_link_check_at is _UNSET
        else next_link_check_at,
        "manual_verified_permanent": _bool_int(
            _row_value(active_row, "manual_verified_permanent", 9, 0)
            if manual_verified_permanent is _UNSET
            else manual_verified_permanent,
            default=0,
        ),
        "manual_verified_until": _row_value(active_row, "manual_verified_until", 10, None)
        if manual_verified_until is _UNSET
        else manual_verified_until,
        "manual_verified_at": _row_value(active_row, "manual_verified_at", 11, None)
        if manual_verified_at is _UNSET
        else manual_verified_at,
        "manual_partner_opt_out": _bool_int(
            _row_value(active_row, "manual_partner_opt_out", 12, 0)
            if manual_partner_opt_out is _UNSET
            else manual_partner_opt_out,
            default=0,
        ),
        "raid_bot_enabled": _bool_int(
            _row_value(source_row, "raid_bot_enabled", 9, 0)
            if raid_bot_enabled is _UNSET
            else raid_bot_enabled,
            default=_bool_int(_row_value(active_row, "raid_bot_enabled", 13, 0), default=0),
        ),
        "silent_ban": _bool_int(
            _row_value(source_row, "silent_ban", 10, 0) if silent_ban is _UNSET else silent_ban,
            default=_bool_int(_row_value(active_row, "silent_ban", 14, 0), default=0),
        ),
        "silent_raid": _bool_int(
            _row_value(source_row, "silent_raid", 11, 0)
            if silent_raid is _UNSET
            else silent_raid,
            default=_bool_int(_row_value(active_row, "silent_raid", 15, 0), default=0),
        ),
        "live_ping_role_id": _row_value(source_row, "live_ping_role_id", 13, None)
        if live_ping_role_id is _UNSET
        else live_ping_role_id,
        "live_ping_enabled": _bool_int(
            _row_value(source_row, "live_ping_enabled", 14, 1)
            if live_ping_enabled is _UNSET
            else live_ping_enabled,
            default=_bool_int(_row_value(active_row, "live_ping_enabled", 17, 1), default=1),
        ),
    }

    identity_discord_user_id = (
        _row_value(active_row, "discord_user_id", 21, None)
        if discord_user_id is _UNSET
        else (str(discord_user_id or "").strip() or None)
    )
    if source_row and not identity_discord_user_id:
        identity_discord_user_id = _row_value(source_row, "discord_user_id", 4, None)
    identity_display_name = (
        _row_value(active_row, "discord_display_name", 22, None)
        if discord_display_name is _UNSET
        else (str(discord_display_name or "").strip() or None)
    )
    if source_row and not identity_display_name:
        identity_display_name = _row_value(source_row, "discord_display_name", 5, None)
    identity_is_on_discord = (
        _row_value(active_row, "is_on_discord", 23, None)
        if is_on_discord is _UNSET
        else _bool_int(is_on_discord, default=0)
    )
    if identity_is_on_discord is None and source_row is not None:
        identity_is_on_discord = _bool_int(_row_value(source_row, "is_on_discord", 6, 0), default=0)

    upsert_streamer_identity(
        conn,
        twitch_user_id=normalized_user_id,
        twitch_login=normalized_login,
        discord_user_id=identity_discord_user_id,
        discord_display_name=identity_display_name,
        is_on_discord=identity_is_on_discord,
    )

    effective_partnered_at = (
        partnered_at
        or _row_value(active_row, "partnered_at", 18, None)
        or _row_value(source_row, "created_at", 7, None)
        or _now_iso()
    )

    if active_row:
        conn.execute(
            """
            UPDATE twitch_partners
            SET twitch_login = ?,
                require_discord_link = ?,
                last_description = ?,
                last_link_ok = ?,
                added_by = ?,
                last_link_checked_at = ?,
                next_link_check_at = ?,
                manual_verified_permanent = ?,
                manual_verified_until = ?,
                manual_verified_at = ?,
                manual_partner_opt_out = ?,
                raid_bot_enabled = ?,
                silent_ban = ?,
                silent_raid = ?,
                live_ping_role_id = ?,
                live_ping_enabled = ?,
                partnered_at = ?,
                departnered_at = NULL,
                status = ?
            WHERE id = ?
            """,
            (
                normalized_login,
                partner_values["require_discord_link"],
                partner_values["last_description"],
                partner_values["last_link_ok"],
                partner_values["added_by"],
                partner_values["last_link_checked_at"],
                partner_values["next_link_check_at"],
                partner_values["manual_verified_permanent"],
                partner_values["manual_verified_until"],
                partner_values["manual_verified_at"],
                partner_values["manual_partner_opt_out"],
                partner_values["raid_bot_enabled"],
                partner_values["silent_ban"],
                partner_values["silent_raid"],
                partner_values["live_ping_role_id"],
                partner_values["live_ping_enabled"],
                effective_partnered_at,
                PARTNER_STATUS_ACTIVE,
                _row_value(active_row, "id", 0),
            ),
        )
    else:
        conn.execute(
            """
            INSERT INTO twitch_partners (
                twitch_user_id,
                twitch_login,
                require_discord_link,
                last_description,
                last_link_ok,
                added_by,
                last_link_checked_at,
                next_link_check_at,
                manual_verified_permanent,
                manual_verified_until,
                manual_verified_at,
                manual_partner_opt_out,
                raid_bot_enabled,
                silent_ban,
                silent_raid,
                live_ping_role_id,
                live_ping_enabled,
                partnered_at,
                departnered_at,
                status
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, ?)
            """,
            (
                normalized_user_id,
                normalized_login,
                partner_values["require_discord_link"],
                partner_values["last_description"],
                partner_values["last_link_ok"],
                partner_values["added_by"],
                partner_values["last_link_checked_at"],
                partner_values["next_link_check_at"],
                partner_values["manual_verified_permanent"],
                partner_values["manual_verified_until"],
                partner_values["manual_verified_at"],
                partner_values["manual_partner_opt_out"],
                partner_values["raid_bot_enabled"],
                partner_values["silent_ban"],
                partner_values["silent_raid"],
                partner_values["live_ping_role_id"],
                partner_values["live_ping_enabled"],
                effective_partnered_at,
                PARTNER_STATUS_ACTIVE,
            ),
        )

    _normalize_related_tables(
        conn,
        twitch_user_id=normalized_user_id,
        twitch_login=normalized_login,
    )
    if clear_source:
        conn.execute(
            """
            DELETE FROM twitch_streamers
            WHERE twitch_user_id = ?
               OR LOWER(twitch_login) = LOWER(?)
            """,
            (normalized_user_id, normalized_login),
        )

    return {
        "twitch_login": normalized_login,
        "twitch_user_id": normalized_user_id,
        "discord_user_id": identity_discord_user_id,
        "discord_display_name": identity_display_name,
        "is_on_discord": identity_is_on_discord,
    }


def archive_active_partner(
    conn: Any,
    *,
    twitch_login: str | None = None,
    twitch_user_id: str | None = None,
    restore_non_partner: bool = False,
    disable_raid_auth: bool = True,
) -> dict[str, Any] | None:
    active_row = load_active_partner(
        conn,
        twitch_login=twitch_login,
        twitch_user_id=twitch_user_id,
    )
    if not active_row:
        return None

    normalized_login = _normalize_login(_row_value(active_row, "twitch_login", 2, twitch_login))
    normalized_user_id = _normalize_user_id(
        _row_value(active_row, "twitch_user_id", 1, twitch_user_id)
    )
    departnered_at = _now_iso()
    discord_user_id = str(_row_value(active_row, "discord_user_id", 21, "") or "").strip() or None
    discord_display_name = (
        str(_row_value(active_row, "discord_display_name", 22, "") or "").strip() or None
    )
    is_on_discord = _bool_int(_row_value(active_row, "is_on_discord", 23, 0), default=0)

    upsert_streamer_identity(
        conn,
        twitch_user_id=normalized_user_id,
        twitch_login=normalized_login,
        discord_user_id=discord_user_id,
        discord_display_name=discord_display_name,
        is_on_discord=is_on_discord,
    )

    conn.execute(
        """
        UPDATE twitch_partners
        SET status = ?,
            departnered_at = ?,
            twitch_login = ?,
            twitch_user_id = ?
        WHERE id = ?
        """,
        (
            PARTNER_STATUS_ARCHIVED,
            departnered_at,
            normalized_login,
            normalized_user_id,
            _row_value(active_row, "id", 0),
        ),
    )

    if restore_non_partner:
        upsert_non_partner_streamer(
            conn,
            twitch_login=normalized_login,
            twitch_user_id=normalized_user_id,
            require_discord_link=_row_value(active_row, "require_discord_link", 3, 0),
            next_link_check_at=_row_value(active_row, "next_link_check_at", 8, None),
            discord_user_id=discord_user_id,
            discord_display_name=discord_display_name,
            is_on_discord=is_on_discord,
            archived_at=departnered_at,
            raid_bot_enabled=0,
            silent_ban=0,
            silent_raid=0,
            is_monitored_only=0,
            live_ping_role_id=_row_value(active_row, "live_ping_role_id", 16, None),
            live_ping_enabled=_row_value(active_row, "live_ping_enabled", 17, 1),
        )
        conn.execute(
            """
            UPDATE twitch_streamers
            SET manual_verified_permanent = 0,
                manual_verified_until = NULL,
                manual_verified_at = NULL,
                manual_partner_opt_out = 0
            WHERE twitch_user_id = ?
               OR LOWER(twitch_login) = LOWER(?)
            """,
            (normalized_user_id, normalized_login),
        )

    if disable_raid_auth:
        conn.execute(
            """
            UPDATE twitch_raid_auth
            SET raid_enabled = FALSE,
                twitch_login = ?
            WHERE twitch_user_id = ?
               OR LOWER(twitch_login) = LOWER(?)
            """,
            (normalized_login, normalized_user_id, normalized_login),
        )

    _normalize_related_tables(
        conn,
        twitch_user_id=normalized_user_id,
        twitch_login=normalized_login,
    )

    return {
        "twitch_login": normalized_login,
        "twitch_user_id": normalized_user_id,
        "discord_user_id": discord_user_id,
        "discord_display_name": discord_display_name,
        "departnered_at": departnered_at,
    }


def reactivate_partner(
    conn: Any,
    *,
    twitch_login: str | None = None,
    twitch_user_id: str | None = None,
) -> dict[str, Any] | None:
    active_row = load_active_partner(
        conn,
        twitch_login=twitch_login,
        twitch_user_id=twitch_user_id,
    )
    if active_row:
        return {
            "twitch_login": _normalize_login(_row_value(active_row, "twitch_login", 2)),
            "twitch_user_id": _normalize_user_id(_row_value(active_row, "twitch_user_id", 1)),
        }

    archived_row = _load_partner_row(
        conn,
        twitch_login=twitch_login,
        twitch_user_id=twitch_user_id,
        status=PARTNER_STATUS_ARCHIVED,
        latest=True,
    )
    if not archived_row:
        return None

    return promote_streamer_to_partner(
        conn,
        twitch_login=_row_value(archived_row, "twitch_login", 2),
        twitch_user_id=_row_value(archived_row, "twitch_user_id", 1),
        require_discord_link=_row_value(archived_row, "require_discord_link", 3, 0),
        last_description=_row_value(archived_row, "last_description", 4, None),
        last_link_ok=_row_value(archived_row, "last_link_ok", 5, None),
        added_by=_row_value(archived_row, "added_by", 6, None),
        last_link_checked_at=_row_value(archived_row, "last_link_checked_at", 7, None),
        next_link_check_at=_row_value(archived_row, "next_link_check_at", 8, None),
        manual_verified_permanent=_row_value(archived_row, "manual_verified_permanent", 9, 0),
        manual_verified_until=_row_value(archived_row, "manual_verified_until", 10, None),
        manual_verified_at=_row_value(archived_row, "manual_verified_at", 11, _now_iso()),
        manual_partner_opt_out=0,
        raid_bot_enabled=_row_value(archived_row, "raid_bot_enabled", 13, 0),
        silent_ban=_row_value(archived_row, "silent_ban", 14, 0),
        silent_raid=_row_value(archived_row, "silent_raid", 15, 0),
        live_ping_role_id=_row_value(archived_row, "live_ping_role_id", 16, None),
        live_ping_enabled=_row_value(archived_row, "live_ping_enabled", 17, 1),
        discord_user_id=_row_value(archived_row, "discord_user_id", 21, None),
        discord_display_name=_row_value(archived_row, "discord_display_name", 22, None),
        is_on_discord=_row_value(archived_row, "is_on_discord", 23, 0),
        partnered_at=_now_iso(),
        clear_source=True,
    )


def save_streamer_discord_profile(
    conn: Any,
    *,
    twitch_login: str,
    twitch_user_id: str | None = None,
    discord_user_id: str | None = None,
    discord_display_name: str | None = None,
    mark_member: bool,
) -> dict[str, Any]:
    normalized_login = _normalize_login(twitch_login)
    normalized_user_id = _normalize_user_id(twitch_user_id)
    if not normalized_login:
        raise ValueError("twitch_login_required")

    active_row = load_active_partner(
        conn,
        twitch_login=normalized_login,
        twitch_user_id=normalized_user_id,
    )
    streamer_row = _load_streamer_row(
        conn,
        twitch_login=normalized_login,
        twitch_user_id=normalized_user_id,
    )
    identity_row = load_streamer_identity(
        conn,
        twitch_login=normalized_login,
        twitch_user_id=normalized_user_id,
    )
    resolved_user_id = (
        normalized_user_id
        or _normalize_user_id(_row_value(active_row, "twitch_user_id", 1))
        or _normalize_user_id(_row_value(streamer_row, "twitch_user_id", 1))
        or _normalize_user_id(_row_value(identity_row, "twitch_user_id", 0))
    )
    normalized_discord_user_id = str(discord_user_id or "").strip() or None
    normalized_display_name = str(discord_display_name or "").strip() or None

    if active_row:
        if resolved_user_id:
            upsert_streamer_identity(
                conn,
                twitch_user_id=resolved_user_id,
                twitch_login=normalized_login,
                discord_user_id=normalized_discord_user_id,
                discord_display_name=normalized_display_name,
                is_on_discord=1 if mark_member else 0,
            )
        conn.execute(
            """
            UPDATE twitch_partners
            SET twitch_login = ?
            WHERE id = ?
            """,
            (normalized_login, _row_value(active_row, "id", 0)),
        )
        _normalize_related_tables(
            conn,
            twitch_user_id=resolved_user_id,
            twitch_login=normalized_login,
        )
        return {
            "twitch_login": normalized_login,
            "twitch_user_id": resolved_user_id,
            "partner": True,
        }

    upsert_non_partner_streamer(
        conn,
        twitch_login=normalized_login,
        twitch_user_id=resolved_user_id or None,
        discord_user_id=normalized_discord_user_id,
        discord_display_name=normalized_display_name,
        is_on_discord=1 if mark_member else 0,
    )
    return {
        "twitch_login": normalized_login,
        "twitch_user_id": resolved_user_id,
        "partner": False,
    }


def set_streamer_discord_member(
    conn: Any,
    *,
    twitch_login: str,
    is_on_discord: bool,
) -> dict[str, Any] | None:
    normalized_login = _normalize_login(twitch_login)
    if not normalized_login:
        return None
    active_row = load_active_partner(conn, twitch_login=normalized_login)
    if active_row:
        user_id = _normalize_user_id(_row_value(active_row, "twitch_user_id", 1))
        if user_id:
            upsert_streamer_identity(
                conn,
                twitch_user_id=user_id,
                twitch_login=normalized_login,
                discord_user_id=_row_value(active_row, "discord_user_id", 21, None),
                discord_display_name=_row_value(active_row, "discord_display_name", 22, None),
                is_on_discord=1 if is_on_discord else 0,
            )
        return {
            "twitch_login": normalized_login,
            "twitch_user_id": user_id,
            "partner": True,
        }

    streamer_row = _load_streamer_row(conn, twitch_login=normalized_login)
    if not streamer_row:
        return None
    upsert_non_partner_streamer(
        conn,
        twitch_login=normalized_login,
        twitch_user_id=_row_value(streamer_row, "twitch_user_id", 1, None),
        is_on_discord=1 if is_on_discord else 0,
    )
    return {
        "twitch_login": normalized_login,
        "twitch_user_id": _row_value(streamer_row, "twitch_user_id", 1, None),
        "partner": False,
    }


def set_partner_raid_bot_enabled(
    conn: Any,
    *,
    twitch_user_id: str | None = None,
    twitch_login: str | None = None,
    enabled: bool,
) -> bool:
    active_row = load_active_partner(
        conn,
        twitch_login=twitch_login,
        twitch_user_id=twitch_user_id,
    )
    if not active_row:
        return False
    normalized_user_id = _normalize_user_id(_row_value(active_row, "twitch_user_id", 1))
    normalized_login = _normalize_login(_row_value(active_row, "twitch_login", 2))
    conn.execute(
        """
        UPDATE twitch_partners
        SET raid_bot_enabled = ?
        WHERE id = ?
        """,
        (_bool_int(enabled, default=0), _row_value(active_row, "id", 0)),
    )
    conn.execute(
        """
        UPDATE twitch_raid_auth
        SET twitch_login = ?
        WHERE twitch_user_id = ?
        """,
        (normalized_login, normalized_user_id),
    )
    return True


def set_partner_silent_flags(
    conn: Any,
    *,
    twitch_login: str | None = None,
    twitch_user_id: str | None = None,
    silent_ban: Any = _UNSET,
    silent_raid: Any = _UNSET,
) -> bool:
    active_row = load_active_partner(
        conn,
        twitch_login=twitch_login,
        twitch_user_id=twitch_user_id,
    )
    if not active_row:
        return False
    new_silent_ban = _bool_int(
        _row_value(active_row, "silent_ban", 14, 0) if silent_ban is _UNSET else silent_ban,
        default=0,
    )
    new_silent_raid = _bool_int(
        _row_value(active_row, "silent_raid", 15, 0) if silent_raid is _UNSET else silent_raid,
        default=0,
    )
    conn.execute(
        """
        UPDATE twitch_partners
        SET silent_ban = ?,
            silent_raid = ?
        WHERE id = ?
        """,
        (new_silent_ban, new_silent_raid, _row_value(active_row, "id", 0)),
    )
    return True


def set_partner_live_ping_settings(
    conn: Any,
    *,
    twitch_login: str | None = None,
    twitch_user_id: str | None = None,
    live_ping_role_id: Any = _UNSET,
    live_ping_enabled: Any = _UNSET,
) -> bool:
    active_row = load_active_partner(
        conn,
        twitch_login=twitch_login,
        twitch_user_id=twitch_user_id,
    )
    if not active_row:
        return False
    new_live_ping_role_id = (
        _row_value(active_row, "live_ping_role_id", 16, None)
        if live_ping_role_id is _UNSET
        else live_ping_role_id
    )
    new_live_ping_enabled = _bool_int(
        _row_value(active_row, "live_ping_enabled", 17, 1)
        if live_ping_enabled is _UNSET
        else live_ping_enabled,
        default=1,
    )
    conn.execute(
        """
        UPDATE twitch_partners
        SET live_ping_role_id = ?,
            live_ping_enabled = ?
        WHERE id = ?
        """,
        (new_live_ping_role_id, new_live_ping_enabled, _row_value(active_row, "id", 0)),
    )
    return True


def bulk_update_partner_flags(
    conn: Any,
    *,
    scope: str,
    raid_bot_enabled: Any = _UNSET,
    live_ping_enabled: Any = _UNSET,
    silent_ban: Any = _UNSET,
    silent_raid: Any = _UNSET,
) -> int:
    normalized_scope = str(scope or PARTNER_STATUS_ACTIVE).strip().lower() or PARTNER_STATUS_ACTIVE
    if normalized_scope != PARTNER_STATUS_ACTIVE:
        normalized_scope = PARTNER_STATUS_ACTIVE
    status_filter = "WHERE status = ?"
    params: list[Any] = [PARTNER_STATUS_ACTIVE]

    row = conn.execute(
        f"SELECT COUNT(*) AS total FROM twitch_partners {status_filter}",
        tuple(params),
    ).fetchone()
    total = int(_row_value(row, "total", 0, 0) or 0)
    assignments: list[str] = []
    update_params: list[Any] = []
    if raid_bot_enabled is not _UNSET:
        assignments.append("raid_bot_enabled = ?")
        update_params.append(_bool_int(raid_bot_enabled, default=0))
    if live_ping_enabled is not _UNSET:
        assignments.append("live_ping_enabled = ?")
        update_params.append(_bool_int(live_ping_enabled, default=1))
    if silent_ban is not _UNSET:
        assignments.append("silent_ban = ?")
        update_params.append(_bool_int(silent_ban, default=0))
    if silent_raid is not _UNSET:
        assignments.append("silent_raid = ?")
        update_params.append(_bool_int(silent_raid, default=0))
    if not assignments:
        return total
    conn.execute(
        f"""
        UPDATE twitch_partners
        SET {", ".join(assignments)}
        {status_filter}
        """,
        tuple(update_params + params),
    )
    return total


def migrate_legacy_partner_registry(conn: Any) -> dict[str, int]:
    stats = {
        "identity_upserts": 0,
        "partner_promotions": 0,
        "partner_archives": 0,
        "source_deletes": 0,
    }
    try:
        rows = conn.execute(
            """
            SELECT
                twitch_login,
                twitch_user_id,
                require_discord_link,
                next_link_check_at,
                discord_user_id,
                discord_display_name,
                is_on_discord,
                manual_verified_permanent,
                manual_verified_until,
                manual_verified_at,
                manual_partner_opt_out,
                created_at,
                archived_at,
                raid_bot_enabled,
                silent_ban,
                silent_raid,
                is_monitored_only,
                live_ping_role_id,
                COALESCE(live_ping_enabled, 1) AS live_ping_enabled
            FROM twitch_streamers
            ORDER BY
                CASE
                    WHEN (
                        (
                            COALESCE(manual_verified_permanent, 0) = 1
                            OR manual_verified_until IS NOT NULL
                            OR manual_verified_at IS NOT NULL
                        )
                        AND COALESCE(manual_partner_opt_out, 0) = 0
                        AND COALESCE(is_monitored_only, 0) = 0
                        AND archived_at IS NULL
                    ) THEN 2
                    WHEN (
                        COALESCE(manual_verified_permanent, 0) = 1
                        OR manual_verified_until IS NOT NULL
                        OR manual_verified_at IS NOT NULL
                    ) THEN 1
                    ELSE 0
                END,
                COALESCE(manual_verified_at, created_at, CURRENT_TIMESTAMP),
                LOWER(twitch_login)
            """
        ).fetchall()
    except Exception:
        return stats

    for row in rows or []:
        login = _normalize_login(_row_value(row, "twitch_login", 0))
        user_id = _normalize_user_id(_row_value(row, "twitch_user_id", 1))
        if not login:
            continue
        if user_id:
            upsert_streamer_identity(
                conn,
                twitch_user_id=user_id,
                twitch_login=login,
                discord_user_id=_row_value(row, "discord_user_id", 4, None),
                discord_display_name=_row_value(row, "discord_display_name", 5, None),
                is_on_discord=_row_value(row, "is_on_discord", 6, 0),
            )
            stats["identity_upserts"] += 1

        if not user_id:
            continue

        manual_verified_permanent = _bool_int(
            _row_value(row, "manual_verified_permanent", 7, 0),
            default=0,
        )
        manual_verified_until = _row_value(row, "manual_verified_until", 8, None)
        manual_verified_at = _row_value(row, "manual_verified_at", 9, None)
        manual_partner_opt_out = _bool_int(
            _row_value(row, "manual_partner_opt_out", 10, 0),
            default=0,
        )
        archived_at = _row_value(row, "archived_at", 12, None)
        is_monitored_only = _bool_int(_row_value(row, "is_monitored_only", 16, 0), default=0)

        has_partner_history = bool(
            manual_verified_permanent or manual_verified_until or manual_verified_at
        )
        if not has_partner_history:
            continue

        is_active = bool(
            has_partner_history
            and not manual_partner_opt_out
            and not is_monitored_only
            and not archived_at
        )
        existing = _load_partner_row(
            conn,
            twitch_login=login,
            twitch_user_id=user_id,
            status=PARTNER_STATUS_ACTIVE if is_active else PARTNER_STATUS_ARCHIVED,
            latest=True,
        )
        if not existing:
            conn.execute(
                """
                INSERT INTO twitch_partners (
                    twitch_user_id,
                    twitch_login,
                    require_discord_link,
                    last_description,
                    last_link_ok,
                    added_by,
                    last_link_checked_at,
                    next_link_check_at,
                    manual_verified_permanent,
                    manual_verified_until,
                    manual_verified_at,
                    manual_partner_opt_out,
                    raid_bot_enabled,
                    silent_ban,
                    silent_raid,
                    live_ping_role_id,
                    live_ping_enabled,
                    partnered_at,
                    departnered_at,
                    status
                ) VALUES (?, ?, ?, NULL, NULL, NULL, NULL, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                """,
                (
                    user_id,
                    login,
                    _bool_int(_row_value(row, "require_discord_link", 2, 0), default=0),
                    _row_value(row, "next_link_check_at", 3, None),
                    manual_verified_permanent,
                    manual_verified_until,
                    manual_verified_at,
                    manual_partner_opt_out,
                    _bool_int(_row_value(row, "raid_bot_enabled", 13, 0), default=0),
                    _bool_int(_row_value(row, "silent_ban", 14, 0), default=0),
                    _bool_int(_row_value(row, "silent_raid", 15, 0), default=0),
                    _row_value(row, "live_ping_role_id", 17, None),
                    _bool_int(_row_value(row, "live_ping_enabled", 18, 1), default=1),
                    _row_value(row, "created_at", 11, None) or manual_verified_at or _now_iso(),
                    None if is_active else (archived_at or _now_iso()),
                    PARTNER_STATUS_ACTIVE if is_active else PARTNER_STATUS_ARCHIVED,
                ),
            )
            if is_active:
                stats["partner_promotions"] += 1
            else:
                stats["partner_archives"] += 1

        if is_active:
            cur = conn.execute(
                """
                DELETE FROM twitch_streamers
                WHERE twitch_user_id = ?
                   OR LOWER(twitch_login) = LOWER(?)
                """,
                (user_id, login),
            )
            deleted = getattr(cur, "rowcount", 0) or 0
            stats["source_deletes"] += int(deleted)
            _normalize_related_tables(
                conn,
                twitch_user_id=user_id,
                twitch_login=login,
            )
            continue

        conn.execute(
            """
            UPDATE twitch_streamers
            SET manual_verified_permanent = 0,
                manual_verified_until = NULL,
                manual_verified_at = NULL,
                manual_partner_opt_out = 0,
                raid_bot_enabled = 0
            WHERE twitch_user_id = ?
               OR LOWER(twitch_login) = LOWER(?)
            """,
            (user_id, login),
        )

    return stats


def verification_payload(mode: str) -> dict[str, Any]:
    normalized_mode = str(mode or "").strip().lower()
    now_iso = _now_iso()
    if normalized_mode == "permanent":
        return {
            "manual_verified_permanent": 1,
            "manual_verified_until": None,
            "manual_verified_at": now_iso,
            "manual_partner_opt_out": 0,
        }
    if normalized_mode == "temp":
        return {
            "manual_verified_permanent": 0,
            "manual_verified_until": (datetime.now(UTC) + timedelta(days=30)).isoformat(),
            "manual_verified_at": now_iso,
            "manual_partner_opt_out": 0,
        }
    if normalized_mode in {"clear", "failed"}:
        return {
            "manual_verified_permanent": 0,
            "manual_verified_until": None,
            "manual_verified_at": None,
            "manual_partner_opt_out": 1,
        }
    raise ValueError("unknown_verification_mode")

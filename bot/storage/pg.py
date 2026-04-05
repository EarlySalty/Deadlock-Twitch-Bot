"""PostgreSQL storage layer for Twitch analytics.

The storage boundary is native psycopg with pooled connections, explicit
transactions, and PostgreSQL-oriented helpers.
"""

from __future__ import annotations

import contextlib
import atexit
import hashlib
import json
import logging
import os
import queue
import threading
import time
from collections.abc import Iterable, Sequence
from urllib.parse import urlsplit

import psycopg
from psycopg.conninfo import conninfo_to_dict

from ._pool import ConnectionPoolRegistry
from ._rows import storage_row_factory
from .partner_registry import (
    bulk_update_partner_flags,
    departner_active_partner,
    archive_active_partner,
    load_active_partner,
    load_latest_partner_history,
    load_offline_auto_raid_eligibility,
    load_partner_by_discord_user_id,
    load_streamer_identity,
    migrate_legacy_partner_registry,
    OfflineAutoRaidEligibility,
    promote_streamer_to_partner,
    reactivate_partner,
    save_streamer_discord_profile,
    set_partner_live_ping_settings,
    set_partner_raid_bot_enabled,
    set_partner_silent_flags,
    set_streamer_archive_state,
    set_streamer_discord_member,
    upsert_non_partner_streamer,
    upsert_streamer_identity,
    verification_payload,
)
from .promo_cooldowns import (
    cleanup_stale_promo_cooldowns,
    load_promo_cooldowns,
    save_promo_cooldown,
)

log = logging.getLogger("TwitchStreams.StoragePG")

__all__ = [
    "prepare_runtime_storage",
    "readonly_connection",
    "transaction",
    "ensure_schema",
    "query_one",
    "query_all",
    "analytics_db_fingerprint",
    "analytics_db_fingerprint_details",
    "backfill_tracked_stats_from_category",
    "insert_observability_event",
    "delete_streamer",
    "bulk_update_partner_flags",
    "departner_active_partner",
    "archive_active_partner",
    "load_active_partner",
    "load_latest_partner_history",
    "load_offline_auto_raid_eligibility",
    "OfflineAutoRaidEligibility",
    "load_partner_by_discord_user_id",
    "load_streamer_identity",
    "promote_streamer_to_partner",
    "reactivate_partner",
    "save_streamer_discord_profile",
    "set_partner_live_ping_settings",
    "set_partner_raid_bot_enabled",
    "set_partner_silent_flags",
    "set_streamer_archive_state",
    "set_streamer_discord_member",
    "upsert_non_partner_streamer",
    "upsert_streamer_identity",
    "verification_payload",
    "save_promo_cooldown",
    "load_promo_cooldowns",
    "cleanup_stale_promo_cooldowns",
]

KEYRING_SERVICE = "DeadlockBot"
ENV_DSN = "TWITCH_ANALYTICS_DSN"
_DB_FINGERPRINT_SALT = b"deadlock.analytics-db-fingerprint.v1"
_DB_FINGERPRINT_ITERATIONS = 100_000
_RUNTIME_SCHEMA_COMPONENT = "storage_pg"
_RUNTIME_SCHEMA_VERSION = 3
_RUNTIME_SCHEMA_BOOTSTRAP_ENV = "TWITCH_ALLOW_RUNTIME_SCHEMA_BOOTSTRAP"


def _env_int(name: str, default: int, *, minimum: int) -> int:
    raw = str(os.getenv(name) or "").strip()
    if not raw:
        return default
    try:
        return max(minimum, int(raw))
    except ValueError:
        log.warning("Invalid %s=%r; using default %s", name, raw, default)
        return default


def _env_float(name: str, default: float, *, minimum: float) -> float:
    raw = str(os.getenv(name) or "").strip()
    if not raw:
        return default
    try:
        return max(minimum, float(raw))
    except ValueError:
        log.warning("Invalid %s=%r; using default %s", name, raw, default)
        return default


_CONNECTION_POOL_MAXSIZE = _env_int(
    "TWITCH_ANALYTICS_POOL_MAXSIZE",
    12,
    minimum=1,
)
_CONNECTION_POOL_TIMEOUT_SECONDS = _env_float(
    "TWITCH_ANALYTICS_POOL_TIMEOUT_SECONDS",
    5.0,
    minimum=0.1,
)


_OBSERVABILITY_QUEUE_MAXSIZE = _env_int(
    "TWITCH_OBSERVABILITY_QUEUE_MAXSIZE",
    5000,
    minimum=100,
)
_OBSERVABILITY_BATCH_SIZE = _env_int(
    "TWITCH_OBSERVABILITY_BATCH_SIZE",
    50,
    minimum=1,
)
_OBSERVABILITY_FLUSH_INTERVAL_SECONDS = _env_float(
    "TWITCH_OBSERVABILITY_FLUSH_INTERVAL_SECONDS",
    0.5,
    minimum=0.05,
)
_OBSERVABILITY_INSERT_SQL = """
    INSERT INTO twitch_observability_events (
        flow_type,
        flow_id,
        entity_login,
        entity_id,
        step,
        decision,
        details_json
    ) VALUES (%s, %s, %s, %s, %s, %s, %s)
"""
_OBSERVABILITY_STOP = object()
_observability_event_queue: queue.Queue[object] | None = None
_observability_writer_lock = threading.Lock()
_observability_writer_thread: threading.Thread | None = None
_observability_writer_stop = threading.Event()
_observability_dropped_events = 0
_observability_drop_log_ts = 0.0


def _safe_observability_text(value: object, *, limit: int = 200) -> str | None:
    text = str(value or "").replace("\r", " ").replace("\n", " ").strip()
    if not text:
        return None
    return text[:limit]


def _normalize_conninfo_value(value: object) -> str:
    return str(value or "").strip()


def _dsn_conninfo(dsn: str | None = None) -> dict[str, str]:
    raw_dsn = (dsn or _load_dsn()).strip()
    if not raw_dsn:
        return {}
    try:
        parsed = conninfo_to_dict(raw_dsn)
        return {
            str(key): _normalize_conninfo_value(value)
            for key, value in parsed.items()
            if _normalize_conninfo_value(value)
        }
    except Exception:
        pass

    # Fallback for URI-style DSNs if psycopg cannot parse the string.
    try:
        parts = urlsplit(raw_dsn if "://" in raw_dsn else f"postgresql://{raw_dsn}")
    except Exception:
        return {}

    info: dict[str, str] = {}
    if parts.hostname:
        info["host"] = str(parts.hostname).strip()
    if parts.port is not None:
        info["port"] = str(parts.port).strip()
    if parts.username:
        info["user"] = str(parts.username).strip()
    dbname = str(parts.path or "").strip().lstrip("/")
    if dbname:
        info["dbname"] = dbname
    return info


def _analytics_db_identity_fields(dsn: str | None = None) -> tuple[str, str, str]:
    """Return the non-secret DB identity fields used for stable fingerprints."""
    info = _dsn_conninfo(dsn)
    host = (info.get("host") or "").strip().lower()
    dbname = (info.get("dbname") or info.get("database") or "").strip().lower()
    port = (info.get("port") or "").strip().lower()
    return host, port, dbname


def _fingerprint_hex(value: str) -> str:
    """Derive a short stable digest without relying on raw fast hashes."""
    return hashlib.pbkdf2_hmac(
        "sha256",
        value.encode("utf-8", errors="ignore"),
        _DB_FINGERPRINT_SALT,
        _DB_FINGERPRINT_ITERATIONS,
        dklen=6,
    ).hex()


def analytics_db_fingerprint(dsn: str | None = None) -> str:
    """Return a stable, non-secret fingerprint for the configured analytics DB."""
    host, port, dbname = _analytics_db_identity_fields(dsn)
    return f"pg:{_fingerprint_hex(f'{host}|{port}|{dbname}')}"


def analytics_db_fingerprint_details(dsn: str | None = None) -> dict[str, str]:
    """Expose hashed DB identity details safe enough for logs and health endpoints."""
    host, port, dbname = _analytics_db_identity_fields(dsn)

    return {
        "fingerprint": analytics_db_fingerprint(dsn),
        "hostHash": _fingerprint_hex(host or "-"),
        "databaseHash": _fingerprint_hex(dbname or "-"),
        "portHash": _fingerprint_hex(port or "-"),
        "engine": "postgres",
    }


def _db_cache_key(dsn: str | None = None) -> str:
    return analytics_db_fingerprint(dsn)


def _align_serial_sequence(conn: psycopg.Connection, table: str, column: str) -> None:
    """
    Ensure the backing sequence for a SERIAL/IDENTITY column is ahead of existing rows.
    Prevents duplicate key errors after migrations or manual imports.
    """
    try:
        row = _execute_with_savepoint(
            conn,
            "SELECT pg_get_serial_sequence(%s, %s)", (table, column)
        ).fetchone()
        seq_name = row[0] if row else None
        if not seq_name:
            return
        _execute_with_savepoint(
            conn,
            f"SELECT setval(%s, COALESCE((SELECT MAX({column}) FROM {table}), 0), true)",
            (seq_name,),
        )
    except Exception as exc:  # pragma: no cover - best effort guard
        log.debug("Could not align serial sequence for %s.%s: %s", table, column, exc)


def _coerce_column_to_boolean(
    conn: psycopg.Connection,
    table: str,
    column: str,
    *,
    default: bool = False,
) -> None:
    """
    Best-effort migration for legacy integer/text flags that are now modeled as BOOLEAN.
    Safe to call repeatedly on startup.
    """
    try:
        row = _execute_with_savepoint(
            conn,
            "SELECT data_type FROM information_schema.columns "
            "WHERE table_schema = current_schema() AND table_name = %s AND column_name = %s",
            (table, column),
        ).fetchone()
    except Exception as exc:  # pragma: no cover - best effort guard
        log.debug("Could not inspect column type for %s.%s: %s", table, column, exc)
        return

    if not row:
        return

    value = row[0] if not hasattr(row, "keys") else row["data_type"]
    normalized = str(value or "").strip().lower()
    if not normalized:
        return

    if normalized != "boolean":
        try:
            _execute_with_savepoint(
                conn,
                f"""
                ALTER TABLE {table}
                ALTER COLUMN {column} TYPE BOOLEAN
                USING CASE
                    WHEN {column} IS NULL THEN FALSE
                    WHEN LOWER(BTRIM({column}::text)) IN ('1', 'true', 't', 'yes', 'y', 'on') THEN TRUE
                    ELSE FALSE
                END
                """
            )
            log.info("DB migration: converted %s.%s to BOOLEAN", table, column)
        except Exception as exc:  # pragma: no cover - best effort guard
            log.warning(
                "DB migration: could not convert %s.%s to BOOLEAN: %s",
                table,
                column,
                exc,
            )
            return

    try:
        _execute_with_savepoint(
            conn,
            f"ALTER TABLE {table} ALTER COLUMN {column} SET DEFAULT {'TRUE' if default else 'FALSE'}"
        )
    except Exception as exc:  # pragma: no cover - best effort guard
        log.debug("Skipping default migration on %s.%s: %s", table, column, exc)


def _table_exists(conn: psycopg.Connection, table: str) -> bool:
    """Return True when the table exists in the current schema."""
    try:
        row = _execute_with_savepoint(
            conn,
            """
            SELECT 1
            FROM information_schema.tables
            WHERE table_schema = current_schema()
              AND table_name = %s
            """,
            (table,),
        ).fetchone()
        return bool(row)
    except Exception as exc:  # pragma: no cover - best effort guard
        log.debug("Could not inspect table %s: %s", table, exc)
        return False


def _index_definition(conn: psycopg.Connection, index_name: str) -> str | None:
    """Return the CREATE INDEX statement for an index in the current schema."""
    try:
        row = _execute_with_savepoint(
            conn,
            """
            SELECT indexdef
            FROM pg_indexes
            WHERE schemaname = current_schema()
              AND indexname = %s
            """,
            (index_name,),
        ).fetchone()
    except Exception as exc:  # pragma: no cover - best effort guard
        log.debug("Could not inspect index %s: %s", index_name, exc)
        return None

    if not row:
        return None

    value = row[0] if not hasattr(row, "keys") else row["indexdef"]
    normalized = str(value or "").strip()
    return normalized or None


def _cleanup_duplicate_global_social_media_auth_rows(conn: psycopg.Connection) -> None:
    """Keep only the newest enabled global auth row per platform."""
    if not _table_exists(conn, "social_media_platform_auth"):
        return

    duplicates = conn.execute(
        """
        SELECT platform, COUNT(*) AS row_count
        FROM social_media_platform_auth
        WHERE streamer_login IS NULL
        GROUP BY platform
        HAVING COUNT(*) > 1
        """
    ).fetchall()
    if not duplicates:
        return

    conn.execute(
        """
        WITH ranked AS (
            SELECT id,
                   ROW_NUMBER() OVER (
                       PARTITION BY platform
                       ORDER BY enabled DESC,
                                COALESCE(last_refreshed_at, authorized_at, '') DESC,
                                id DESC
                   ) AS rn
            FROM social_media_platform_auth
            WHERE streamer_login IS NULL
        )
        DELETE FROM social_media_platform_auth
        WHERE id IN (SELECT id FROM ranked WHERE rn > 1)
        """
    )
    log.warning(
        "Removed duplicate global social_media_platform_auth rows for platforms=%s",
        ",".join(
            sorted(
                str(row[0] if not hasattr(row, "keys") else row["platform"])
                for row in duplicates
            )
        ),
    )


def _ensure_social_media_auth_indexes(conn: psycopg.Connection) -> None:
    """Enforce correct uniqueness for streamer-specific and global social auth rows."""
    if not _table_exists(conn, "social_media_platform_auth"):
        return

    _cleanup_duplicate_global_social_media_auth_rows(conn)
    _execute_with_savepoint(
        conn,
        """
        CREATE UNIQUE INDEX IF NOT EXISTS idx_social_platform_auth_streamer_unique
            ON social_media_platform_auth(platform, streamer_login)
         WHERE streamer_login IS NOT NULL
        """
    )
    _execute_with_savepoint(
        conn,
        """
        CREATE UNIQUE INDEX IF NOT EXISTS idx_social_platform_auth_global_unique
            ON social_media_platform_auth(platform)
         WHERE streamer_login IS NULL
        """
    )


def _ensure_twitch_raid_auth_login_index(conn: psycopg.Connection) -> None:
    """Align runtime bootstrap with the case-insensitive login uniqueness migration."""
    if not _table_exists(conn, "twitch_raid_auth"):
        return

    indexdef = (_index_definition(conn, "idx_twitch_raid_auth_login") or "").lower()
    if "lower(" in indexdef and "twitch_login" in indexdef:
        return

    if indexdef:
        _execute_with_savepoint(conn, "DROP INDEX IF EXISTS idx_twitch_raid_auth_login")

    try:
        _execute_with_savepoint(
            conn,
            """
            CREATE UNIQUE INDEX IF NOT EXISTS idx_twitch_raid_auth_login
                ON twitch_raid_auth (LOWER(twitch_login))
            """
        )
    except (
        psycopg.errors.UniqueViolation
    ) as exc:  # pragma: no cover - best effort guard
        log.warning(
            "Could not enforce case-insensitive twitch_raid_auth login uniqueness: %s",
            exc,
        )


def _run_startup_maintenance(
    conn: psycopg.Connection, *, dsn: str | None = None
) -> None:
    """
    One-time runtime maintenance for existing schemas.
    Keeps known SERIAL sequences aligned even when ensure_schema() is skipped
    (for example when a migration-managed schema_version table exists).
    """
    cache_key = _db_cache_key(dsn)
    done_for = set(getattr(_run_startup_maintenance, "_done_for", set()))
    if cache_key in done_for:
        return

    def _run_best_effort(step_name: str, func, *args, **kwargs) -> None:
        if not hasattr(conn, "execute"):
            func(conn, *args, **kwargs)
            return
        savepoint = f"maintenance_guard_{time.monotonic_ns()}"
        conn.execute(f"SAVEPOINT {savepoint}")
        try:
            func(conn, *args, **kwargs)
        except Exception as exc:  # pragma: no cover - best effort guard
            with contextlib.suppress(Exception):
                conn.execute(f"ROLLBACK TO SAVEPOINT {savepoint}")
            log.debug("Skipping %s: %s", step_name, exc)
        finally:
            with contextlib.suppress(Exception):
                conn.execute(f"RELEASE SAVEPOINT {savepoint}")

    # Keep this list focused on tables where stale sequences have caused issues.
    _run_best_effort(
        "twitch_stream_sessions serial alignment",
        _align_serial_sequence,
        "twitch_stream_sessions",
        "id",
    )
    _run_best_effort(
        "twitch_raid_history serial alignment",
        _align_serial_sequence,
        "twitch_raid_history",
        "id",
    )
    _run_best_effort(
        "clip_fetch_history serial alignment",
        _align_serial_sequence,
        "clip_fetch_history",
        "id",
    )
    _run_best_effort(
        "twitch_clips_social_media serial alignment",
        _align_serial_sequence,
        "twitch_clips_social_media",
        "id",
    )
    _run_best_effort(
        "twitch_session_chatters.is_first_time_streamer boolean coercion",
        _coerce_column_to_boolean,
        "twitch_session_chatters",
        "is_first_time_streamer",
        default=False,
    )
    _run_best_effort(
        "twitch_session_chatters.seen_via_chatters_api boolean coercion",
        _coerce_column_to_boolean,
        "twitch_session_chatters",
        "seen_via_chatters_api",
        default=False,
    )
    _run_best_effort(
        "twitch_chat_messages.is_command boolean coercion",
        _coerce_column_to_boolean,
        "twitch_chat_messages",
        "is_command",
        default=False,
    )
    _run_best_effort(
        "twitch_live_state duplicate cleanup",
        _cleanup_duplicate_live_state_rows,
    )
    _run_best_effort(
        "twitch_live_state login uniqueness index",
        _ensure_unique_live_state_login_index,
    )
    _run_best_effort(
        "twitch_raid_auth login index maintenance",
        _ensure_twitch_raid_auth_login_index,
    )
    _run_best_effort(
        "social_media_platform_auth index maintenance",
        _ensure_social_media_auth_indexes,
    )

    done_for.add(cache_key)
    _run_startup_maintenance._done_for = done_for


def _cleanup_duplicate_live_state_rows(conn: psycopg.Connection) -> None:
    """Remove legacy live-state rows where login was incorrectly stored as user id."""
    conn.execute(
        """
        DELETE FROM twitch_live_state legacy
        USING twitch_live_state canonical
        WHERE LOWER(COALESCE(legacy.twitch_user_id, '')) = LOWER(COALESCE(legacy.streamer_login, ''))
          AND LOWER(canonical.streamer_login) = LOWER(legacy.streamer_login)
          AND LOWER(COALESCE(canonical.twitch_user_id, '')) <> LOWER(COALESCE(legacy.streamer_login, ''))
        """
    )


def _ensure_unique_live_state_login_index(conn: psycopg.Connection) -> None:
    """Enforce one live-state row per streamer login."""
    conn.execute(
        """
        CREATE UNIQUE INDEX IF NOT EXISTS idx_twitch_live_state_login_lower
            ON twitch_live_state(LOWER(streamer_login))
         WHERE streamer_login IS NOT NULL AND streamer_login <> ''
        """
    )


def _load_dsn() -> str:
    env_dsn = (os.getenv(ENV_DSN) or "").strip()
    if env_dsn:
        return env_dsn
    try:
        import keyring  # type: ignore

        val = keyring.get_password(KEYRING_SERVICE, ENV_DSN) or keyring.get_password(
            f"{ENV_DSN}@{KEYRING_SERVICE}", ENV_DSN
        )
        if val:
            return val
    except Exception as exc:  # pragma: no cover - best-effort Tresor lookup
        log.debug("Keyring lookup failed: %s", exc)
    raise RuntimeError(
        f"{ENV_DSN} not set (env or Windows Credential Manager '{KEYRING_SERVICE}')"
    )


def _execute_with_savepoint(conn, sql: str, params=None):
    if getattr(conn, "autocommit", False):
        return conn.execute(sql, params or ())

    savepoint = f"ddl_guard_{time.monotonic_ns()}"
    conn.execute(f"SAVEPOINT {savepoint}")
    try:
        result = conn.execute(sql, params or ())
    except Exception:
        with contextlib.suppress(Exception):
            conn.execute(f"ROLLBACK TO SAVEPOINT {savepoint}")
        with contextlib.suppress(Exception):
            conn.execute(f"RELEASE SAVEPOINT {savepoint}")
        raise
    conn.execute(f"RELEASE SAVEPOINT {savepoint}")
    return result


def _connect_raw_connection(dsn: str, autocommit: bool) -> psycopg.Connection:
    return psycopg.connect(dsn, row_factory=storage_row_factory, autocommit=autocommit)


def _mark_schema_ready(cache_key: str) -> None:
    schema_ok_for = set(getattr(_ensure_storage_bootstrap, "_schema_ok_for", set()))
    schema_ok_for.add(cache_key)
    _ensure_storage_bootstrap._schema_ok_for = schema_ok_for


def _mark_runtime_storage_ready(cache_key: str) -> None:
    ready_for = set(getattr(_require_runtime_storage_ready, "_ready_for", set()))
    ready_for.add(cache_key)
    _require_runtime_storage_ready._ready_for = ready_for


def _runtime_schema_bootstrap_allowed() -> bool:
    raw = str(os.getenv(_RUNTIME_SCHEMA_BOOTSTRAP_ENV) or "").strip().lower()
    if not raw:
        return True
    return raw not in {"0", "false", "no", "off"}


def _schema_version_table_exists(conn: psycopg.Connection) -> bool:
    row = conn.execute(
        """
        SELECT 1
          FROM information_schema.tables
         WHERE table_schema = 'public'
           AND table_name = 'schema_version'
        """
    ).fetchone()
    return bool(row)


def _schema_version_columns(conn: psycopg.Connection) -> set[str]:
    rows = conn.execute(
        """
        SELECT column_name
          FROM information_schema.columns
         WHERE table_schema = 'public'
           AND table_name = 'schema_version'
        """
    ).fetchall()
    columns: set[str] = set()
    for row in rows or []:
        if hasattr(row, "get"):
            value = row.get("column_name")
        else:
            value = row[0] if row else None
        column_name = str(value or "").strip().lower()
        if column_name:
            columns.add(column_name)
    return columns


def _load_runtime_schema_version(conn: psycopg.Connection) -> int | None:
    if not _schema_version_table_exists(conn):
        return None

    columns = _schema_version_columns(conn)
    if not {"component", "version"}.issubset(columns):
        log.info(
            "schema_version table is externally managed; missing runtime columns %s",
            sorted({"component", "version"} - columns),
        )
        return _RUNTIME_SCHEMA_VERSION

    row = conn.execute(
        """
        SELECT version
          FROM schema_version
         WHERE component = %s
         LIMIT 1
        """,
        (_RUNTIME_SCHEMA_COMPONENT,),
    ).fetchone()

    if not row:
        return 0
    try:
        return int(row[0])
    except (TypeError, ValueError, IndexError):
        log.warning("Invalid schema_version row for %s: %r", _RUNTIME_SCHEMA_COMPONENT, row)
        return 0


def _record_runtime_schema_version(conn: psycopg.Connection, version: int) -> None:
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS schema_version (
            component TEXT PRIMARY KEY,
            version INTEGER NOT NULL,
            updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )
        """
    )
    conn.execute(
        """
        INSERT INTO schema_version (component, version, updated_at)
        VALUES (%s, %s, now())
        ON CONFLICT (component)
        DO UPDATE SET
            version = EXCLUDED.version,
            updated_at = now()
        """,
        (_RUNTIME_SCHEMA_COMPONENT, int(version)),
    )


def _apply_runtime_schema_migrations(
    conn: psycopg.Connection, *, current_version: int | None
) -> int:
    version = int(current_version or 0)
    if version < 1:
        if not _runtime_schema_bootstrap_allowed():
            raise RuntimeError(
                "Runtime schema bootstrap is disabled. Apply the PostgreSQL migrations before startup "
                f"or set {_RUNTIME_SCHEMA_BOOTSTRAP_ENV}=1 only for controlled local bootstrap."
            )
        ensure_schema(conn)
        _record_runtime_schema_version(conn, 1)
        version = 1
    if version < 2:
        if not _runtime_schema_bootstrap_allowed():
            raise RuntimeError(
                "Runtime schema bootstrap is disabled. Apply the PostgreSQL migrations before startup "
                f"or set {_RUNTIME_SCHEMA_BOOTSTRAP_ENV}=1 only for controlled local bootstrap."
            )
        ensure_schema(conn)
        _record_runtime_schema_version(conn, 2)
        version = 2
    if version < 3:
        if not _runtime_schema_bootstrap_allowed():
            raise RuntimeError(
                "Runtime schema bootstrap is disabled. Apply the PostgreSQL migrations before startup "
                f"or set {_RUNTIME_SCHEMA_BOOTSTRAP_ENV}=1 only for controlled local bootstrap."
            )
        ensure_schema(conn)
        _record_runtime_schema_version(conn, 3)
        version = 3
    return version


def _require_runtime_storage_ready(dsn: str) -> None:
    cache_key = _db_cache_key(dsn)
    ready_for = set(getattr(_require_runtime_storage_ready, "_ready_for", set()))
    if cache_key in ready_for:
        return
    raise RuntimeError(
        "PostgreSQL storage is not initialized. Call prepare_runtime_storage() during startup "
        "before serving runtime requests."
    )


def _ensure_storage_bootstrap(conn: psycopg.Connection, *, dsn: str) -> None:
    cache_key = _db_cache_key(dsn)
    schema_ok_for = set(getattr(_ensure_storage_bootstrap, "_schema_ok_for", set()))
    if cache_key in schema_ok_for:
        return

    current_version = _load_runtime_schema_version(conn)
    if current_version is not None and current_version >= _RUNTIME_SCHEMA_VERSION:
        _mark_schema_ready(cache_key)
        return

    try:
        _apply_runtime_schema_migrations(conn, current_version=current_version)
        _mark_schema_ready(cache_key)
    except Exception as exc:  # pragma: no cover - best effort
        log.warning("Schema initialization failed: %s", exc, exc_info=True)
        raise


def _prepare_postgres_connection(conn: psycopg.Connection, *, dsn: str) -> None:
    _ensure_storage_bootstrap(conn, dsn=dsn)
    _run_startup_maintenance(conn, dsn=dsn)


def prepare_runtime_storage() -> None:
    """Run explicit storage bootstrap before runtime traffic is served."""
    dsn = _load_dsn()
    cache_key = _db_cache_key(dsn)
    ready_for = set(getattr(_require_runtime_storage_ready, "_ready_for", set()))
    if cache_key in ready_for:
        return

    registry = _connection_pool_registry()
    conn = registry.get_pool(dsn).open_dedicated(autocommit=False)
    try:
        _prepare_postgres_connection(conn, dsn=dsn)
        conn.commit()
        _mark_runtime_storage_ready(cache_key)
    except Exception:
        with contextlib.suppress(Exception):
            conn.rollback()
        raise
    finally:
        conn.close()


def _connection_pool_registry() -> ConnectionPoolRegistry:
    registry = getattr(_connection_pool_registry, "_registry", None)
    if registry is None:
        registry = ConnectionPoolRegistry(
            max_size=_CONNECTION_POOL_MAXSIZE,
            checkout_timeout=_CONNECTION_POOL_TIMEOUT_SECONDS,
            connect_fn=_connect_raw_connection,
        )
        _connection_pool_registry._registry = registry
        atexit.register(registry.close_all)
    return registry


def _reset_connection_pools() -> None:
    registry = getattr(_connection_pool_registry, "_registry", None)
    if registry is not None:
        registry.close_all()
        delattr(_connection_pool_registry, "_registry")
    if hasattr(_require_runtime_storage_ready, "_ready_for"):
        delattr(_require_runtime_storage_ready, "_ready_for")


@contextlib.contextmanager
def readonly_connection():
    """Yield a pooled raw PostgreSQL connection for PostgreSQL-first reads."""
    dsn = _load_dsn()
    _require_runtime_storage_ready(dsn)
    pool = _connection_pool_registry().get_pool(dsn)
    with pool.connection(autocommit=True) as conn:
        yield conn


@contextlib.contextmanager
def transaction():
    """Yield a pooled raw PostgreSQL connection and commit or rollback explicitly."""
    dsn = _load_dsn()
    _require_runtime_storage_ready(dsn)
    pool = _connection_pool_registry().get_pool(dsn)
    with pool.connection(autocommit=False) as conn:
        try:
            yield conn
        except Exception:
            with contextlib.suppress(Exception):
                conn.rollback()
            raise
        else:
            conn.commit()


def execute(sql: str, params: Iterable | None = None):
    with transaction() as conn:
        return conn.execute(sql, params or [])


def query_one(sql: str, params: Iterable | None = None):
    with readonly_connection() as conn:
        return conn.execute(sql, params or []).fetchone()


def query_all(sql: str, params: Iterable | None = None):
    with readonly_connection() as conn:
        return conn.execute(sql, params or []).fetchall()


def _observability_queue_instance() -> queue.Queue[object]:
    global _observability_event_queue
    if _observability_event_queue is None:
        _observability_event_queue = queue.Queue(maxsize=_OBSERVABILITY_QUEUE_MAXSIZE)
    return _observability_event_queue


def _ensure_observability_schema(
    conn: psycopg.Connection, *, dsn: str | None = None
) -> None:
    cache_key = _db_cache_key(dsn)
    schema_ok_for = set(getattr(_ensure_storage_bootstrap, "_schema_ok_for", set()))
    if cache_key in schema_ok_for:
        return

    try:
        current_version = _load_runtime_schema_version(conn)
    except Exception as exc:  # pragma: no cover - writer should not mask bootstrap problems
        log.warning(
            "schema_version lookup failed for observability writer: %s",
            exc,
            exc_info=True,
        )
        return

    if current_version is not None and current_version >= _RUNTIME_SCHEMA_VERSION:
        _mark_schema_ready(cache_key)
        return

    try:
        _apply_runtime_schema_migrations(conn, current_version=current_version)
        _mark_schema_ready(cache_key)
    except Exception as exc:  # pragma: no cover - best effort
        log.warning(
            "Schema initialization failed in observability writer: %s",
            exc,
            exc_info=True,
        )


def _open_observability_writer_connection() -> psycopg.Connection:
    dsn = _load_dsn()
    _require_runtime_storage_ready(dsn)
    raw_conn = (
        _connection_pool_registry().get_pool(dsn).open_dedicated(autocommit=False)
    )
    try:
        return raw_conn
    except Exception:
        raw_conn.close()
        raise


def _flush_observability_batch(
    conn: psycopg.Connection | None,
    batch: list[tuple[str, str, str | None, str | None, str, str, str]],
) -> psycopg.Connection | None:
    if not batch:
        return conn

    active_conn = conn
    try:
        if active_conn is None or getattr(active_conn, "closed", False):
            active_conn = _open_observability_writer_connection()
        with active_conn.cursor() as cursor:
            cursor.executemany(_OBSERVABILITY_INSERT_SQL, batch)
        active_conn.commit()
        return active_conn
    except Exception:
        log.debug(
            "Could not persist observability batch size=%s",
            len(batch),
            exc_info=True,
        )
        if active_conn is not None:
            try:
                active_conn.rollback()
            except Exception:
                pass
            try:
                active_conn.close()
            except Exception:
                pass
        return None


def _observability_writer_loop() -> None:
    conn: psycopg.Connection | None = None
    batch: list[tuple[str, str, str | None, str | None, str, str, str]] = []

    while True:
        stop_requested = False
        try:
            item = _observability_queue_instance().get(
                timeout=_OBSERVABILITY_FLUSH_INTERVAL_SECONDS
            )
        except queue.Empty:
            item = None

        if item is _OBSERVABILITY_STOP:
            stop_requested = True
        elif item is not None:
            batch.append(item)

        while len(batch) < _OBSERVABILITY_BATCH_SIZE:
            try:
                queued_item = _observability_queue_instance().get_nowait()
            except queue.Empty:
                break
            if queued_item is _OBSERVABILITY_STOP:
                stop_requested = True
                break
            batch.append(queued_item)

        if batch and (
            stop_requested or len(batch) >= _OBSERVABILITY_BATCH_SIZE or item is None
        ):
            conn = _flush_observability_batch(conn, batch)
            batch = []

        if stop_requested or (
            _observability_writer_stop.is_set()
            and not batch
            and _observability_queue_instance().empty()
        ):
            break

    if batch:
        conn = _flush_observability_batch(conn, batch)
    if conn is not None:
        try:
            conn.close()
        except Exception:
            pass


def _shutdown_observability_writer() -> None:
    global _observability_writer_thread

    with _observability_writer_lock:
        thread = _observability_writer_thread
        if thread is None:
            return
        _observability_writer_stop.set()
        try:
            _observability_queue_instance().put_nowait(_OBSERVABILITY_STOP)
        except queue.Full:
            pass

    thread.join(timeout=max(10.0, _OBSERVABILITY_FLUSH_INTERVAL_SECONDS * 20.0))
    with _observability_writer_lock:
        if _observability_writer_thread is thread and not thread.is_alive():
            _observability_writer_thread = None
    if thread.is_alive():
        log.warning("Observability writer did not stop cleanly before shutdown.")


def _ensure_observability_writer_started() -> None:
    global _observability_writer_thread

    with _observability_writer_lock:
        thread = _observability_writer_thread
        if thread is not None and thread.is_alive():
            return
        _observability_writer_stop.clear()
        thread = threading.Thread(
            target=_observability_writer_loop,
            name="TwitchObservabilityWriter",
            daemon=True,
        )
        thread.start()
        _observability_writer_thread = thread
        atexit.unregister(_shutdown_observability_writer)
        atexit.register(_shutdown_observability_writer)


def _enqueue_observability_event(
    record: tuple[str, str, str | None, str | None, str, str, str],
) -> None:
    global _observability_dropped_events
    global _observability_drop_log_ts

    _ensure_observability_writer_started()
    try:
        _observability_queue_instance().put_nowait(record)
    except queue.Full:
        _observability_dropped_events += 1
        now = time.time()
        if now - _observability_drop_log_ts >= 60.0:
            _observability_drop_log_ts = now
            log.warning(
                "Observability queue full; dropped events=%s capacity=%s",
                _observability_dropped_events,
                _OBSERVABILITY_QUEUE_MAXSIZE,
            )


def insert_observability_event(
    *,
    flow_type: str,
    flow_id: str,
    step: str,
    decision: str,
    entity_login: str | None = None,
    entity_id: str | None = None,
    details: dict[str, object] | None = None,
) -> None:
    safe_flow_type = _safe_observability_text(flow_type, limit=40)
    safe_flow_id = _safe_observability_text(flow_id, limit=80)
    safe_step = _safe_observability_text(step, limit=80)
    safe_decision = _safe_observability_text(decision, limit=80)
    if not safe_flow_type or not safe_flow_id or not safe_step or not safe_decision:
        return

    # Skip failed events – they are noise for the DB (high volume, low value)
    if safe_decision == "failed":
        return

    safe_login = _safe_observability_text(entity_login, limit=80)
    safe_entity_id = _safe_observability_text(entity_id, limit=80)
    try:
        details_json = json.dumps(details or {}, sort_keys=True, default=str)
    except Exception:
        details_json = "{}"

    _enqueue_observability_event(
        (
            safe_flow_type,
            safe_flow_id,
            safe_login,
            safe_entity_id,
            safe_step,
            safe_decision,
            details_json,
        )
    )


def backfill_tracked_stats_from_category(conn, login: str) -> int:
    """Copy historic category stats into tracked stats for one streamer (idempotent)."""
    normalized = (login or "").strip().lower()
    if not normalized:
        return 0

    cur = conn.execute(
        """
        INSERT INTO twitch_stats_tracked
            (ts_utc, streamer, viewer_count, is_partner, game_name, stream_title, tags)
        SELECT c.ts_utc, c.streamer, c.viewer_count, c.is_partner,
               c.game_name, c.stream_title, c.tags
          FROM twitch_stats_category c
         WHERE LOWER(c.streamer) = %s
           AND NOT EXISTS (
               SELECT 1
                 FROM twitch_stats_tracked t
                WHERE LOWER(t.streamer) = LOWER(c.streamer)
                  AND t.ts_utc = c.ts_utc
           )
        """,
        (normalized,),
    )
    return int(cur.rowcount or 0)


def delete_streamer(conn, login: str) -> int:
    """Delete a streamer and related clip records (manual cascade helper)."""
    normalized = (login or "").strip()
    if not normalized:
        return 0

    # Grandchild tables (depend on clip ids)
    conn.execute(
        """DELETE FROM twitch_clips_social_analytics
           WHERE clip_id IN (
               SELECT id FROM twitch_clips_social_media WHERE streamer_login = %s
           )""",
        (normalized,),
    )
    conn.execute(
        """DELETE FROM twitch_clips_upload_queue
           WHERE clip_id IN (
               SELECT id FROM twitch_clips_social_media WHERE streamer_login = %s
           )""",
        (normalized,),
    )

    # Child tables
    conn.execute(
        "DELETE FROM twitch_clips_social_media WHERE streamer_login = %s", (normalized,)
    )
    conn.execute(
        "DELETE FROM clip_templates_streamer WHERE streamer_login = %s", (normalized,)
    )
    conn.execute(
        "DELETE FROM clip_last_hashtags WHERE streamer_login = %s", (normalized,)
    )
    conn.execute(
        "DELETE FROM clip_fetch_history WHERE streamer_login = %s", (normalized,)
    )

    # The streamer itself
    cur = conn.execute(
        "DELETE FROM twitch_streamers WHERE twitch_login = %s", (normalized,)
    )
    return int(getattr(cur, "rowcount", 0) or 0)


# ---------------------------------------------------------------------------
# Schema bootstrap for Twitch runtime tables
# ---------------------------------------------------------------------------


def _pg_add_col_if_missing(conn, table: str, column: str, col_type: str) -> None:
    conn.execute(f"ALTER TABLE {table} ADD COLUMN IF NOT EXISTS {column} {col_type}")


def _seed_default_templates_pg(conn) -> None:
    existing = conn.execute("SELECT COUNT(*) FROM clip_templates_global").fetchone()[0]
    if existing and int(existing) > 0:
        return
    templates = [
        (
            "Gaming Highlight",
            "Epic {{game}} moment by {{streamer}}! 🎮",
            '["gaming","twitch","{{game}}"]',
            "Gaming",
            "system",
        ),
        (
            "Funny Moment",
            "😂 {{title}} | {{streamer}}",
            '["funny","gaming","twitch"]',
            "Entertainment",
            "system",
        ),
        (
            "Pro Play",
            "Insane {{game}} play by {{streamer}} 🔥",
            '["esports","progaming","{{game}}"]',
            "Competitive",
            "system",
        ),
        (
            "Clutch Moment",
            "CLUTCH! {{title}} 💪",
            '["clutch","gaming","{{game}}"]',
            "Gaming",
            "system",
        ),
        (
            "Fails & Funnies",
            "This didn't go as planned 😅 | {{streamer}}",
            '["fail","funny","gaming"]',
            "Entertainment",
            "system",
        ),
    ]
    conn.execute(
        """
        INSERT INTO clip_templates_global (template_name, description_template, hashtags, category, created_by)
        VALUES (%s, %s, %s, %s, %s)
        ON CONFLICT (template_name) DO NOTHING
        """,
        templates[0],
    )
    for t in templates[1:]:
        conn.execute(
            """
            INSERT INTO clip_templates_global (template_name, description_template, hashtags, category, created_by)
            VALUES (%s, %s, %s, %s, %s)
            ON CONFLICT (template_name) DO NOTHING
            """,
            t,
        )


def ensure_schema(conn) -> None:
    """Create/update all non-auth Twitch tables in PostgreSQL. Idempotent."""

    def _timescale_compression_enabled(table: str) -> bool:
        """Return True when the table is a Timescale hypertable with compression on."""
        try:
            row = conn.execute(
                "SELECT compression_enabled "
                "FROM timescaledb_information.hypertables "
                "WHERE hypertable_name = %s",
                (table,),
            ).fetchone()
            return bool(row and row[0])
        except Exception:
            return False

    def _timescale_dimension_columns(table: str) -> set[str]:
        """Return Timescale dimension columns for a hypertable (lowercase)."""
        try:
            rows = conn.execute(
                "SELECT column_name "
                "FROM timescaledb_information.dimensions "
                "WHERE hypertable_name = %s",
                (table,),
            ).fetchall()
            dims: set[str] = set()
            for row in rows or []:
                col = str(
                    (row[0] if not hasattr(row, "keys") else row["column_name"]) or ""
                ).strip()
                if col:
                    dims.add(col.lower())
            return dims
        except Exception:
            return set()

    def _index_exists(index_name: str) -> bool:
        """Check for an index in the current schema."""
        try:
            row = conn.execute(
                "SELECT 1 FROM pg_indexes WHERE schemaname = current_schema() AND indexname = %s",
                (index_name,),
            ).fetchone()
            return bool(row)
        except Exception:
            return False

    def _table_exists(table: str) -> bool:
        try:
            row = conn.execute(
                """
                SELECT 1
                FROM information_schema.tables
                WHERE table_schema = current_schema()
                  AND table_name = %s
                """,
                (table,),
            ).fetchone()
            return bool(row)
        except Exception:
            return False

    def _constraint_exists(table: str, constraint_name: str) -> bool:
        try:
            row = conn.execute(
                """
                SELECT 1
                FROM information_schema.table_constraints
                WHERE table_schema = current_schema()
                  AND table_name = %s
                  AND constraint_name = %s
                """,
                (table, constraint_name),
            ).fetchone()
            return bool(row)
        except Exception:
            return False

    def _matching_constraint_names(
        table: str,
        columns: Sequence[str],
        constraint_type: str,
    ) -> list[str]:
        try:
            rows = conn.execute(
                """
                SELECT tc.constraint_name
                FROM information_schema.table_constraints tc
                JOIN information_schema.key_column_usage kcu
                  ON tc.constraint_name = kcu.constraint_name
                 AND tc.table_schema = kcu.table_schema
                 AND tc.table_name = kcu.table_name
                WHERE tc.table_schema = current_schema()
                  AND tc.table_name = %s
                  AND tc.constraint_type = %s
                GROUP BY tc.constraint_name
                HAVING array_agg(kcu.column_name ORDER BY kcu.ordinal_position) = %s
                ORDER BY tc.constraint_name
                """,
                (table, constraint_type, list(columns)),
            ).fetchall()
            return [
                str(
                    (row[0] if not hasattr(row, "keys") else row["constraint_name"])
                    or ""
                ).strip()
                for row in rows or []
                if str(
                    (row[0] if not hasattr(row, "keys") else row["constraint_name"])
                    or ""
                ).strip()
            ]
        except Exception as exc:
            log.debug(
                "Could not inspect %s constraints on %s(%s): %s",
                constraint_type,
                table,
                ",".join(columns),
                exc,
            )
            return []

    def _has_key_constraint(
        table: str,
        columns: Sequence[str],
        constraint_types: Sequence[str],
    ) -> bool:
        """
        Return True when a key constraint with the given type matches the provided
        column list exactly (order-sensitive).
        """
        try:
            row = conn.execute(
                """
                SELECT 1
                  FROM information_schema.table_constraints tc
                  JOIN information_schema.key_column_usage kcu
                   ON tc.constraint_name = kcu.constraint_name
                   AND tc.table_schema = kcu.table_schema
                 WHERE tc.table_schema = current_schema()
                   AND tc.table_name = %s
                   AND tc.constraint_type = ANY(%s)
                 GROUP BY tc.constraint_name
                HAVING array_agg(kcu.column_name ORDER BY kcu.ordinal_position) = %s
                 LIMIT 1
                """,
                (table, list(constraint_types), list(columns)),
            ).fetchone()
            return bool(row)
        except Exception as exc:
            log.debug(
                "Could not inspect key constraint on %s(%s): %s",
                table,
                ",".join(columns),
                exc,
            )
            return False

    def _has_unique_constraint(table: str, columns: Sequence[str]) -> bool:
        """
        Return True when there is a PRIMARY KEY or UNIQUE constraint matching the
        provided column list exactly (order-sensitive).
        """
        return _has_key_constraint(table, columns, ["PRIMARY KEY", "UNIQUE"])

    def _drop_constraint_if_exists(table: str, constraint_name: str) -> bool:
        if not _constraint_exists(table, constraint_name):
            return False
        conn.execute(f"ALTER TABLE {table} DROP CONSTRAINT {constraint_name}")
        return True

    def _ensure_unique_constraint_allowing_compressed_hypertable(
        table: str,
        constraint_name: str,
        columns: Sequence[str],
    ) -> bool:
        if _has_unique_constraint(table, columns):
            return True

        compression_was_enabled = _timescale_compression_enabled(table)
        if compression_was_enabled and not _set_timescale_compression(table, False):
            log.warning(
                "Could not add unique constraint %s on %s because compression could not be disabled.",
                constraint_name,
                table,
            )
            return False

        columns_sql = ", ".join(columns)
        try:
            _execute_with_savepoint(
                conn,
                f"ALTER TABLE {table} ADD CONSTRAINT {constraint_name} UNIQUE ({columns_sql})",
            )
        except psycopg.errors.DuplicateObject:
            pass
        except Exception as exc:
            log.warning(
                "Could not add unique constraint %s on %s(%s): %s",
                constraint_name,
                table,
                columns_sql,
                exc,
            )
        finally:
            if compression_was_enabled:
                _set_timescale_compression(table, True)

        return _has_unique_constraint(table, columns)

    def _column_data_type(table: str, column: str) -> str | None:
        """Return the normalized information_schema data_type for a column."""
        try:
            row = conn.execute(
                "SELECT data_type FROM information_schema.columns "
                "WHERE table_schema = current_schema() AND table_name = %s AND column_name = %s",
                (table, column),
            ).fetchone()
            if not row:
                return None
            value = row[0] if not hasattr(row, "keys") else row["data_type"]
            normalized = str(value or "").strip().lower()
            return normalized or None
        except Exception as exc:
            log.debug("Could not inspect column type for %s.%s: %s", table, column, exc)
            return None

    def _decompress_compressed_chunks(table: str) -> bool:
        """Decompress all compressed chunks for a hypertable. Returns success flag."""
        try:
            _execute_with_savepoint(
                conn,
                """
                SELECT decompress_chunk((quote_ident(chunk_schema) || '.' || quote_ident(chunk_name))::regclass)
                FROM timescaledb_information.chunks
                WHERE hypertable_name = %s AND is_compressed
                """,
                (table,),
            )
            return True
        except Exception as exc:
            log.warning("Could not decompress compressed chunks on %s: %s", table, exc)
            return False

    def _set_timescale_compression(table: str, enable: bool) -> bool:
        """Best-effort toggle for Timescale compression; returns success flag."""
        action = "enable" if enable else "disable"
        try:
            _execute_with_savepoint(
                conn,
                f"ALTER TABLE {table} SET (timescaledb.compress = {'true' if enable else 'false'})",
            )
            return True
        except psycopg.errors.FeatureNotSupported as exc:
            # Disabling fails when compressed chunks exist; try to decompress once.
            if enable:
                log.warning("Could not %s compression on %s: %s", action, table, exc)
                return False
            log.warning("Could not disable compression on %s: %s", table, exc)
            if not _decompress_compressed_chunks(table):
                log.warning(
                    "Unable to disable compression on %s because chunks could not be decompressed.",
                    table,
                )
                return False
            try:
                _execute_with_savepoint(
                    conn,
                    f"ALTER TABLE {table} SET (timescaledb.compress = false)",
                )
                return True
            except Exception as exc2:  # pragma: no cover - defensive
                log.warning(
                    "Disabling compression on %s still failed after decompressing chunks: %s",
                    table,
                    exc2,
                )
                return False
        except Exception as exc:
            log.warning("Could not %s compression on %s: %s", action, table, exc)
            return False

    def _create_index_allowing_compressed_hypertable(table: str, sql: str) -> bool:
        """
        Try to create an index even if the hypertable has compression enabled.
        Timescale refuses DDL while compression is on, so we disable it temporarily.
        """
        try:
            _execute_with_savepoint(conn, sql)
            return True
        except psycopg.errors.FeatureNotSupported:
            if not _timescale_compression_enabled(table):
                raise
            log.warning(
                "Compression detected on %s; disabling temporarily to create missing index.",
                table,
            )
            if not _set_timescale_compression(table, False):
                log.warning(
                    "Index skipped because compression could not be disabled on %s.",
                    table,
                )
                return False
            try:
                _execute_with_savepoint(conn, sql)
                return True
            except Exception as exc:
                log.warning(
                    "Creating index on %s failed even after disabling compression: %s",
                    table,
                    exc,
                )
            finally:
                _set_timescale_compression(table, True)
            return False

    def _backup_table_if_missing(source_table: str, backup_table: str) -> None:
        if not _table_exists(source_table) or _table_exists(backup_table):
            return
        conn.execute(f"CREATE TABLE {backup_table} AS TABLE {source_table} WITH DATA")

    def _repair_raid_identity_schema() -> None:
        if not (
            _table_exists("twitch_raid_history")
            and _table_exists("twitch_raid_retention")
            and _table_exists("twitch_partner_raid_score_tracking")
        ):
            return

        history_has_reference_key = _has_unique_constraint(
            "twitch_raid_history", ["id", "executed_at"]
        )
        retention_has_reference_key = _has_key_constraint(
            "twitch_raid_retention",
            ["raid_id", "executed_at"],
            ["PRIMARY KEY"],
        )
        retention_has_fk = _constraint_exists(
            "twitch_raid_retention",
            "twitch_raid_retention_raid_history_ref_fkey",
        )
        partner_has_fk = _constraint_exists(
            "twitch_partner_raid_score_tracking",
            "twitch_partner_raid_score_tracking_raid_history_ref_fkey",
        )

        migration_needed = any(
            [
                not history_has_reference_key,
                _column_data_type("twitch_raid_retention", "raid_id") != "bigint",
                _column_data_type("twitch_raid_retention", "executed_at")
                != "timestamp with time zone",
                _column_data_type("twitch_raid_retention", "computed_at")
                != "timestamp with time zone",
                not retention_has_reference_key,
                not retention_has_fk,
                _column_data_type(
                    "twitch_partner_raid_score_tracking", "raid_history_id"
                )
                != "bigint",
                _column_data_type(
                    "twitch_partner_raid_score_tracking", "raid_history_executed_at"
                )
                != "timestamp with time zone",
                not partner_has_fk,
                not _index_exists("idx_twitch_raid_retention_raid_id"),
                not _index_exists("idx_partner_raid_tracking_history_ref"),
            ]
        )
        if not migration_needed:
            _align_serial_sequence(conn, "twitch_raid_history", "id")
            return

        log.info(
            "DB migration: repairing raid identity references with (raid_id, executed_at)"
        )
        with conn.transaction():
            conn.execute(
                """
                LOCK TABLE
                    twitch_raid_history,
                    twitch_raid_retention,
                    twitch_partner_raid_score_tracking
                IN ACCESS EXCLUSIVE MODE
                """
            )

            _backup_table_if_missing(
                "twitch_raid_history",
                "twitch_raid_history_raid_identity_fix_backup",
            )
            _backup_table_if_missing(
                "twitch_raid_retention",
                "twitch_raid_retention_raid_identity_fix_backup",
            )
            _backup_table_if_missing(
                "twitch_partner_raid_score_tracking",
                "twitch_partner_raid_score_tracking_raid_identity_fix_backup",
            )

            _align_serial_sequence(conn, "twitch_raid_history", "id")
            if not history_has_reference_key:
                if _drop_constraint_if_exists(
                    "twitch_raid_retention",
                    "twitch_raid_retention_raid_history_ref_fkey",
                ):
                    retention_has_fk = False
                if _drop_constraint_if_exists(
                    "twitch_partner_raid_score_tracking",
                    "twitch_partner_raid_score_tracking_raid_history_ref_fkey",
                ):
                    partner_has_fk = False
            if not history_has_reference_key:
                _create_index_allowing_compressed_hypertable(
                    "twitch_raid_history",
                    "CREATE UNIQUE INDEX IF NOT EXISTS idx_twitch_raid_history_id_executed_at "
                    "ON twitch_raid_history(id, executed_at)",
                )
                history_has_reference_key = (
                    _ensure_unique_constraint_allowing_compressed_hypertable(
                        "twitch_raid_history",
                        "twitch_raid_history_id_executed_at_key",
                        ["id", "executed_at"],
                    )
                )
            if not history_has_reference_key:
                raise RuntimeError(
                    "twitch_raid_history(id, executed_at) is missing a PRIMARY KEY/UNIQUE constraint; "
                    "raid identity repair cannot continue."
                )

            if _column_data_type("twitch_raid_retention", "raid_id") != "bigint":
                conn.execute(
                    """
                    ALTER TABLE twitch_raid_retention
                    ALTER COLUMN raid_id TYPE BIGINT
                    USING raid_id::bigint
                    """
                )
            if (
                _column_data_type("twitch_raid_retention", "executed_at")
                != "timestamp with time zone"
            ):
                conn.execute(
                    """
                    ALTER TABLE twitch_raid_retention
                    ALTER COLUMN executed_at TYPE TIMESTAMPTZ
                    USING NULLIF(BTRIM(executed_at::text), '')::timestamptz
                    """
                )
            if (
                _column_data_type("twitch_raid_retention", "computed_at")
                != "timestamp with time zone"
            ):
                conn.execute(
                    """
                    ALTER TABLE twitch_raid_retention
                    ALTER COLUMN computed_at TYPE TIMESTAMPTZ
                    USING NULLIF(BTRIM(computed_at::text), '')::timestamptz
                    """
                )
            if not _has_key_constraint(
                "twitch_raid_retention", ["raid_id", "executed_at"], ["PRIMARY KEY"]
            ):
                if _constraint_exists(
                    "twitch_raid_retention", "twitch_raid_retention_pkey"
                ):
                    conn.execute(
                        "ALTER TABLE twitch_raid_retention DROP CONSTRAINT twitch_raid_retention_pkey"
                    )
                conn.execute(
                    """
                    ALTER TABLE twitch_raid_retention
                    ADD CONSTRAINT twitch_raid_retention_pkey PRIMARY KEY (raid_id, executed_at)
                    """
                )
            if not _constraint_exists(
                "twitch_raid_retention",
                "twitch_raid_retention_raid_history_ref_fkey",
            ):
                conn.execute(
                    """
                    ALTER TABLE twitch_raid_retention
                    ADD CONSTRAINT twitch_raid_retention_raid_history_ref_fkey
                    FOREIGN KEY (raid_id, executed_at)
                    REFERENCES twitch_raid_history(id, executed_at)
                    ON DELETE CASCADE
                    """
                )
            conn.execute(
                """
                CREATE INDEX IF NOT EXISTS idx_twitch_raid_retention_raid_id
                ON twitch_raid_retention(raid_id)
                """
            )

            if (
                _column_data_type(
                    "twitch_partner_raid_score_tracking", "raid_history_id"
                )
                != "bigint"
            ):
                conn.execute(
                    """
                    ALTER TABLE twitch_partner_raid_score_tracking
                    ALTER COLUMN raid_history_id TYPE BIGINT
                    USING raid_history_id::bigint
                    """
                )
            conn.execute(
                """
                ALTER TABLE twitch_partner_raid_score_tracking
                ADD COLUMN IF NOT EXISTS raid_history_executed_at TIMESTAMPTZ
                """
            )
            conn.execute(
                """
                WITH resolved_history AS (
                    SELECT
                        tracking.id,
                        history.id AS raid_history_id,
                        history.executed_at AS raid_history_executed_at
                    FROM twitch_partner_raid_score_tracking tracking
                    LEFT JOIN LATERAL (
                        SELECT rh.id, rh.executed_at
                        FROM twitch_raid_history rh
                        WHERE rh.to_broadcaster_id = tracking.to_broadcaster_id
                          AND LOWER(rh.to_broadcaster_login) = LOWER(tracking.to_broadcaster_login)
                          AND LOWER(rh.from_broadcaster_login) = LOWER(tracking.from_broadcaster_login)
                          AND COALESCE(rh.success, FALSE) IS TRUE
                          AND (
                              COALESCE(NULLIF(BTRIM(tracking.from_broadcaster_id::text), ''), '') = ''
                              OR rh.from_broadcaster_id = tracking.from_broadcaster_id
                          )
                          AND NULLIF(BTRIM(tracking.confirmed_at::text), '') IS NOT NULL
                          AND rh.executed_at
                              <= NULLIF(BTRIM(tracking.confirmed_at::text), '')::timestamptz
                                 + INTERVAL '10 minutes'
                        ORDER BY rh.executed_at DESC, rh.id DESC
                        LIMIT 1
                    ) history ON TRUE
                )
                UPDATE twitch_partner_raid_score_tracking tracking
                SET raid_history_id = resolved_history.raid_history_id,
                    raid_history_executed_at = resolved_history.raid_history_executed_at
                FROM resolved_history
                WHERE tracking.id = resolved_history.id
                  AND (
                      tracking.raid_history_id IS DISTINCT FROM resolved_history.raid_history_id
                      OR tracking.raid_history_executed_at
                         IS DISTINCT FROM resolved_history.raid_history_executed_at
                  )
                """
            )
            if not _constraint_exists(
                "twitch_partner_raid_score_tracking",
                "twitch_partner_raid_score_tracking_raid_history_ref_fkey",
            ):
                conn.execute(
                    """
                    ALTER TABLE twitch_partner_raid_score_tracking
                    ADD CONSTRAINT twitch_partner_raid_score_tracking_raid_history_ref_fkey
                    FOREIGN KEY (raid_history_id, raid_history_executed_at)
                    REFERENCES twitch_raid_history(id, executed_at)
                    ON DELETE SET NULL
                    """
                )
            conn.execute(
                """
                CREATE INDEX IF NOT EXISTS idx_partner_raid_tracking_history_ref
                ON twitch_partner_raid_score_tracking(raid_history_id, raid_history_executed_at)
                """
            )

    # 1) twitch_streamers
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_streamers (
            twitch_login               TEXT PRIMARY KEY,
            twitch_user_id             TEXT,
            require_discord_link       INTEGER DEFAULT 0,
            next_link_check_at         TEXT,
            discord_user_id            TEXT,
            discord_display_name       TEXT,
            is_on_discord              INTEGER DEFAULT 0,
            manual_verified_permanent  INTEGER DEFAULT 0,
            manual_verified_until      TEXT,
            manual_verified_at         TEXT,
            manual_partner_opt_out     INTEGER DEFAULT 0,
            created_at                 TEXT DEFAULT CURRENT_TIMESTAMP,
            archived_at                TEXT,
            raid_bot_enabled           INTEGER DEFAULT 0,
            silent_ban                 INTEGER DEFAULT 0,
            silent_raid                INTEGER DEFAULT 0,
            is_monitored_only          INTEGER DEFAULT 0,
            live_ping_role_id          BIGINT,
            live_ping_enabled          INTEGER DEFAULT 1
        )
        """
    )
    _pg_add_col_if_missing(conn, "twitch_streamers", "live_ping_role_id", "BIGINT")
    _pg_add_col_if_missing(
        conn, "twitch_streamers", "live_ping_enabled", "INTEGER DEFAULT 1"
    )
    conn.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_twitch_streamers_user_id ON twitch_streamers(twitch_user_id)"
    )
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_streamer_identities (
            twitch_user_id       TEXT PRIMARY KEY,
            twitch_login         TEXT NOT NULL,
            discord_user_id      TEXT,
            discord_display_name TEXT,
            is_on_discord        INTEGER DEFAULT 0,
            created_at           TEXT DEFAULT CURRENT_TIMESTAMP,
            updated_at           TEXT DEFAULT CURRENT_TIMESTAMP
        )
        """
    )
    conn.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_twitch_streamer_identities_login_lower "
        "ON twitch_streamer_identities(LOWER(twitch_login)) "
        "WHERE twitch_login IS NOT NULL AND twitch_login <> ''"
    )
    conn.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_twitch_streamer_identities_discord_user "
        "ON twitch_streamer_identities(discord_user_id) "
        "WHERE discord_user_id IS NOT NULL AND discord_user_id <> ''"
    )

    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_partners (
            id                        BIGSERIAL PRIMARY KEY,
            twitch_user_id            TEXT NOT NULL,
            twitch_login              TEXT NOT NULL,
            require_discord_link      INTEGER DEFAULT 0,
            last_description          TEXT,
            last_link_ok              INTEGER,
            added_by                  TEXT,
            last_link_checked_at      TEXT,
            next_link_check_at        TEXT,
            manual_verified_permanent INTEGER DEFAULT 0,
            manual_verified_until     TEXT,
            manual_verified_at        TEXT,
            manual_partner_opt_out    INTEGER DEFAULT 0,
            raid_bot_enabled          INTEGER DEFAULT 0,
            silent_ban                INTEGER DEFAULT 0,
            silent_raid               INTEGER DEFAULT 0,
            live_ping_role_id         BIGINT,
            live_ping_enabled         INTEGER DEFAULT 1,
            partnered_at              TEXT DEFAULT CURRENT_TIMESTAMP,
            admin_archived_at         TEXT,
            departnered_at            TEXT,
            status                    TEXT NOT NULL DEFAULT 'active'
        )
        """
    )
    _pg_add_col_if_missing(conn, "twitch_partners", "admin_archived_at", "TEXT")
    conn.execute(
        """
        UPDATE twitch_partners
        SET admin_archived_at = COALESCE(admin_archived_at, departnered_at, CURRENT_TIMESTAMP::text)
        WHERE status = 'archived'
        """
    )
    conn.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_twitch_partners_active_user_id "
        "ON twitch_partners(twitch_user_id) WHERE status = 'active'"
    )
    conn.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_twitch_partners_active_login_lower "
        "ON twitch_partners(LOWER(twitch_login)) WHERE status = 'active'"
    )

    conn.execute("DROP VIEW IF EXISTS twitch_streamers_partner_state")
    conn.execute("DROP VIEW IF EXISTS twitch_partners_all_state")
    conn.execute(
        """
        CREATE VIEW twitch_partners_all_state AS
        SELECT
            p.id,
            p.twitch_login,
            p.twitch_user_id,
            p.require_discord_link,
            p.next_link_check_at,
            i.discord_user_id,
            i.discord_display_name,
            COALESCE(i.is_on_discord, 0) AS is_on_discord,
            p.manual_verified_permanent,
            p.manual_verified_until,
            p.manual_verified_at,
            p.manual_partner_opt_out,
            p.partnered_at AS created_at,
            COALESCE(
                p.admin_archived_at,
                CASE WHEN p.status = 'archived' THEN p.departnered_at ELSE NULL END
            ) AS archived_at,
            p.raid_bot_enabled,
            p.silent_ban,
            p.silent_raid,
            0 AS is_monitored_only,
            CASE
                WHEN (
                    COALESCE(p.manual_verified_permanent, 0) = 1
                    OR (
                        p.manual_verified_until IS NOT NULL
                        AND p.manual_verified_until::timestamptz >= NOW()
                    )
                    OR p.manual_verified_at IS NOT NULL
                )
                THEN 1 ELSE 0
            END AS is_verified,
            1 AS is_partner,
            CASE
                WHEN p.status = 'active'
                     AND COALESCE(p.manual_partner_opt_out, 0) = 0
                THEN 1 ELSE 0
            END AS is_partner_active,
            p.live_ping_role_id,
            COALESCE(p.live_ping_enabled, 1) AS live_ping_enabled,
            p.status,
            p.departnered_at
        FROM twitch_partners p
        LEFT JOIN twitch_streamer_identities i
          ON i.twitch_user_id = p.twitch_user_id
        """
    )
    conn.execute(
        """
        CREATE VIEW twitch_streamers_partner_state AS
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
            is_verified,
            is_partner,
            is_partner_active,
            live_ping_role_id,
            live_ping_enabled
        FROM twitch_partners_all_state
        WHERE status = 'active'
        """
    )

    conn.execute(
        """
        CREATE OR REPLACE FUNCTION sync_twitch_streamer_identity_from_streamers()
        RETURNS trigger
        LANGUAGE plpgsql AS $$
        BEGIN
            IF COALESCE(NEW.twitch_user_id, '') <> '' THEN
                INSERT INTO twitch_streamer_identities (
                    twitch_user_id,
                    twitch_login,
                    discord_user_id,
                    discord_display_name,
                    is_on_discord,
                    created_at,
                    updated_at
                ) VALUES (
                    NEW.twitch_user_id,
                    LOWER(NEW.twitch_login),
                    NEW.discord_user_id,
                    NEW.discord_display_name,
                    COALESCE(NEW.is_on_discord, 0),
                    CURRENT_TIMESTAMP::text,
                    CURRENT_TIMESTAMP::text
                )
                ON CONFLICT (twitch_user_id) DO UPDATE SET
                    twitch_login = EXCLUDED.twitch_login,
                    discord_user_id = COALESCE(EXCLUDED.discord_user_id, twitch_streamer_identities.discord_user_id),
                    discord_display_name = COALESCE(EXCLUDED.discord_display_name, twitch_streamer_identities.discord_display_name),
                    is_on_discord = COALESCE(EXCLUDED.is_on_discord, twitch_streamer_identities.is_on_discord),
                    updated_at = CURRENT_TIMESTAMP::text;
            END IF;
            RETURN NEW;
        END;
        $$;
        """
    )
    conn.execute(
        "DROP TRIGGER IF EXISTS trg_twitch_streamers_sync_identity ON twitch_streamers"
    )
    conn.execute(
        """
        CREATE TRIGGER trg_twitch_streamers_sync_identity
        AFTER INSERT OR UPDATE OF twitch_login, twitch_user_id, discord_user_id, discord_display_name, is_on_discord
        ON twitch_streamers
        FOR EACH ROW
        EXECUTE FUNCTION sync_twitch_streamer_identity_from_streamers()
        """
    )

    conn.execute(
        """
        CREATE OR REPLACE FUNCTION sync_twitch_streamer_identity_from_partners()
        RETURNS trigger
        LANGUAGE plpgsql AS $$
        BEGIN
            IF NEW.status = 'active' AND COALESCE(NEW.twitch_user_id, '') <> '' THEN
                INSERT INTO twitch_streamer_identities (
                    twitch_user_id,
                    twitch_login,
                    discord_user_id,
                    discord_display_name,
                    is_on_discord,
                    created_at,
                    updated_at
                ) VALUES (
                    NEW.twitch_user_id,
                    LOWER(NEW.twitch_login),
                    (SELECT discord_user_id FROM twitch_streamer_identities WHERE twitch_user_id = NEW.twitch_user_id),
                    (SELECT discord_display_name FROM twitch_streamer_identities WHERE twitch_user_id = NEW.twitch_user_id),
                    COALESCE((SELECT is_on_discord FROM twitch_streamer_identities WHERE twitch_user_id = NEW.twitch_user_id), 0),
                    COALESCE((SELECT created_at FROM twitch_streamer_identities WHERE twitch_user_id = NEW.twitch_user_id), CURRENT_TIMESTAMP::text),
                    CURRENT_TIMESTAMP::text
                )
                ON CONFLICT (twitch_user_id) DO UPDATE SET
                    twitch_login = EXCLUDED.twitch_login,
                    updated_at = CURRENT_TIMESTAMP::text;
            END IF;
            RETURN NEW;
        END;
        $$;
        """
    )
    conn.execute(
        "DROP TRIGGER IF EXISTS trg_twitch_partners_sync_identity ON twitch_partners"
    )
    conn.execute(
        """
        CREATE TRIGGER trg_twitch_partners_sync_identity
        AFTER INSERT OR UPDATE OF twitch_login, twitch_user_id, status
        ON twitch_partners
        FOR EACH ROW
        EXECUTE FUNCTION sync_twitch_streamer_identity_from_partners()
        """
    )

    # 2) twitch_live_state
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_live_state (
            twitch_user_id              TEXT PRIMARY KEY,
            streamer_login              TEXT NOT NULL,
            last_stream_id              TEXT,
            last_started_at             TEXT,
            last_title                  TEXT,
            last_game_id                TEXT,
            last_discord_message_id     TEXT,
            last_notified_at            TEXT,
            is_live                     INTEGER DEFAULT 0,
            last_seen_at                TEXT,
            last_game                   TEXT,
            last_viewer_count           INTEGER DEFAULT 0,
            last_tracking_token         TEXT,
            active_session_id           INTEGER,
            had_deadlock_in_session     INTEGER DEFAULT 0,
            last_deadlock_seen_at       TEXT
        )
        """
    )

    # 3) Stats logs
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_stats_tracked (
            ts_utc       TEXT,
            streamer     TEXT,
            viewer_count INTEGER,
            is_partner   INTEGER DEFAULT 0,
            game_name    TEXT,
            stream_title TEXT,
            tags         TEXT
        )
        """
    )
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_stats_category (
            ts_utc       TEXT,
            streamer     TEXT,
            viewer_count INTEGER,
            is_partner   INTEGER DEFAULT 0,
            game_name    TEXT,
            stream_title TEXT,
            tags         TEXT
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_stats_tracked_streamer ON twitch_stats_tracked(streamer)"
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_stats_category_streamer ON twitch_stats_category(streamer)"
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_stats_category_ts ON twitch_stats_category(ts_utc)"
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_stats_tracked_ts ON twitch_stats_tracked(ts_utc)"
    )

    # 4) Link click tracking
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_link_clicks (
            id               SERIAL PRIMARY KEY,
            clicked_at       TEXT DEFAULT CURRENT_TIMESTAMP,
            streamer_login   TEXT NOT NULL,
            tracking_token   TEXT,
            discord_user_id  TEXT,
            discord_username TEXT,
            guild_id         TEXT,
            channel_id       TEXT,
            message_id       TEXT,
            ref_code         TEXT,
            source_hint      TEXT
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_link_clicks_streamer ON twitch_link_clicks(streamer_login)"
    )

    # 4b) Per-streamer live-announcement builder config
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_live_announcement_configs (
            streamer_login          TEXT PRIMARY KEY,
            config_json             TEXT NOT NULL,
            allowed_editor_role_ids TEXT,
            updated_at              TEXT DEFAULT CURRENT_TIMESTAMP,
            updated_by              TEXT
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_live_announce_configs_updated_at ON twitch_live_announcement_configs(updated_at)"
    )

    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_global_promo_modes (
            config_key     TEXT PRIMARY KEY,
            mode           TEXT NOT NULL DEFAULT 'standard',
            custom_message TEXT,
            starts_at      TEXT,
            ends_at        TEXT,
            is_enabled     INTEGER NOT NULL DEFAULT 0,
            updated_at     TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_by     TEXT
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_global_promo_modes_updated_at "
        "ON twitch_global_promo_modes(updated_at)"
    )

    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_global_settings (
            setting_key   TEXT PRIMARY KEY,
            setting_value TEXT NOT NULL,
            updated_at    TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_by    TEXT
        )
        """
    )
    conn.execute(
        "ALTER TABLE twitch_global_settings ADD COLUMN IF NOT EXISTS updated_by TEXT"
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_global_settings_updated_at "
        "ON twitch_global_settings(updated_at)"
    )

    # 5) Stream sessions & engagement
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_stream_sessions (
            id                      SERIAL PRIMARY KEY,
            streamer_login          TEXT NOT NULL,
            stream_id               TEXT,
            started_at              TEXT NOT NULL,
            ended_at                TEXT,
            duration_seconds        INTEGER DEFAULT 0,
            start_viewers           INTEGER DEFAULT 0,
            peak_viewers            INTEGER DEFAULT 0,
            end_viewers             INTEGER DEFAULT 0,
            avg_viewers             REAL    DEFAULT 0,
            samples                 INTEGER DEFAULT 0,
            retention_5m            REAL,
            retention_10m           REAL,
            retention_20m           REAL,
            dropoff_pct             REAL,
            dropoff_label           TEXT,
            unique_chatters         INTEGER DEFAULT 0,
            first_time_chatters     INTEGER DEFAULT 0,
            returning_chatters      INTEGER DEFAULT 0,
            followers_start         INTEGER,
            followers_end           INTEGER,
            follower_delta          INTEGER,
            stream_title            TEXT,
            notification_text       TEXT,
            language                TEXT,
            is_mature               INTEGER DEFAULT 0,
            tags                    TEXT,
            had_deadlock_in_session INTEGER DEFAULT 0,
            game_name               TEXT,
            notes                   TEXT
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_sessions_login ON twitch_stream_sessions(streamer_login, started_at)"
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_sessions_open ON twitch_stream_sessions(streamer_login) WHERE ended_at IS NULL"
    )
    _align_serial_sequence(conn, "twitch_stream_sessions", "id")

    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_session_viewers (
            session_id         INTEGER NOT NULL,
            ts_utc             TEXT    NOT NULL,
            minutes_from_start INTEGER,
            viewer_count       INTEGER NOT NULL,
            PRIMARY KEY (session_id, ts_utc)
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_session_viewers_session ON twitch_session_viewers(session_id)"
    )

    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_session_chatters (
            session_id               INTEGER NOT NULL,
            streamer_login           TEXT    NOT NULL,
            chatter_login            TEXT    NOT NULL,
            chatter_id               TEXT,
            first_message_at         TEXT    NOT NULL,
            messages                 INTEGER DEFAULT 0,
            is_first_time_streamer   BOOLEAN DEFAULT FALSE,
            seen_via_chatters_api    BOOLEAN DEFAULT FALSE,
            last_seen_at             TEXT,
            PRIMARY KEY (session_id, chatter_login)
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_session_chatters_login ON twitch_session_chatters(streamer_login, session_id)"
    )
    # Migration: rename is_first_time_global → is_first_time_streamer (clarifies scope)
    try:
        old_col = conn.execute(
            "SELECT 1 FROM information_schema.columns"
            " WHERE table_schema = current_schema()"
            " AND table_name = 'twitch_session_chatters'"
            " AND column_name = 'is_first_time_global'"
        ).fetchone()
        if old_col:
            conn.execute(
                "ALTER TABLE twitch_session_chatters"
                " RENAME COLUMN is_first_time_global TO is_first_time_streamer"
            )
            log.info(
                "DB migration: renamed twitch_session_chatters.is_first_time_global → is_first_time_streamer"
            )
    except Exception as exc:
        log.warning("DB migration: could not rename is_first_time_global: %s", exc)

    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_chatter_rollup (
            streamer_login  TEXT NOT NULL,
            chatter_login   TEXT NOT NULL,
            chatter_id      TEXT,
            first_seen_at   TEXT NOT NULL,
            last_seen_at    TEXT NOT NULL,
            total_messages  INTEGER DEFAULT 0,
            total_sessions  INTEGER DEFAULT 0,
            PRIMARY KEY (streamer_login, chatter_login)
        )
        """
    )

    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_chat_messages (
            id             SERIAL PRIMARY KEY,
            session_id     INTEGER NOT NULL,
            streamer_login TEXT    NOT NULL,
            chatter_login  TEXT,
            chatter_id     TEXT,
            message_id     TEXT,
            message_ts     TEXT    NOT NULL,
            is_command     BOOLEAN DEFAULT FALSE,
            content        TEXT
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_chat_messages_session ON twitch_chat_messages(session_id, message_ts)"
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_chat_messages_streamer_ts ON twitch_chat_messages(streamer_login, message_ts)"
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_chat_messages_chatter ON twitch_chat_messages(streamer_login, chatter_login, message_ts)"
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_chat_messages_message_id ON twitch_chat_messages(message_id)"
    )
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_raw_chat_ingest_health (
            streamer_login             TEXT PRIMARY KEY,
            last_raw_chat_message_at   TEXT,
            last_raw_chat_insert_ok_at TEXT,
            last_raw_chat_insert_error_at TEXT,
            last_raw_chat_error        TEXT,
            raw_chat_lag_seconds       INTEGER,
            updated_at                 TEXT NOT NULL
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_raw_chat_ingest_health_updated "
        "ON twitch_raw_chat_ingest_health(updated_at)"
    )
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_raw_chat_backfill_runs (
            id               SERIAL PRIMARY KEY,
            streamer_login   TEXT NOT NULL,
            started_at       TEXT NOT NULL,
            finished_at      TEXT,
            status           TEXT NOT NULL DEFAULT 'not_started',
            source_label     TEXT,
            imported_messages INTEGER DEFAULT 0,
            deduped_messages  INTEGER DEFAULT 0,
            affected_sessions INTEGER DEFAULT 0,
            note             TEXT
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_raw_chat_backfill_runs_streamer "
        "ON twitch_raw_chat_backfill_runs(streamer_login, started_at DESC)"
    )
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_observability_events (
            id           BIGSERIAL PRIMARY KEY,
            flow_type    TEXT NOT NULL,
            flow_id      TEXT NOT NULL,
            entity_login TEXT,
            entity_id    TEXT,
            step         TEXT NOT NULL,
            decision     TEXT NOT NULL,
            details_json TEXT NOT NULL DEFAULT '{}',
            created_at   TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
        )
        """
    )
    try:
        _execute_with_savepoint(
            conn,
            "SELECT create_hypertable("
            "'twitch_observability_events', "
            "'created_at', "
            "if_not_exists => TRUE, "
            "migrate_data => TRUE, "
            "chunk_time_interval => INTERVAL '7 days'"
            ")",
        )
    except Exception as exc:
        log.debug(
            "Could not convert twitch_observability_events to hypertable: %s", exc
        )
    try:
        _execute_with_savepoint(
            conn,
            "ALTER TABLE twitch_observability_events "
            "SET (timescaledb.compress, "
            "timescaledb.compress_segmentby = 'flow_type,flow_id', "
            "timescaledb.compress_orderby = 'created_at DESC')",
        )
    except Exception as exc:
        log.debug(
            "Could not enable compression on twitch_observability_events: %s", exc
        )
    try:
        _execute_with_savepoint(
            conn,
            "SELECT add_compression_policy("
            "'twitch_observability_events', "
            "INTERVAL '7 days', "
            "if_not_exists => TRUE"
            ")",
        )
    except Exception as exc:
        log.debug(
            "Could not add compression policy on twitch_observability_events: %s",
            exc,
        )
    _create_index_allowing_compressed_hypertable(
        "twitch_observability_events",
        "CREATE INDEX IF NOT EXISTS idx_twitch_observability_events_flow "
        "ON twitch_observability_events(flow_type, flow_id, created_at DESC)",
    )
    _create_index_allowing_compressed_hypertable(
        "twitch_observability_events",
        "CREATE INDEX IF NOT EXISTS idx_twitch_observability_events_entity "
        "ON twitch_observability_events(entity_login, created_at DESC)",
    )

    # 6) Raid history & blacklist
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_raid_history (
            id                       BIGSERIAL PRIMARY KEY,
            from_broadcaster_id      TEXT NOT NULL,
            from_broadcaster_login   TEXT NOT NULL,
            to_broadcaster_id        TEXT NOT NULL,
            to_broadcaster_login     TEXT NOT NULL,
            viewer_count             INTEGER DEFAULT 0,
            stream_duration_sec      INTEGER,
            reason                   TEXT,
            executed_at              TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
            success                  BOOLEAN DEFAULT TRUE,
            error_message            TEXT,
            target_stream_started_at TIMESTAMPTZ,
            candidates_count         INTEGER DEFAULT 0
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_raid_history_from ON twitch_raid_history(from_broadcaster_id)"
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_raid_history_to ON twitch_raid_history(to_broadcaster_id)"
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_raid_history_executed ON twitch_raid_history(executed_at)"
    )
    raid_history_success_type = _column_data_type("twitch_raid_history", "success")
    if raid_history_success_type and raid_history_success_type != "boolean":
        try:
            conn.execute(
                """
                ALTER TABLE twitch_raid_history
                ALTER COLUMN success TYPE BOOLEAN
                USING CASE
                    WHEN success IS NULL THEN FALSE
                    WHEN LOWER(BTRIM(success::text)) IN ('1', 'true', 't', 'yes', 'y', 'on') THEN TRUE
                    ELSE FALSE
                END
                """
            )
            log.info("DB migration: converted twitch_raid_history.success to BOOLEAN")
        except Exception as exc:
            log.warning(
                "DB migration: could not convert twitch_raid_history.success to BOOLEAN: %s",
                exc,
            )
    try:
        conn.execute(
            "ALTER TABLE twitch_raid_history ALTER COLUMN success SET DEFAULT TRUE"
        )
    except Exception as exc:
        log.debug("Skipping default migration on twitch_raid_history.success: %s", exc)
    raid_history_has_reference_key = _has_unique_constraint(
        "twitch_raid_history", ["id", "executed_at"]
    )
    if not raid_history_has_reference_key:
        _create_index_allowing_compressed_hypertable(
            "twitch_raid_history",
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_twitch_raid_history_id_executed_at "
            "ON twitch_raid_history(id, executed_at)",
        )
        raid_history_has_reference_key = (
            _ensure_unique_constraint_allowing_compressed_hypertable(
                "twitch_raid_history",
                "twitch_raid_history_id_executed_at_key",
                ["id", "executed_at"],
            )
        )
        if (
            _index_exists("idx_twitch_raid_history_id_executed_at")
            and not raid_history_has_reference_key
        ):
            log.warning(
                "Index idx_twitch_raid_history_id_executed_at exists but "
                "twitch_raid_history(id, executed_at) still has no PRIMARY KEY/UNIQUE constraint; "
                "raid reference foreign keys remain deferred until the table is repaired."
            )

    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_raid_blacklist (
            target_id    TEXT,
            target_login TEXT NOT NULL PRIMARY KEY,
            reason       TEXT,
            added_at     TEXT DEFAULT CURRENT_TIMESTAMP
        )
        """
    )
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_confirmed_external_recruitment_raids (
            id                     BIGSERIAL PRIMARY KEY,
            raid_flow_id           TEXT UNIQUE,
            from_broadcaster_id    TEXT,
            from_broadcaster_login TEXT NOT NULL,
            to_broadcaster_id      TEXT NOT NULL,
            to_broadcaster_login   TEXT NOT NULL,
            viewer_count           INTEGER DEFAULT 0,
            confirmation_signal    TEXT,
            confirmed_at           TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
        )
        """
    )
    conn.execute(
        """
        CREATE INDEX IF NOT EXISTS idx_confirmed_external_recruitment_raids_target
        ON twitch_confirmed_external_recruitment_raids(to_broadcaster_id)
        """
    )
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_external_recruitment_blacklist_pending (
            target_id           TEXT PRIMARY KEY,
            target_login        TEXT NOT NULL,
            confirmed_raid_count INTEGER NOT NULL,
            threshold_reached_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
            blacklist_after     TIMESTAMPTZ NOT NULL,
            last_raid_flow_id   TEXT,
            updated_at          TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
        )
        """
    )
    conn.execute(
        """
        CREATE INDEX IF NOT EXISTS idx_external_recruitment_blacklist_pending_due
        ON twitch_external_recruitment_blacklist_pending(blacklist_after)
        """
    )
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_external_bot_ban_check_pending (
            target_id     TEXT PRIMARY KEY,
            target_login  TEXT NOT NULL,
            source        TEXT NOT NULL,
            run_after     TIMESTAMPTZ NOT NULL,
            scheduled_at  TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at    TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
        )
        """
    )
    conn.execute(
        """
        CREATE INDEX IF NOT EXISTS idx_external_bot_ban_check_pending_due
        ON twitch_external_bot_ban_check_pending(run_after)
        """
    )

    # 6b) Raid retention rollup (computed)
    if not raid_history_has_reference_key:
        log.warning(
            "twitch_raid_history(id, executed_at) is still missing a PRIMARY KEY/UNIQUE constraint; "
            "raid reference foreign keys will be added by the repair path once the constraint exists."
        )

    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_raid_retention (
            raid_id                BIGINT NOT NULL,
            from_broadcaster_login TEXT NOT NULL,
            to_broadcaster_login   TEXT NOT NULL,
            viewer_count_sent      INTEGER NOT NULL,
            executed_at            TIMESTAMPTZ NOT NULL,
            target_session_id      INTEGER REFERENCES twitch_stream_sessions(id),
            chatters_at_plus5m     INTEGER,
            chatters_at_plus15m    INTEGER,
            chatters_at_plus30m    INTEGER,
            known_from_raider      INTEGER,
            new_to_target          INTEGER,
            new_chatters           INTEGER,
            computed_at            TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY (raid_id, executed_at)
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_raid_retention_raid_id ON twitch_raid_retention(raid_id)"
    )

    # 7) Token blacklist
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_token_blacklist (
            twitch_user_id   TEXT PRIMARY KEY,
            twitch_login     TEXT NOT NULL,
            error_message    TEXT,
            error_count      INTEGER DEFAULT 1,
            first_error_at   TEXT NOT NULL,
            last_error_at    TEXT NOT NULL,
            notified         INTEGER DEFAULT 0,
            grace_expires_at TEXT,
            user_dm_sent     INTEGER DEFAULT 0,
            reminder_sent    INTEGER DEFAULT 0,
            role_removed     INTEGER DEFAULT 0
        )
        """
    )

    # 8) Subscription / EventSub / Ads snapshots
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_subscriptions_snapshot (
            id             SERIAL PRIMARY KEY,
            twitch_user_id TEXT NOT NULL,
            twitch_login   TEXT,
            total          INTEGER DEFAULT 0,
            tier1          INTEGER DEFAULT 0,
            tier2          INTEGER DEFAULT 0,
            tier3          INTEGER DEFAULT 0,
            points         INTEGER DEFAULT 0,
            snapshot_at    TEXT DEFAULT CURRENT_TIMESTAMP
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_subs_user_ts ON twitch_subscriptions_snapshot(twitch_user_id, snapshot_at)"
    )

    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_eventsub_capacity_snapshot (
            id                 SERIAL PRIMARY KEY,
            ts_utc             TEXT DEFAULT CURRENT_TIMESTAMP,
            trigger_reason     TEXT,
            listener_count     INTEGER DEFAULT 0,
            ready_listeners    INTEGER DEFAULT 0,
            failed_listeners   INTEGER DEFAULT 0,
            used_slots         INTEGER DEFAULT 0,
            total_slots        INTEGER DEFAULT 0,
            headroom_slots     INTEGER DEFAULT 0,
            listeners_at_limit INTEGER DEFAULT 0,
            utilization_pct    REAL DEFAULT 0,
            listeners_json     TEXT
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_eventsub_capacity_ts ON twitch_eventsub_capacity_snapshot(ts_utc)"
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_eventsub_capacity_reason ON twitch_eventsub_capacity_snapshot(trigger_reason, ts_utc)"
    )
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS eventsub_guard_state (
            kind       TEXT NOT NULL,
            guard_key  TEXT NOT NULL,
            expires_at DOUBLE PRECISION NOT NULL,
            updated_at DOUBLE PRECISION NOT NULL,
            PRIMARY KEY (kind, guard_key)
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_eventsub_guard_state_expiry ON eventsub_guard_state(expires_at)"
    )
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_eventsub_bridge_outbox (
            message_id      TEXT PRIMARY KEY,
            sub_type        TEXT NOT NULL,
            payload_json    TEXT NOT NULL,
            queued_at       DOUBLE PRECISION NOT NULL,
            next_attempt_at DOUBLE PRECISION NOT NULL,
            attempt_count   INTEGER NOT NULL DEFAULT 0,
            last_error      TEXT
        )
        """
    )
    conn.execute(
        """
        CREATE INDEX IF NOT EXISTS idx_twitch_eventsub_bridge_outbox_due
        ON twitch_eventsub_bridge_outbox(next_attempt_at, queued_at)
        """
    )
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_eventsub_bridge_dead_letter (
            message_id        TEXT PRIMARY KEY,
            sub_type          TEXT NOT NULL,
            payload_json      TEXT NOT NULL,
            queued_at         DOUBLE PRECISION NOT NULL,
            dead_lettered_at  DOUBLE PRECISION NOT NULL,
            attempt_count     INTEGER NOT NULL,
            last_error        TEXT
        )
        """
    )
    conn.execute(
        """
        CREATE INDEX IF NOT EXISTS idx_twitch_eventsub_bridge_dead_lettered_at
        ON twitch_eventsub_bridge_dead_letter(dead_lettered_at)
        """
    )
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_eventsub_processing_inbox (
            work_id          TEXT PRIMARY KEY,
            work_type        TEXT NOT NULL,
            message_id       TEXT,
            payload_json     TEXT NOT NULL,
            queued_at        DOUBLE PRECISION NOT NULL,
            next_attempt_at  DOUBLE PRECISION NOT NULL,
            attempt_count    INTEGER NOT NULL DEFAULT 0,
            last_error       TEXT
        )
        """
    )
    conn.execute(
        """
        CREATE INDEX IF NOT EXISTS idx_twitch_eventsub_processing_inbox_due
        ON twitch_eventsub_processing_inbox(next_attempt_at, queued_at)
        """
    )
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_eventsub_processing_dead_letter (
            work_id           TEXT PRIMARY KEY,
            work_type         TEXT NOT NULL,
            message_id        TEXT,
            payload_json      TEXT NOT NULL,
            queued_at         DOUBLE PRECISION NOT NULL,
            dead_lettered_at  DOUBLE PRECISION NOT NULL,
            attempt_count     INTEGER NOT NULL,
            last_error        TEXT
        )
        """
    )
    conn.execute(
        """
        CREATE INDEX IF NOT EXISTS idx_twitch_eventsub_processing_dead_lettered_at
        ON twitch_eventsub_processing_dead_letter(dead_lettered_at)
        """
    )

    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_ads_schedule_snapshot (
            id                SERIAL PRIMARY KEY,
            twitch_user_id    TEXT NOT NULL,
            twitch_login      TEXT,
            next_ad_at        TEXT,
            last_ad_at        TEXT,
            duration          INTEGER,
            preroll_free_time INTEGER,
            snooze_count      INTEGER,
            snooze_refresh_at TEXT,
            snapshot_at       TEXT DEFAULT CURRENT_TIMESTAMP
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_ads_user_ts ON twitch_ads_schedule_snapshot(twitch_user_id, snapshot_at)"
    )

    # 9) Discord invite codes & streamer invites
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS discord_invite_codes (
            guild_id     BIGINT NOT NULL,
            invite_code  TEXT    NOT NULL,
            created_at   TEXT DEFAULT CURRENT_TIMESTAMP,
            last_seen_at TEXT DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY (guild_id, invite_code)
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_discord_invites_guild ON discord_invite_codes(guild_id)"
    )
    try:  # migrate existing INT -> BIGINT if needed
        conn.execute(
            "ALTER TABLE discord_invite_codes ALTER COLUMN guild_id TYPE BIGINT USING guild_id::bigint"
        )
    except Exception as exc:
        log.debug(
            "Skipping guild_id type migration on discord_invite_codes: %s",
            exc,
        )

    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_streamer_invites (
            streamer_login TEXT PRIMARY KEY,
            guild_id       BIGINT NOT NULL,
            channel_id     BIGINT NOT NULL,
            invite_code    TEXT    NOT NULL,
            invite_url     TEXT    NOT NULL,
            created_at     TEXT DEFAULT CURRENT_TIMESTAMP,
            last_sent_at   TEXT
        )
        """
    )
    conn.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_twitch_streamer_invites_code ON twitch_streamer_invites(invite_code)"
    )
    try:
        conn.execute(
            "ALTER TABLE twitch_streamer_invites ALTER COLUMN guild_id TYPE BIGINT USING guild_id::bigint"
        )
        conn.execute(
            "ALTER TABLE twitch_streamer_invites ALTER COLUMN channel_id TYPE BIGINT USING channel_id::bigint"
        )
    except Exception as exc:
        log.debug(
            "Skipping BIGINT migration on twitch_streamer_invites columns: %s",
            exc,
        )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_streamer_invites_guild ON twitch_streamer_invites(guild_id)"
    )

    # 10) Partner outreach
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_partner_outreach (
            streamer_login   TEXT PRIMARY KEY,
            streamer_user_id TEXT,
            detected_at      TEXT NOT NULL,
            contacted_at     TEXT,
            status           TEXT DEFAULT 'pending',
            cooldown_until   TEXT,
            notes            TEXT
        )
        """
    )

    # 11) Event tables
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_bits_events (
            id             SERIAL PRIMARY KEY,
            session_id     INTEGER,
            twitch_user_id TEXT    NOT NULL,
            donor_login    TEXT,
            amount         INTEGER NOT NULL,
            message        TEXT,
            received_at    TEXT    NOT NULL DEFAULT CURRENT_TIMESTAMP
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_bits_events_session ON twitch_bits_events(session_id)"
    )

    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_hype_train_events (
            id               SERIAL PRIMARY KEY,
            session_id       INTEGER,
            twitch_user_id   TEXT NOT NULL,
            started_at       TEXT,
            ended_at         TEXT,
            duration_seconds INTEGER,
            level            INTEGER,
            total_progress   INTEGER,
            event_phase      TEXT DEFAULT 'end'
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_hype_train_events_session ON twitch_hype_train_events(session_id)"
    )

    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_subscription_events (
            id                SERIAL PRIMARY KEY,
            session_id        INTEGER,
            twitch_user_id    TEXT NOT NULL,
            event_type        TEXT NOT NULL,
            user_login        TEXT,
            tier              TEXT,
            is_gift           INTEGER DEFAULT 0,
            gifter_login      TEXT,
            cumulative_months INTEGER,
            streak_months     INTEGER,
            message           TEXT,
            total_gifted      INTEGER,
            received_at       TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_subscription_events_session ON twitch_subscription_events(session_id)"
    )

    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_channel_updates (
            id             SERIAL PRIMARY KEY,
            twitch_user_id TEXT NOT NULL,
            title          TEXT,
            game_name      TEXT,
            language       TEXT,
            recorded_at    TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_channel_updates_user ON twitch_channel_updates(twitch_user_id, recorded_at)"
    )

    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_ad_break_events (
            id               SERIAL PRIMARY KEY,
            session_id       INTEGER,
            twitch_user_id   TEXT NOT NULL,
            duration_seconds INTEGER,
            is_automatic     INTEGER DEFAULT 0,
            started_at       TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_ad_break_events_session ON twitch_ad_break_events(session_id)"
    )

    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_ban_events (
            id              SERIAL PRIMARY KEY,
            session_id      INTEGER,
            twitch_user_id  TEXT NOT NULL,
            event_type      TEXT NOT NULL,
            target_login    TEXT,
            target_id       TEXT,
            moderator_login TEXT,
            reason          TEXT,
            ends_at         TEXT,
            received_at     TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_ban_events_user ON twitch_ban_events(twitch_user_id, received_at)"
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_ban_events_user_type_received "
        "ON twitch_ban_events(twitch_user_id, event_type, received_at)"
    )

    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_shoutout_events (
            id                       SERIAL PRIMARY KEY,
            twitch_user_id           TEXT NOT NULL,
            direction                TEXT NOT NULL,
            other_broadcaster_id     TEXT,
            other_broadcaster_login  TEXT,
            moderator_login          TEXT,
            viewer_count             INTEGER DEFAULT 0,
            received_at              TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_shoutout_events_user ON twitch_shoutout_events(twitch_user_id, received_at)"
    )

    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_follow_events (
            id             SERIAL PRIMARY KEY,
            streamer_login TEXT NOT NULL,
            twitch_user_id TEXT NOT NULL,
            follower_login TEXT NOT NULL,
            follower_id    TEXT,
            followed_at    TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_follow_events_streamer ON twitch_follow_events(streamer_login, followed_at)"
    )

    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_channel_points_events (
            id             SERIAL PRIMARY KEY,
            session_id     INTEGER,
            twitch_user_id TEXT NOT NULL,
            user_login     TEXT,
            reward_id      TEXT,
            reward_title   TEXT,
            reward_cost    INTEGER,
            user_input     TEXT,
            redeemed_at    TEXT NOT NULL
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_channel_points_events_user ON twitch_channel_points_events(twitch_user_id, redeemed_at)"
    )

    # 12) Social media clips
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_clips_social_media (
            id                    SERIAL PRIMARY KEY,
            clip_id               TEXT   NOT NULL UNIQUE,
            clip_url              TEXT   NOT NULL,
            clip_title            TEXT,
            clip_thumbnail_url    TEXT,
            streamer_login        TEXT   NOT NULL,
            twitch_user_id        TEXT,
            created_at            TEXT   NOT NULL,
            duration_seconds      REAL,
            view_count            INTEGER DEFAULT 0,
            game_name             TEXT,
            status                TEXT DEFAULT 'pending',
            downloaded_at         TEXT,
            local_file_path       TEXT,
            converted_file_path   TEXT,
            uploaded_tiktok       INTEGER DEFAULT 0,
            uploaded_youtube      INTEGER DEFAULT 0,
            uploaded_instagram    INTEGER DEFAULT 0,
            tiktok_video_id       TEXT,
            youtube_video_id      TEXT,
            instagram_media_id    TEXT,
            tiktok_uploaded_at    TEXT,
            youtube_uploaded_at   TEXT,
            instagram_uploaded_at TEXT,
            custom_title          TEXT,
            custom_description    TEXT,
            hashtags              TEXT,
            music_track           TEXT,
            last_analytics_sync   TEXT
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_clips_social_media_streamer ON twitch_clips_social_media(streamer_login, created_at)"
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_clips_social_media_status ON twitch_clips_social_media(status)"
    )
    _align_serial_sequence(conn, "twitch_clips_social_media", "id")

    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_clips_social_analytics (
            id                SERIAL PRIMARY KEY,
            clip_id           INTEGER NOT NULL,
            platform          TEXT    NOT NULL,
            platform_video_id TEXT,
            views             INTEGER DEFAULT 0,
            likes             INTEGER DEFAULT 0,
            comments          INTEGER DEFAULT 0,
            shares            INTEGER DEFAULT 0,
            saves             INTEGER DEFAULT 0,
            watch_time_avg    REAL,
            completion_rate   REAL,
            ctr               REAL,
            engagement_rate   REAL,
            external_clicks   INTEGER DEFAULT 0,
            new_followers     INTEGER DEFAULT 0,
            synced_at         TEXT    NOT NULL,
            posted_at         TEXT,
            FOREIGN KEY (clip_id) REFERENCES twitch_clips_social_media(id)
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_clips_social_analytics_clip ON twitch_clips_social_analytics(clip_id, synced_at)"
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_clips_social_analytics_platform ON twitch_clips_social_analytics(platform, posted_at)"
    )

    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_clips_upload_queue (
            id              SERIAL PRIMARY KEY,
            clip_id         INTEGER NOT NULL,
            platform        TEXT    NOT NULL,
            status          TEXT DEFAULT 'pending',
            priority        INTEGER DEFAULT 0,
            title           TEXT,
            description     TEXT,
            hashtags        TEXT,
            scheduled_at    TEXT,
            attempts        INTEGER DEFAULT 0,
            last_error      TEXT,
            last_attempt_at TEXT,
            created_at      TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            completed_at    TEXT,
            FOREIGN KEY (clip_id) REFERENCES twitch_clips_social_media(id)
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_clips_upload_queue_status ON twitch_clips_upload_queue(status, priority DESC)"
    )

    # 13) Templates & clip fetch history
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS clip_templates_global (
            id                   SERIAL PRIMARY KEY,
            template_name        TEXT NOT NULL UNIQUE,
            description_template TEXT NOT NULL,
            hashtags             TEXT NOT NULL,
            category             TEXT,
            usage_count          INTEGER DEFAULT 0,
            created_at           TEXT DEFAULT CURRENT_TIMESTAMP,
            created_by           TEXT
        )
        """
    )

    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS clip_templates_streamer (
            id                   SERIAL PRIMARY KEY,
            streamer_login       TEXT NOT NULL,
            template_name        TEXT NOT NULL,
            description_template TEXT NOT NULL,
            hashtags             TEXT NOT NULL,
            is_default           INTEGER DEFAULT 0,
            created_at           TEXT DEFAULT CURRENT_TIMESTAMP,
            updated_at           TEXT DEFAULT CURRENT_TIMESTAMP,
            UNIQUE (streamer_login, template_name)
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_clip_templates_streamer_login ON clip_templates_streamer(streamer_login)"
    )

    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS clip_last_hashtags (
            streamer_login TEXT PRIMARY KEY,
            hashtags       TEXT NOT NULL,
            last_used_at   TEXT NOT NULL
        )
        """
    )

    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS clip_fetch_history (
            id               SERIAL PRIMARY KEY,
            streamer_login   TEXT NOT NULL,
            fetched_at       TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            clips_found      INTEGER DEFAULT 0,
            clips_new        INTEGER DEFAULT 0,
            fetch_duration_ms INTEGER,
            error            TEXT
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_clip_fetch_history_streamer ON clip_fetch_history(streamer_login, fetched_at DESC)"
    )
    _align_serial_sequence(conn, "clip_fetch_history", "id")
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_clip_templates_global_category ON clip_templates_global(category)"
    )

    _seed_default_templates_pg(conn)

    # -----------------------------------------------------------------------
    # Auth tables
    # -----------------------------------------------------------------------

    # 14) Raid OAuth tokens (encrypted)
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_raid_auth (
            twitch_user_id       TEXT PRIMARY KEY,
            twitch_login         TEXT NOT NULL,
            access_token         TEXT DEFAULT 'ENC',
            refresh_token        TEXT DEFAULT 'ENC',
            token_expires_at     TEXT NOT NULL,
            scopes               TEXT NOT NULL,
            authorized_at        TEXT DEFAULT CURRENT_TIMESTAMP,
            last_refreshed_at    TEXT,
            raid_enabled         BOOLEAN DEFAULT TRUE,
            created_at           TEXT DEFAULT CURRENT_TIMESTAMP,
            needs_reauth         BOOLEAN DEFAULT FALSE,
            reauth_notified_at   TEXT,
            access_token_enc     BYTEA,
            refresh_token_enc    BYTEA,
            enc_version          INTEGER DEFAULT 1,
            enc_kid              TEXT DEFAULT 'v1',
            enc_migrated_at      TEXT
        )
        """
    )
    _ensure_twitch_raid_auth_login_index(conn)
    # Legacy-Plaintext-Spalten wurden per drop_legacy_tokens.py Migration entfernt.

    # 15) Social media platform OAuth (encrypted)
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS social_media_platform_auth (
            id                INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
            platform          TEXT NOT NULL,
            streamer_login    TEXT,
            access_token_enc  BYTEA NOT NULL,
            refresh_token_enc BYTEA,
            client_id         TEXT,
            client_secret_enc BYTEA,
            token_expires_at  TEXT,
            scopes            TEXT,
            platform_user_id  TEXT,
            platform_username TEXT,
            enc_version       INTEGER DEFAULT 1,
            enc_kid           TEXT DEFAULT 'v1',
            authorized_at     TEXT DEFAULT CURRENT_TIMESTAMP,
            last_refreshed_at TEXT,
            enabled           INTEGER DEFAULT 1
        )
        """
    )
    _ensure_social_media_auth_indexes(conn)
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_social_platform_auth ON social_media_platform_auth(platform, streamer_login, enabled)"
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_social_platform_auth_expires ON social_media_platform_auth(token_expires_at) WHERE enabled = 1"
    )

    # 16) OAuth CSRF state tokens
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS oauth_state_tokens (
            state_token    TEXT PRIMARY KEY,
            platform       TEXT NOT NULL,
            streamer_login TEXT,
            redirect_uri   TEXT,
            pkce_verifier  TEXT,
            created_at     TEXT DEFAULT CURRENT_TIMESTAMP,
            expires_at     TEXT NOT NULL
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_oauth_state_expires ON oauth_state_tokens(expires_at)"
    )

    # 17) Streamer-Pläne / Abonnements (zukünftiges Feature, noch inaktiv)
    # Verwaltet kostenpflichtige Bot-Pläne pro Streamer. Prüfung erfolgt nur wenn
    # SUBSCRIPTION_PLANS_ENABLED=True gesetzt wird. Bis dahin hat diese Tabelle
    # keinen Einfluss auf das Bot-Verhalten.
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS streamer_plans (
            twitch_user_id  TEXT PRIMARY KEY,
            twitch_login    TEXT,
            plan_name       TEXT NOT NULL DEFAULT 'free',
            promo_disabled  INTEGER NOT NULL DEFAULT 0,
            activated_at    TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            expires_at      TEXT,
            notes           TEXT
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_streamer_plans_login ON streamer_plans(twitch_login)"
    )
    conn.execute(
        "ALTER TABLE streamer_plans ADD COLUMN IF NOT EXISTS raid_boost_enabled INTEGER NOT NULL DEFAULT 0"
    )
    conn.execute(
        "ALTER TABLE streamer_plans ADD COLUMN IF NOT EXISTS promo_message TEXT"
    )
    conn.execute(
        "ALTER TABLE streamer_plans ADD COLUMN IF NOT EXISTS manual_plan_id TEXT"
    )
    conn.execute(
        "ALTER TABLE streamer_plans ADD COLUMN IF NOT EXISTS manual_plan_expires_at TEXT"
    )
    conn.execute(
        "ALTER TABLE streamer_plans ADD COLUMN IF NOT EXISTS manual_plan_notes TEXT NOT NULL DEFAULT ''"
    )
    conn.execute(
        "ALTER TABLE streamer_plans ADD COLUMN IF NOT EXISTS manual_plan_updated_at TEXT"
    )
    conn.execute(
        "ALTER TABLE streamer_plans ADD COLUMN IF NOT EXISTS trial_ever_granted INTEGER NOT NULL DEFAULT 0"
    )
    conn.execute(
        "ALTER TABLE streamer_plans ADD COLUMN IF NOT EXISTS first_login_at TEXT"
    )

    # 18) Vorgecachte Partner-Raid-Scores
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_partner_raid_scores (
            twitch_user_id                  TEXT PRIMARY KEY,
            twitch_login                    TEXT NOT NULL,
            avg_duration_sec                INTEGER NOT NULL DEFAULT 0,
            time_pattern_score_base         DOUBLE PRECISION NOT NULL DEFAULT 0.5,
            received_successful_raids_total INTEGER NOT NULL DEFAULT 0,
            is_new_partner_preferred        INTEGER NOT NULL DEFAULT 1,
            new_partner_multiplier          DOUBLE PRECISION NOT NULL DEFAULT 1.0,
            raid_boost_multiplier           DOUBLE PRECISION NOT NULL DEFAULT 1.0,
            is_live                         INTEGER NOT NULL DEFAULT 0,
            current_started_at              TEXT,
            current_uptime_sec              INTEGER NOT NULL DEFAULT 0,
            duration_score                  DOUBLE PRECISION NOT NULL DEFAULT 0.5,
            time_pattern_score              DOUBLE PRECISION NOT NULL DEFAULT 0.5,
            readiness_score                 DOUBLE PRECISION NOT NULL DEFAULT 0.5,
            fairness_score                  DOUBLE PRECISION NOT NULL DEFAULT 0.5,
            base_score                      DOUBLE PRECISION NOT NULL DEFAULT 0.5,
            final_score                     DOUBLE PRECISION NOT NULL DEFAULT 0.5,
            internal_sent_raids_30d         INTEGER NOT NULL DEFAULT 0,
            internal_received_raids_30d     INTEGER NOT NULL DEFAULT 0,
            internal_received_raids_7d      INTEGER NOT NULL DEFAULT 0,
            today_received_raids            INTEGER NOT NULL DEFAULT 0,
            last_computed_at                TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        )
        """
    )
    _pg_add_col_if_missing(
        conn, "twitch_partner_raid_scores", "readiness_score", "DOUBLE PRECISION NOT NULL DEFAULT 0.5"
    )
    _pg_add_col_if_missing(
        conn, "twitch_partner_raid_scores", "fairness_score", "DOUBLE PRECISION NOT NULL DEFAULT 0.5"
    )
    _pg_add_col_if_missing(
        conn, "twitch_partner_raid_scores", "internal_sent_raids_30d", "INTEGER NOT NULL DEFAULT 0"
    )
    _pg_add_col_if_missing(
        conn,
        "twitch_partner_raid_scores",
        "internal_received_raids_30d",
        "INTEGER NOT NULL DEFAULT 0",
    )
    _pg_add_col_if_missing(
        conn,
        "twitch_partner_raid_scores",
        "internal_received_raids_7d",
        "INTEGER NOT NULL DEFAULT 0",
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_partner_raid_scores_live_score "
        "ON twitch_partner_raid_scores(is_live, final_score DESC)"
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_partner_raid_scores_login "
        "ON twitch_partner_raid_scores(twitch_login)"
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_partner_raid_scores_computed "
        "ON twitch_partner_raid_scores(last_computed_at)"
    )

    # 19) Partner-Raid-Score-Tracking
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_partner_raid_score_tracking (
            id                        SERIAL PRIMARY KEY,
            raid_history_id           BIGINT,
            raid_history_executed_at  TIMESTAMPTZ,
            from_broadcaster_id       TEXT,
            from_broadcaster_login    TEXT NOT NULL,
            to_broadcaster_id         TEXT NOT NULL,
            to_broadcaster_login      TEXT NOT NULL,
            viewer_count              INTEGER NOT NULL DEFAULT 0,
            confirmed_at              TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            target_session_id         INTEGER,
            target_stream_started_at  TEXT,
            score_last_computed_at    TEXT,
            final_score               DOUBLE PRECISION,
            base_score                DOUBLE PRECISION,
            duration_score            DOUBLE PRECISION,
            time_pattern_score        DOUBLE PRECISION,
            readiness_score           DOUBLE PRECISION,
            fairness_score            DOUBLE PRECISION,
            new_partner_multiplier    DOUBLE PRECISION,
            raid_boost_multiplier     DOUBLE PRECISION,
            today_received_raids      INTEGER NOT NULL DEFAULT 0,
            was_deadlock_at_raid      INTEGER NOT NULL DEFAULT 0,
            deadlock_continued_until  TEXT,
            deadlock_continued_sec    INTEGER,
            resolved_at               TEXT,
            resolution_reason         TEXT
        )
        """
    )
    conn.execute(
        """
        ALTER TABLE twitch_partner_raid_score_tracking
        ADD COLUMN IF NOT EXISTS raid_history_executed_at TIMESTAMPTZ
        """
    )
    conn.execute(
        """
        ALTER TABLE twitch_partner_raid_score_tracking
        ADD COLUMN IF NOT EXISTS readiness_score DOUBLE PRECISION
        """
    )
    conn.execute(
        """
        ALTER TABLE twitch_partner_raid_score_tracking
        ADD COLUMN IF NOT EXISTS fairness_score DOUBLE PRECISION
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_partner_raid_tracking_target "
        "ON twitch_partner_raid_score_tracking(to_broadcaster_id, confirmed_at)"
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_partner_raid_tracking_session "
        "ON twitch_partner_raid_score_tracking(target_session_id, resolved_at)"
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_partner_raid_tracking_history "
        "ON twitch_partner_raid_score_tracking(raid_history_id)"
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_partner_raid_tracking_history_ref "
        "ON twitch_partner_raid_score_tracking(raid_history_id, raid_history_executed_at)"
    )
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_raid_arrival_tracking (
            id                        SERIAL PRIMARY KEY,
            detected_at               TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            last_signal_at            TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            from_broadcaster_id       TEXT,
            from_broadcaster_login    TEXT NOT NULL,
            to_broadcaster_id         TEXT NOT NULL,
            to_broadcaster_login      TEXT NOT NULL,
            viewer_count              INTEGER NOT NULL DEFAULT 0,
            classification            TEXT NOT NULL,
            confirmation_signals      TEXT NOT NULL DEFAULT '',
            primary_signal            TEXT,
            correlation_status        TEXT,
            correlation_detail        TEXT,
            source_resolution         TEXT,
            raid_history_id           BIGINT,
            raid_history_executed_at  TIMESTAMPTZ,
            unraid_seen               BOOLEAN NOT NULL DEFAULT FALSE,
            last_unraid_at            TIMESTAMPTZ
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_raid_arrival_tracking_target "
        "ON twitch_raid_arrival_tracking(to_broadcaster_id, detected_at DESC)"
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_raid_arrival_tracking_source "
        "ON twitch_raid_arrival_tracking(from_broadcaster_login, detected_at DESC)"
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_raid_arrival_tracking_history_ref "
        "ON twitch_raid_arrival_tracking(raid_history_id, raid_history_executed_at)"
    )
    _repair_raid_identity_schema()

    # 20) Web-Sessions
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS dashboard_sessions (
            session_id   TEXT PRIMARY KEY,
            session_type TEXT NOT NULL DEFAULT 'twitch',
            payload_enc  BYTEA NOT NULL,
            created_at   DOUBLE PRECISION NOT NULL,
            expires_at   DOUBLE PRECISION NOT NULL
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_dashboard_sessions_expires ON dashboard_sessions(expires_at)"
    )

    # 21) Promo-Cooldown-Persistenz
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_promo_cooldowns (
            login           TEXT NOT NULL,
            cooldown_type   TEXT NOT NULL,
            wall_ts         DOUBLE PRECISION NOT NULL,
            updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
            PRIMARY KEY (login, cooldown_type)
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_promo_cooldowns_wall_ts ON twitch_promo_cooldowns(wall_ts)"
    )

    # 22) First-Message-Events (channel.chat.user_first_message EventSub)
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_first_message_events (
            id             BIGSERIAL PRIMARY KEY,
            streamer_login TEXT NOT NULL,
            broadcaster_id TEXT NOT NULL,
            chatter_login  TEXT NOT NULL,
            chatter_id     TEXT,
            message_id     TEXT,
            message_text   TEXT,
            event_ts       TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_first_message_events_streamer "
        "ON twitch_first_message_events(streamer_login, event_ts DESC)"
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_twitch_first_message_events_chatter "
        "ON twitch_first_message_events(chatter_login)"
    )
    try:
        _pg_add_col_if_missing(
            conn,
            "twitch_session_chatters",
            "confirmed_first_ever",
            "BOOLEAN DEFAULT FALSE",
        )
    except Exception as exc:
        log.debug(
            "DB migration: confirmed_first_ever konnte nicht zu twitch_session_chatters hinzugefügt werden: %s",
            exc,
        )

    migrate_legacy_partner_registry(conn)

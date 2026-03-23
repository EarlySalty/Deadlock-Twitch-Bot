"""Encrypted PostgreSQL session storage for dashboard web sessions.

Sessions are persisted in the shared PostgreSQL DB (table: dashboard_sessions).
The payload is encrypted with Fernet (AES-128-CBC + HMAC-SHA256) using a key stored
in the Windows Credential Manager (service: DeadlockBot, key: SESSIONS_ENCRYPTION_KEY).

If the key is missing it is auto-generated and saved to the keyring on first run.
Without the key the ciphertext is useless to an attacker even if they access the DB.

Public API is identical to the previous SQLite implementation - callers are unchanged.
"""

from __future__ import annotations

import hashlib
import json
from typing import TYPE_CHECKING

from ..core.constants import log
from .pg import readonly_connection, transaction

if TYPE_CHECKING:
    from cryptography.fernet import Fernet as _FernetT

_KEYRING_SERVICE = "DeadlockBot"
_KEYRING_KEY_NAME = "SESSIONS_ENCRYPTION_KEY"

# Module-level Fernet singleton - initialised lazily
_fernet: _FernetT | None = None


# ---------------------------------------------------------------------------
# Key management
# ---------------------------------------------------------------------------

def _load_or_create_key() -> bytes:
    """Return the Fernet key from keyring, creating and storing it on first use."""
    try:
        import keyring  # type: ignore

        val = keyring.get_password(_KEYRING_SERVICE, _KEYRING_KEY_NAME)
        if val:
            return val.encode()
    except Exception as exc:
        log.debug("Keyring read for sessions key failed: %s", exc)

    from cryptography.fernet import Fernet

    key = Fernet.generate_key()
    log.info("Sessions: generated new encryption key")
    try:
        import keyring  # type: ignore

        keyring.set_password(_KEYRING_SERVICE, _KEYRING_KEY_NAME, key.decode())
        log.info("Sessions: stored encryption key in Windows Credential Manager")
    except Exception as exc:
        log.error(
            "Sessions: CRITICAL - could not store encryption key in keyring (%s). "
            "Sessions will not survive restarts until the key is persisted. "
            "Store manually: keyring.set_password('DeadlockBot', 'SESSIONS_ENCRYPTION_KEY', <key>)",
            exc,
        )
    return key


def _get_fernet() -> _FernetT:
    global _fernet
    if _fernet is None:
        from cryptography.fernet import Fernet

        _fernet = Fernet(_load_or_create_key())
    return _fernet


# ---------------------------------------------------------------------------
# Encryption helpers
# ---------------------------------------------------------------------------

def _encrypt(payload: dict) -> bytes:
    return _get_fernet().encrypt(json.dumps(payload, default=str).encode())


def _decrypt(data: bytes | memoryview) -> dict:
    raw = bytes(data) if isinstance(data, memoryview) else data
    return json.loads(_get_fernet().decrypt(raw).decode())


def _row_payload(row) -> dict | None:
    if not row:
        return None
    try:
        payload_enc = row["payload_enc"]
        session_id = str(row["session_id"] or "")
    except Exception:
        payload_enc = row[0]
        session_id = str(row[1] or "") if len(row) > 1 else ""

    try:
        return _decrypt(payload_enc)
    except Exception as exc:
        log.debug("Sessions: could not decrypt row %s: %s", session_id, exc)
        return None


# ---------------------------------------------------------------------------
# Public API  (mirrors what auth_mixin / routes_mixin call)
# ---------------------------------------------------------------------------

def upsert_session(
    session_id: str,
    session_type: str,
    payload: dict,
    created_at: float,
    expires_at: float,
) -> None:
    """Insert or refresh a session (payload is Fernet-encrypted)."""
    enc = _encrypt(payload)
    with transaction() as conn:
        conn.execute(
            """
            INSERT INTO dashboard_sessions
                (session_id, session_type, payload_enc, created_at, expires_at)
            VALUES (%s, %s, %s, %s, %s)
            ON CONFLICT(session_id) DO UPDATE SET
                payload_enc = EXCLUDED.payload_enc,
                expires_at  = EXCLUDED.expires_at
            """,
            (session_id, session_type, enc, created_at, expires_at),
        )


def delete_session(session_id: str) -> None:
    """Remove a session (logout / invalidation)."""
    with transaction() as conn:
        conn.execute("DELETE FROM dashboard_sessions WHERE session_id = %s", (session_id,))


def load_valid_sessions(
    session_type: str, min_expires_at: float
) -> list[tuple[str, dict]]:
    """Return all non-expired sessions as (session_id, payload) tuples."""
    with readonly_connection() as conn:
        rows = conn.execute(
            """
            SELECT session_id, payload_enc
            FROM   dashboard_sessions
            WHERE  session_type = %s AND expires_at > %s
            """,
            (session_type, min_expires_at),
        ).fetchall()

    result: list[tuple[str, dict]] = []
    fernet = _get_fernet()
    for row in rows:
        try:
            payload = json.loads(fernet.decrypt(bytes(row["payload_enc"])).decode())
            result.append((row["session_id"], payload))
        except Exception as exc:
            log.debug("Sessions: could not decrypt row %s: %s", row["session_id"], exc)
    return result


def load_session(
    session_id: str,
    session_type: str,
    min_expires_at: float,
) -> dict | None:
    """Load a single non-expired session/state payload by id."""
    with readonly_connection() as conn:
        row = conn.execute(
            """
            SELECT payload_enc, session_id
            FROM   dashboard_sessions
            WHERE  session_id = %s
              AND  session_type = %s
              AND  expires_at > %s
            LIMIT 1
            """,
            (session_id, session_type, min_expires_at),
        ).fetchone()
    return _row_payload(row)


def pop_session(
    session_id: str,
    session_type: str,
    min_expires_at: float,
) -> dict | None:
    """Atomically consume a single non-expired session/state payload."""
    with transaction() as conn:
        row = conn.execute(
            """
            DELETE FROM dashboard_sessions
            WHERE session_id = %s
              AND session_type = %s
              AND expires_at > %s
            RETURNING payload_enc, session_id
            """,
            (session_id, session_type, min_expires_at),
        ).fetchone()
    return _row_payload(row)


def count_valid_sessions(
    session_type: str,
    min_expires_at: float,
    *,
    session_id_prefix: str | None = None,
) -> int:
    """Count active sessions/states, optionally filtered by session_id prefix."""
    sql = """
        SELECT COUNT(*)
        FROM dashboard_sessions
        WHERE session_type = %s
          AND expires_at > %s
    """
    params: list[object] = [session_type, min_expires_at]
    if session_id_prefix:
        sql += " AND session_id LIKE %s"
        params.append(f"{session_id_prefix}%")

    with readonly_connection() as conn:
        row = conn.execute(sql, tuple(params)).fetchone()
    if not row:
        return 0
    try:
        return int(row[0])
    except Exception:
        return int(row["count"] or 0)


def delete_expired_sessions(now: float) -> None:
    """Purge all sessions that have already expired."""
    with transaction() as conn:
        conn.execute("DELETE FROM dashboard_sessions WHERE expires_at <= %s", (now,))


def reserve_rate_limit_slot(
    *,
    bucket_key: str,
    session_type: str,
    session_id: str,
    payload: dict,
    created_at: float,
    expires_at: float,
    max_requests: int,
) -> bool:
    """Atomically reserve one rate-limit slot for a bucket."""
    if max_requests <= 0:
        return False

    enc = _encrypt(payload)
    lock_a, lock_b = _advisory_lock_pair(f"{session_type}:{bucket_key}")
    session_prefix = f"{bucket_key}%"

    with transaction() as conn:
        conn.execute("SELECT pg_advisory_xact_lock(%s, %s)", (lock_a, lock_b))
        row = conn.execute(
            """
            SELECT COUNT(*)
            FROM dashboard_sessions
            WHERE session_type = %s
              AND expires_at > %s
              AND session_id LIKE %s
            """,
            (session_type, created_at, session_prefix),
        ).fetchone()
        active_hits = 0
        if row:
            try:
                active_hits = int(row[0])
            except Exception:
                active_hits = int(row["count"] or 0)
        if active_hits >= max_requests:
            return False

        conn.execute(
            """
            INSERT INTO dashboard_sessions
                (session_id, session_type, payload_enc, created_at, expires_at)
            VALUES (%s, %s, %s, %s, %s)
            ON CONFLICT(session_id) DO UPDATE SET
                payload_enc = EXCLUDED.payload_enc,
                expires_at  = EXCLUDED.expires_at
            """,
            (session_id, session_type, enc, created_at, expires_at),
        )
        return True


def _advisory_lock_pair(value: str) -> tuple[int, int]:
    digest = hashlib.sha256(str(value or "").encode("utf-8", errors="ignore")).digest()
    return (
        int.from_bytes(digest[:4], byteorder="big", signed=True),
        int.from_bytes(digest[4:8], byteorder="big", signed=True),
    )

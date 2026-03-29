"""Durable auth/session state repository for dashboard OAuth and rate limits."""

from __future__ import annotations

import hashlib
import secrets
import time
from typing import Any

from ...storage import sessions_db

_TWITCH_OAUTH_STATE_TYPE = "oauth_state:twitch"
_DISCORD_ADMIN_OAUTH_STATE_TYPE = "oauth_state:discord_admin"
_AFFILIATE_OAUTH_STATE_TYPE = "oauth_state:affiliate"
_AFFILIATE_CONNECT_STATE_TYPE = "oauth_state:affiliate_connect"
_TWITCH_SESSION_TYPE = "twitch"
_DISCORD_ADMIN_SESSION_TYPE = "discord_admin"
_RATE_LIMIT_SESSION_TYPE = "rate_limit:dashboard_auth"


def _now() -> float:
    return time.time()


class DashboardAuthRateLimitStoreUnavailable(RuntimeError):
    """Raised when the durable auth rate-limit store cannot be reached."""


class DashboardAuthStateRepository:
    """Persist OAuth states and authenticated sessions in the shared session store."""

    def save_twitch_oauth_state(
        self,
        *,
        state: str,
        payload: dict[str, Any],
        ttl_seconds: float,
        now: float | None = None,
    ) -> None:
        self._save_state(
            state_type=_TWITCH_OAUTH_STATE_TYPE,
            state=state,
            payload=payload,
            ttl_seconds=ttl_seconds,
            now=now,
        )

    def consume_twitch_oauth_state(
        self,
        state: str,
        *,
        now: float | None = None,
    ) -> dict[str, Any] | None:
        return self._consume_state(_TWITCH_OAUTH_STATE_TYPE, state, now=now)

    def save_discord_admin_oauth_state(
        self,
        *,
        state: str,
        payload: dict[str, Any],
        ttl_seconds: float,
        now: float | None = None,
    ) -> None:
        self._save_state(
            state_type=_DISCORD_ADMIN_OAUTH_STATE_TYPE,
            state=state,
            payload=payload,
            ttl_seconds=ttl_seconds,
            now=now,
        )

    def consume_discord_admin_oauth_state(
        self,
        state: str,
        *,
        now: float | None = None,
    ) -> dict[str, Any] | None:
        return self._consume_state(_DISCORD_ADMIN_OAUTH_STATE_TYPE, state, now=now)

    def save_affiliate_oauth_state(
        self,
        *,
        state: str,
        payload: dict[str, Any],
        ttl_seconds: float,
        now: float | None = None,
    ) -> None:
        self._save_state(
            state_type=_AFFILIATE_OAUTH_STATE_TYPE,
            state=state,
            payload=payload,
            ttl_seconds=ttl_seconds,
            now=now,
        )

    def consume_affiliate_oauth_state(
        self,
        state: str,
        *,
        now: float | None = None,
    ) -> dict[str, Any] | None:
        return self._consume_state(_AFFILIATE_OAUTH_STATE_TYPE, state, now=now)

    def save_affiliate_connect_state(
        self,
        *,
        state: str,
        payload: dict[str, Any],
        ttl_seconds: float,
        now: float | None = None,
    ) -> None:
        self._save_state(
            state_type=_AFFILIATE_CONNECT_STATE_TYPE,
            state=state,
            payload=payload,
            ttl_seconds=ttl_seconds,
            now=now,
        )

    def consume_affiliate_connect_state(
        self,
        state: str,
        *,
        now: float | None = None,
    ) -> dict[str, Any] | None:
        return self._consume_state(_AFFILIATE_CONNECT_STATE_TYPE, state, now=now)

    def load_dashboard_session(self, session_id: str, *, now: float | None = None) -> dict[str, Any] | None:
        return self._load_session(_TWITCH_SESSION_TYPE, session_id, now=now)

    def save_dashboard_session(
        self,
        *,
        session_id: str,
        payload: dict[str, Any],
        created_at: float,
        expires_at: float,
    ) -> None:
        sessions_db.upsert_session(session_id, _TWITCH_SESSION_TYPE, payload, created_at, expires_at)

    def load_discord_admin_session(
        self,
        session_id: str,
        *,
        now: float | None = None,
    ) -> dict[str, Any] | None:
        return self._load_session(_DISCORD_ADMIN_SESSION_TYPE, session_id, now=now)

    def save_discord_admin_session(
        self,
        *,
        session_id: str,
        payload: dict[str, Any],
        created_at: float,
        expires_at: float,
    ) -> None:
        sessions_db.upsert_session(
            session_id,
            _DISCORD_ADMIN_SESSION_TYPE,
            payload,
            created_at,
            expires_at,
        )

    @staticmethod
    def delete_session(session_id: str) -> None:
        sessions_db.delete_session(session_id)

    @staticmethod
    def delete_expired(now: float | None = None) -> None:
        sessions_db.delete_expired_sessions(_now() if now is None else float(now))

    def _save_state(
        self,
        *,
        state_type: str,
        state: str,
        payload: dict[str, Any],
        ttl_seconds: float,
        now: float | None,
    ) -> None:
        current = _now() if now is None else float(now)
        record = dict(payload)
        record.setdefault("created_at", current)
        sessions_db.upsert_session(
            state,
            state_type,
            record,
            float(record.get("created_at", current)),
            current + max(1.0, float(ttl_seconds)),
        )

    def _consume_state(
        self,
        state_type: str,
        state: str,
        *,
        now: float | None,
    ) -> dict[str, Any] | None:
        return sessions_db.pop_session(
            state,
            state_type,
            _now() if now is None else float(now),
        )

    def _load_session(
        self,
        session_type: str,
        session_id: str,
        *,
        now: float | None,
    ) -> dict[str, Any] | None:
        return sessions_db.load_session(
            session_id,
            session_type,
            _now() if now is None else float(now),
        )


class DashboardAuthRateLimitStore:
    """Durable sliding-window limiter backed by shared PostgreSQL session storage."""

    def allow_request(
        self,
        *,
        key: str,
        max_requests: int,
        window_seconds: float,
        now: float | None = None,
    ) -> bool:
        current = _now() if now is None else float(now)
        if max_requests <= 0:
            return False
        if window_seconds <= 0:
            return True

        bucket_prefix = self._bucket_prefix(key=key, window_seconds=window_seconds)
        try:
            return sessions_db.reserve_rate_limit_slot(
                bucket_key=bucket_prefix,
                session_type=_RATE_LIMIT_SESSION_TYPE,
                session_id=self._hit_record_id(bucket_prefix, current),
                payload={
                    "key_hash": bucket_prefix.rsplit(":", 1)[-1],
                    "seen_at": current,
                    "window_seconds": float(window_seconds),
                },
                created_at=current,
                expires_at=current + float(window_seconds),
                max_requests=max_requests,
            )
        except Exception as exc:
            raise DashboardAuthRateLimitStoreUnavailable(
                "dashboard auth rate limit store unavailable"
            ) from exc

    @staticmethod
    def _bucket_prefix(*, key: str, window_seconds: float) -> str:
        key_hash = hashlib.sha256(str(key or "").encode("utf-8", errors="ignore")).hexdigest()
        return f"rl:{int(window_seconds)}:{key_hash}"

    @staticmethod
    def _hit_record_id(bucket_prefix: str, now: float) -> str:
        return f"{bucket_prefix}:{int(now * 1000)}:{secrets.token_urlsafe(6)}"
